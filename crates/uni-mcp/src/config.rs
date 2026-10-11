use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uni_core::{Error, Result};

/// What the student chose on the dashboard. Holds no secrets.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Id of the chosen university.
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
}

/// They travel with every conversation, so they have to stay short.
pub const MAX_CUSTOM_INSTRUCTIONS_CHARS: usize = 2000;

pub fn custom_instructions(text: &str) -> Result<Option<String>> {
    let text = text.trim();
    if text.chars().count() > MAX_CUSTOM_INSTRUCTIONS_CHARS {
        return Err(Error::InvalidInput(format!(
            "custom instructions can be at most {MAX_CUSTOM_INSTRUCTIONS_CHARS} characters long"
        )));
    }
    Ok((!text.is_empty()).then(|| text.to_owned()))
}

fn path() -> Result<PathBuf> {
    dirs::config_dir()
        .map(|dir| dir.join(env!("CARGO_PKG_NAME")).join("config.toml"))
        .ok_or_else(|| Error::Config("this system has no user configuration folder".into()))
}

pub fn load() -> Result<Config> {
    let path = path()?;
    match fs::read_to_string(&path) {
        Ok(source) => toml::from_str(&source)
            .map_err(|e| Error::Config(format!("invalid {}: {e}", path.display()))),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(Error::Config(format!(
            "cannot read {}: {e}",
            path.display()
        ))),
    }
}

pub fn save(config: &Config) -> Result<()> {
    let path = path()?;
    let cannot_write =
        |e: std::io::Error| Error::Config(format!("cannot write {}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(cannot_write)?;
    }
    let source = toml::to_string(config).map_err(|e| Error::Config(e.to_string()))?;
    fs::write(&path, source).map_err(cannot_write)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_instructions_are_trimmed_and_empty_ones_dropped() {
        assert_eq!(
            custom_instructions("  Answer in German.\n")
                .expect("instructions")
                .as_deref(),
            Some("Answer in German.")
        );
        assert_eq!(custom_instructions(" \n ").expect("empty"), None);
    }

    #[test]
    fn custom_instructions_have_a_length_limit_counted_in_characters() {
        assert!(custom_instructions(&"ä".repeat(MAX_CUSTOM_INSTRUCTIONS_CHARS)).is_ok());
        assert!(custom_instructions(&"ä".repeat(MAX_CUSTOM_INSTRUCTIONS_CHARS + 1)).is_err());
    }

    #[test]
    fn a_config_without_custom_instructions_still_loads_and_stays_small() {
        let config: Config = toml::from_str("profile = \"x\"\n").expect("config");
        assert_eq!(config.custom_instructions, None);
        assert_eq!(toml::to_string(&config).expect("toml"), "profile = \"x\"\n");
    }
}
