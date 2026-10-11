//! The people who may use a home server: their accounts, the invites that
//! create them and the sessions of their browsers.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uni_core::{Error, Result};

const FILE: &str = "accounts.json";
const INVITE_VALID_SECONDS: u64 = 7 * 24 * 60 * 60;
pub const SESSION_VALID_SECONDS: u64 = 30 * 24 * 60 * 60;
const MIN_PASSWORD_CHARS: usize = 10;
const MAX_PASSWORD_CHARS: usize = 200;
const MIN_NAME_CHARS: usize = 2;
const MAX_NAME_CHARS: usize = 30;
const ATTEMPTS_BEFORE_LOCK: u32 = 5;
const LOCKED_SECONDS: u64 = 15 * 60;
const WRONG_LOGIN: &str = "wrong name or password";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Person {
    pub id: String,
    pub name: String,
    /// The owner invites and removes the others.
    pub owner: bool,
}

#[derive(Serialize, Deserialize)]
struct Account {
    id: String,
    name: String,
    password: String,
    owner: bool,
}

impl Account {
    fn person(&self) -> Person {
        Person {
            id: self.id.clone(),
            name: self.name.clone(),
            owner: self.owner,
        }
    }
}

/// Invites and sessions are kept as fingerprints, so the file alone opens no
/// account.
#[derive(Serialize, Deserialize)]
struct Invite {
    fingerprint: String,
    expires: u64,
}

#[derive(Serialize, Deserialize)]
struct Session {
    fingerprint: String,
    account: String,
    expires: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    accounts: Vec<Account>,
    invites: Vec<Invite>,
    sessions: Vec<Session>,
}

pub struct Accounts {
    file: PathBuf,
    state: Mutex<State>,
    /// Wrong passwords per name: how many, and when the count started.
    failures: Mutex<HashMap<String, (u32, u64)>>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

impl Accounts {
    pub fn open(folder: &Path) -> Result<Self> {
        let file = folder.join(FILE);
        let cannot = |what: &str, e: &dyn std::fmt::Display| {
            Error::Config(format!("cannot {what} {}: {e}", file.display()))
        };
        fs::create_dir_all(folder).map_err(|e| cannot("create the folder of", &e))?;
        let state = match fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| cannot("read", &e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(cannot("read", &e)),
        };
        Ok(Self {
            file,
            state: Mutex::new(state),
            failures: Mutex::default(),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, state: &mut State, now: u64) -> Result<()> {
        state.invites.retain(|invite| invite.expires > now);
        state.sessions.retain(|session| session.expires > now);
        let cannot_write =
            |e: std::io::Error| Error::Config(format!("cannot write {}: {e}", self.file.display()));
        let text = serde_json::to_string_pretty(state).map_err(|e| Error::Config(e.to_string()))?;
        // Written next to the file and moved over it, so a crash leaves the old one.
        let draft = self.file.with_extension("json.new");
        fs::write(&draft, text).map_err(cannot_write)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&draft, fs::Permissions::from_mode(0o600)).map_err(cannot_write)?;
        }
        fs::rename(&draft, &self.file).map_err(cannot_write)
    }

    /// A new server with nobody on it and no invite on its way.
    pub fn waits_for_its_owner(&self, now: u64) -> bool {
        let state = self.state();
        state.accounts.is_empty() && !state.invites.iter().any(|invite| invite.expires > now)
    }

    /// A new invite. The code is shown once and cannot be read back.
    pub fn invite(&self, now: u64) -> Result<String> {
        let code = random_code()?;
        let mut state = self.state();
        state.invites.push(Invite {
            fingerprint: fingerprint(&code),
            expires: now + INVITE_VALID_SECONDS,
        });
        self.save(&mut state, now)?;
        Ok(code)
    }

    /// Creates the account an invite was made for and logs its browser in.
    /// The first account on a server is its owner.
    pub fn accept(
        &self,
        invite: &str,
        name: &str,
        password: &str,
        now: u64,
    ) -> Result<(Person, String)> {
        let name = name.trim();
        let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
        if !(MIN_NAME_CHARS..=MAX_NAME_CHARS).contains(&name.len()) || !name.chars().all(allowed) {
            return Err(Error::InvalidInput(format!(
                "a username has {MIN_NAME_CHARS} to {MAX_NAME_CHARS} letters, digits, dots, hyphens or underscores, and no spaces"
            )));
        }
        let length = password.chars().count();
        if !(MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&length) {
            return Err(Error::InvalidInput(format!(
                "a password has {MIN_PASSWORD_CHARS} to {MAX_PASSWORD_CHARS} characters"
            )));
        }
        let password = hash_password(password)?;

        let mut state = self.state();
        let wanted = fingerprint(invite.trim());
        let Some(position) = state
            .invites
            .iter()
            .position(|invite| invite.fingerprint == wanted && invite.expires > now)
        else {
            return Err(Error::InvalidInput(
                "this invite is not valid any more; ask for a new one".into(),
            ));
        };
        if state.accounts.iter().any(|a| same_name(&a.name, name)) {
            return Err(Error::InvalidInput(format!("the name `{name}` is taken")));
        }
        state.invites.remove(position);
        let account = Account {
            id: random_code()?[..16].to_owned(),
            name: name.to_owned(),
            password,
            owner: state.accounts.is_empty(),
        };
        let person = account.person();
        state.accounts.push(account);
        let session = new_session(&mut state, &person.id, now)?;
        self.save(&mut state, now)?;
        Ok((person, session))
    }

    pub fn log_in(&self, name: &str, password: &str, now: u64) -> Result<(Person, String)> {
        let key = name.trim().to_lowercase();
        if self.is_locked(&key, now) {
            return Err(Error::InvalidInput(
                "too many wrong passwords; try again in a few minutes".into(),
            ));
        }
        let mut state = self.state();
        let account = state.accounts.iter().find(|a| same_name(&a.name, &key));
        // An unknown name takes as long as a wrong password.
        let stored = account.map_or(unused_hash(), |account| account.password.as_str());
        let person = account.map(Account::person);
        match (password_matches(stored, password), person) {
            (true, Some(person)) => {
                self.failures().remove(&key);
                let session = new_session(&mut state, &person.id, now)?;
                self.save(&mut state, now)?;
                Ok((person, session))
            }
            _ => {
                let mut failures = self.failures();
                let entry = failures.entry(key).or_insert((0, now));
                entry.0 += 1;
                Err(Error::InvalidInput(WRONG_LOGIN.into()))
            }
        }
    }

    fn failures(&self) -> MutexGuard<'_, HashMap<String, (u32, u64)>> {
        self.failures.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_locked(&self, key: &str, now: u64) -> bool {
        let mut failures = self.failures();
        match failures.get(key) {
            Some((_, since)) if now >= since + LOCKED_SECONDS => {
                failures.remove(key);
                false
            }
            Some((count, _)) => *count >= ATTEMPTS_BEFORE_LOCK,
            None => false,
        }
    }

    pub fn person_of(&self, session: &str, now: u64) -> Option<Person> {
        let state = self.state();
        let wanted = fingerprint(session);
        let session = state
            .sessions
            .iter()
            .find(|s| s.fingerprint == wanted && s.expires > now)?;
        let account = state.accounts.iter().find(|a| a.id == session.account)?;
        Some(account.person())
    }

    pub fn log_out(&self, session: &str, now: u64) -> Result<()> {
        let mut state = self.state();
        let wanted = fingerprint(session);
        state.sessions.retain(|s| s.fingerprint != wanted);
        self.save(&mut state, now)
    }

    pub fn people(&self) -> Vec<Person> {
        self.state().accounts.iter().map(Account::person).collect()
    }

    /// Removes an account and logs it out everywhere. The owner stays.
    pub fn remove(&self, id: &str, now: u64) -> Result<()> {
        let mut state = self.state();
        match state.accounts.iter().find(|a| a.id == id) {
            None => return Err(Error::InvalidInput("no such account".into())),
            Some(account) if account.owner => {
                return Err(Error::InvalidInput(
                    "the owner of the server cannot be removed".into(),
                ));
            }
            Some(_) => {}
        }
        state.accounts.retain(|a| a.id != id);
        state.sessions.retain(|s| s.account != id);
        self.save(&mut state, now)
    }
}

/// The hash of a password nobody has, to compare against when a name is unknown.
fn unused_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        random_code()
            .and_then(|password| hash_password(&password))
            .unwrap_or_default()
    })
}

fn new_session(state: &mut State, account: &str, now: u64) -> Result<String> {
    let code = random_code()?;
    state.sessions.push(Session {
        fingerprint: fingerprint(&code),
        account: account.to_owned(),
        expires: now + SESSION_VALID_SECONDS,
    });
    Ok(code)
}

fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes)
        .map_err(|e| Error::Config(format!("no random numbers from the system: {e}")))?;
    Ok(bytes)
}

fn random_code() -> Result<String> {
    Ok(hex(&random_bytes::<32>()?))
}

fn fingerprint(code: &str) -> String {
    hex(&Sha256::digest(code.as_bytes()))
}

fn hash_password(password: &str) -> Result<String> {
    let failed = |e: argon2::password_hash::Error| Error::Config(format!("cannot hash: {e}"));
    let salt = SaltString::encode_b64(&random_bytes::<16>()?).map_err(failed)?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(failed)?
        .to_string())
}

fn password_matches(stored: &str, password: &str) -> bool {
    PasswordHash::new(stored).is_ok_and(|stored| {
        Argon2::default()
            .verify_password(password.as_bytes(), &stored)
            .is_ok()
    })
}

#[cfg(test)]
pub mod testing {
    use super::*;

    pub const PASSWORD: &str = "correct horse battery";

    /// An empty server in a folder of its own; remove the folder when done.
    pub fn accounts(name: &str) -> (Accounts, PathBuf) {
        let folder =
            std::env::temp_dir().join(format!("unimcp-accounts-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        (Accounts::open(&folder).expect("accounts"), folder)
    }

    pub fn join(accounts: &Accounts, name: &str) -> (Person, String) {
        let invite = accounts.invite(0).expect("invite");
        accounts
            .accept(&invite, name, PASSWORD, 0)
            .expect("account")
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{PASSWORD, accounts, join};
    use super::*;

    #[test]
    fn the_first_account_owns_the_server_and_later_ones_do_not() {
        let (accounts, folder) = accounts("owner");
        assert!(accounts.waits_for_its_owner(0));
        let (first, _) = join(&accounts, "Zahraa");
        let (second, _) = join(&accounts, "Bestie");
        assert!(first.owner && !second.owner);
        assert_eq!(accounts.people(), [first, second]);
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn a_new_server_waits_for_its_owner_until_an_invite_is_out() {
        let (accounts, folder) = accounts("waits");
        assert!(accounts.waits_for_its_owner(0));
        let invite = accounts.invite(0).expect("invite");
        assert!(!accounts.waits_for_its_owner(0));
        assert!(accounts.waits_for_its_owner(INVITE_VALID_SECONDS));

        accounts
            .accept(&invite, "Zahraa", PASSWORD, 0)
            .expect("owner");
        assert!(!accounts.waits_for_its_owner(INVITE_VALID_SECONDS));
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn an_invite_works_once_and_only_for_a_week() {
        let (accounts, folder) = accounts("invite");
        let invite = accounts.invite(0).expect("invite");
        assert!(accounts.accept(&invite, "Zahraa", PASSWORD, 0).is_ok());
        assert!(accounts.accept(&invite, "Again", PASSWORD, 0).is_err());

        let old = accounts.invite(0).expect("invite");
        let late = INVITE_VALID_SECONDS + 1;
        assert!(accounts.accept(&old, "Late", PASSWORD, late).is_err());
        assert!(accounts.accept("made up", "Guess", PASSWORD, 0).is_err());
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn a_refused_account_does_not_use_up_the_invite() {
        let (accounts, folder) = accounts("refused");
        join(&accounts, "Zahraa");
        let invite = accounts.invite(0).expect("invite");
        assert!(accounts.accept(&invite, "zahraa", PASSWORD, 0).is_err());
        assert!(accounts.accept(&invite, "Bestie", "short", 0).is_err());
        for name in ["  ", "z", "zahraa bou zeid", "zähraa", "a@b"] {
            assert!(
                accounts.accept(&invite, name, PASSWORD, 0).is_err(),
                "{name}"
            );
        }
        let long = "a".repeat(MAX_NAME_CHARS + 1);
        assert!(accounts.accept(&invite, &long, PASSWORD, 0).is_err());
        assert!(
            accounts
                .accept(&invite, "best.friend_2-go", PASSWORD, 0)
                .is_ok()
        );
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn logging_in_needs_the_right_password_and_says_no_more_than_that() {
        let (accounts, folder) = accounts("login");
        join(&accounts, "Zahraa");
        let wrong = accounts.log_in("Zahraa", "not the password", 0);
        let unknown = accounts.log_in("Nobody", PASSWORD, 0);
        assert_eq!(
            wrong.expect_err("wrong").to_string(),
            unknown.expect_err("unknown").to_string()
        );

        let (person, session) = accounts.log_in(" zahraa ", PASSWORD, 0).expect("login");
        assert_eq!(accounts.person_of(&session, 0), Some(person));
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn repeated_wrong_passwords_lock_the_name_for_a_while() {
        let (accounts, folder) = accounts("lock");
        join(&accounts, "Zahraa");
        for _ in 0..ATTEMPTS_BEFORE_LOCK {
            assert!(accounts.log_in("Zahraa", "guess guess", 0).is_err());
        }
        let locked = accounts.log_in("Zahraa", PASSWORD, 0).expect_err("locked");
        assert!(locked.to_string().contains("too many"));
        assert!(accounts.log_in("Zahraa", PASSWORD, LOCKED_SECONDS).is_ok());
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn a_session_ends_by_logging_out_or_after_a_month() {
        let (accounts, folder) = accounts("session");
        let (_, session) = join(&accounts, "Zahraa");
        assert!(
            accounts
                .person_of(&session, SESSION_VALID_SECONDS - 1)
                .is_some()
        );
        assert!(
            accounts
                .person_of(&session, SESSION_VALID_SECONDS)
                .is_none()
        );
        assert!(accounts.person_of("made up", 0).is_none());

        accounts.log_out(&session, 0).expect("log out");
        assert!(accounts.person_of(&session, 0).is_none());
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn removing_someone_logs_them_out_but_the_owner_stays() {
        let (accounts, folder) = accounts("remove");
        let (owner, _) = join(&accounts, "Zahraa");
        let (other, session) = join(&accounts, "Bestie");
        assert!(accounts.remove(&owner.id, 0).is_err());

        accounts.remove(&other.id, 0).expect("remove");
        assert!(accounts.person_of(&session, 0).is_none());
        assert!(accounts.log_in("Bestie", PASSWORD, 0).is_err());
        assert_eq!(accounts.people(), [owner]);
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn accounts_survive_a_restart_and_the_file_holds_no_password_or_code() {
        let (accounts, folder) = accounts("restart");
        let (person, session) = join(&accounts, "Zahraa");
        let invite = accounts.invite(0).expect("invite");
        drop(accounts);

        let stored = fs::read_to_string(folder.join(FILE)).expect("file");
        assert!(!stored.contains(PASSWORD));
        assert!(!stored.contains(&session) && !stored.contains(&invite));

        let reopened = Accounts::open(&folder).expect("accounts");
        assert_eq!(reopened.person_of(&session, 0), Some(person));
        assert!(reopened.accept(&invite, "Bestie", PASSWORD, 0).is_ok());
        fs::remove_dir_all(folder).expect("clean up");
    }
}
