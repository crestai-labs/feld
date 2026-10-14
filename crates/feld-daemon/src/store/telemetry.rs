use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use feld_core::model::Session;
use feld_core::telemetry::telemetry_token;
use feld_core::time::{self, now};

use super::Store;
use super::session::{read_session_rows, session_overlaps};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TelemetryAdmission {
    Stored { session_id: String },
    Duplicate { session_id: String },
    Unattributed,
    Conflict,
}

#[derive(Clone, Debug)]
pub struct Telemetry {
    pub request_id: String,
    pub session_id: String,
    pub signal: String,
    pub content_type: String,
    pub body: Vec<u8>,
    pub received_at: String,
}

impl Store {
    /// The Sessions of live Actors in every Workspace that overlap the half-open range, each
    /// with the Telemetries of the range. `before` gives each Session what tells when the
    /// Telemetries read before the range hold its readings from before it.
    pub fn range_telemetries<H: FnMut(&[Telemetry]) -> bool>(
        &self,
        start: Option<&str>,
        end: Option<&str>,
        before: impl Fn(&Session) -> H,
    ) -> Result<Vec<(Session, Vec<Telemetry>)>> {
        let since = received_since(start)?;
        self.read(|connection| {
            // One read transaction keeps the Sessions and their Telemetries consistent.
            let transaction = connection
                .unchecked_transaction()
                .context("begin range telemetry read")?;
            let sessions = read_session_rows(
                &transaction,
                "WHERE a.deleted_at IS NULL ORDER BY s.start, s.id",
                [],
            )?
            .into_iter()
            .filter(|session| session_overlaps(&session.start, session.end.as_deref(), start, end))
            .map(|session| {
                let telemetry = Self::range_telemetries_for(
                    &transaction,
                    &session.id,
                    since.as_deref(),
                    &mut before(&session),
                )?;
                Ok((session, telemetry))
            })
            .collect::<Result<Vec<_>>>()?;
            transaction
                .commit()
                .context("commit range telemetry read")?;
            Ok(sessions)
        })
    }

    /// Removes the Telemetries of the Sessions that closed before `closed_before` and the
    /// oldest ones of the Sessions over the limit, and answers how many it removed.
    pub fn prune_telemetries(&self, closed_before: &str) -> Result<usize> {
        self.write("telemetry retention", |transaction, _| {
            let mut removed = transaction
                .execute(
                    "DELETE FROM telemetries WHERE session_id IN
                     (SELECT id FROM sessions WHERE end IS NOT NULL AND end < ?1)",
                    [closed_before],
                )
                .context("remove closed Session telemetry")?;
            transaction
                .execute(
                    "UPDATE sessions SET telemetry_bytes = 0
                     WHERE end IS NOT NULL AND end < ?1 AND telemetry_bytes > 0",
                    [closed_before],
                )
                .context("count closed Session telemetry bytes")?;
            // A store migrated to the byte count may hold Sessions over the limit.
            let over = transaction
                .prepare("SELECT id, telemetry_bytes FROM sessions WHERE telemetry_bytes > ?1")
                .context("prepare Sessions over the telemetry limit")?
                .query_map([SESSION_TELEMETRY_LIMIT], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .context("query Sessions over the telemetry limit")?
                .collect::<rusqlite::Result<Vec<_>>>()
                .context("read Sessions over the telemetry limit")?;
            for (session_id, bytes) in over {
                removed += drop_oldest_telemetries(transaction, &session_id, bytes)?;
            }
            Ok(removed)
        })
    }

    /// The Telemetries of one Session for a range from `start`. `holds` takes each page read
    /// before the range, newest first, and says when the pages so far hold the Session's
    /// readings from before it.
    pub fn session_telemetries(
        &self,
        session_id: &str,
        start: Option<&str>,
        mut holds: impl FnMut(&[Telemetry]) -> bool,
    ) -> Result<Vec<Telemetry>> {
        let since = received_since(start)?;
        self.read(|connection| {
            // One read transaction keeps the Telemetries before the range and in it consistent.
            let transaction = connection
                .unchecked_transaction()
                .context("begin session telemetry read")?;
            let telemetries = Self::range_telemetries_for(
                &transaction,
                session_id,
                since.as_deref(),
                &mut holds,
            )?;
            transaction
                .commit()
                .context("commit session telemetry read")?;
            Ok(telemetries)
        })
    }

    pub fn record_telemetry(
        &self,
        request_id: &str,
        token: &str,
        signal: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<TelemetryAdmission> {
        let mut digest = Sha256::new();
        digest.update(signal.as_bytes());
        digest.update([0]);
        digest.update(content_type.as_bytes());
        digest.update([0]);
        digest.update(token.as_bytes());
        digest.update([0]);
        digest.update(body);
        let digest = format!("{:x}", digest.finalize());
        self.write("telemetry admission", |transaction, notices| {
            if let Some((stored_digest, session_id)) = transaction
                .query_row(
                    "SELECT digest, session_id FROM telemetries WHERE request_id = ?1",
                    [request_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()
                .context("read telemetry retry")?
            {
                return Ok(if stored_digest == digest {
                    TelemetryAdmission::Duplicate { session_id }
                } else {
                    TelemetryAdmission::Conflict
                });
            }
            let session = telemetry_session(transaction, token)?;
            let Some((session_id, workspace)) = session else {
                return Ok(TelemetryAdmission::Unattributed);
            };
            transaction
                .execute(
                    "INSERT INTO telemetries
                     (request_id, session_id, signal, content_type, body, digest, received_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        request_id,
                        session_id,
                        signal,
                        content_type,
                        body,
                        digest,
                        now()
                    ],
                )
                .context("store telemetry")?;
            let added = i64::try_from(body.len()).context("telemetry body length")?
                + TELEMETRY_ROW_OVERHEAD;
            let bytes: i64 = transaction
                .query_row(
                    "UPDATE sessions SET telemetry_bytes = telemetry_bytes + ?2 WHERE id = ?1
                     RETURNING telemetry_bytes",
                    params![session_id, added],
                    |row| row.get(0),
                )
                .context("count session telemetry bytes")?;
            if bytes > SESSION_TELEMETRY_LIMIT {
                drop_oldest_telemetries(transaction, &session_id, bytes)?;
            }
            notices.push("session", &session_id, Some(&workspace));
            notices.push("quota", request_id, None);
            Ok(TelemetryAdmission::Stored { session_id })
        })
    }

    /// The open Session whose Session token `token` was derived from, if any.
    pub fn telemetry_token_session(&self, token: &str) -> Result<Option<String>> {
        self.read(|connection| {
            Ok(telemetry_session(connection, token)?.map(|(session_id, _)| session_id))
        })
    }

    pub fn telemetries(&self, session_id: &str) -> Result<Vec<Telemetry>> {
        self.read(|connection| Self::telemetries_for(connection, session_id, None))
    }

    /// The Telemetries of one Session received since the range began, after the ones received
    /// before it that hold the readings from before the range.
    fn range_telemetries_for(
        connection: &Connection,
        session_id: &str,
        since: Option<&str>,
        holds: &mut dyn FnMut(&[Telemetry]) -> bool,
    ) -> Result<Vec<Telemetry>> {
        let mut telemetries = match since {
            Some(since) => Self::telemetries_before(connection, session_id, since, holds)?,
            None => Vec::new(),
        };
        telemetries.extend(Self::telemetries_for(connection, session_id, since)?);
        Ok(telemetries)
    }

    /// The Telemetries of a Session received before a time, read back from the latest a page
    /// at a time until the pages hold the readings or none are left; oldest first.
    fn telemetries_before(
        connection: &Connection,
        session_id: &str,
        before: &str,
        holds: &mut dyn FnMut(&[Telemetry]) -> bool,
    ) -> Result<Vec<Telemetry>> {
        let mut statement = connection
            .prepare(HELD_TELEMETRIES_SQL)
            .context("prepare held telemetry listing")?;
        let mut telemetries = Vec::new();
        // The empty request ID sorts before every other, so the first page is everything
        // received before the time.
        let mut cursor = (before.to_owned(), String::new());
        loop {
            let page = statement
                .query_map(
                    params![session_id, cursor.0, cursor.1, HELD_TELEMETRIES_PAGE],
                    telemetry_row,
                )
                .context("query held telemetries")?
                .collect::<rusqlite::Result<Vec<_>>>()
                .context("read held telemetries")?;
            let done = page.len() < HELD_TELEMETRIES_PAGE || holds(&page);
            if let Some(oldest) = page.last() {
                cursor = (oldest.received_at.clone(), oldest.request_id.clone());
            }
            telemetries.extend(page);
            if done {
                break;
            }
        }
        telemetries.reverse();
        Ok(telemetries)
    }

    fn telemetries_for(
        connection: &Connection,
        session_id: &str,
        since: Option<&str>,
    ) -> Result<Vec<Telemetry>> {
        // A plain bound keeps the read a range search of the Session's index; the empty
        // string compares below every received time.
        let mut statement = connection
            .prepare(RECEIVED_TELEMETRIES_SQL)
            .context("prepare telemetry listing")?;
        statement
            .query_map(params![session_id, since.unwrap_or("")], telemetry_row)
            .context("query telemetries")?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("read telemetries")
    }
}

fn telemetry_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Telemetry> {
    Ok(Telemetry {
        request_id: row.get(0)?,
        session_id: row.get(1)?,
        signal: row.get(2)?,
        content_type: row.get(3)?,
        body: row.get(4)?,
        received_at: row.get(5)?,
    })
}

/// The received time a range read takes Telemetries from: the range's start, less the clock
/// allowance.
fn received_since(start: Option<&str>) -> Result<Option<String>> {
    start
        .map(|start| {
            time::parse(start)
                .map(|start| time::format(start - TELEMETRY_CLOCK_ALLOWANCE))
                .context("parse range start")
        })
        .transpose()
}

/// A Telemetry arrives after the points it carries were measured, give or take the clock of the
/// Node that measured them; a range read takes Telemetries received this long before it.
const TELEMETRY_CLOCK_ALLOWANCE: chrono::TimeDelta = chrono::TimeDelta::minutes(5);

/// The bytes of Telemetry kept for one Session, each Telemetry counting its body and the
/// row overhead.
pub const SESSION_TELEMETRY_LIMIT: i64 = 64 * 1024 * 1024;

/// The bytes each Telemetry counts toward the Session limit beyond its body, for its row and
/// index entry, so that small bodies are bounded too. Migration 015 counts the same.
pub const TELEMETRY_ROW_OVERHEAD: i64 = 1024;

/// How long the Telemetries of a closed Session are kept.
pub const CLOSED_SESSION_TELEMETRY_RETENTION: chrono::TimeDelta = chrono::TimeDelta::days(30);

/// Drops the oldest Telemetries of the Session until the rest fit in the limit, and answers
/// how many it dropped.
fn drop_oldest_telemetries(
    connection: &Connection,
    session_id: &str,
    mut bytes: i64,
) -> Result<usize> {
    let mut statement = connection
        .prepare(
            "SELECT request_id, length(body) FROM telemetries WHERE session_id = ?1
             ORDER BY received_at, request_id",
        )
        .context("prepare oldest telemetry listing")?;
    let mut rows = statement
        .query([session_id])
        .context("query oldest telemetries")?;
    let mut dropped = Vec::new();
    while bytes > SESSION_TELEMETRY_LIMIT {
        let Some(row) = rows.next().context("read oldest telemetries")? else {
            break;
        };
        dropped.push(row.get::<_, String>(0)?);
        bytes -= row.get::<_, i64>(1)? + TELEMETRY_ROW_OVERHEAD;
    }
    drop(rows);
    for request_id in &dropped {
        connection
            .execute(
                "DELETE FROM telemetries WHERE request_id = ?1",
                [request_id],
            )
            .context("drop telemetry over the Session limit")?;
    }
    connection
        .execute(
            "UPDATE sessions SET telemetry_bytes = ?2 WHERE id = ?1",
            params![session_id, bytes],
        )
        .context("count session telemetry bytes")?;
    Ok(dropped.len())
}

/// The Telemetries a range read takes at a time from before the range.
const HELD_TELEMETRIES_PAGE: usize = 16;

/// The latest Telemetries of a Session received before a received time and request ID,
/// newest first.
const HELD_TELEMETRIES_SQL: &str = "SELECT request_id, session_id, signal, content_type, body,
    received_at FROM telemetries WHERE session_id = ?1 AND (received_at, request_id) < (?2, ?3)
    ORDER BY received_at DESC, request_id DESC LIMIT ?4";

/// The Telemetries of a Session received since a time, oldest first.
const RECEIVED_TELEMETRIES_SQL: &str = "SELECT request_id, session_id, signal, content_type,
    body, received_at FROM telemetries WHERE session_id = ?1 AND received_at >= ?2
    ORDER BY received_at, request_id";

/// The open Session, with its Workspace, whose Session token `token` was derived from.
/// Telemetry carries the telemetry token of a running Session's token, never the Session
/// token itself, so the Session is found by deriving each one's.
fn telemetry_session(connection: &Connection, token: &str) -> Result<Option<(String, String)>> {
    let mut statement = connection
        .prepare(
            "SELECT s.id, a.workspace_key, s.token FROM sessions s
             JOIN actors a ON a.id = s.actor_id
             WHERE s.end IS NULL AND s.token IS NOT NULL",
        )
        .context("prepare telemetry attribution")?;
    let sessions = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .context("attribute telemetry token")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("attribute telemetry token")?;
    Ok(sessions
        .into_iter()
        .find(|(_, _, session_token)| telemetry_token(session_token) == token)
        .map(|(session_id, workspace, _)| (session_id, workspace)))
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{HELD_TELEMETRIES_SQL, RECEIVED_TELEMETRIES_SQL};

    fn query_plan(sql: &str) -> String {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(include_str!("sql/schema.sql"))
            .unwrap();
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        let parameters = vec![rusqlite::types::Null; statement.parameter_count()];
        statement
            .query_map(rusqlite::params_from_iter(parameters), |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n")
    }

    #[test]
    fn telemetry_reads_search_the_session_index_by_received_time() {
        assert_eq!(
            query_plan(RECEIVED_TELEMETRIES_SQL),
            "SEARCH telemetries USING INDEX telemetries_session (session_id=? AND received_at>?)"
        );
        assert_eq!(
            query_plan(HELD_TELEMETRIES_SQL),
            "SEARCH telemetries USING INDEX telemetries_session \
             (session_id=? AND (received_at,request_id)<(?,?))"
        );
    }
}
