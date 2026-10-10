use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use uni_core::profile::{CalendarProfile, MoodleProfile, Profile, WebdavProfile};
use uni_core::secrets::{self, SecretStore};
use uni_core::{Calendar, Courses, Error, Files, Result};
use uni_ical::IcalClient;
use uni_moodle::MoodleClient;
use uni_webdav::WebdavClient;

#[derive(Serialize, Deserialize)]
pub struct WebdavLogin {
    pub user: String,
    pub password: String,
}

/// Decides how a service is logged in to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceKind {
    Moodle,
    Webdav,
    Calendar,
}

pub struct ServiceState {
    /// Name used for `unimcp login <id>`.
    pub id: String,
    pub name: String,
    pub kind: ServiceKind,
    pub logged_in: Result<bool>,
}

/// Connectors are built per call, so a new login takes effect immediately.
pub struct Connectors {
    profile: Profile,
    secrets: Arc<dyn SecretStore>,
    http: reqwest::Client,
}

impl Connectors {
    pub fn new(profile: Profile, secrets: Arc<dyn SecretStore>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout(Duration::from_secs(30))
            .https_only(true)
            .build()
            .expect("static HTTP client configuration is valid");
        Self {
            profile,
            secrets,
            http,
        }
    }

    pub fn secrets(&self) -> &dyn SecretStore {
        self.secrets.as_ref()
    }

    pub fn profile_id(&self) -> &str {
        &self.profile.id
    }

    pub fn secret_key(&self, service: &str) -> String {
        secrets::key(&self.profile.id, service)
    }

    pub fn moodle_profile(&self) -> Result<&MoodleProfile> {
        self.profile
            .moodle
            .as_ref()
            .ok_or_else(|| self.not_offered("a learning platform"))
    }

    pub fn webdav_profile(&self) -> Result<&WebdavProfile> {
        self.profile
            .webdav
            .as_ref()
            .ok_or_else(|| self.not_offered("a file store"))
    }

    pub fn calendar_profile(&self) -> Result<&CalendarProfile> {
        self.profile
            .calendar
            .as_ref()
            .ok_or_else(|| self.not_offered("a calendar"))
    }

    pub fn courses(&self) -> Result<Arc<dyn Courses>> {
        let moodle = self.moodle_profile()?;
        let token = self.secret(uni_moodle::SERVICE)?;
        Ok(Arc::new(MoodleClient::new(
            self.http.clone(),
            &moodle.base_url,
            token,
        )?))
    }

    pub fn files(&self) -> Result<Arc<dyn Files>> {
        let id = &self.webdav_profile()?.id;
        let login =
            serde_json::from_str(&self.secret(id)?).map_err(|_| Error::NotLoggedIn(id.clone()))?;
        self.files_as(login)
    }

    /// For checking a login before it is stored.
    pub fn files_as(&self, login: WebdavLogin) -> Result<Arc<dyn Files>> {
        let webdav = self.webdav_profile()?;
        Ok(Arc::new(WebdavClient::new(
            self.http.clone(),
            &webdav.id,
            &webdav.files_url,
            login.user,
            login.password,
        )?))
    }

    pub fn calendar(&self) -> Result<Arc<dyn Calendar>> {
        self.calendar_profile()?;
        self.calendar_as(&self.secret(uni_ical::SERVICE)?)
    }

    /// For checking a link before it is stored.
    pub fn calendar_as(&self, link: &str) -> Result<Arc<dyn Calendar>> {
        let calendar = self.calendar_profile()?;
        Ok(Arc::new(IcalClient::new(
            self.http.clone(),
            &calendar.host,
            &calendar.timezone,
            link,
        )?))
    }

    pub fn services(&self) -> Vec<ServiceState> {
        let profile = &self.profile;
        let configured = [
            profile
                .moodle
                .as_ref()
                .map(|_| (uni_moodle::SERVICE, "Moodle", ServiceKind::Moodle)),
            profile
                .webdav
                .as_ref()
                .map(|w| (w.id.as_str(), w.name.as_str(), ServiceKind::Webdav)),
            profile
                .calendar
                .as_ref()
                .map(|_| (uni_ical::SERVICE, "Calendar", ServiceKind::Calendar)),
        ];
        configured
            .into_iter()
            .flatten()
            .map(|(id, name, kind)| ServiceState {
                id: id.to_owned(),
                name: name.to_owned(),
                kind,
                logged_in: self
                    .secrets
                    .get(&self.secret_key(id))
                    .map(|secret| secret.is_some()),
            })
            .collect()
    }

    /// One line per service, without secrets.
    pub fn status(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "university: {} ({})",
            self.profile.name, self.profile.id
        )];
        for service in self.services() {
            let state = match service.logged_in {
                Ok(true) => "logged in".to_owned(),
                Ok(false) => format!("not logged in (run `unimcp login {}`)", service.id),
                Err(e) => format!("keyring unavailable: {e}"),
            };
            lines.push(format!("{}: {state}", service.id));
        }
        lines
    }

    fn secret(&self, service: &str) -> Result<String> {
        self.secrets
            .get(&self.secret_key(service))?
            .ok_or_else(|| Error::NotLoggedIn(service.to_owned()))
    }

    fn not_offered(&self, what: &str) -> Error {
        Error::Config(format!(
            "{} does not have {what} in uni-mcp",
            self.profile.name
        ))
    }
}

#[cfg(test)]
pub mod testing {
    use uni_core::secrets::MemoryStore;

    use super::*;

    pub const MOODLE_URL: &str = "https://moodle.example";

    const UNIVERSITY: &str = r#"
        id = "test"
        name = "Test University"
        [moodle]
        base_url = "https://moodle.example"
        [webdav]
        id = "cloud"
        name = "Cloud"
        files_url = "https://cloud.example/dav/{user}/"
        [calendar]
        host = "campus.example"
        timezone = "Europe/Berlin"
    "#;

    /// A made-up university with every service and an empty keyring.
    pub fn connectors() -> Connectors {
        let profile = Profile::parse(UNIVERSITY).expect("profile");
        Connectors::new(profile, Arc::new(MemoryStore::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::testing::connectors;
    use super::*;

    #[test]
    fn lists_every_service_of_the_profile() {
        let services = connectors().services();
        let kinds: Vec<_> = services.iter().map(|s| (s.id.as_str(), s.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("moodle", ServiceKind::Moodle),
                ("cloud", ServiceKind::Webdav),
                ("calendar", ServiceKind::Calendar),
            ]
        );
        assert!(services.iter().all(|s| matches!(s.logged_in, Ok(false))));
    }

    #[test]
    fn a_stored_secret_counts_as_logged_in() {
        let connectors = connectors();
        connectors
            .secrets()
            .set(&connectors.secret_key("moodle"), "t0ken")
            .expect("set");
        let moodle = &connectors.services()[0];
        assert!(matches!(moodle.logged_in, Ok(true)));
    }

    #[test]
    fn status_never_shows_a_secret() {
        let connectors = connectors();
        connectors
            .secrets()
            .set(&connectors.secret_key("moodle"), "t0ken")
            .expect("set");
        let status = connectors.status().join("\n");
        assert!(status.contains("moodle: logged in"));
        assert!(status.contains("cloud: not logged in"));
        assert!(!status.contains("t0ken"));
    }

    #[test]
    fn a_service_without_login_says_how_to_log_in() {
        let error = connectors().files().err().expect("not logged in");
        assert!(error.to_string().contains("unimcp login cloud"));
    }
}
