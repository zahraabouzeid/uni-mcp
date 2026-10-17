//! The database of a home server: one SQLite file, brought up to date at
//! start by the numbered scripts in `migrations/`.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rusqlite::Connection;
use uni_core::{Error, Result};

const FILE: &str = "unimcp.db";

/// In the order they are applied. A released script is never changed again;
/// a change to the tables is a new script at the end.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_accounts.sql"),
    include_str!("../migrations/0002_oauth.sql"),
];

pub struct Database {
    connection: Mutex<Connection>,
}

pub fn sql(e: rusqlite::Error) -> Error {
    Error::Config(format!("database error: {e}"))
}

impl Database {
    pub fn open(folder: &Path) -> Result<Arc<Self>> {
        let file = folder.join(FILE);
        let cannot = |e: &dyn std::fmt::Display| {
            Error::Config(format!("cannot open {}: {e}", file.display()))
        };
        fs::create_dir_all(folder).map_err(|e| cannot(&e))?;
        let connection = Connection::open(&file).map_err(|e| cannot(&e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o600))
                .map_err(|e| cannot(&e))?;
        }
        Self::prepare(connection, MIGRATIONS)
    }

    fn prepare(mut connection: Connection, migrations: &[&str]) -> Result<Arc<Self>> {
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(sql)?;
        // Off by default in SQLite; removing an account relies on it.
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(sql)?;
        migrate(&mut connection, migrations)?;
        Ok(Arc::new(Self {
            connection: Mutex::new(connection),
        }))
    }

    /// Runs `work` on the database, one caller at a time.
    pub fn with<T>(&self, work: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        work(&mut connection)
    }
}

/// Applies the scripts the database has not seen yet. Each runs in a
/// transaction of its own, so a failing one leaves the database as it was.
fn migrate(connection: &mut Connection, migrations: &[&str]) -> Result<()> {
    let applied: usize = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(sql)?;
    if applied > migrations.len() {
        return Err(Error::Config(
            "the database was made by a newer version of uniMCP; update uniMCP to open it".into(),
        ));
    }
    for (index, script) in migrations.iter().enumerate().skip(applied) {
        let transaction = connection.transaction().map_err(sql)?;
        transaction.execute_batch(script).map_err(sql)?;
        transaction
            .pragma_update(None, "user_version", index + 1)
            .map_err(sql)?;
        transaction.commit().map_err(sql)?;
    }
    Ok(())
}

#[cfg(test)]
pub mod testing {
    use std::path::PathBuf;

    use super::*;

    /// An empty database in a folder of its own; remove the folder when done.
    pub fn database(name: &str) -> (Arc<Database>, PathBuf) {
        let folder =
            std::env::temp_dir().join(format!("unimcp-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        (Database::open(&folder).expect("database"), folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(connection: &Connection) -> usize {
        connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("version")
    }

    fn tables(connection: &Connection) -> Vec<String> {
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .expect("statement");
        let names = statement
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<rusqlite::Result<_>>();
        names.expect("names")
    }

    #[test]
    fn scripts_are_applied_once_and_in_order() {
        let mut connection = Connection::open_in_memory().expect("database");
        let first = ["CREATE TABLE a (x INTEGER);"];
        migrate(&mut connection, &first).expect("first");
        assert_eq!(version(&connection), 1);

        let both = [
            first[0],
            "ALTER TABLE a ADD COLUMN y TEXT; CREATE TABLE b (z TEXT);",
        ];
        migrate(&mut connection, &both).expect("second");
        migrate(&mut connection, &both).expect("again");
        assert_eq!(version(&connection), 2);
        assert_eq!(tables(&connection), ["a", "b"]);
    }

    #[test]
    fn a_failing_script_leaves_the_database_as_it_was() {
        let mut connection = Connection::open_in_memory().expect("database");
        let broken = [
            "CREATE TABLE a (x INTEGER);",
            "CREATE TABLE b (z TEXT); NOT SQL;",
        ];
        assert!(migrate(&mut connection, &broken).is_err());
        assert_eq!(version(&connection), 1);
        assert_eq!(tables(&connection), ["a"]);
    }

    #[test]
    fn a_database_from_a_newer_version_is_not_touched() {
        let mut connection = Connection::open_in_memory().expect("database");
        migrate(
            &mut connection,
            &["CREATE TABLE a (x INTEGER);", "CREATE TABLE b (z TEXT);"],
        )
        .expect("newer");
        let error = migrate(&mut connection, &["CREATE TABLE a (x INTEGER);"]).expect_err("older");
        assert!(error.to_string().contains("newer version"));
        assert_eq!(tables(&connection), ["a", "b"]);
    }

    #[test]
    fn the_real_scripts_build_the_tables_and_removing_an_account_takes_its_rows_along() {
        let connection = Connection::open_in_memory().expect("database");
        let database = Database::prepare(connection, MIGRATIONS).expect("prepare");
        database
            .with(|connection| {
                assert_eq!(version(connection), MIGRATIONS.len());
                assert_eq!(
                    tables(connection),
                    [
                        "access_tokens",
                        "accounts",
                        "app_redirects",
                        "apps",
                        "authorization_codes",
                        "connections",
                        "invites",
                        "refresh_tokens",
                        "secrets",
                        "sessions",
                    ]
                );
                connection
                    .execute_batch(
                        "INSERT INTO accounts (id, name, name_key, password, created)
                             VALUES ('a', 'A', 'a', 'hash', 0);
                         INSERT INTO sessions VALUES ('f', 'a', 1);
                         INSERT INTO secrets VALUES ('a', 'k', x'00', x'00');
                         INSERT INTO apps VALUES ('claude', 'Claude', 0);
                         INSERT INTO app_redirects VALUES ('claude', 'https://claude.ai/cb');
                         INSERT INTO authorization_codes
                             VALUES ('c', 'a', 'claude', 'https://claude.ai/cb', 'x', 1);
                         INSERT INTO connections VALUES ('n', 'a', 'claude', 0, 0);
                         INSERT INTO access_tokens VALUES ('t', 'n', 1);
                         INSERT INTO refresh_tokens VALUES ('r', 'n');
                         DELETE FROM accounts WHERE id = 'a';",
                    )
                    .map_err(sql)?;
                let left: i64 = connection
                    .query_row(
                        "SELECT (SELECT count(*) FROM sessions) + (SELECT count(*) FROM secrets)
                             + (SELECT count(*) FROM authorization_codes)
                             + (SELECT count(*) FROM connections)
                             + (SELECT count(*) FROM access_tokens)
                             + (SELECT count(*) FROM refresh_tokens)",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(sql)?;
                assert_eq!(left, 0);
                Ok(())
            })
            .expect("work");
    }

    #[test]
    fn a_code_and_a_second_refresh_token_are_refused_where_they_do_not_belong() {
        let connection = Connection::open_in_memory().expect("database");
        let database = Database::prepare(connection, MIGRATIONS).expect("prepare");
        database
            .with(|connection| {
                connection
                    .execute_batch(
                        "INSERT INTO accounts (id, name, name_key, password, created)
                             VALUES ('a', 'A', 'a', 'hash', 0);
                         INSERT INTO apps VALUES ('claude', 'Claude', 0);
                         INSERT INTO app_redirects VALUES ('claude', 'https://claude.ai/cb');
                         INSERT INTO connections VALUES ('n', 'a', 'claude', 0, 0);
                         INSERT INTO refresh_tokens VALUES ('r', 'n');",
                    )
                    .map_err(sql)?;
                let elsewhere = connection.execute(
                    "INSERT INTO authorization_codes
                         VALUES ('c', 'a', 'claude', 'https://attacker.example/cb', 'x', 1)",
                    [],
                );
                assert!(elsewhere.is_err());
                let second = connection.execute("INSERT INTO refresh_tokens VALUES ('s', 'n')", []);
                assert!(second.is_err());
                Ok(())
            })
            .expect("work");
    }
}
