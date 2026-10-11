//! The logins of the accounts on a home server. Each one is encrypted on its
//! own before it goes into the database, and the key is in a file next to the
//! database, so a copy of the database alone gives nothing away.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rusqlite::{OptionalExtension, params};
use uni_core::secrets::SecretStore;
use uni_core::{Error, Result};

use crate::store::Database;

const KEY_FILE: &str = "vault.key";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

pub struct Vault {
    key: [u8; KEY_BYTES],
    database: Arc<Database>,
}

fn failed(what: &str, file: &Path, why: impl std::fmt::Display) -> Error {
    Error::Secrets(format!("cannot {what} {}: {why}", file.display()))
}

fn sql(e: rusqlite::Error) -> Error {
    Error::Secrets(format!("database error: {e}"))
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes)
        .map_err(|e| Error::Secrets(format!("no random numbers from the system: {e}")))?;
    Ok(bytes)
}

fn write_private(file: &Path, content: &[u8]) -> Result<()> {
    fs::write(file, content).map_err(|e| failed("write", file, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o600))
            .map_err(|e| failed("protect", file, e))?;
    }
    Ok(())
}

impl Vault {
    /// Opens the vault of the server in `data`, with a new key on first use.
    pub fn open(data: &Path, database: Arc<Database>) -> Result<Self> {
        let file = data.join(KEY_FILE);
        let key = match fs::read(&file) {
            Ok(stored) => stored
                .try_into()
                .map_err(|_| failed("use", &file, "it is not a vault key"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let key = random::<KEY_BYTES>()?;
                write_private(&file, &key)?;
                key
            }
            Err(e) => return Err(failed("read", &file, e)),
        };
        Ok(Self { key, database })
    }

    pub fn secrets_of(&self, account: &str) -> VaultStore {
        VaultStore {
            key: self.key,
            database: self.database.clone(),
            account: account.to_owned(),
        }
    }
}

/// The logins of one account.
pub struct VaultStore {
    key: [u8; KEY_BYTES],
    database: Arc<Database>,
    account: String,
}

impl VaultStore {
    fn cipher(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(Key::from_slice(&self.key))
    }

    /// What a secret belongs to is sealed in with it, so a row copied to
    /// another account or another name does not open.
    fn owner_of(&self, key: &str) -> Vec<u8> {
        [self.account.as_bytes(), b"\0", key.as_bytes()].concat()
    }
}

impl SecretStore for VaultStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        let stored: Option<(Vec<u8>, Vec<u8>)> = self.database.with(|connection| {
            connection
                .query_row(
                    "SELECT nonce, sealed FROM secrets WHERE account = ?1 AND key = ?2",
                    params![self.account, key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(sql)
        })?;
        let Some((nonce, sealed)) = stored else {
            return Ok(None);
        };
        let damaged = || Error::Secrets("a stored login is damaged or from another server".into());
        if nonce.len() != NONCE_BYTES {
            return Err(damaged());
        }
        let sealed = Payload {
            msg: &sealed,
            aad: &self.owner_of(key),
        };
        let plain = self
            .cipher()
            .decrypt(Nonce::from_slice(&nonce), sealed)
            .map_err(|_| damaged())?;
        String::from_utf8(plain).map(Some).map_err(|_| damaged())
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        let nonce = random::<NONCE_BYTES>()?;
        let plain = Payload {
            msg: value.as_bytes(),
            aad: &self.owner_of(key),
        };
        let sealed = self
            .cipher()
            .encrypt(Nonce::from_slice(&nonce), plain)
            .map_err(|_| Error::Secrets("cannot encrypt a login".into()))?;
        self.database.with(|connection| {
            connection
                .execute(
                    "INSERT INTO secrets (account, key, nonce, sealed) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (account, key)
                     DO UPDATE SET nonce = excluded.nonce, sealed = excluded.sealed",
                    params![self.account, key, nonce.as_slice(), sealed],
                )
                .map_err(sql)?;
            Ok(())
        })
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.database.with(|connection| {
            connection
                .execute(
                    "DELETE FROM secrets WHERE account = ?1 AND key = ?2",
                    params![self.account, key],
                )
                .map_err(sql)?;
            Ok(())
        })
    }
}

#[cfg(test)]
pub mod testing {
    use std::path::PathBuf;

    use super::*;
    use crate::accounts::Accounts;
    use crate::accounts::testing::join;

    /// A server with two accounts, as a vault and their ids; remove the folder
    /// when done.
    pub fn vault(name: &str) -> (Vault, Arc<Database>, [String; 2], PathBuf) {
        let (database, folder) = crate::store::testing::database(name);
        let accounts = Accounts::new(database.clone());
        let ids = ["Zahraa", "Bestie"].map(|name| join(&accounts, name).0.id);
        let vault = Vault::open(&folder, database.clone()).expect("vault");
        (vault, database, ids, folder)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::vault;
    use super::*;
    use crate::accounts::Accounts;

    #[test]
    fn each_account_has_its_own_secrets() {
        let (vault, _, [mine, hers], folder) = vault("separate");
        let (mine, hers) = (vault.secrets_of(&mine), vault.secrets_of(&hers));
        mine.set("uni:moodle", "my token").expect("set");
        assert_eq!(
            mine.get("uni:moodle").expect("get").as_deref(),
            Some("my token")
        );
        assert_eq!(hers.get("uni:moodle").expect("get"), None);

        hers.set("uni:moodle", "her token").expect("set");
        mine.set("uni:moodle", "my new token").expect("replace");
        assert_eq!(
            mine.get("uni:moodle").expect("get").as_deref(),
            Some("my new token")
        );
        mine.delete("uni:moodle").expect("delete");
        assert_eq!(mine.get("uni:moodle").expect("get"), None);
        assert_eq!(
            hers.get("uni:moodle").expect("get").as_deref(),
            Some("her token")
        );
        drop((vault, mine, hers));
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn nothing_readable_is_on_disk_and_it_survives_a_restart() {
        let (vault, database, [mine, _], folder) = vault("sealed");
        vault
            .secrets_of(&mine)
            .set("uni:cloud", "app password 123")
            .expect("set");
        drop((vault, database));

        let mut stored = Vec::new();
        for file in fs::read_dir(&folder).expect("folder") {
            stored.extend(fs::read(file.expect("file").path()).expect("content"));
        }
        assert!(!String::from_utf8_lossy(&stored).contains("app password"));

        let database = Database::open(&folder).expect("database");
        let reopened = Vault::open(&folder, database).expect("vault");
        let secret = reopened.secrets_of(&mine).get("uni:cloud").expect("get");
        assert_eq!(secret.as_deref(), Some("app password 123"));
        drop(reopened);
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn a_row_given_to_another_account_or_name_does_not_open() {
        let (vault, database, [mine, hers], folder) = vault("moved");
        vault.secrets_of(&mine).set("k", "v").expect("set");
        database
            .with(|connection| {
                connection
                    .execute(
                        "INSERT INTO secrets SELECT ?1, 'k', nonce, sealed FROM secrets",
                        [&hers],
                    )
                    .map_err(sql)?;
                connection
                    .execute(
                        "INSERT INTO secrets SELECT account, 'other', nonce, sealed FROM secrets
                         WHERE account = ?1",
                        [&mine],
                    )
                    .map_err(sql)?;
                Ok(())
            })
            .expect("copy");

        assert!(vault.secrets_of(&hers).get("k").is_err());
        assert!(vault.secrets_of(&mine).get("other").is_err());
        assert_eq!(
            vault.secrets_of(&mine).get("k").expect("get").as_deref(),
            Some("v")
        );
        drop((vault, database));
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn another_key_or_a_changed_row_does_not_open() {
        let (vault, database, [mine, _], folder) = vault("tampered");
        vault.secrets_of(&mine).set("k", "v").expect("set");
        vault.secrets_of(&mine).set("untouched", "v").expect("set");
        database
            .with(|connection| {
                connection
                    .execute(
                        "UPDATE secrets SET nonce = zeroblob(12) WHERE key = 'k'",
                        [],
                    )
                    .map_err(sql)?;
                Ok(())
            })
            .expect("tamper");
        assert!(vault.secrets_of(&mine).get("k").is_err());
        drop(vault);

        fs::remove_file(folder.join(KEY_FILE)).expect("remove key");
        let other = Vault::open(&folder, database.clone()).expect("vault with a new key");
        assert!(other.secrets_of(&mine).get("untouched").is_err());
        drop((other, database));
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn removing_an_account_removes_its_logins() {
        let (vault, database, [_, hers], folder) = vault("forget");
        vault.secrets_of(&hers).set("k", "v").expect("set");
        Accounts::new(database.clone())
            .remove(&hers)
            .expect("remove");

        assert_eq!(vault.secrets_of(&hers).get("k").expect("get"), None);
        assert!(vault.secrets_of(&hers).set("k", "v").is_err());
        drop((vault, database));
        fs::remove_dir_all(folder).expect("clean up");
    }
}
