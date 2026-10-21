use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};

use feld_core::model::Browser;
use feld_core::time;

use super::Store;

/// A browser token lapses this long after its last use.
pub const BROWSER_IDLE: Duration = Duration::days(7);
/// A browser token lapses this long after its pairing, however often it is used.
pub const BROWSER_LIFETIME: Duration = Duration::days(30);
/// A use this soon after the recorded one leaves the record as it is.
const LAST_USED_GRAIN: Duration = Duration::minutes(1);

const COLUMNS: &str = "id, name, paired_at, last_used_at, revoked_at";

/// The browser token a pairing issues.
pub struct NewBrowser<'a> {
    pub id: &'a str,
    pub token_hash: &'a str,
    pub name: &'a str,
}

impl Store {
    /// Issues a browser token, first revoking the ones whose hashes are in `replacing`, which
    /// the pairing browser still holds.
    pub fn pair_browser(
        &self,
        browser: NewBrowser<'_>,
        replacing: &[String],
        now: DateTime<Utc>,
    ) -> Result<Browser> {
        let at = time::format(now);
        self.write("browser pairing", |transaction, _| {
            for replacing in replacing {
                transaction
                    .execute(
                        "UPDATE browser_tokens SET revoked_at = ?2
                         WHERE token_hash = ?1 AND revoked_at IS NULL",
                        params![replacing, at],
                    )
                    .context("revoke the browser token a pairing replaces")?;
            }
            transaction
                .execute(
                    "INSERT INTO browser_tokens
                       (id, token_hash, name, paired_at, last_used_at)
                     VALUES (?1, ?2, ?3, ?4, ?4)",
                    params![browser.id, browser.token_hash, browser.name, at],
                )
                .context("issue a browser token")?;
            Ok(Browser {
                id: browser.id.to_owned(),
                name: browser.name.to_owned(),
                paired_at: at.clone(),
                last_used_at: at.clone(),
                expires_at: time::format(now + BROWSER_IDLE.min(BROWSER_LIFETIME)),
                revoked_at: None,
            })
        })
    }

    /// The `id` and the last use of the valid browser token whose value hashes to
    /// `token_hash`.
    pub fn authenticate_browser(
        &self,
        token_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<(String, String)>> {
        let (idle, lifetime) = thresholds(now);
        self.read(|connection| {
            connection
                .query_row(
                    "SELECT id, last_used_at FROM browser_tokens
                     WHERE token_hash = ?1 AND revoked_at IS NULL
                       AND last_used_at > ?2 AND paired_at > ?3",
                    params![token_hash, idle, lifetime],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .context("authenticate browser token")
        })
    }

    /// Whether the browser token `id` is valid at `now`.
    pub fn browser_valid(&self, id: &str, now: DateTime<Utc>) -> Result<bool> {
        let (idle, lifetime) = thresholds(now);
        self.read(|connection| {
            connection
                .query_row(
                    "SELECT 1 FROM browser_tokens
                     WHERE id = ?1 AND revoked_at IS NULL
                       AND last_used_at > ?2 AND paired_at > ?3",
                    params![id, idle, lifetime],
                    |_| Ok(()),
                )
                .optional()
                .map(|found| found.is_some())
                .context("check browser token")
        })
    }

    /// Records a use of the browser token `id` last used at `last_used_at`. A use within a
    /// minute of the recorded one leaves the store alone, so that a browser's polling does
    /// not take the write lock on every request.
    pub fn record_browser_use(
        &self,
        id: &str,
        last_used_at: &str,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let grain = time::format(now - LAST_USED_GRAIN);
        if last_used_at >= grain.as_str() {
            return Ok(());
        }
        let at = time::format(now);
        self.write("browser token use", |transaction, _| {
            transaction
                .execute(
                    "UPDATE browser_tokens SET last_used_at = ?2
                     WHERE id = ?1 AND last_used_at < ?3",
                    params![id, at, grain],
                )
                .context("record the use of a browser token")?;
            Ok(())
        })
    }

    /// The valid browser tokens, oldest pairing first.
    pub fn list_browsers(&self, now: DateTime<Utc>) -> Result<Vec<Browser>> {
        let (idle, lifetime) = thresholds(now);
        self.read(|connection| {
            let mut statement = connection
                .prepare(&format!(
                    "SELECT {COLUMNS} FROM browser_tokens
                     WHERE revoked_at IS NULL AND last_used_at > ?1 AND paired_at > ?2
                     ORDER BY paired_at, id"
                ))
                .context("prepare browser token listing")?;
            statement
                .query_map(params![idle, lifetime], browser)
                .context("query browser tokens")?
                .collect::<rusqlite::Result<Vec<_>>>()
                .context("read browser tokens")
        })
    }

    /// Revokes the valid browser token `id`, or answers `None` when there is none.
    pub fn revoke_browser(&self, id: &str, now: DateTime<Utc>) -> Result<Option<Browser>> {
        let (idle, lifetime) = thresholds(now);
        let at = time::format(now);
        self.write("browser token revocation", |transaction, _| {
            let revoked = transaction
                .execute(
                    "UPDATE browser_tokens SET revoked_at = ?2
                     WHERE id = ?1 AND revoked_at IS NULL
                       AND last_used_at > ?3 AND paired_at > ?4",
                    params![id, at, idle, lifetime],
                )
                .context("revoke browser token")?;
            if revoked == 0 {
                return Ok(None);
            }
            browser_by_id(transaction, id).map(Some)
        })
    }
}

/// The oldest last use and the oldest pairing a valid browser token can have at `now`.
fn thresholds(now: DateTime<Utc>) -> (String, String) {
    (
        time::format(now - BROWSER_IDLE),
        time::format(now - BROWSER_LIFETIME),
    )
}

fn browser_by_id(connection: &Connection, id: &str) -> Result<Browser> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM browser_tokens WHERE id = ?1"),
            [id],
            browser,
        )
        .context("read browser token")
}

fn browser(row: &Row<'_>) -> rusqlite::Result<Browser> {
    let paired_at: String = row.get(2)?;
    let last_used_at: String = row.get(3)?;
    let expires_at = match (time::parse(&paired_at), time::parse(&last_used_at)) {
        (Ok(paired), Ok(used)) => {
            time::format((used + BROWSER_IDLE).min(paired + BROWSER_LIFETIME))
        }
        _ => String::new(),
    };
    Ok(Browser {
        id: row.get(0)?,
        name: row.get(1)?,
        paired_at,
        last_used_at,
        expires_at,
        revoked_at: row.get(4)?,
    })
}
