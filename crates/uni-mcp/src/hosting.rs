//! The account API of a home server: accepting an invite, logging in and out,
//! and, for the owner, inviting and removing people.

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
use crate::vault::Vault;

const COOKIE_NAME: &str = "unimcp_session";

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

pub fn api(accounts: Arc<Accounts>, vault: Arc<Vault>, launch: Launch) -> Router {
    let settings = Router::new()
        .route("/server", get(server_settings))
        .route("/server/autostart", post(set_autostart))
        .with_state((accounts.clone(), Arc::new(launch)));
    let removal = Router::new()
        .route("/people/remove", post(remove))
        .with_state((accounts.clone(), vault));
    Router::new()
        .route("/account/accept", post(accept))
        .route("/account/login", post(log_in))
        .route("/account/logout", post(log_out))
        .route("/account/me", get(me))
        .route("/people", get(people))
        .route("/invites", post(invite))
        .with_state(accounts)
        .merge(removal)
        .merge(settings)
}

type Settings = State<(Arc<Accounts>, Arc<Launch>)>;

fn autostart_state(note: Option<String>) -> Response {
    Json(json!({
        "autostart": { "on": autostart::is_on(), "moment": autostart::MOMENT, "note": note },
    }))
    .into_response()
}

async fn server_settings(State((accounts, _)): Settings, headers: HeaderMap) -> Answer {
    owner(&accounts, &headers)?;
    Ok(autostart_state(None))
}

#[derive(Deserialize)]
struct Switch {
    on: bool,
}

async fn set_autostart(
    State((accounts, launch)): Settings,
    headers: HeaderMap,
    Json(switch): Json<Switch>,
) -> Answer {
    owner(&accounts, &headers)?;
    let note = match switch.on {
        true => autostart::turn_on(&launch, false)?,
        false => autostart::turn_off().map(|()| None)?,
    };
    Ok(autostart_state(note))
}

fn session_of(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .find_map(|cookie| cookie.trim().strip_prefix(COOKIE_NAME)?.strip_prefix('='))
}

/// Whose browser a request comes from, if it is logged in.
pub(crate) fn person_in(accounts: &Accounts, headers: &HeaderMap) -> Option<Person> {
    session_of(headers).and_then(|session| accounts.person_of(session, accounts::now()))
}

fn logged_in(accounts: &Accounts, headers: &HeaderMap) -> Result<Person, Refusal> {
    person_in(accounts, headers)
        .ok_or_else(|| Refusal(StatusCode::UNAUTHORIZED, "log in first".into()))
}

fn owner(accounts: &Accounts, headers: &HeaderMap) -> Result<Person, Refusal> {
    let person = logged_in(accounts, headers)?;
    if !person.owner {
        return Err(Refusal(
            StatusCode::FORBIDDEN,
            "only the owner of the server can do this".into(),
        ));
    }
    Ok(person)
}

/// The cookie is for this site's own pages only and hidden from scripts.
fn with_session(person: Person, session: Option<&str>) -> Response {
    let cookie = match session {
        Some(session) => format!(
            "{COOKIE_NAME}={session}; Max-Age={SESSION_VALID_SECONDS}; Path=/; HttpOnly; Secure; SameSite=Strict"
        ),
        None => format!("{COOKIE_NAME}=; Max-Age=0; Path=/; HttpOnly; Secure; SameSite=Strict"),
    };
    ([(SET_COOKIE, cookie)], Json(json!({ "person": person }))).into_response()
}

#[derive(Deserialize)]
struct Acceptance {
    invite: String,
    name: String,
    password: String,
}

async fn accept(State(accounts): State<Arc<Accounts>>, Json(body): Json<Acceptance>) -> Answer {
    // Hashing a password takes a moment, so it runs off the request threads.
    let (person, session) = blocking(move || {
        accounts.accept(&body.invite, &body.name, &body.password, accounts::now())
    })
    .await?;
    Ok(with_session(person, Some(&session)))
}

#[derive(Deserialize)]
struct Login {
    name: String,
    password: String,
}

async fn log_in(State(accounts): State<Arc<Accounts>>, Json(body): Json<Login>) -> Answer {
    let (person, session) =
        blocking(move || accounts.log_in(&body.name, &body.password, accounts::now())).await?;
    Ok(with_session(person, Some(&session)))
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> uni_core::Result<T> + Send + 'static,
) -> Result<T, Refusal> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Refusal(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(Refusal::from)
}

async fn log_out(State(accounts): State<Arc<Accounts>>, headers: HeaderMap) -> Answer {
    let person = logged_in(&accounts, &headers)?;
    if let Some(session) = session_of(&headers) {
        accounts.log_out(session, accounts::now())?;
    }
    Ok(with_session(person, None))
}

async fn me(State(accounts): State<Arc<Accounts>>, headers: HeaderMap) -> Answer {
    let person = logged_in(&accounts, &headers)?;
    Ok(Json(json!({ "person": person })).into_response())
}

async fn people(State(accounts): State<Arc<Accounts>>, headers: HeaderMap) -> Answer {
    owner(&accounts, &headers)?;
    Ok(Json(json!({ "people": accounts.people() })).into_response())
}

#[derive(Deserialize)]
struct Removal {
    id: String,
}

async fn remove(
    State((accounts, vault)): State<(Arc<Accounts>, Arc<Vault>)>,
    headers: HeaderMap,
    Json(body): Json<Removal>,
) -> Answer {
    owner(&accounts, &headers)?;
    accounts.remove(&body.id, accounts::now())?;
    vault.forget(&body.id)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn invite(State(accounts): State<Arc<Accounts>>, headers: HeaderMap) -> Answer {
    owner(&accounts, &headers)?;
    let invite: Value = accounts.invite(accounts::now())?.into();
    Ok(Json(json!({ "invite": invite })).into_response())
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;
    use crate::accounts::testing::{PASSWORD, accounts};

    /// One vault for every test here; none of them stores a login.
    fn vault() -> Arc<Vault> {
        static VAULT: std::sync::OnceLock<Arc<Vault>> = std::sync::OnceLock::new();
        VAULT
            .get_or_init(|| Arc::new(crate::vault::testing::vault("api").0))
            .clone()
    }

    async fn call(
        accounts: &Arc<Accounts>,
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
        let response = api(
            accounts.clone(),
            vault(),
            crate::autostart::testing::launch(),
        )
        .oneshot(request)
        .await
        .expect("response");
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

    #[tokio::test]
    async fn an_invite_becomes_an_account_with_a_logged_in_browser() {
        let (accounts, folder) = accounts("api-accept");
        let accounts = Arc::new(accounts);
        let invite = accounts.invite(accounts::now()).expect("invite");

        let join = json!({ "invite": invite, "name": "Zahraa", "password": PASSWORD });
        let (status, cookie, body) = call(&accounts, "POST", "/account/accept", None, join).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["person"]["owner"], true);
        let session = session_in(cookie);

        let (status, _, body) =
            call(&accounts, "GET", "/account/me", Some(&session), json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["person"]["name"], "Zahraa");

        let (status, _, _) = call(
            &accounts,
            "POST",
            "/account/logout",
            Some(&session),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _, _) = call(&accounts, "GET", "/account/me", Some(&session), json!({})).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
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
            let (status, _, _) = call(&accounts, method, path, None, json!({})).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
            let (status, _, _) = call(&accounts, method, path, Some("made up"), json!({})).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        }
        let login = json!({ "name": "Zahraa", "password": PASSWORD });
        let (status, cookie, _) = call(&accounts, "POST", "/account/login", None, login).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(cookie.is_none());
        std::fs::remove_dir_all(folder).expect("clean up");
    }

    // Only reads the setting: switching it would change this computer.
    #[tokio::test]
    async fn only_the_owner_sees_the_server_settings() {
        let (accounts, folder) = accounts("api-settings");
        let accounts = Arc::new(accounts);
        let now = accounts::now();
        let first = accounts.invite(now).expect("invite");
        let (_, owner) = accounts
            .accept(&first, "Zahraa", PASSWORD, now)
            .expect("owner");
        let second = accounts.invite(now).expect("invite");
        let (_, guest) = accounts
            .accept(&second, "Bestie", PASSWORD, now)
            .expect("guest");

        let (status, _, body) = call(&accounts, "GET", "/server", Some(&owner), json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["autostart"]["on"].is_boolean());

        let (status, _, _) = call(&accounts, "GET", "/server", Some(&guest), json!({})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let switch = json!({ "on": true });
        let (status, _, _) = call(
            &accounts,
            "POST",
            "/server/autostart",
            Some(&guest),
            switch.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = call(&accounts, "POST", "/server/autostart", None, switch).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        std::fs::remove_dir_all(folder).expect("clean up");
    }

    #[tokio::test]
    async fn only_the_owner_invites_lists_and_removes_people() {
        let (accounts, folder) = accounts("api-owner");
        let accounts = Arc::new(accounts);
        let now = accounts::now();
        let first = accounts.invite(now).expect("invite");
        let (_, owner) = accounts
            .accept(&first, "Zahraa", PASSWORD, now)
            .expect("owner");

        let (status, _, body) = call(&accounts, "POST", "/invites", Some(&owner), json!({})).await;
        assert_eq!(status, StatusCode::OK);
        let invite = body["invite"].as_str().expect("invite").to_owned();
        let (guest, guest_session) = accounts
            .accept(&invite, "Bestie", PASSWORD, now)
            .expect("guest");

        for (method, path) in [("GET", "/people"), ("POST", "/invites")] {
            let (status, _, _) =
                call(&accounts, method, path, Some(&guest_session), json!({})).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        }
        let removal = json!({ "id": guest.id });
        let (status, _, _) = call(
            &accounts,
            "POST",
            "/people/remove",
            Some(&guest_session),
            removal.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _, body) = call(&accounts, "GET", "/people", Some(&owner), json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["people"].as_array().map(Vec::len), Some(2));

        let (status, _, _) = call(&accounts, "POST", "/people/remove", Some(&owner), removal).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _, _) = call(
            &accounts,
            "GET",
            "/account/me",
            Some(&guest_session),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        std::fs::remove_dir_all(folder).expect("clean up");
    }
}
