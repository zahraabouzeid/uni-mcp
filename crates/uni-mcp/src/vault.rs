//! Where a home server keeps what belongs to each account: a folder per
//! account, with its logins in an encrypted file. The key is in a file of its
//! own, so a copy of an account folder alone gives nothing away.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use uni_core::secrets::SecretStore;
use uni_core::{Error, Result};

const KEY_FILE: &str = "vault.key";
const SECRETS_FILE: &str = "secrets.bin";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

pub struct Vault {
    key: [u8; KEY_BYTES],
    folder: PathBuf,
    /// One writer at a time, across the stores handed out.
    writing: Arc<Mutex<()>>,
}

fn failed(what: &str, file: &Path, why: impl std::fmt::Display) -> Error {
    Error::Secrets(format!("cannot {what} {}: {why}", file.display()))
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes)
        .map_err(|e| Error::Secrets(format!("no random numbers from the system: {e}")))?;
    Ok(bytes)
}

fn write_private(file: &Path, content: &[u8]) -> Result<()> {
    // Written next to the file and moved over it, so a crash leaves the old one.
    let draft = file.with_extension("new");
    fs::write(&draft, content).map_err(|e| failed("write", file, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&draft, fs::Permissions::from_mode(0o600))
            .map_err(|e| failed("protect", file, e))?;
    }
    fs::rename(&draft, file).map_err(|e| failed("write", file, e))
}

impl Vault {
    /// Opens the vault in the server's data folder, with a new key on first use.
    pub fn open(data: &Path) -> Result<Self> {
        fs::create_dir_all(data).map_err(|e| failed("create", data, e))?;
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
        Ok(Self {
            key,
            folder: data.join("accounts"),
            writing: Arc::default(),
        })
    }

    pub fn folder_of(&self, account: &str) -> PathBuf {
        self.folder.join(account)
    }

    pub fn secrets_of(&self, account: &str) -> VaultStore {
        VaultStore {
            key: self.key,
            file: self.folder_of(account).join(SECRETS_FILE),
            account: account.to_owned(),
            writing: self.writing.clone(),
        }
    }

    /// Deletes everything an account had on the server.
    pub fn forget(&self, account: &str) -> Result<()> {
        let folder = self.folder_of(account);
        match fs::remove_dir_all(&folder) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(failed("remove", &folder, e)),
            _ => Ok(()),
        }
    }
}

/// The logins of one account.
pub struct VaultStore {
    key: [u8; KEY_BYTES],
    file: PathBuf,
    account: String,
    writing: Arc<Mutex<()>>,
}

impl VaultStore {
    fn cipher(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(Key::from_slice(&self.key))
    }

    /// The account is sealed in with the secrets, so a file moved into another
    /// account's folder does not open.
    fn sealed<'a>(&'a self, content: &'a [u8]) -> Payload<'a, 'a> {
        Payload {
            msg: content,
            aad: self.account.as_bytes(),
        }
    }

    fn read(&self) -> Result<BTreeMap<String, String>> {
        let stored = match fs::read(&self.file) {
            Ok(stored) => stored,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(failed("read", &self.file, e)),
        };
        let damaged = || failed("open", &self.file, "it is damaged or from another server");
        if stored.len() < NONCE_BYTES {
            return Err(damaged());
        }
        let (nonce, sealed) = stored.split_at(NONCE_BYTES);
        let plain = self
            .cipher()
            .decrypt(Nonce::from_slice(nonce), self.sealed(sealed))
            .map_err(|_| damaged())?;
        serde_json::from_slice(&plain).map_err(|_| damaged())
    }

    fn write(&self, secrets: &BTreeMap<String, String>) -> Result<()> {
        if let Some(folder) = self.file.parent() {
            fs::create_dir_all(folder).map_err(|e| failed("create", folder, e))?;
        }
        let plain = serde_json::to_vec(secrets).map_err(|e| failed("write", &self.file, e))?;
        let nonce = random::<NONCE_BYTES>()?;
        let sealed = self
            .cipher()
            .encrypt(Nonce::from_slice(&nonce), self.sealed(&plain))
            .map_err(|_| failed("seal", &self.file, "encryption failed"))?;
        write_private(&self.file, &[nonce.as_slice(), &sealed].concat())
    }

    fn change(&self, change: impl FnOnce(&mut BTreeMap<String, String>)) -> Result<()> {
        let _one_writer = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let mut secrets = self.read()?;
        change(&mut secrets);
        self.write(&secrets)
    }
}

impl SecretStore for VaultStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.read()?.remove(key))
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.change(|secrets| {
            secrets.insert(key.to_owned(), value.to_owned());
        })
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.change(|secrets| {
            secrets.remove(key);
        })
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;

    /// A vault in a folder of its own; remove the folder when done.
    pub fn vault(name: &str) -> (Vault, PathBuf) {
        let folder =
            std::env::temp_dir().join(format!("unimcp-vault-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        (Vault::open(&folder).expect("vault"), folder)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::vault;
    use super::*;

    #[test]
    fn each_account_has_its_own_secrets() {
        let (vault, folder) = vault("separate");
        let mine = vault.secrets_of("aaaa");
        let hers = vault.secrets_of("bbbb");
        mine.set("uni:moodle", "my token").expect("set");

        assert_eq!(
            mine.get("uni:moodle").expect("get").as_deref(),
            Some("my token")
        );
        assert_eq!(hers.get("uni:moodle").expect("get"), None);

        hers.set("uni:moodle", "her token").expect("set");
        mine.delete("uni:moodle").expect("delete");
        assert_eq!(mine.get("uni:moodle").expect("get"), None);
        assert_eq!(
            hers.get("uni:moodle").expect("get").as_deref(),
            Some("her token")
        );
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn nothing_readable_is_on_disk_and_it_survives_a_restart() {
        let (vault, folder) = vault("sealed");
        vault
            .secrets_of("aaaa")
            .set("uni:cloud", "app password 123")
            .expect("set");
        let file = vault.folder_of("aaaa").join(SECRETS_FILE);
        let stored = fs::read(&file).expect("file");
        let readable = String::from_utf8_lossy(&stored);
        assert!(!readable.contains("app password") && !readable.contains("uni:cloud"));
        drop(vault);

        let reopened = Vault::open(&folder).expect("vault");
        let secret = reopened.secrets_of("aaaa").get("uni:cloud").expect("get");
        assert_eq!(secret.as_deref(), Some("app password 123"));
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn a_file_moved_to_another_account_does_not_open() {
        let (vault, folder) = vault("moved");
        vault.secrets_of("aaaa").set("k", "v").expect("set");
        let hers = vault.folder_of("bbbb");
        fs::create_dir_all(&hers).expect("folder");
        fs::copy(
            vault.folder_of("aaaa").join(SECRETS_FILE),
            hers.join(SECRETS_FILE),
        )
        .expect("copy");

        assert!(vault.secrets_of("bbbb").get("k").is_err());
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn another_key_or_a_changed_file_does_not_open() {
        let (vault, folder) = vault("tampered");
        vault.secrets_of("aaaa").set("k", "v").expect("set");
        let file = vault.folder_of("aaaa").join(SECRETS_FILE);

        let mut changed = fs::read(&file).expect("file");
        let last = changed.len() - 1;
        changed[last] ^= 1;
        fs::write(&file, &changed).expect("write");
        assert!(vault.secrets_of("aaaa").get("k").is_err());

        fs::remove_file(folder.join(KEY_FILE)).expect("remove key");
        let other = Vault::open(&folder).expect("vault with a new key");
        assert!(other.secrets_of("aaaa").get("k").is_err());
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn forgetting_an_account_removes_its_folder() {
        let (vault, folder) = vault("forget");
        vault.secrets_of("aaaa").set("k", "v").expect("set");
        vault.forget("aaaa").expect("forget");
        assert!(!vault.folder_of("aaaa").exists());
        assert_eq!(vault.secrets_of("aaaa").get("k").expect("get"), None);
        vault.forget("never there").expect("forget");
        fs::remove_dir_all(folder).expect("clean up");
    }
}
