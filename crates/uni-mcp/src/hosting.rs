//! The account API of a home server. The admin side, for inviting and removing
//! people and the server's settings, is only served on the server itself; the
//! member side, which is the one reachable from outside, has no admin at all.

use std::sync::Arc;

use axum::extract::State;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use uni_core::Error;

use crate::accounts::{self, Accounts, Person, SESSION_VALID_SECONDS};
use crate::autostart::{self, Launch};

const COOKIE_NAME: &str = "unimcp_session";
const ADMIN_ELSEWHERE: &str = "the admin signs in on the server itself";

pub struct Host {
    pub accounts: Arc<Accounts>,
    pub launch: Launch,
    /// Where members reach the server, for the links in invites.
    pub public_url: Option<String>,
}

#[derive(Clone)]
struct Side {
    host: Arc<Host>,
    members_only: bool,
}

struct Refusal(StatusCode, String);

impl From<Error> for Refusal {
    fn from(e: Error) -> Self {
        Self(StatusCode::BAD_REQUEST, e.to_string())
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

type Answer = Result<Response, Refusal>;

fn account_routes() -> Router<Side> {
    Router::new()
        .route("/account/accept", post(accept))
        .route("/account/login", post(log_in))
        .route("/account/logout", post(log_out))
        .route("/account/me", get(me))
}

pub fn admin_api(host: Arc<Host>) -> Router {
    account_routes()
        .route("/people", get(people))
        .route("/people/remove", post(remove))
        .route("/invites", post(invite))
        .route("/server", get(server_settings))
        .route("/server/autostart", post(set_autostart))
        .with_state(Side {
            host,
            members_only: false,
        })
}

pub fn member_api(host: Arc<Host>) -> Router {
    account_routes().with_state(Side {
        host,
        members_only: true,
    })
}

fn session_of(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .find_map(|cookie| cookie.trim().strip_prefix(COOKIE_NAME)?.strip_prefix('='))
}

pub(crate) fn person_in(accounts: &Accounts, headers: &HeaderMap) -> Option<Person> {
    session_of(headers).and_then(|session| accounts.person_of(session, accounts::now()))
}

impl Side {
    /// Browsers send a cookie to every port of a host, so an admin session
    /// can arrive on the member side too, where it counts for nothing.
    fn logged_in(&self, headers: &HeaderMap) -> Result<Person, Refusal> {
        person_in(&self.host.accounts, headers)
            .filter(|person| !(self.members_only && person.admin))
            .ok_or_else(|| Refusal(StatusCode::UNAUTHORIZED, "log in first".into()))
    }

    fn admin(&self, headers: &HeaderMap) -> Result<Person, Refusal> {
        let person = self.logged_in(headers)?;
        if !person.admin {
            return Err(Refusal(
                StatusCode::FORBIDDEN,
                "only the admin can do this".into(),
            ));
        }
        Ok(person)
    }

    fn about(&self, person: Person) -> Value {
        json!({
            "person": person,
            "about": {
                "version": env!("CARGO_PKG_VERSION"),
                "repository": env!("CARGO_PKG_REPOSITORY"),
                "public_url": self.host.public_url,
            },
        })
    }

    /// The cookie is for this site's own pages only and hidden from scripts.
    fn with_session(&self, person: Person, session: Option<&str>) -> Response {
        let cookie = match session {
            Some(session) => format!(
                "{COOKIE_NAME}={session}; Max-Age={SESSION_VALID_SECONDS}; Path=/; HttpOnly; Secure; SameSite=Strict"
            ),
            None => format!("{COOKIE_NAME}=; Max-Age=0; Path=/; HttpOnly; Secure; SameSite=Strict"),
        };
        ([(SET_COOKIE, cookie)], Json(self.about(person))).into_response()
    }
}

fn autostart_state(note: Option<String>) -> Response {
    Json(json!({
        "autostart": { "on": autostart::is_on(), "moment": autostart::MOMENT, "note": note },
    }))
    .into_response()
}

async fn server_settings(State(side): State<Side>, headers: HeaderMap) -> Answer {
    side.admin(&headers)?;
    Ok(autostart_state(None))
}

#[derive(Deserialize)]
struct Switch {
    on: bool,
}

async fn set_autostart(
    State(side): State<Side>,
    headers: HeaderMap,
    Json(switch): Json<Switch>,
) -> Answer {
    side.admin(&headers)?;
    let note = match switch.on {
        true => autostart::turn_on(&side.host.launch, false)?,
        false => autostart::turn_off().map(|()| None)?,
    };
    Ok(autostart_state(note))
}

#[derive(Deserialize)]
struct Acceptance {
    invite: String,
    name: String,
    password: String,
}

async fn accept(State(side): State<Side>, Json(body): Json<Acceptance>) -> Answer {
    let accounts = side.host.accounts.clone();
    // The first account becomes the admin, so it is only made on the server itself.
    if side.members_only && accounts.people()?.is_empty() {
        return Err(Refusal(StatusCode::FORBIDDEN, ADMIN_ELSEWHERE.into()));
    }
    // Hashing a password takes a moment, so it runs off the request threads.
    let (person, session) = blocking(move || {
        accounts.accept(&body.invite, &body.name, &body.password, accounts::now())
    })
    .await?;
    Ok(side.with_session(person, Some(&session)))
}

#[derive(Deserialize)]
struct Login {
    name: String,
    password: String,
}

async fn log_in(State(side): State<Side>, Json(body): Json<Login>) -> Answer {
    let accounts = side.host.accounts.clone();
    let (person, session) =
        blocking(move || accounts.log_in(&body.name, &body.password, accounts::now())).await?;
    if side.members_only && person.admin {
        side.host.accounts.log_out(&session)?;
        return Err(Refusal(StatusCode::FORBIDDEN, ADMIN_ELSEWHERE.into()));
    }
    Ok(side.with_session(person, Some(&session)))
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> uni_core::Result<T> + Send + 'static,
) -> Result<T, Refusal> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Refusal(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(Refusal::from)
}

async fn log_out(State(side): State<Side>, headers: HeaderMap) -> Answer {
    let person = side.logged_in(&headers)?;
    if let Some(session) = session_of(&headers) {
        side.host.accounts.log_out(session)?;
    }
    Ok(side.with_session(person, None))
}

async fn me(State(side): State<Side>, headers: HeaderMap) -> Answer {
    let person = side.logged_in(&headers)?;
    Ok(Json(side.about(person)).into_response())
}

async fn people(State(side): State<Side>, headers: HeaderMap) -> Answer {
    side.admin(&headers)?;
    let people = side.host.accounts.people()?;
    Ok(Json(json!({ "people": people })).into_response())
}

#[derive(Deserialize)]
struct Removal {
    id: String,
}

async fn remove(State(side): State<Side>, headers: HeaderMap, Json(body): Json<Removal>) -> Answer {
    side.admin(&headers)?;
    side.host.accounts.remove(&body.id)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn invite(State(side): State<Side>, headers: HeaderMap) -> Answer {
    side.admin(&headers)?;
    let invite: Value = side.host.accounts.invite(accounts::now())?.into();
    Ok(Json(json!({ "invite": invite })).into_response())
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;
    use crate::accounts::testing::{PASSWORD, accounts};

    #[derive(Clone, Copy)]
    enum On {
        Admin,
        Members,
    }

    fn host(accounts: &Arc<Accounts>) -> Arc<Host> {
        Arc::new(Host {
            accounts: accounts.clone(),
            launch: crate::autostart::testing::launch(),
            public_url: Some("https://uni.example".into()),
        })
    }

    async fn call(
        accounts: &Arc<Accounts>,
        on: On,
        method: &str,
        path: &str,
        session: Option<&str>,
        body: Value,
    ) -> (StatusCode, Option<String>, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if let Some(session) = session {
            request = request.header(COOKIE, format!("other=1; {COOKIE_NAME}={session}"));
        }
        let request = request.body(Body::from(body.to_string())).expect("request");
        let api = match on {
            On::Admin => admin_api(host(accounts)),
            On::Members => member_api(host(accounts)),
        };
        let response = api.oneshot(request).await.expect("response");
        let status = response.status();
        let cookie = response
            .headers()
            .get(SET_COOKIE)
            .map(|cookie| cookie.to_str().expect("cookie").to_owned());
        let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        (
            status,
            cookie,
            serde_json::from_slice(&bytes).unwrap_or_default(),
        )
    }

    fn session_in(cookie: Option<String>) -> String {
        let cookie = cookie.expect("a session cookie");
        assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
        let value = cookie.split(';').next().expect("value");
        value
            .strip_prefix(&format!("{COOKIE_NAME}="))
            .expect("name")
            .to_owned()
    }

    /// An admin and a member, with a session each.
    fn server(name: &str) -> (Arc<Accounts>, String, Person, String, std::path::PathBuf) {
        let (accounts, folder) = accounts(name);
        let now = accounts::now();
        let first = accounts.invite(now).expect("invite");
        let (_, admin) = accounts
            .accept(&first, "Zahraa", PASSWORD, now)
            .expect("admin");
        let second = accounts.invite(now).expect("invite");
        let (member, member_session) = accounts
            .accept(&second, "Bestie", PASSWORD, now)
            .expect("member");
        (Arc::new(accounts), admin, member, member_session, folder)
    }

    #[tokio::test]
    async fn an_invite_becomes_an_account_with_a_logged_in_browser() {
        let (accounts, folder) = accounts("api-accept");
        let accounts = Arc::new(accounts);
        let invite = accounts.invite(accounts::now()).expect("invite");

        let join = json!({ "invite": invite, "name": "Zahraa", "password": PASSWORD });
        let (status, cookie, body) =
            call(&accounts, On::Admin, "POST", "/account/accept", None, join).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["person"]["admin"], true);
        assert_eq!(body["about"]["public_url"], "https://uni.example");
        let session = session_in(cookie);

        let me = |session: String| {
            let accounts = accounts.clone();
            async move {
                call(
                    &accounts,
                    On::Admin,
                    "GET",
                    "/account/me",
                    Some(&session),
                    json!({}),
                )
                .await
            }
        };
        let (status, _, body) = me(session.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["person"]["name"], "Zahraa");

        let logout = call(
            &accounts,
            On::Admin,
            "POST",
            "/account/logout",
            Some(&session),
            json!({}),
        );
        assert_eq!(logout.await.0, StatusCode::OK);
        assert_eq!(me(session).await.0, StatusCode::UNAUTHORIZED);
        drop(accounts);
        std::fs::remove_dir_all(folder).expect("clean up");
    }

    #[tokio::test]
    async fn nothing_works_without_logging_in() {
        let (accounts, folder) = accounts("api-anonymous");
        let accounts = Arc::new(accounts);
        for (method, path) in [
            ("GET", "/account/me"),
            ("GET", "/people"),
            ("POST", "/invites"),
        ] {
            let (status, _, _) = call(&accounts, On::Admin, method, path, None, json!({})).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
            let made_up = Some("made up");
            let (status, _, _) = call(&accounts, On::Admin, method, path, made_up, json!({})).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        }
        let login = json!({ "name": "Zahraa", "password": PASSWORD });
        let (status, cookie, _) =
            call(&accounts, On::Admin, "POST", "/account/login", None, login).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(cookie.is_none());
        drop(accounts);
        std::fs::remove_dir_all(folder).expect("clean up");
    }

    #[tokio::test]
    async fn from_outside_there_is_no_admin() {
        let (accounts, admin, _, member_session, folder) = server("api-outside");

        let login = json!({ "name": "Zahraa", "password": PASSWORD });
        let (status, cookie, body) = call(
            &accounts,
            On::Members,
            "POST",
            "/account/login",
            None,
            login,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], ADMIN_ELSEWHERE);
        assert!(cookie.is_none());

        let (status, _, _) = call(
            &accounts,
            On::Members,
            "GET",
            "/account/me",
            Some(&admin),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        for path in ["/people", "/invites", "/server"] {
            let (status, _, _) =
                call(&accounts, On::Members, "GET", path, Some(&admin), json!({})).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        }

        let login = json!({ "name": "Bestie", "password": PASSWORD });
        let (status, cookie, _) = call(
            &accounts,
            On::Members,
            "POST",
            "/account/login",
            None,
            login,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(cookie.is_some());
        let (status, _, _) = call(
            &accounts,
            On::Members,
            "GET",
            "/account/me",
            Some(&member_session),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        drop(accounts);
        std::fs::remove_dir_all(folder).expect("clean up");
    }

    #[tokio::test]
    async fn the_admin_account_cannot_be_made_from_outside() {
        let (accounts, folder) = accounts("api-first-outside");
        let accounts = Arc::new(accounts);
        let invite = accounts.invite(accounts::now()).expect("invite");
        let join = json!({ "invite": invite, "name": "Zahraa", "password": PASSWORD });
        let (status, _, _) = call(
            &accounts,
            On::Members,
            "POST",
            "/account/accept",
            None,
            join.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _, _) =
            call(&accounts, On::Admin, "POST", "/account/accept", None, join).await;
        assert_eq!(status, StatusCode::OK);
        drop(accounts);
        std::fs::remove_dir_all(folder).expect("clean up");
    }

    // Only reads the setting: switching it would change this computer.
    #[tokio::test]
    async fn only_the_admin_sees_the_server_settings() {
        let (accounts, admin, _, member_session, folder) = server("api-settings");

        let (status, _, body) = call(
            &accounts,
            On::Admin,
            "GET",
            "/server",
            Some(&admin),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["autostart"]["on"].is_boolean());

        let member = Some(member_session.as_str());
        let (status, _, _) = call(&accounts, On::Admin, "GET", "/server", member, json!({})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let switch = json!({ "on": true });
        let path = "/server/autostart";
        let (status, _, _) = call(&accounts, On::Admin, "POST", path, member, switch.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = call(&accounts, On::Admin, "POST", path, None, switch).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        drop(accounts);
        std::fs::remove_dir_all(folder).expect("clean up");
    }

    #[tokio::test]
    async fn only_the_admin_invites_lists_and_removes_people() {
        let (accounts, admin, member, member_session, folder) = server("api-admin");
        let admin = Some(admin.as_str());
        let guest = Some(member_session.as_str());

        let (status, _, body) =
            call(&accounts, On::Admin, "POST", "/invites", admin, json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["invite"].is_string());

        for (method, path) in [("GET", "/people"), ("POST", "/invites")] {
            let (status, _, _) = call(&accounts, On::Admin, method, path, guest, json!({})).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        }
        let removal = json!({ "id": member.id });
        let path = "/people/remove";
        let (status, _, _) = call(&accounts, On::Admin, "POST", path, guest, removal.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _, body) =
            call(&accounts, On::Admin, "GET", "/people", admin, json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["people"].as_array().map(Vec::len), Some(2));

        let (status, _, _) = call(&accounts, On::Admin, "POST", path, admin, removal).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _, _) =
            call(&accounts, On::Admin, "GET", "/account/me", guest, json!({})).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        drop(accounts);
        std::fs::remove_dir_all(folder).expect("clean up");
    }
}
