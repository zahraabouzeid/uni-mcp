//! Tokens and app passwords live in the OS keyring and nowhere else.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::{Error, Result};

const SERVICE: &str = "uni-mcp";

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

pub fn key(profile_id: &str, service: &str) -> String {
    format!("{profile_id}:{service}")
}

#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
    fn entry(key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, key).map_err(to_error)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(to_error(e)),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        Self::entry(key)?.set_password(value).map_err(to_error)
    }

    fn delete(&self, key: &str) -> Result<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(to_error(e)),
        }
    }
}

fn to_error(e: keyring::Error) -> Error {
    match e {
        keyring::Error::NoDefaultStore => Error::Secrets(
            "no system keyring available (on Linux a Secret Service such as GNOME Keyring or KWallet must be running)".into(),
        ),
        other => Error::Secrets(other.to_string()),
    }
}

/// For tests.
#[derive(Debug, Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.lock()?.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.lock()?.insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.lock()?.remove(key);
        Ok(())
    }
}

impl MemoryStore {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, String>>> {
        self.0
            .lock()
            .map_err(|_| Error::Secrets("memory store poisoned".into()))
    }
}
