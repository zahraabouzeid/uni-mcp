use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uni_core::{Error, Result};

/// What the student chose during setup. Holds no secrets.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Id of the chosen university.
    pub profile: Option<String>,
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
