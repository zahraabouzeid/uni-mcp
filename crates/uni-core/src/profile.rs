use include_dir::{Dir, include_dir};
use serde::Deserialize;

use crate::{Error, Result};

/// Every `profiles/*.toml` is built in; adding a university is adding a file.
static PROFILES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../profiles");

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// `#rrggbb`, dark enough to sit behind white text.
    pub color: Option<String>,
    /// IANA name, e.g. `Europe/Berlin`.
    pub timezone: String,
    pub moodle: Option<MoodleProfile>,
    pub webdav: Option<WebdavProfile>,
    pub calendar: Option<CalendarProfile>,
    pub canteens: Option<CanteensProfile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoodleProfile {
    pub base_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebdavProfile {
    /// Name used for `unimcp login <id>`.
    pub id: String,
    pub name: String,
    /// `{user}` is replaced by the username.
    pub files_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarProfile {
    /// The only host a student's iCal link may point to.
    pub host: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanteensProfile {
    /// Base address of an OpenMensa API.
    pub api: String,
    pub list: Vec<CanteenEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanteenEntry {
    /// The canteen's number in the OpenMensa API.
    pub id: u64,
    pub name: String,
}

impl Profile {
    pub fn parse(source: &str) -> Result<Self> {
        let profile: Profile =
            toml::from_str(source).map_err(|e| Error::Config(format!("invalid profile: {e}")))?;
        if let Some(color) = &profile.color {
            let hex = color.strip_prefix('#').unwrap_or_default();
            if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(Error::Config(format!(
                    "profile colour must look like #rrggbb: {color}"
                )));
            }
        }
        let urls = [
            profile.moodle.as_ref().map(|m| &m.base_url),
            profile.webdav.as_ref().map(|w| &w.files_url),
            profile.canteens.as_ref().map(|c| &c.api),
        ];
        // Credentials are sent to most of these URLs.
        if let Some(url) = urls
            .into_iter()
            .flatten()
            .find(|u| !u.starts_with("https://"))
        {
            return Err(Error::Config(format!("profile URL must use https: {url}")));
        }
        Ok(profile)
    }

    pub fn builtin_all() -> Result<Vec<Self>> {
        let mut profiles = PROFILES
            .files()
            .filter(|file| file.path().extension().is_some_and(|ext| ext == "toml"))
            .map(|file| {
                let source = file.contents_utf8().ok_or_else(|| {
                    Error::Config(format!("{} is not UTF-8", file.path().display()))
                })?;
                Self::parse(source)
            })
            .collect::<Result<Vec<_>>>()?;
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(profiles)
    }

    pub fn builtin(id: &str) -> Result<Self> {
        Self::builtin_all()?
            .into_iter()
            .find(|profile| profile.id == id)
            .ok_or_else(|| Error::Config(format!("unknown university `{id}`")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "id = \"x\"\nname = \"X\"\ntimezone = \"Europe/Berlin\"\n";

    #[test]
    fn every_builtin_profile_parses_and_ids_are_unique() {
        let profiles = Profile::builtin_all().expect("profiles");
        assert!(!profiles.is_empty());
        for profile in &profiles {
            assert_eq!(profiles.iter().filter(|p| p.id == profile.id).count(), 1);
            assert_eq!(
                Profile::builtin(&profile.id).expect("by id").name,
                profile.name
            );
        }
    }

    #[test]
    fn unknown_university_is_an_error() {
        assert!(Profile::builtin("nowhere").is_err());
    }

    #[test]
    fn colour_must_be_hex() {
        assert!(Profile::parse(&format!("{MINIMAL}color = \"#00549f\"\n")).is_ok());
        assert!(Profile::parse(&format!("{MINIMAL}color = \"blue\"\n")).is_err());
    }

    #[test]
    fn plain_http_is_rejected() {
        let source = format!("{MINIMAL}[moodle]\nbase_url = \"http://moodle.example\"\n");
        assert!(Profile::parse(&source).is_err());
    }
}
