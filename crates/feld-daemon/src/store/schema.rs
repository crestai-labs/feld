use anyhow::{Context, Result, bail};
use feld_core::model::Ownership;
use rusqlite::{Connection, TransactionBehavior};
use serde_json::Value;

use super::sql::Text;

/// The DDL of a new store, of [`SCHEMA_VERSION`].
const SCHEMA: &str = include_str!("sql/schema.sql");

/// The steps that migrate a store, each by the version it migrates from and the DDL that
/// takes it to the next version.
const MIGRATIONS: &[(u32, &str)] = &[
    (5, include_str!("sql/migrate/006.sql")),
    (6, include_str!("sql/migrate/007.sql")),
    (7, include_str!("sql/migrate/008.sql")),
    (8, include_str!("sql/migrate/009.sql")),
    (9, include_str!("sql/migrate/010.sql")),
    (10, include_str!("sql/migrate/011.sql")),
    (11, include_str!("sql/migrate/012.sql")),
    (12, include_str!("sql/migrate/013.sql")),
    (13, include_str!("sql/migrate/014.sql")),
    (14, include_str!("sql/migrate/015.sql")),
];

const OLDEST_VERSION: u32 = MIGRATIONS[0].0;

pub const SCHEMA_VERSION: u32 = MIGRATIONS[MIGRATIONS.len() - 1].0 + 1;

pub(super) fn migrate(connection: &mut Connection) -> Result<()> {
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .context("read store schema version")?;
    match support(version) {
        Support::Empty => apply(connection, SCHEMA, SCHEMA_VERSION).context("create store schema"),
        Support::Current => Ok(()),
        Support::Migrated => {
            for &(from, sql) in MIGRATIONS.iter().filter(|(from, _)| *from >= version) {
                apply(connection, sql, from + 1)
                    .with_context(|| format!("migrate store schema {from} to {}", from + 1))?;
            }
            Ok(())
        }
        Support::Unsupported(reason) => bail!(reason),
    }
}

/// Runs `sql` and sets the schema version to `version` in one transaction.
fn apply(connection: &mut Connection, sql: &str, version: u32) -> rusqlite::Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(sql)?;
    transaction.pragma_update(None, "user_version", version)?;
    transaction.commit()
}

/// How a store of one schema version opens with this version of Feld.
pub enum Support {
    /// The store has no schema yet, which the daemon creates when it opens it.
    Empty,
    Current,
    /// The daemon migrates the store to [`SCHEMA_VERSION`] when it opens it.
    Migrated,
    /// The daemon refuses the store, for the reason it gives.
    Unsupported(String),
}

pub fn support(version: u32) -> Support {
    match version {
        0 => Support::Empty,
        SCHEMA_VERSION => Support::Current,
        OLDEST_VERSION..SCHEMA_VERSION => Support::Migrated,
        _ => Support::Unsupported(unsupported(version)),
    }
}

fn unsupported(version: u32) -> String {
    format!(
        "store schema version {version} is not supported version {SCHEMA_VERSION}; move the store of the Feld home aside to start a new one"
    )
}

/// The schema version of the SQLite store at `path`, read through a read-only connection
/// that also reads the store's schema, so that a store the daemon cannot open fails here;
/// None when there is no store. The store is opened as immutable, which writes nothing
/// beside it and reads no write-ahead log, so it answers for a store without one.
pub fn read_version(path: &std::path::Path) -> Result<Option<u32>> {
    if !path.exists() {
        return Ok(None);
    }
    let connection = open_immutable(path)?;
    let read = || -> rusqlite::Result<u32> {
        connection.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))?;
        connection.query_row("PRAGMA user_version", [], |row| row.get(0))
    };
    read()
        .map(Some)
        .with_context(|| format!("read SQLite store {}", path.display()))
}

/// A Session the store holds open, as [`read_open_sessions`] reads it.
pub struct OpenSession {
    pub id: String,
    pub workspace: String,
    pub actor: String,
    pub ownership: Ownership,
    /// The Runtime Adapter of the Session, or its execution type when it runs none.
    pub execution: String,
}

/// The Sessions the SQLite store at `path` holds open, read as [`read_version`] reads it;
/// none when there is no store or it has no schema yet. A store whose write-ahead log
/// holds writes fails, as reading it without the log misses them.
pub fn read_open_sessions(path: &std::path::Path) -> Result<Vec<OpenSession>> {
    if let Some(log) = write_ahead_log(path) {
        bail!(
            "{} holds writes the daemon has not read back",
            log.display()
        );
    }
    let Some(version) = read_version(path)? else {
        return Ok(Vec::new());
    };
    match support(version) {
        Support::Empty => return Ok(Vec::new()),
        Support::Current | Support::Migrated => {}
        Support::Unsupported(reason) => bail!(reason),
    }
    let connection = open_immutable(path)?;
    let read = || -> rusqlite::Result<Vec<OpenSession>> {
        connection
            .prepare(
                "SELECT s.id, a.workspace_key, a.name, s.ownership, s.execution
                 FROM sessions s JOIN actors a ON a.id = s.actor_id
                 WHERE s.end IS NULL ORDER BY s.start",
            )?
            .query_map([], |row| {
                let execution: Value =
                    serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default();
                Ok(OpenSession {
                    id: row.get(0)?,
                    workspace: row.get(1)?,
                    actor: row.get(2)?,
                    ownership: row.get::<_, Text<Ownership>>(3)?.0,
                    execution: execution
                        .get("runtime")
                        .or_else(|| execution.get("type"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                })
            })?
            .collect()
    };
    read().with_context(|| format!("read the Sessions of {}", path.display()))
}

/// A read-only connection to the SQLite store at `path`, opened as immutable.
fn open_immutable(path: &std::path::Path) -> Result<Connection> {
    use rusqlite::OpenFlags;

    Connection::open_with_flags(
        format!("file:{}?immutable=1", uri_path(path)),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .with_context(|| format!("open SQLite store {}", path.display()))
}

/// The write-ahead log of the SQLite store at `path`, when it holds anything. Reading a
/// store through its log writes the shared memory beside it.
pub fn write_ahead_log(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut log = path.as_os_str().to_owned();
    log.push("-wal");
    let log = std::path::PathBuf::from(log);
    std::fs::metadata(&log)
        .is_ok_and(|metadata| metadata.len() > 0)
        .then_some(log)
}

/// `path` as the path of an SQLite URI, with the characters a URI reads otherwise escaped.
fn uri_path(path: &std::path::Path) -> String {
    let mut escaped = String::new();
    for character in path.to_string_lossy().chars() {
        match character {
            '%' | '?' | '#' => escaped.push_str(&format!("%{:02X}", character as u32)),
            _ => escaped.push(character),
        }
    }
    escaped
}
