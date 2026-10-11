//! The dashboard and its API, on 127.0.0.1 for as long as the page is open.
//! The page itself is public; every API request needs the random key from the
//! address printed at start, which other websites and users cannot know.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use axum::extract::{Extension, Request, State};
use axum::http::{StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use include_dir::{Dir, include_dir};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use uni_core::Error;
use uni_core::profile::Profile;
use uni_core::secrets::SecretStore;

use crate::accounts::Accounts;
use crate::apps::{self, App as McpClient};
use crate::connectors::{Connectors, ServiceKind, ServiceState, WebdavLogin};
use crate::login::{self, Progress};
use crate::server::Needs;
use crate::vault::Vault;
use crate::{config, hosting, launch, scheme, server};

static PAGE: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web/out");
const KEY_HEADER: &str = "x-unimcp-key";
const CANTEENS: &str = "Canteens";
/// The page asks for its state every few seconds, but browsers slow that down
/// to once a minute in background tabs.
const PAGE_GONE_AFTER: Duration = Duration::from_secs(150);
/// A page that said it is closing may only be reloading; it gets this long to
/// ask again.
const CLOSING_GRACE: Duration = Duration::from_secs(4);

// Shared with `web/next.config.ts` and `web/dev.mjs`.
const DEV_PORT: u16 = 4517;
const DEV_PAGE: &str = "http://localhost:3000";
const DEV_KEY_VARIABLE: &str = "UNIMCP_DEV_KEY";

pub enum Mode {
    /// The embedded page, on any free port.
    Page { open_browser: bool },
    /// Only the API, for the live page of `npm run dev`.
    Dev,
}

/// What one person's dashboard works on: their settings and their logins.
#[derive(Clone)]
struct App {
    secrets: Arc<dyn SecretStore>,
    settings: Arc<config::Settings>,
    /// On a home server, where the page belongs to an account and the program
    /// is not on the computer of the person looking at it.
    hosted: bool,
    key: Arc<str>,
    started: Instant,
    /// Seconds after `started` of the latest request; zero until the first.
    last_seen: Arc<AtomicU64>,
    closing: Arc<AtomicBool>,
    checks: Arc<Mutex<Checks>>,
}

/// Whether a service still accepts the login that is stored for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    Running,
    Accepted,
    Rejected,
}

/// One check per stored login for as long as the dashboard runs. A login that
/// changes starts a new round, so an answer about the old one is dropped.
#[derive(Default)]
struct Checks {
    round: u64,
    by_secret: HashMap<String, Check>,
}

impl Checks {
    /// The state of a check, and the round to start one in if there is none yet.
    fn look_up(&mut self, secret_key: &str) -> (Check, Option<u64>) {
        match self.by_secret.get(secret_key) {
            Some(check) => (*check, None),
            None => {
                self.by_secret.insert(secret_key.to_owned(), Check::Running);
                (Check::Running, Some(self.round))
            }
        }
    }

    fn finish(&mut self, round: u64, secret_key: String, accepted: bool) {
        if round == self.round {
            let check = if accepted {
                Check::Accepted
            } else {
                Check::Rejected
            };
            self.by_secret.insert(secret_key, check);
        }
    }

    fn forget(&mut self) {
        self.round += 1;
        self.by_secret.clear();
    }
}

impl App {
    fn checks(&self) -> std::sync::MutexGuard<'_, Checks> {
        self.checks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Looks up whether a stored login is still accepted, asking the service
    /// in the background the first time.
    fn check(&self, secret_key: String, kind: ServiceKind) -> Check {
        let (check, start_in) = self.checks().look_up(&secret_key);
        if let Some(round) = start_in {
            let app = self.clone();
            tokio::spawn(async move {
                let result = match app.connectors() {
                    Ok(connectors) => connectors.verify(kind).await,
                    Err(_) => Ok(()),
                };
                // Only a refusal counts: being offline says nothing about the login.
                let accepted = !matches!(result, Err(Error::Auth { .. }));
                app.checks().finish(round, secret_key, accepted);
            });
        }
        check
    }

    fn seen_now(&self) {
        // Plus one, because zero means "never".
        self.last_seen
            .store(self.started.elapsed().as_secs() + 1, Ordering::Relaxed);
    }
}

impl App {
    fn connectors(&self) -> Result<Connectors, ApiError> {
        let id = self
            .settings
            .load()?
            .profile
            .ok_or_else(|| ApiError("choose a university first".into()))?;
        Ok(Connectors::new(
            Profile::builtin(&id)?,
            self.secrets.clone(),
        ))
    }
}

struct ApiError(String);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(json!({ "error": self.0 }))).into_response()
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

pub const DETACHABLE: bool = cfg!(windows);

/// Starts the dashboard as a process of its own, without a window, and comes
/// back at once: after a double-click nothing stays open but the browser.
#[cfg(windows)]
pub fn open_detached() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut dashboard = Command::new(std::env::current_exe().map_err(text)?)
        .arg("dashboard")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(text)?;
    // Long enough for a failing start to show.
    std::thread::sleep(Duration::from_millis(700));
    if let Ok(Some(status)) = dashboard.try_wait()
        && !status.success()
    {
        return Err("the dashboard could not start; run `unimcp dashboard` to see why".into());
    }
    println!("uniMCP is opening in your browser. It stops when you close the page.");
    Ok(())
}

#[cfg(not(windows))]
pub fn open_detached() -> Result<(), String> {
    Err("not supported on this system".into())
}

pub async fn run(secrets: Arc<dyn SecretStore>, mode: Mode) -> Result<(), String> {
    let dev = matches!(mode, Mode::Dev);
    // `npm run dev` restarts the backend on every change and passes the same
    // key each time, so the open page keeps working.
    let key = std::env::var(DEV_KEY_VARIABLE)
        .ok()
        .filter(|key| dev && !key.is_empty())
        .map_or_else(random_key, Ok)?;
    let app = App {
        secrets,
        settings: Arc::new(config::Settings::of_this_user().map_err(text)?),
        hosted: false,
        key: key.into(),
        started: Instant::now(),
        last_seen: Arc::new(AtomicU64::new(0)),
        closing: Arc::new(AtomicBool::new(false)),
        checks: Arc::default(),
    };

    let port = if dev { DEV_PORT } else { 0 };
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|e| format!("cannot listen on port {port}: {e}"))?;
    let address = listener.local_addr().map_err(text)?;

    let router = router(app.clone());

    match mode {
        Mode::Dev => {
            println!("Backend for the live page is running.");
            println!("Page: {DEV_PAGE}/?key={}", app.key);
        }
        Mode::Page { open_browser } => {
            let url = format!("http://{address}/?key={}", app.key);
            println!("Dashboard: {url}");
            if open_browser && open::that(&url).is_err() {
                println!("Could not open the browser. Open the address above yourself.");
            }
        }
    }
    println!("Press Ctrl+C to stop.");

    tokio::select! {
        served = axum::serve(listener, router) => served.map_err(text),
        () = page_closed(&app), if !dev => {
            println!("The dashboard was closed.");
            Ok(())
        }
        // Without this, Ctrl+C ends the program with an error code.
        _ = tokio::signal::ctrl_c() => {
            println!("Stopped.");
            Ok(())
        }
    }
}

/// The API of a dashboard. A layer in front of it has to say whose it is, by
/// putting their [`App`] on the request.
fn api() -> Router {
    Router::new()
        .route("/state", get(state))
        .route("/university", post(choose_university))
        .route("/custom-instructions", post(save_custom_instructions))
        .route("/login/moodle", post(start_moodle).get(moodle_progress))
        .route("/login/moodle/link", post(finish_moodle))
        .route("/login/webdav", post(login_webdav))
        .route("/login/calendar", post(login_calendar))
        .route("/logout", post(logout))
        .route("/tools/try", post(try_tool))
        .route("/apps/add", post(add_to_app))
}

/// The dashboards of the accounts on a home server.
#[derive(Clone)]
struct Hosted {
    accounts: Arc<Accounts>,
    vault: Arc<Vault>,
    checks: Arc<Mutex<HashMap<String, Arc<Mutex<Checks>>>>>,
}

impl Hosted {
    fn app_of(&self, account: &str) -> App {
        let checks = self
            .checks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(account.to_owned())
            .or_default()
            .clone();
        let settings = self.vault.folder_of(account).join(config::Settings::FILE);
        App {
            secrets: Arc::new(self.vault.secrets_of(account)),
            settings: Arc::new(config::Settings::at(settings)),
            hosted: true,
            key: "".into(),
            started: Instant::now(),
            last_seen: Arc::default(),
            closing: Arc::default(),
            checks,
        }
    }
}

/// The dashboard API for whoever is logged in to the server.
pub fn hosted_api(accounts: Arc<Accounts>, vault: Arc<Vault>) -> Router {
    let hosted = Hosted {
        accounts,
        vault,
        checks: Arc::default(),
    };
    api().layer(middleware::from_fn_with_state(hosted, require_account))
}

async fn require_account(
    State(hosted): State<Hosted>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(person) = hosting::person_in(&hosted.accounts, request.headers()) else {
        let refusal = Json(json!({ "error": "log in first" }));
        return (StatusCode::UNAUTHORIZED, refusal).into_response();
    };
    // The admin looks after the server and keeps no university logins on it.
    if person.owner {
        let refusal = Json(json!({ "error": "the admin account has no university dashboard" }));
        return (StatusCode::FORBIDDEN, refusal).into_response();
    }
    request.extensions_mut().insert(hosted.app_of(&person.id));
    next.run(request).await
}

fn router(app: App) -> Router {
    let api = api().layer(middleware::from_fn_with_state(app.clone(), require_key));
    let goodbye = Router::new()
        .route("/api/closing", post(closing))
        .with_state(app);
    Router::new()
        .nest("/api", api)
        .merge(goodbye)
        .fallback(page)
}

async fn require_key(State(app): State<App>, mut request: Request, next: Next) -> Response {
    let sent = request
        .headers()
        .get(KEY_HEADER)
        .and_then(|value| value.to_str().ok());
    if sent != Some(&*app.key) {
        return StatusCode::FORBIDDEN.into_response();
    }
    app.seen_now();
    app.closing.store(false, Ordering::Relaxed);
    request.extensions_mut().insert(app);
    next.run(request).await
}

/// The page announces that it is being closed. A browser can only send this
/// without headers, so the key comes in the address.
async fn closing(State(app): State<App>, uri: Uri) -> StatusCode {
    let key = uri
        .query()
        .and_then(|query| query.split('&').find_map(|pair| pair.strip_prefix("key=")));
    if key != Some(&*app.key) {
        return StatusCode::FORBIDDEN;
    }
    app.seen_now();
    app.closing.store(true, Ordering::Relaxed);
    StatusCode::NO_CONTENT
}

async fn page_closed(app: &App) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let last_seen = app.last_seen.load(Ordering::Relaxed);
        let now = app.started.elapsed().as_secs() + 1;
        let patience = match app.closing.load(Ordering::Relaxed) {
            true => CLOSING_GRACE,
            false => PAGE_GONE_AFTER,
        };
        if last_seen != 0 && now - last_seen > patience.as_secs() {
            return;
        }
    }
}

/// A page is exported as `<name>.html` and opened without the ending.
pub(crate) async fn page(uri: Uri) -> Response {
    let path = match uri.path().trim_matches('/') {
        "" => "index.html",
        path => path,
    };
    let as_page = format!("{path}.html");
    let path = match PAGE.get_file(path) {
        None if PAGE.get_file(&as_page).is_some() => as_page.as_str(),
        _ => path,
    };
    match PAGE.get_file(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.to_string())], file.contents()).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn state(Extension(app): Extension<App>) -> ApiResult {
    let config::Config {
        profile: selected,
        custom_instructions,
    } = app.settings.load()?;
    let (services, canteens) = match &selected {
        Some(_) => {
            let connectors = app.connectors()?;
            let services: Vec<_> = connectors
                .services()
                .into_iter()
                .map(|service| {
                    let check = match service.logged_in {
                        Ok(true) => app.check(connectors.secret_key(&service.id), service.kind),
                        _ => Check::Accepted,
                    };
                    (service, check)
                })
                .collect();
            (services, connectors.offers_canteens())
        }
        None => (Vec::new(), false),
    };
    let usable = |(service, check): &(ServiceState, Check)| {
        matches!(service.logged_in, Ok(true)) && *check != Check::Rejected
    };

    // On a server the program is not where the person's AI apps are.
    let command = launch::command().filter(|_| !app.hosted);
    let universities: Vec<Value> = Profile::builtin_all()?
        .into_iter()
        .map(|p| json!({ "id": p.id, "name": p.name, "color": p.color }))
        .collect();
    let tools: Vec<Value> = server::tools()
        .into_iter()
        .map(|tool| {
            let (service, available) = match tool.needs {
                Needs::Nothing => (None, true),
                Needs::Login(kind) => {
                    let service = services.iter().find(|(s, _)| s.kind == kind);
                    (
                        service.map(|(s, _)| s.name.as_str()),
                        service.is_some_and(usable),
                    )
                }
                Needs::Canteens => (canteens.then_some(CANTEENS), canteens),
            };
            json!({
                "name": tool.name,
                "description": tool.description,
                "params": tool.params,
                "service": service,
                "available": available,
            })
        })
        .collect();
    let services: Vec<Value> = services
        .iter()
        .map(|(s, check)| {
            json!({
                "id": s.id,
                "name": s.name,
                "kind": s.kind,
                "logged_in": matches!(s.logged_in, Ok(true)),
                "checking": *check == Check::Running,
                "expired": *check == Check::Rejected,
                "error": s.logged_in.as_ref().err().map(ToString::to_string),
            })
        })
        .collect();

    Ok(Json(json!({
        "universities": universities,
        "selected": selected,
        "services": services,
        "tools": tools,
        "custom_instructions": custom_instructions.unwrap_or_default(),
        "custom_instructions_limit": config::MAX_CUSTOM_INSTRUCTIONS_CHARS,
        "apps": command.as_deref().map(apps::states).unwrap_or_default(),
        "command": command,
        "version": env!("CARGO_PKG_VERSION"),
        "repository": env!("CARGO_PKG_REPOSITORY"),
        "browser_sign_in": scheme::SUPPORTED && !app.hosted,
        "hosted": app.hosted,
    })))
}

#[derive(Deserialize)]
struct UniversityChoice {
    id: String,
}

async fn choose_university(
    Extension(app): Extension<App>,
    Json(choice): Json<UniversityChoice>,
) -> ApiResult {
    let profile = Profile::builtin(&choice.id)?;
    app.settings.save(&config::Config {
        profile: Some(profile.id),
        ..app.settings.load()?
    })?;
    app.checks().forget();
    Ok(done())
}

#[derive(Deserialize)]
struct CustomInstructions {
    custom_instructions: String,
}

async fn save_custom_instructions(
    Extension(app): Extension<App>,
    Json(body): Json<CustomInstructions>,
) -> ApiResult {
    app.settings.save(&config::Config {
        custom_instructions: config::custom_instructions(&body.custom_instructions)?,
        ..app.settings.load()?
    })?;
    Ok(done())
}

async fn start_moodle(Extension(app): Extension<App>) -> ApiResult {
    app.checks().forget();
    if app.hosted {
        // The browser to open is the one showing the page, not one on the server.
        let url = login::begin_moodle_on_page(&app.connectors()?)?;
        return Ok(Json(json!({ "url": url, "opened": false })));
    }
    let url = login::begin_moodle(&app.connectors()?)?;
    let opened = open::that(&url).is_ok();
    Ok(Json(json!({ "url": url, "opened": opened })))
}

#[derive(Deserialize)]
struct MoodleLink {
    link: String,
}

async fn finish_moodle(Extension(app): Extension<App>, Json(body): Json<MoodleLink>) -> ApiResult {
    login::finish_moodle_on_page(&app.connectors()?, &body.link)?;
    app.checks().forget();
    Ok(done())
}

async fn moodle_progress(Extension(app): Extension<App>) -> ApiResult {
    let (state, message) = match login::moodle_progress(&app.connectors()?)? {
        Progress::Waiting => ("waiting", None),
        Progress::Done => {
            app.checks().forget();
            ("done", None)
        }
        Progress::Failed(reason) => ("failed", Some(reason)),
    };
    Ok(Json(json!({ "state": state, "message": message })))
}

async fn login_webdav(Extension(app): Extension<App>, Json(login): Json<WebdavLogin>) -> ApiResult {
    let login = WebdavLogin {
        user: login.user.trim().to_owned(),
        password: login.password.trim().to_owned(),
    };
    login::store_webdav(&app.connectors()?, login).await?;
    app.checks().forget();
    Ok(done())
}

#[derive(Deserialize)]
struct CalendarLink {
    link: String,
}

async fn login_calendar(
    Extension(app): Extension<App>,
    Json(body): Json<CalendarLink>,
) -> ApiResult {
    login::store_calendar(&app.connectors()?, &body.link).await?;
    app.checks().forget();
    Ok(done())
}

#[derive(Deserialize)]
struct ServiceChoice {
    service: String,
}

async fn logout(Extension(app): Extension<App>, Json(choice): Json<ServiceChoice>) -> ApiResult {
    login::logout(&app.connectors()?, &choice.service)?;
    app.checks().forget();
    Ok(done())
}

#[derive(Deserialize)]
struct AppChoice {
    app: McpClient,
}

async fn add_to_app(Extension(app): Extension<App>, Json(choice): Json<AppChoice>) -> ApiResult {
    if app.hosted {
        return Err(ApiError("AI apps are not on the server".into()));
    }
    let command = launch::command()
        .ok_or_else(|| ApiError("cannot tell where uniMCP is installed".into()))?;
    // Starting another program can take a moment.
    tokio::task::spawn_blocking(move || apps::add(choice.app, &command))
        .await
        .map_err(|e| ApiError(e.to_string()))??;
    Ok(done())
}

#[derive(Deserialize)]
struct ToolCall {
    name: String,
    arguments: serde_json::Map<String, Value>,
}

/// A failing tool is a result to show, not an API error.
async fn try_tool(Extension(app): Extension<App>, Json(call): Json<ToolCall>) -> ApiResult {
    let result = server::try_tool(app.connectors()?, &call.name, call.arguments.into()).await;
    Ok(Json(match result {
        Ok(output) => {
            let images: Vec<Value> = output
                .images
                .into_iter()
                .map(|image| {
                    let data = STANDARD.encode(image.data);
                    json!({ "page": image.page, "url": format!("data:{};base64,{data}", image.mime_type) })
                })
                .collect();
            json!({ "ok": true, "output": output.text, "images": images })
        }
        Err(message) => json!({ "ok": false, "output": message, "images": [] }),
    }))
}

fn done() -> Json<Value> {
    Json(json!({ "ok": true }))
}

fn random_key() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(text)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use tower::ServiceExt;
    use uni_core::secrets::MemoryStore;

    use super::*;

    const KEY: &str = "0123456789abcdef";

    fn app() -> App {
        let settings = std::env::temp_dir().join("unimcp-dashboard-test-settings.toml");
        App {
            secrets: Arc::new(MemoryStore::default()),
            settings: Arc::new(config::Settings::at(settings)),
            hosted: false,
            key: KEY.into(),
            started: Instant::now(),
            last_seen: Arc::new(AtomicU64::new(0)),
            closing: Arc::new(AtomicBool::new(false)),
            checks: Arc::default(),
        }
    }

    #[test]
    fn a_login_is_checked_once_and_a_refusal_is_remembered() {
        let mut checks = Checks::default();
        let (check, start_in) = checks.look_up("uni:moodle");
        assert_eq!((check, start_in), (Check::Running, Some(0)));
        assert_eq!(checks.look_up("uni:moodle"), (Check::Running, None));

        checks.finish(0, "uni:moodle".into(), false);
        assert_eq!(checks.look_up("uni:moodle"), (Check::Rejected, None));
    }

    #[test]
    fn an_answer_about_a_replaced_login_is_dropped() {
        let mut checks = Checks::default();
        let (_, round) = checks.look_up("uni:moodle");
        checks.forget();
        checks.finish(round.expect("round"), "uni:moodle".into(), false);

        assert_eq!(checks.look_up("uni:moodle"), (Check::Running, Some(1)));
        checks.finish(1, "uni:moodle".into(), true);
        assert_eq!(checks.look_up("uni:moodle"), (Check::Accepted, None));
    }

    async fn status_of(path: &str, key: Option<&str>) -> StatusCode {
        let mut request = Request::builder().uri(path);
        if let Some(key) = key {
            request = request.header(KEY_HEADER, key);
        }
        let request = request.body(Body::empty()).expect("request");
        router(app())
            .oneshot(request)
            .await
            .expect("response")
            .status()
    }

    #[tokio::test]
    async fn the_api_refuses_requests_without_the_key() {
        assert_eq!(status_of("/api/state", None).await, StatusCode::FORBIDDEN);
        assert_eq!(
            status_of("/api/state", Some("wrong")).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(status_of("/api/logout", None).await, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn the_key_gets_a_request_past_the_check() {
        // `logout` only takes POST, so this answer comes from behind the check.
        assert_eq!(
            status_of("/api/logout", Some(KEY)).await,
            StatusCode::METHOD_NOT_ALLOWED
        );
    }

    #[tokio::test]
    async fn a_closing_page_is_given_up_on_quickly_unless_it_comes_back() {
        let app = app();
        let announce = |key: &str| {
            let request = Request::post(format!("/api/closing?key={key}"))
                .body(Body::empty())
                .expect("request");
            router(app.clone()).oneshot(request)
        };

        let refused = announce("wrong").await.expect("response");
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
        assert!(!app.closing.load(Ordering::Relaxed));

        let accepted = announce(KEY).await.expect("response");
        assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
        assert!(app.closing.load(Ordering::Relaxed));

        // A reload asks for something again, which cancels the closing.
        let request = Request::get("/api/logout")
            .header(KEY_HEADER, KEY)
            .body(Body::empty())
            .expect("request");
        router(app.clone())
            .oneshot(request)
            .await
            .expect("response");
        assert!(!app.closing.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn each_account_has_a_dashboard_of_its_own() {
        use crate::accounts::testing::{PASSWORD, accounts};
        use crate::vault::testing::vault;

        let (accounts, folder) = accounts("dashboards");
        let (vault, vault_folder) = vault("dashboards");
        // Sessions are checked against the real time here.
        let join = |name: &str| {
            let now = crate::accounts::now();
            let invite = accounts.invite(now).expect("invite");
            accounts
                .accept(&invite, name, PASSWORD, now)
                .expect("account")
                .1
        };
        let (admin, mine, hers) = (join("Admin"), join("Zahraa"), join("Bestie"));
        let api = hosted_api(Arc::new(accounts), Arc::new(vault));

        let call = |session: Option<&str>, method: &str, path: &str, body: Value| {
            let mut request = Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json");
            if let Some(session) = session {
                request = request.header("cookie", format!("unimcp_session={session}"));
            }
            let request = request.body(Body::from(body.to_string())).expect("request");
            let api = api.clone();
            async move {
                let response = api.oneshot(request).await.expect("response");
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
                    .await
                    .expect("body");
                let body: Value = serde_json::from_slice(&bytes).unwrap_or_default();
                (status, body)
            }
        };

        let (status, _) = call(None, "GET", "/state", json!({})).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = call(Some(&admin), "GET", "/state", json!({})).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let login = json!({ "user": "ab123456", "password": "app password" });
        let (status, _) = call(Some(&admin), "POST", "/login/webdav", login).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let note = json!({ "custom_instructions": "Only for me." });
        let (status, _) = call(Some(&mine), "POST", "/custom-instructions", note).await;
        assert_eq!(status, StatusCode::OK);

        let (_, my_state) = call(Some(&mine), "GET", "/state", json!({})).await;
        let (_, her_state) = call(Some(&hers), "GET", "/state", json!({})).await;
        assert_eq!(my_state["custom_instructions"], "Only for me.");
        assert_eq!(her_state["custom_instructions"], "");
        assert_eq!(my_state["hosted"], true);
        assert!(my_state["command"].is_null());

        let add = json!({ "app": "codex" });
        let (status, _) = call(Some(&mine), "POST", "/apps/add", add).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        std::fs::remove_dir_all(folder).expect("clean up");
        std::fs::remove_dir_all(vault_folder).expect("clean up");
    }

    #[tokio::test]
    async fn the_page_is_served_without_a_key() {
        assert_eq!(status_of("/", None).await, StatusCode::OK);
        assert_eq!(status_of("/server", None).await, StatusCode::OK);
        assert_eq!(
            status_of("/no-such-file.js", None).await,
            StatusCode::NOT_FOUND
        );
    }
}
