//! The people who may use a home server: their accounts, the invites that
//! create them and the sessions of their browsers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uni_core::{Error, Result};

use crate::store::{Database, sql};

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

/// Invites and sessions are stored as fingerprints, so the database alone
/// opens no account.
pub struct Accounts {
    database: Arc<Database>,
    /// Wrong passwords per name: how many, and when the count started.
    failures: Mutex<HashMap<String, (u32, u64)>>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn person(row: &rusqlite::Row) -> rusqlite::Result<Person> {
    Ok(Person {
        id: row.get("id")?,
        name: row.get("name")?,
        owner: row.get("owner")?,
    })
}

fn forget_expired(connection: &Connection, now: u64) -> Result<()> {
    connection
        .execute("DELETE FROM invites WHERE expires <= ?1", [now])
        .map_err(sql)?;
    connection
        .execute("DELETE FROM sessions WHERE expires <= ?1", [now])
        .map_err(sql)?;
    Ok(())
}

fn new_session(connection: &Connection, account: &str, now: u64) -> Result<String> {
    let code = random_code()?;
    connection
        .execute(
            "INSERT INTO sessions (fingerprint, account, expires) VALUES (?1, ?2, ?3)",
            params![fingerprint(&code), account, now + SESSION_VALID_SECONDS],
        )
        .map_err(sql)?;
    Ok(code)
}

impl Accounts {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            database,
            failures: Mutex::default(),
        }
    }

    /// A new server with nobody on it and no invite on its way.
    pub fn waits_for_its_owner(&self, now: u64) -> Result<bool> {
        self.database.with(|connection| {
            connection
                .query_row(
                    "SELECT NOT EXISTS (SELECT 1 FROM accounts)
                        AND NOT EXISTS (SELECT 1 FROM invites WHERE expires > ?1)",
                    [now],
                    |row| row.get(0),
                )
                .map_err(sql)
        })
    }

    /// A new invite. The code is shown once and cannot be read back.
    pub fn invite(&self, now: u64) -> Result<String> {
        let code = random_code()?;
        self.database.with(|connection| {
            forget_expired(connection, now)?;
            connection
                .execute(
                    "INSERT INTO invites (fingerprint, expires) VALUES (?1, ?2)",
                    params![fingerprint(&code), now + INVITE_VALID_SECONDS],
                )
                .map_err(sql)?;
            Ok(())
        })?;
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
        let invite = fingerprint(invite.trim());
        let id = random_code()?[..16].to_owned();

        self.database.with(|connection| {
            let transaction = connection.transaction().map_err(sql)?;
            let used = transaction
                .execute(
                    "DELETE FROM invites WHERE fingerprint = ?1 AND expires > ?2",
                    params![invite, now],
                )
                .map_err(sql)?;
            if used == 0 {
                return Err(Error::InvalidInput(
                    "this invite is not valid any more; ask for a new one".into(),
                ));
            }
            let taken: bool = transaction
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM accounts WHERE name_key = ?1)",
                    [name.to_lowercase()],
                    |row| row.get(0),
                )
                .map_err(sql)?;
            if taken {
                // Leaving without a commit gives the invite back.
                return Err(Error::InvalidInput(format!("the name `{name}` is taken")));
            }
            let person = transaction
                .query_row(
                    "INSERT INTO accounts (id, name, name_key, password, owner, created)
                     VALUES (?1, ?2, ?3, ?4, NOT EXISTS (SELECT 1 FROM accounts), ?5)
                     RETURNING id, name, owner",
                    params![id, name, name.to_lowercase(), password, now],
                    person,
                )
                .map_err(sql)?;
            let session = new_session(&transaction, &person.id, now)?;
            transaction.commit().map_err(sql)?;
            Ok((person, session))
        })
    }

    pub fn log_in(&self, name: &str, password: &str, now: u64) -> Result<(Person, String)> {
        let key = name.trim().to_lowercase();
        if self.is_locked(&key, now) {
            return Err(Error::InvalidInput(
                "too many wrong passwords; try again in a few minutes".into(),
            ));
        }
        let found = self.database.with(|connection| {
            connection
                .query_row(
                    "SELECT id, name, owner, password FROM accounts WHERE name_key = ?1",
                    [&key],
                    |row| Ok((person(row)?, row.get::<_, String>("password")?)),
                )
                .optional()
                .map_err(sql)
        })?;
        // An unknown name takes as long as a wrong password.
        let stored = found.as_ref().map_or(unused_hash(), |(_, hash)| hash);
        match (password_matches(stored, password), found) {
            (true, Some((person, _))) => {
                self.failures().remove(&key);
                let session = self.database.with(|connection| {
                    forget_expired(connection, now)?;
                    new_session(connection, &person.id, now)
                })?;
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
        let found = self.database.with(|connection| {
            connection
                .query_row(
                    "SELECT accounts.id, accounts.name, accounts.owner
                     FROM sessions JOIN accounts ON accounts.id = sessions.account
                     WHERE sessions.fingerprint = ?1 AND sessions.expires > ?2",
                    params![fingerprint(session), now],
                    person,
                )
                .optional()
                .map_err(sql)
        });
        found.ok().flatten()
    }

    pub fn log_out(&self, session: &str) -> Result<()> {
        self.database.with(|connection| {
            connection
                .execute(
                    "DELETE FROM sessions WHERE fingerprint = ?1",
                    [fingerprint(session)],
                )
                .map_err(sql)?;
            Ok(())
        })
    }

    pub fn people(&self) -> Result<Vec<Person>> {
        self.database.with(|connection| {
            let mut statement = connection
                .prepare("SELECT id, name, owner FROM accounts ORDER BY created, rowid")
                .map_err(sql)?;
            let people = statement
                .query_map([], person)
                .map_err(sql)?
                .collect::<rusqlite::Result<_>>();
            people.map_err(sql)
        })
    }

    /// Removes an account with everything it has on the server: its sessions,
    /// its settings and its logins. The owner stays.
    pub fn remove(&self, id: &str) -> Result<()> {
        self.database.with(|connection| {
            let owner: Option<bool> = connection
                .query_row("SELECT owner FROM accounts WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(sql)?;
            match owner {
                None => Err(Error::InvalidInput("no such account".into())),
                Some(true) => Err(Error::InvalidInput(
                    "the owner of the server cannot be removed".into(),
                )),
                Some(false) => {
                    connection
                        .execute("DELETE FROM accounts WHERE id = ?1", [id])
                        .map_err(sql)?;
                    Ok(())
                }
            }
        })
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
    use std::path::PathBuf;

    use super::*;

    pub const PASSWORD: &str = "correct horse battery";

    /// An empty server in a folder of its own; remove the folder when done.
    pub fn accounts(name: &str) -> (Accounts, PathBuf) {
        let (database, folder) = crate::store::testing::database(name);
        (Accounts::new(database), folder)
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
    use std::fs;

    use super::testing::{PASSWORD, accounts, join};
    use super::*;

    #[test]
    fn the_first_account_owns_the_server_and_later_ones_do_not() {
        let (accounts, folder) = accounts("owner");
        assert!(accounts.waits_for_its_owner(0).expect("waits"));
        let (first, _) = join(&accounts, "Zahraa");
        let (second, _) = join(&accounts, "Bestie");
        assert!(first.owner && !second.owner);
        assert_eq!(accounts.people().expect("people"), [first, second]);
        drop(accounts);
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn a_new_server_waits_for_its_owner_until_an_invite_is_out() {
        let (accounts, folder) = accounts("waits");
        let waits = |now| accounts.waits_for_its_owner(now).expect("waits");
        assert!(waits(0));
        let invite = accounts.invite(0).expect("invite");
        assert!(!waits(0));
        assert!(waits(INVITE_VALID_SECONDS));

        accounts
            .accept(&invite, "Zahraa", PASSWORD, 0)
            .expect("owner");
        assert!(!waits(INVITE_VALID_SECONDS));
        drop(accounts);
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
        drop(accounts);
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
        drop(accounts);
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
        drop(accounts);
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
        drop(accounts);
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

        accounts.log_out(&session).expect("log out");
        assert!(accounts.person_of(&session, 0).is_none());
        drop(accounts);
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn removing_someone_logs_them_out_but_the_owner_stays() {
        let (accounts, folder) = accounts("remove");
        let (owner, _) = join(&accounts, "Zahraa");
        let (other, session) = join(&accounts, "Bestie");
        assert!(accounts.remove(&owner.id).is_err());
        assert!(accounts.remove("nobody").is_err());

        accounts.remove(&other.id).expect("remove");
        assert!(accounts.person_of(&session, 0).is_none());
        assert!(accounts.log_in("Bestie", PASSWORD, 0).is_err());
        assert_eq!(accounts.people().expect("people"), [owner]);
        drop(accounts);
        fs::remove_dir_all(folder).expect("clean up");
    }

    #[test]
    fn accounts_survive_a_restart_and_the_database_holds_no_password_or_code() {
        let (accounts, folder) = accounts("restart");
        let (person, session) = join(&accounts, "Zahraa");
        let invite = accounts.invite(0).expect("invite");
        drop(accounts);

        let mut stored = Vec::new();
        for file in fs::read_dir(&folder).expect("folder") {
            stored.extend(fs::read(file.expect("file").path()).expect("content"));
        }
        let stored = String::from_utf8_lossy(&stored);
        assert!(stored.contains("Zahraa"));
        assert!(!stored.contains(PASSWORD));
        assert!(!stored.contains(&session) && !stored.contains(&invite));

        let reopened = Accounts::new(Database::open(&folder).expect("database"));
        assert_eq!(reopened.person_of(&session, 0), Some(person));
        assert!(reopened.accept(&invite, "Bestie", PASSWORD, 0).is_ok());
        drop(reopened);
        fs::remove_dir_all(folder).expect("clean up");
    }
}
