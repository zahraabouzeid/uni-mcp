//! Sign-in for AI apps on a server: OAuth 2.1 with PKCE. Codes and tokens are
//! stored as fingerprints, so the database alone connects no app.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::Url;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uni_core::Error;

use crate::accounts::{self, Accounts, fingerprint, random_code};
use crate::hosting;
use crate::store::{Database, sql};

const CODE_VALID_SECONDS: u64 = 5 * 60;
pub const ACCESS_VALID_SECONDS: u64 = 60 * 60;
const UNUSED_CONNECTION_SECONDS: u64 = 90 * 24 * 60 * 60;
const UNCONNECTED_APP_SECONDS: u64 = 24 * 60 * 60;
// Writing down every use would cost a write per tool call.
const LAST_USED_STEP_SECONDS: u64 = 60 * 60;
const MAX_UNCONNECTED_APPS: u64 = 1000;
const MAX_REDIRECTS: usize = 5;
const MAX_URI_CHARS: usize = 500;
const MAX_APP_NAME_CHARS: usize = 100;

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Request(String),
    App,
    Grant,
    Server(String),
}

impl From<Error> for Refusal {
    fn from(e: Error) -> Self {
        Self::Server(e.to_string())
    }
}

type Result<T> = std::result::Result<T, Refusal>;

#[derive(Debug)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
}

pub struct OAuth {
    database: Arc<Database>,
}

fn db<T>(result: rusqlite::Result<T>) -> Result<T> {
    result.map_err(|e| Refusal::Server(sql(e).to_string()))
}

fn forget_expired(connection: &Connection, now: u64) -> Result<()> {
    db(connection.execute_batch(&format!(
        "DELETE FROM authorization_codes WHERE expires <= {now};
         DELETE FROM access_tokens WHERE expires <= {now};
         DELETE FROM connections WHERE last_used + {UNUSED_CONNECTION_SECONDS} <= {now};
         DELETE FROM apps WHERE registered + {UNCONNECTED_APP_SECONDS} <= {now}
             AND NOT EXISTS (SELECT 1 FROM connections WHERE connections.app = apps.id);",
    )))
}

fn allowed_redirect(uri: &str) -> bool {
    let Ok(url) = Url::parse(uri) else {
        return false;
    };
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    uri.len() <= MAX_URI_CHARS
        && url.fragment().is_none()
        && url.username().is_empty()
        && (url.scheme() == "https" || (url.scheme() == "http" && loopback))
}

fn pkce_text(text: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
    (43..=128).contains(&text.len()) && text.chars().all(allowed)
}

pub(crate) fn challenge_of(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn issue(connection: &Connection, id: &str, now: u64) -> Result<Tokens> {
    let tokens = Tokens {
        access: random_code()?,
        refresh: random_code()?,
    };
    db(connection.execute(
        "INSERT INTO access_tokens (fingerprint, connection, expires) VALUES (?1, ?2, ?3)",
        params![fingerprint(&tokens.access), id, now + ACCESS_VALID_SECONDS],
    ))?;
    db(connection.execute(
        "INSERT INTO refresh_tokens (fingerprint, connection) VALUES (?1, ?2)",
        params![fingerprint(&tokens.refresh), id],
    ))?;
    Ok(tokens)
}

impl OAuth {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }

    /// Anyone may register, so an app nobody connects is forgotten after a day.
    pub fn register(&self, name: &str, redirects: &[String], now: u64) -> Result<String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_APP_NAME_CHARS {
            return Err(Refusal::Request(format!(
                "an app name has 1 to {MAX_APP_NAME_CHARS} characters"
            )));
        }
        if redirects.is_empty() || redirects.len() > MAX_REDIRECTS {
            return Err(Refusal::Request(format!(
                "an app has 1 to {MAX_REDIRECTS} redirect URIs"
            )));
        }
        if let Some(refused) = redirects.iter().find(|uri| !allowed_redirect(uri)) {
            return Err(Refusal::Request(format!(
                "redirect URI not allowed: {refused}"
            )));
        }
        let id = random_code()?;
        self.database
            .with(|connection| {
                let transaction = connection.transaction().map_err(sql)?;
                forget_expired(&transaction, now).map_err(server)?;
                let waiting: u64 = transaction
                    .query_row(
                        "SELECT count(*) FROM apps WHERE NOT EXISTS
                             (SELECT 1 FROM connections WHERE connections.app = apps.id)",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(sql)?;
                if waiting >= MAX_UNCONNECTED_APPS {
                    return Ok(Err(Refusal::Request(
                        "too many apps are waiting to be connected; try again tomorrow".into(),
                    )));
                }
                transaction
                    .execute(
                        "INSERT INTO apps (id, name, registered) VALUES (?1, ?2, ?3)",
                        params![id, name, now],
                    )
                    .map_err(sql)?;
                for uri in redirects {
                    transaction
                        .execute(
                            "INSERT OR IGNORE INTO app_redirects (app, uri) VALUES (?1, ?2)",
                            params![id, uri],
                        )
                        .map_err(sql)?;
                }
                transaction.commit().map_err(sql)?;
                Ok(Ok(id))
            })
            .map_err(Refusal::from)?
    }

    pub fn app_name(&self, app: &str, redirect: &str) -> Result<String> {
        let name: Option<String> = self.database.with(|connection| {
            connection
                .query_row(
                    "SELECT apps.name FROM apps
                     JOIN app_redirects ON app_redirects.app = apps.id
                     WHERE apps.id = ?1 AND app_redirects.uri = ?2",
                    params![app, redirect],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql)
        })?;
        name.ok_or(Refusal::App)
    }

    pub fn allow(
        &self,
        account: &str,
        app: &str,
        redirect: &str,
        challenge: &str,
        now: u64,
    ) -> Result<String> {
        if !pkce_text(challenge) {
            return Err(Refusal::Request("a PKCE code challenge is required".into()));
        }
        self.app_name(app, redirect)?;
        let code = random_code()?;
        self.database.with(|connection| {
            connection
                .execute(
                    "INSERT INTO authorization_codes
                         (fingerprint, account, app, redirect, challenge, expires)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        fingerprint(&code),
                        account,
                        app,
                        redirect,
                        challenge,
                        now + CODE_VALID_SECONDS
                    ],
                )
                .map_err(sql)
        })?;
        Ok(code)
    }

    /// A code works once, even when the attempt fails.
    pub fn exchange(
        &self,
        code: &str,
        app: &str,
        redirect: &str,
        verifier: &str,
        now: u64,
    ) -> Result<Tokens> {
        let id = random_code()?;
        self.database
            .with(|connection| {
                let transaction = connection.transaction().map_err(sql)?;
                let used: Option<(String, String, String, String, u64)> = transaction
                    .query_row(
                        "DELETE FROM authorization_codes WHERE fingerprint = ?1
                         RETURNING account, app, redirect, challenge, expires",
                        [fingerprint(code)],
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sql)?;
                let tokens = match used {
                    Some((account, for_app, for_redirect, challenge, expires))
                        if expires > now
                            && for_app == app
                            && for_redirect == redirect
                            && pkce_text(verifier)
                            && challenge_of(verifier) == challenge =>
                    {
                        transaction
                            .execute(
                                "INSERT INTO connections (id, account, app, connected, last_used)
                                 VALUES (?1, ?2, ?3, ?4, ?4)",
                                params![id, account, app, now],
                            )
                            .map_err(sql)?;
                        issue(&transaction, &id, now)
                    }
                    _ => Err(Refusal::Grant),
                };
                if !matches!(tokens, Err(Refusal::Server(_))) {
                    transaction.commit().map_err(sql)?;
                }
                Ok(tokens)
            })
            .map_err(Refusal::from)?
    }

    pub fn refresh(&self, refresh: &str, app: &str, now: u64) -> Result<Tokens> {
        self.database
            .with(|connection| {
                let transaction = connection.transaction().map_err(sql)?;
                forget_expired(&transaction, now).map_err(server)?;
                let id: Option<String> = transaction
                    .query_row(
                        "DELETE FROM refresh_tokens WHERE fingerprint = ?1
                         AND connection IN (SELECT id FROM connections WHERE app = ?2)
                         RETURNING connection",
                        params![fingerprint(refresh), app],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(sql)?;
                let tokens = match id {
                    Some(id) => {
                        transaction
                            .execute("DELETE FROM access_tokens WHERE connection = ?1", [&id])
                            .map_err(sql)?;
                        transaction
                            .execute(
                                "UPDATE connections SET last_used = ?2 WHERE id = ?1",
                                params![id, now],
                            )
                            .map_err(sql)?;
                        issue(&transaction, &id, now)
                    }
                    None => Err(Refusal::Grant),
                };
                if !matches!(tokens, Err(Refusal::Server(_))) {
                    transaction.commit().map_err(sql)?;
                }
                Ok(tokens)
            })
            .map_err(Refusal::from)?
    }

    pub fn account_of(&self, access: &str, now: u64) -> Option<String> {
        let found = self.database.with(|connection| {
            let found: Option<(String, String, u64)> = connection
                .query_row(
                    "SELECT connections.id, connections.account, connections.last_used
                     FROM access_tokens JOIN connections ON connections.id = access_tokens.connection
                     WHERE access_tokens.fingerprint = ?1 AND access_tokens.expires > ?2
                         AND connections.last_used + ?3 > ?2",
                    params![fingerprint(access), now, UNUSED_CONNECTION_SECONDS],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(sql)?;
            if let Some((id, _, last_used)) = &found
                && now.saturating_sub(*last_used) >= LAST_USED_STEP_SECONDS
            {
                connection
                    .execute(
                        "UPDATE connections SET last_used = ?2 WHERE id = ?1",
                        params![id, now],
                    )
                    .map_err(sql)?;
            }
            Ok(found.map(|(_, account, _)| account))
        });
        found.ok().flatten()
    }
}

fn server(refusal: Refusal) -> Error {
    match refusal {
        Refusal::Server(message) => Error::Config(message),
        other => Error::Config(format!("{other:?}")),
    }
}

#[derive(Clone)]
struct Web {
    oauth: Arc<OAuth>,
    accounts: Arc<Accounts>,
    base: Arc<str>,
}

pub fn routes(oauth: Arc<OAuth>, accounts: Arc<Accounts>, base: &str) -> Router {
    Router::new()
        .route("/.well-known/oauth-protected-resource", get(resource))
        .route("/.well-known/oauth-protected-resource/mcp", get(resource))
        .route("/.well-known/oauth-authorization-server", get(issuer))
        .route("/oauth/register", post(register))
        .route("/oauth/token", post(token))
        .with_state(web(oauth, accounts, base))
}

pub fn api(oauth: Arc<OAuth>, accounts: Arc<Accounts>, base: &str) -> Router {
    Router::new()
        .route("/oauth/app", get(app))
        .route("/oauth/answer", post(answer))
        .with_state(web(oauth, accounts, base))
}

fn web(oauth: Arc<OAuth>, accounts: Arc<Accounts>, base: &str) -> Web {
    Web {
        oauth,
        accounts,
        base: base.into(),
    }
}

pub fn resource_metadata(base: &str) -> String {
    format!("{base}/.well-known/oauth-protected-resource/mcp")
}

async fn resource(State(web): State<Web>) -> Json<Value> {
    Json(json!({
        "resource": format!("{}/mcp", web.base),
        "authorization_servers": [web.base],
        "bearer_methods_supported": ["header"],
        "resource_name": "uniMCP",
    }))
}

async fn issuer(State(web): State<Web>) -> Json<Value> {
    let base = &web.base;
    Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/oauth/authorize"),
        "token_endpoint": format!("{base}/oauth/token"),
        "registration_endpoint": format!("{base}/oauth/register"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "authorization_response_iss_parameter_supported": true,
    }))
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        let (status, error, description) = match self {
            Self::Request(description) => (StatusCode::BAD_REQUEST, "invalid_request", description),
            Self::App => (
                StatusCode::UNAUTHORIZED,
                "invalid_client",
                "unknown app".into(),
            ),
            Self::Grant => (
                StatusCode::BAD_REQUEST,
                "invalid_grant",
                "the code or token is not valid".into(),
            ),
            Self::Server(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "something went wrong on the server".into(),
            ),
        };
        let body = json!({ "error": error, "error_description": description });
        (status, [(CACHE_CONTROL, "no-store")], Json(body)).into_response()
    }
}

#[derive(Deserialize)]
struct Registration {
    client_name: Option<String>,
    #[serde(default)]
    redirect_uris: Vec<String>,
}

async fn register(
    State(web): State<Web>,
    Json(registration): Json<Registration>,
) -> Result<Response> {
    let name = registration
        .client_name
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            let first = registration.redirect_uris.first()?;
            Url::parse(first).ok()?.host_str().map(str::to_owned)
        })
        .unwrap_or_default();
    let now = accounts::now();
    let id = web
        .oauth
        .register(&name, &registration.redirect_uris, now)?;
    let body = json!({
        "client_id": id,
        "client_id_issued_at": now,
        "client_name": name.trim(),
        "redirect_uris": registration.redirect_uris,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    client_id: Option<String>,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
}

fn required(value: Option<String>, name: &str) -> Result<String> {
    value.ok_or_else(|| Refusal::Request(format!("{name} is missing")))
}

async fn token(State(web): State<Web>, Form(request): Form<TokenRequest>) -> Result<Response> {
    let now = accounts::now();
    let app = required(request.client_id, "client_id")?;
    let tokens = match request.grant_type.as_str() {
        "authorization_code" => web.oauth.exchange(
            &required(request.code, "code")?,
            &app,
            &required(request.redirect_uri, "redirect_uri")?,
            &required(request.code_verifier, "code_verifier")?,
            now,
        )?,
        "refresh_token" => web.oauth.refresh(
            &required(request.refresh_token, "refresh_token")?,
            &app,
            now,
        )?,
        _ => {
            let body = json!({ "error": "unsupported_grant_type" });
            return Ok((StatusCode::BAD_REQUEST, Json(body)).into_response());
        }
    };
    let body = json!({
        "access_token": tokens.access,
        "token_type": "Bearer",
        "expires_in": ACCESS_VALID_SECONDS,
        "refresh_token": tokens.refresh,
    });
    Ok(([(CACHE_CONTROL, "no-store")], Json(body)).into_response())
}

#[derive(Deserialize)]
struct Asking {
    client_id: String,
    redirect_uri: String,
}

async fn app(State(web): State<Web>, Query(asking): Query<Asking>) -> Result<Response> {
    let name = web
        .oauth
        .app_name(&asking.client_id, &asking.redirect_uri)?;
    Ok(Json(json!({ "name": name })).into_response())
}

#[derive(Deserialize)]
struct Answer {
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    code_challenge_method: Option<String>,
    state: Option<String>,
    allow: bool,
}

async fn answer(
    State(web): State<Web>,
    headers: HeaderMap,
    Json(answer): Json<Answer>,
) -> Result<Response> {
    let Some(person) = hosting::person_in(&web.accounts, &headers).filter(|p| !p.admin) else {
        let body = json!({ "error": "log in first" });
        return Ok((StatusCode::UNAUTHORIZED, Json(body)).into_response());
    };
    web.oauth
        .app_name(&answer.client_id, &answer.redirect_uri)?;
    let mut back = Url::parse(&answer.redirect_uri)
        .map_err(|_| Refusal::Request("redirect_uri is not a URI".into()))?;
    {
        let mut query = back.query_pairs_mut();
        let plain = answer.code_challenge_method.as_deref() != Some("S256");
        if !answer.allow {
            query.append_pair("error", "access_denied");
        } else if plain {
            query.append_pair("error", "invalid_request");
            query.append_pair("error_description", "PKCE with S256 is required");
        } else {
            let code = web.oauth.allow(
                &person.id,
                &answer.client_id,
                &answer.redirect_uri,
                &answer.code_challenge,
                accounts::now(),
            )?;
            query.append_pair("code", &code);
        }
        if let Some(state) = &answer.state {
            query.append_pair("state", state);
        }
        query.append_pair("iss", &web.base);
    }
    Ok(Json(json!({ "redirect": back.as_str() })).into_response())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::accounts::testing::{accounts, join};

    const REDIRECT: &str = "https://claude.ai/api/mcp/auth_callback";
    const VERIFIER: &str = "a-verifier-that-is-long-enough-for-pkce-0123456789";
    const DAY: u64 = 24 * 60 * 60;

    struct Server {
        oauth: OAuth,
        member: String,
        folder: std::path::PathBuf,
        accounts: crate::accounts::Accounts,
    }

    fn server(name: &str) -> Server {
        let (accounts, folder) = accounts(name);
        join(&accounts, "Zahraa");
        let (member, _) = join(&accounts, "Bestie");
        let database = crate::store::Database::open(&folder).expect("database");
        Server {
            oauth: OAuth::new(database),
            member: member.id,
            folder,
            accounts,
        }
    }

    impl Server {
        fn connect(&self, now: u64) -> (String, Tokens) {
            let app = self
                .oauth
                .register("Claude", &[REDIRECT.into()], now)
                .expect("register");
            let code = self
                .oauth
                .allow(&self.member, &app, REDIRECT, &challenge_of(VERIFIER), now)
                .expect("allow");
            let tokens = self
                .oauth
                .exchange(&code, &app, REDIRECT, VERIFIER, now)
                .expect("exchange");
            (app, tokens)
        }

        fn done(self) {
            drop(self.oauth);
            drop(self.accounts);
            fs::remove_dir_all(self.folder).expect("clean up");
        }
    }

    #[test]
    fn only_safe_redirects_can_be_registered() {
        let server = server("oauth-register");
        let register = |uris: &[&str]| {
            let uris: Vec<String> = uris.iter().map(|uri| uri.to_string()).collect();
            server.oauth.register("Claude", &uris, 0)
        };
        assert!(register(&[REDIRECT]).is_ok());
        assert!(register(&["http://127.0.0.1:6274/callback"]).is_ok());
        for refused in [
            "http://claude.ai/callback",
            "https://claude.ai/callback#part",
            "https://user@claude.ai/callback",
            "javascript:alert(1)",
            "not a uri",
        ] {
            assert!(
                matches!(register(&[refused]), Err(Refusal::Request(_))),
                "{refused}"
            );
        }
        assert!(register(&[]).is_err());
        assert!(server.oauth.register(" ", &[REDIRECT.into()], 0).is_err());
        server.done();
    }

    #[test]
    fn an_allowed_app_acts_for_that_account_only() {
        let server = server("oauth-flow");
        let (_, tokens) = server.connect(0);
        assert_eq!(
            server.oauth.account_of(&tokens.access, 1),
            Some(server.member.clone())
        );
        assert_eq!(server.oauth.account_of(&tokens.refresh, 1), None);
        assert_eq!(server.oauth.account_of("made up", 1), None);
        assert_eq!(
            server
                .oauth
                .account_of(&tokens.access, ACCESS_VALID_SECONDS),
            None
        );
        server.done();
    }

    #[test]
    fn a_code_works_once_and_only_with_its_verifier_and_redirect() {
        let server = server("oauth-code");
        let app = server
            .oauth
            .register("Claude", &[REDIRECT.into()], 0)
            .expect("register");
        let challenge = challenge_of(VERIFIER);
        let allow = || {
            server
                .oauth
                .allow(&server.member, &app, REDIRECT, &challenge, 0)
                .expect("allow")
        };
        let wrong = "another-verifier-that-is-long-enough-for-pkce-012345";
        assert_eq!(
            server
                .oauth
                .exchange(&allow(), &app, REDIRECT, wrong, 0)
                .expect_err("refused"),
            Refusal::Grant
        );
        let elsewhere = "https://claude.ai/elsewhere";
        assert!(
            server
                .oauth
                .exchange(&allow(), &app, elsewhere, VERIFIER, 0)
                .is_err()
        );
        let late = allow();
        assert!(
            server
                .oauth
                .exchange(&late, &app, REDIRECT, VERIFIER, CODE_VALID_SECONDS)
                .is_err()
        );

        let code = allow();
        assert!(
            server
                .oauth
                .exchange(&code, &app, REDIRECT, VERIFIER, 0)
                .is_ok()
        );
        assert!(
            server
                .oauth
                .exchange(&code, &app, REDIRECT, VERIFIER, 0)
                .is_err()
        );
        server.done();
    }

    #[test]
    fn a_code_is_only_given_for_a_registered_redirect_and_with_a_challenge() {
        let server = server("oauth-allow");
        let app = server
            .oauth
            .register("Claude", &[REDIRECT.into()], 0)
            .expect("register");
        let challenge = challenge_of(VERIFIER);
        let other = "https://attacker.example/callback";
        let allow = |app: &str, redirect: &str, challenge: &str| {
            server
                .oauth
                .allow(&server.member, app, redirect, challenge, 0)
        };
        assert_eq!(
            allow(&app, other, &challenge).expect_err("refused"),
            Refusal::App
        );
        assert_eq!(
            allow("unknown", REDIRECT, &challenge).expect_err("refused"),
            Refusal::App
        );
        assert!(matches!(
            allow(&app, REDIRECT, ""),
            Err(Refusal::Request(_))
        ));
        assert_eq!(server.oauth.app_name(&app, REDIRECT), Ok("Claude".into()));
        server.done();
    }

    #[test]
    fn refreshing_replaces_both_tokens() {
        let server = server("oauth-refresh");
        let (app, first) = server.connect(0);
        let second = server
            .oauth
            .refresh(&first.refresh, &app, 10)
            .expect("refresh");
        assert_eq!(server.oauth.account_of(&first.access, 11), None);
        assert!(server.oauth.account_of(&second.access, 11).is_some());
        assert_eq!(
            server
                .oauth
                .refresh(&first.refresh, &app, 12)
                .expect_err("refused"),
            Refusal::Grant
        );
        assert!(
            server
                .oauth
                .refresh(&second.refresh, "another app", 12)
                .is_err()
        );
        server.done();
    }

    #[test]
    fn a_connection_unused_for_90_days_ends() {
        let server = server("oauth-unused");
        let (app, tokens) = server.connect(0);
        let still = server
            .oauth
            .refresh(&tokens.refresh, &app, 89 * DAY)
            .expect("in time");
        assert!(
            server
                .oauth
                .refresh(&still.refresh, &app, 179 * DAY + 1)
                .is_err()
        );
        server.done();
    }

    #[test]
    fn removing_an_account_disconnects_its_apps() {
        let server = server("oauth-remove");
        let (app, tokens) = server.connect(0);
        server.accounts.remove(&server.member).expect("remove");
        assert_eq!(server.oauth.account_of(&tokens.access, 1), None);
        assert!(server.oauth.refresh(&tokens.refresh, &app, 1).is_err());
        server.done();
    }

    mod over_http {
        use axum::body::{Body, to_bytes};
        use axum::http::Request;
        use tower::ServiceExt;

        use super::*;

        const BASE: &str = "https://uni.example";

        async fn call(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
            let response = router.clone().oneshot(request).await.expect("response");
            let status = response.status();
            let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
            (status, serde_json::from_slice(&bytes).unwrap_or_default())
        }

        fn post_json(path: &str, body: Value, session: Option<&str>) -> Request<Body> {
            let mut request = Request::post(path).header("content-type", "application/json");
            if let Some(session) = session {
                request = request.header("cookie", format!("unimcp_session={session}"));
            }
            request.body(Body::from(body.to_string())).expect("request")
        }

        fn post_form(path: &str, fields: &[(&str, &str)]) -> Request<Body> {
            let body = fields
                .iter()
                .map(|(key, value)| format!("{key}={}", urlencode(value)))
                .collect::<Vec<_>>()
                .join("&");
            Request::post(path)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .expect("request")
        }

        fn urlencode(value: &str) -> String {
            Url::parse_with_params("https://x", [("v", value)])
                .expect("url")
                .query()
                .expect("query")["v=".len()..]
                .to_owned()
        }

        fn router(name: &str) -> (Router, String, String, std::path::PathBuf) {
            let (accounts, folder) = accounts(name);
            let now = accounts::now();
            let join_now = |name: &str| {
                let invite = accounts.invite(now).expect("invite");
                accounts
                    .accept(&invite, name, crate::accounts::testing::PASSWORD, now)
                    .expect("account")
            };
            let (admin, admin_session) = join_now("Zahraa");
            assert!(admin.admin);
            let (_, member_session) = join_now("Bestie");
            let database = crate::store::Database::open(&folder).expect("database");
            let oauth = Arc::new(OAuth::new(database));
            let accounts = Arc::new(accounts);
            let router = routes(oauth.clone(), accounts.clone(), BASE)
                .nest("/api", api(oauth, accounts, BASE));
            (router, member_session, admin_session, folder)
        }

        #[tokio::test]
        async fn an_app_finds_the_server_registers_and_gets_tokens_after_allow() {
            let (router, member, admin, folder) = router("oauth-http");

            let get = |path: &str| Request::get(path).body(Body::empty()).expect("request");
            let (_, resource) =
                call(&router, get("/.well-known/oauth-protected-resource/mcp")).await;
            assert_eq!(resource["resource"], "https://uni.example/mcp");
            let (_, issuer) = call(&router, get("/.well-known/oauth-authorization-server")).await;
            assert_eq!(issuer["code_challenge_methods_supported"], json!(["S256"]));

            let registration = json!({ "client_name": "Claude", "redirect_uris": [REDIRECT] });
            let (status, app) =
                call(&router, post_json("/oauth/register", registration, None)).await;
            assert_eq!(status, StatusCode::CREATED);
            let app = app["client_id"].as_str().expect("client id").to_owned();

            let answer = |allow: bool| {
                json!({
                    "client_id": app,
                    "redirect_uri": REDIRECT,
                    "code_challenge": challenge_of(VERIFIER),
                    "code_challenge_method": "S256",
                    "state": "s1",
                    "allow": allow,
                })
            };
            let (status, _) =
                call(&router, post_json("/api/oauth/answer", answer(true), None)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            let as_admin = post_json("/api/oauth/answer", answer(true), Some(&admin));
            assert_eq!(call(&router, as_admin).await.0, StatusCode::UNAUTHORIZED);

            let denied = post_json("/api/oauth/answer", answer(false), Some(&member));
            let (_, denied) = call(&router, denied).await;
            let denied = denied["redirect"].as_str().expect("redirect");
            assert!(denied.starts_with(REDIRECT) && denied.contains("error=access_denied"));

            let allowed = post_json("/api/oauth/answer", answer(true), Some(&member));
            let (_, allowed) = call(&router, allowed).await;
            let back = Url::parse(allowed["redirect"].as_str().expect("redirect")).expect("url");
            let pairs: std::collections::HashMap<_, _> = back.query_pairs().into_owned().collect();
            assert_eq!(pairs["state"], "s1");
            assert_eq!(pairs["iss"], BASE);

            let exchange = post_form(
                "/oauth/token",
                &[
                    ("grant_type", "authorization_code"),
                    ("client_id", &app),
                    ("code", &pairs["code"]),
                    ("redirect_uri", REDIRECT),
                    ("code_verifier", VERIFIER),
                ],
            );
            let (status, tokens) = call(&router, exchange).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(tokens["token_type"], "Bearer");

            let refresh = post_form(
                "/oauth/token",
                &[
                    ("grant_type", "refresh_token"),
                    ("client_id", &app),
                    (
                        "refresh_token",
                        tokens["refresh_token"].as_str().expect("refresh"),
                    ),
                ],
            );
            assert_eq!(call(&router, refresh).await.0, StatusCode::OK);
            let reused = post_form(
                "/oauth/token",
                &[
                    ("grant_type", "refresh_token"),
                    ("client_id", &app),
                    (
                        "refresh_token",
                        tokens["refresh_token"].as_str().expect("refresh"),
                    ),
                ],
            );
            let (status, refused) = call(&router, reused).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(refused["error"], "invalid_grant");

            drop(router);
            fs::remove_dir_all(folder).expect("clean up");
        }
    }

    #[test]
    fn the_database_holds_no_code_or_token() {
        let server = server("oauth-stored");
        let (_, tokens) = server.connect(0);
        drop(server.oauth);
        drop(server.accounts);
        let raw = fs::read(server.folder.join("unimcp.db")).expect("database file");
        let raw = String::from_utf8_lossy(&raw);
        assert!(!raw.contains(&tokens.access) && !raw.contains(&tokens.refresh));
        fs::remove_dir_all(server.folder).expect("clean up");
    }
}
