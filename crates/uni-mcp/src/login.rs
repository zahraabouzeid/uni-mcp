//! Logging in happens in the terminal or on the setup page, never through the
//! chat, so credentials do not pass through the model.

use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use uni_core::{Error, Result};

use crate::connectors::{Connectors, ServiceKind, WebdavLogin};
use crate::scheme;

/// Keyring entry for a Moodle sign-in that waits for the browser: its
/// passport, or `FAILED` and the reason once the callback rejected it.
const PENDING: &str = "moodle-login";
const FAILED: &str = "failed: ";
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

pub enum Progress {
    Waiting,
    Done,
    Failed(String),
}

pub async fn login(connectors: &Connectors, service: &str) -> Result<()> {
    match kind_of(connectors, service)? {
        ServiceKind::Moodle => moodle(connectors)?,
        ServiceKind::Webdav => store_webdav(connectors, prompt_webdav(service)?).await?,
        ServiceKind::Calendar => {
            println!("Paste the iCal link of your university calendar.\n");
            let link = rpassword::prompt_password("Link (input hidden): ").map_err(io_error)?;
            store_calendar(connectors, &link).await?;
        }
    }
    println!("Logged in to {service}. The login is stored in your system keyring.");
    Ok(())
}

pub fn logout(connectors: &Connectors, service: &str) -> Result<()> {
    kind_of(connectors, service)?;
    connectors.secrets().delete(&connectors.secret_key(service))
}

/// Starts a Moodle sign-in and returns the page the browser has to open. The
/// browser then runs [`callback`]; [`moodle_progress`] tells how it went.
pub fn begin_moodle(connectors: &Connectors) -> Result<String> {
    let (passport, url) = new_attempt(connectors)?;
    scheme::register(connectors.profile_id()).map_err(|e| {
        Error::Config(format!(
            "cannot register {}:// links on this system: {e}",
            scheme::SCHEME
        ))
    })?;
    connectors
        .secrets()
        .set(&connectors.secret_key(PENDING), &passport)?;
    Ok(url)
}

pub fn moodle_progress(connectors: &Connectors) -> Result<Progress> {
    let secrets = connectors.secrets();
    let pending = connectors.secret_key(PENDING);
    let Some(state) = secrets.get(&pending)? else {
        return Ok(Progress::Done);
    };
    match state.strip_prefix(FAILED) {
        Some(reason) => {
            secrets.delete(&pending)?;
            Ok(Progress::Failed(reason.to_owned()))
        }
        None => Ok(Progress::Waiting),
    }
}

/// Handles the `unimcp://` link the browser opens after the sign-in.
pub fn callback(connectors: &Connectors, redirect: &str) -> Result<()> {
    let secrets = connectors.secrets();
    let pending = connectors.secret_key(PENDING);
    let passport = secrets
        .get(&pending)?
        .filter(|state| !state.starts_with(FAILED))
        .ok_or_else(|| {
            Error::InvalidInput(
                "no Moodle login is in progress; run `unimcp login moodle` first".into(),
            )
        })?;
    let base_url = &connectors.moodle_profile()?.base_url;

    match uni_moodle::login::parse_redirect(redirect, scheme::SCHEME, base_url, &passport) {
        Ok(token) => {
            secrets.set(&connectors.secret_key(uni_moodle::SERVICE), &token)?;
            secrets.delete(&pending)
        }
        Err(e) => {
            // The message never contains the token, so it is safe to store.
            secrets.set(&pending, &format!("{FAILED}{e}"))?;
            Err(e)
        }
    }
}

pub async fn store_calendar(connectors: &Connectors, link: &str) -> Result<()> {
    let link = link.trim();
    connectors
        .calendar_as(link)?
        .events(None, None, None)
        .await
        .map_err(|e| match e {
            Error::Auth { .. } => {
                Error::InvalidInput("the university did not accept this calendar link".into())
            }
            other => other,
        })?;
    connectors
        .secrets()
        .set(&connectors.secret_key(uni_ical::SERVICE), link)
}

pub async fn store_webdav(connectors: &Connectors, login: WebdavLogin) -> Result<()> {
    let service = connectors.webdav_profile()?.id.clone();
    if login.user.is_empty() || login.password.is_empty() {
        return Err(Error::InvalidInput(
            "username and app password are both required".into(),
        ));
    }
    let secret = serde_json::to_string(&login).map_err(|e| Error::Secrets(e.to_string()))?;
    connectors
        .files_as(login)?
        .list("")
        .await
        .map_err(|e| match e {
            Error::Auth { service } => Error::InvalidInput(format!(
                "{service} did not accept this username and app password"
            )),
            other => other,
        })?;
    connectors
        .secrets()
        .set(&connectors.secret_key(&service), &secret)
}

fn kind_of(connectors: &Connectors, service: &str) -> Result<ServiceKind> {
    connectors
        .services()
        .into_iter()
        .find(|s| s.id == service)
        .map(|s| s.kind)
        .ok_or_else(|| {
            Error::InvalidInput(format!("unknown service `{service}`; see `unimcp status`"))
        })
}

fn moodle(connectors: &Connectors) -> Result<()> {
    if !scheme::SUPPORTED {
        return moodle_by_paste(connectors);
    }
    let url = begin_moodle(connectors)?;
    if open::that(&url).is_ok() {
        println!("Your browser is opening the Moodle sign-in page.");
    } else {
        println!("Open this page in your browser:\n\n   {url}\n");
    }
    println!("Sign in as usual. When the browser asks whether it may open unimcp, allow it.");
    println!("Waiting for the sign-in to finish (Ctrl+C to cancel)...");

    let deadline = Instant::now() + SIGN_IN_TIMEOUT;
    while Instant::now() < deadline {
        thread::sleep(Duration::from_secs(1));
        match moodle_progress(connectors)? {
            Progress::Waiting => {}
            Progress::Done => return Ok(()),
            Progress::Failed(reason) => return Err(Error::InvalidInput(reason)),
        }
    }
    connectors
        .secrets()
        .delete(&connectors.secret_key(PENDING))?;
    Err(Error::InvalidInput(
        "the sign-in was not finished in time; run `unimcp login moodle` again".into(),
    ))
}

fn new_attempt(connectors: &Connectors) -> Result<(String, String)> {
    let base_url = &connectors.moodle_profile()?.base_url;
    // The passport ties the redirect to this attempt; it is not a secret.
    let passport = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
        .to_string();
    let url = uni_moodle::login::launch_url(base_url, &passport, scheme::SCHEME);
    Ok((passport, url))
}

/// Where `unimcp://` links cannot be registered, the student copies the link.
fn moodle_by_paste(connectors: &Connectors) -> Result<()> {
    let (passport, url) = new_attempt(connectors)?;
    println!("1. Open this page and sign in as usual:\n");
    println!("   {url}\n");
    println!(
        "2. Moodle then tries to open a link starting with {}://token=",
        scheme::SCHEME
    );
    println!("   Copy that whole link and paste it here.\n");
    let redirect = rpassword::prompt_password("Link (input hidden): ").map_err(io_error)?;
    let base_url = &connectors.moodle_profile()?.base_url;
    let token = uni_moodle::login::parse_redirect(&redirect, scheme::SCHEME, base_url, &passport)?;
    connectors
        .secrets()
        .set(&connectors.secret_key(uni_moodle::SERVICE), &token)
}

fn prompt_webdav(service: &str) -> Result<WebdavLogin> {
    println!("Use an app password from the security settings of {service},");
    println!("not your university password.\n");
    print!("Username: ");
    io::stdout().flush().map_err(io_error)?;
    let mut user = String::new();
    io::stdin().read_line(&mut user).map_err(io_error)?;
    let password = rpassword::prompt_password("App password (input hidden): ").map_err(io_error)?;
    Ok(WebdavLogin {
        user: user.trim().to_owned(),
        password: password.trim().to_owned(),
    })
}

fn io_error(e: io::Error) -> Error {
    Error::InvalidInput(format!("could not read input: {e}"))
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    use super::*;
    use crate::connectors::testing::{MOODLE_URL, connectors};

    const PASSPORT: &str = "p1";

    fn redirect(passport: &str, token: &str) -> String {
        let signature = format!("{:x}", md5::compute(format!("{MOODLE_URL}{passport}")));
        let payload = STANDARD.encode(format!("{signature}:::{token}"));
        format!("{}://token={payload}", scheme::SCHEME)
    }

    fn waiting_for_sign_in() -> Connectors {
        let connectors = connectors();
        connectors
            .secrets()
            .set(&connectors.secret_key(PENDING), PASSPORT)
            .expect("set");
        connectors
    }

    fn stored(connectors: &Connectors, service: &str) -> Option<String> {
        connectors
            .secrets()
            .get(&connectors.secret_key(service))
            .expect("get")
    }

    #[test]
    fn the_callback_stores_the_token_and_finishes_the_sign_in() {
        let connectors = waiting_for_sign_in();
        assert!(matches!(
            moodle_progress(&connectors),
            Ok(Progress::Waiting)
        ));

        callback(&connectors, &redirect(PASSPORT, "abc123")).expect("callback");

        assert_eq!(
            stored(&connectors, uni_moodle::SERVICE).as_deref(),
            Some("abc123")
        );
        assert!(matches!(moodle_progress(&connectors), Ok(Progress::Done)));
    }

    #[test]
    fn a_redirect_from_another_attempt_fails_the_sign_in() {
        let connectors = waiting_for_sign_in();

        assert!(callback(&connectors, &redirect("other", "abc123")).is_err());

        assert_eq!(stored(&connectors, uni_moodle::SERVICE), None);
        assert!(matches!(
            moodle_progress(&connectors),
            Ok(Progress::Failed(_))
        ));
        // The failure is reported once, then the attempt is over.
        assert!(matches!(moodle_progress(&connectors), Ok(Progress::Done)));
    }

    #[test]
    fn a_callback_without_a_sign_in_is_rejected() {
        let connectors = connectors();
        assert!(callback(&connectors, &redirect(PASSPORT, "abc123")).is_err());
        assert_eq!(stored(&connectors, uni_moodle::SERVICE), None);
    }

    #[test]
    fn logout_forgets_the_login_of_a_known_service_only() {
        let connectors = connectors();
        connectors
            .secrets()
            .set(&connectors.secret_key("cloud"), "secret")
            .expect("set");

        assert!(logout(&connectors, "nowhere").is_err());
        assert!(stored(&connectors, "cloud").is_some());

        logout(&connectors, "cloud").expect("logout");
        assert_eq!(stored(&connectors, "cloud"), None);
    }

    #[tokio::test]
    async fn an_incomplete_cloud_login_is_not_stored() {
        let connectors = connectors();
        let login = WebdavLogin {
            user: "ab123456".into(),
            password: String::new(),
        };
        assert!(store_webdav(&connectors, login).await.is_err());
        assert_eq!(stored(&connectors, "cloud"), None);
    }

    #[tokio::test]
    async fn a_calendar_link_to_another_host_is_not_stored() {
        let connectors = connectors();
        let link = "https://evil.example/ical?pToken=abc";
        assert!(store_calendar(&connectors, link).await.is_err());
        assert_eq!(stored(&connectors, uni_ical::SERVICE), None);
    }
}
