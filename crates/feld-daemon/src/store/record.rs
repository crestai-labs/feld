use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};

use feld_core::model::{ActorReference, Record, RecordKind, Schedule};

use super::message::get_message;
use super::schedule::tick_recorded;
use super::sql::Text;
use super::{Notices, Store};

#[derive(Default)]
pub struct RecordFilter<'a> {
    pub workspace: Option<&'a str>,
    /// The name of a live Actor.
    pub actor: Option<&'a str>,
    /// The records naming the Session: its own, and the `spawned` records of the Sessions
    /// it spawned.
    pub session: Option<&'a str>,
    pub kind: Option<RecordKind>,
    pub start: Option<&'a str>,
    pub end: Option<&'a str>,
    /// Only the records whose sequence is greater.
    pub after: u64,
    /// Only the records of live Actors.
    pub live_actors: bool,
}

impl Store {
    /// Records that the tick of a live Actor's Schedule planned at `planned` never fired, and why,
    /// unless it fired or was recorded as missed already. Returns whether it recorded.
    pub fn record_missed_tick(
        &self,
        workspace: &str,
        actor: &str,
        planned: &str,
        reason: &str,
    ) -> Result<bool> {
        self.write("missed tick", |transaction, notices: &mut Notices| {
            let Some((actor_id, _)) = super::actor::live_actor(transaction, workspace, actor)?
            else {
                return Ok(false);
            };
            if super::schedule::get_schedule(transaction, &actor_id)?.is_none() {
                return Ok(false);
            }
            if !append_missed_tick(transaction, &actor_id, planned, reason)? {
                return Ok(false);
            }
            notices.push("actor", actor, Some(workspace));
            Ok(true)
        })
    }

    /// Records the ticks planned at `planned` as missed, and why, as
    /// [`Store::record_missed_tick`] does, while the Schedule of the live Actor still fires as
    /// `expected`. Returns false, recording nothing, when it does not.
    pub fn record_missed_ticks_of(
        &self,
        workspace: &str,
        actor: &str,
        expected: &Schedule,
        planned: &[String],
        reason: &str,
    ) -> Result<bool> {
        self.write("missed ticks", |transaction, notices: &mut Notices| {
            let Some((actor_id, _)) = super::actor::live_actor(transaction, workspace, actor)?
            else {
                return Ok(false);
            };
            if !super::schedule::get_schedule(transaction, &actor_id)?
                .is_some_and(|schedule| schedule.fires_as(expected))
            {
                return Ok(false);
            }
            let mut recorded = false;
            for planned in planned {
                recorded |= append_missed_tick(transaction, &actor_id, planned, reason)?;
            }
            if recorded {
                notices.push("actor", actor, Some(workspace));
            }
            Ok(true)
        })
    }

    pub fn list_records(&self, filter: &RecordFilter<'_>) -> Result<Vec<Record>> {
        self.read(|connection| list_records(connection, filter))
    }
}

fn append_missed_tick(
    connection: &Connection,
    actor_id: &str,
    planned: &str,
    reason: &str,
) -> Result<bool> {
    if tick_recorded(connection, actor_id, planned)? {
        return Ok(false);
    }
    connection
        .execute(
            "INSERT INTO records (at, kind, actor_id, session_id, data)
             VALUES (?1, ?2, ?3, NULL, ?4)",
            params![
                feld_core::time::now(),
                Text(RecordKind::ScheduleMissed),
                actor_id,
                serde_json::json!({ "planned": planned, "reason": reason }).to_string()
            ],
        )
        .context("append missed tick")?;
    Ok(true)
}

fn list_records(connection: &Connection, filter: &RecordFilter<'_>) -> Result<Vec<Record>> {
    let mut statement = connection
        .prepare(
            "SELECT r.sequence, r.at, r.kind, r.actor_id, a.workspace_key, a.name, r.session_id,
                    r.data
             FROM records r JOIN actors a ON a.id = r.actor_id
             WHERE (?1 IS NULL OR a.workspace_id = (SELECT id FROM workspaces WHERE key = ?1 AND deleted_at IS NULL))
               AND (?2 IS NULL OR (a.name = ?2 AND a.deleted_at IS NULL))
               AND (?3 IS NULL OR r.session_id = ?3
                    OR (r.kind = 'spawned' AND json_extract(r.data, '$.spawnerSessionId') = ?3))
               AND (?4 IS NULL OR r.kind = ?4)
               AND (?5 IS NULL OR r.at >= ?5)
               AND (?6 IS NULL OR r.at < ?6)
               AND r.sequence > ?7
               AND (?8 = 0 OR a.deleted_at IS NULL)
             ORDER BY r.sequence",
        )
        .context("prepare record listing")?;
    let mut records = statement
        .query_map(
            params![
                filter.workspace,
                filter.actor,
                filter.session,
                filter.kind.map(Text),
                filter.start,
                filter.end,
                filter.after,
                filter.live_actors,
            ],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Text<RecordKind>>(2)?.0,
                    ActorReference {
                        id: row.get(3)?,
                        workspace: row.get(4)?,
                        name: row.get(5)?,
                    },
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .context("query records")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("read records")?
        .into_iter()
        .map(|(sequence, at, kind, actor, session_id, data)| {
            Ok(Record {
                sequence,
                at,
                kind,
                actor,
                session_id,
                data: serde_json::from_str(&data).context("decode record data")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    drop(statement);
    for record in &mut records {
        if record.kind == RecordKind::MessageAccepted
            && let Some(message_id) = record.data.get("messageId").and_then(|id| id.as_str())
            && let Some(message) = get_message(connection, message_id)?
            && let Some(data) = record.data.as_object_mut()
        {
            data.insert("waiting".to_owned(), serde_json::json!(message.waiting));
        }
    }
    Ok(records)
}

/// Records a fact about a Session, under the Workspace and Actor the Session belongs to.
pub(super) fn append_record(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    at: &str,
    kind: RecordKind,
    data: &serde_json::Value,
) -> Result<()> {
    let appended = transaction
        .execute(
            "INSERT INTO records (at, kind, actor_id, session_id, data)
             SELECT ?2, ?3, s.actor_id, s.id, ?4 FROM sessions s WHERE s.id = ?1",
            params![session_id, at, Text(kind), serde_json::to_string(data)?],
        )
        .context("append record")?;
    if appended != 1 {
        bail!("Session '{session_id}' was not found");
    }
    Ok(())
}

/// Records a fact about an Actor that no Session holds.
pub(super) fn append_actor_record(
    transaction: &rusqlite::Transaction<'_>,
    actor_id: &str,
    at: &str,
    kind: RecordKind,
    data: &serde_json::Value,
) -> Result<()> {
    transaction
        .execute(
            "INSERT INTO records (at, kind, actor_id, session_id, data)
             VALUES (?2, ?3, ?1, NULL, ?4)",
            params![actor_id, at, Text(kind), serde_json::to_string(data)?],
        )
        .context("append record")?;
    Ok(())
}
