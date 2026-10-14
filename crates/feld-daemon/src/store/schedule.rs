use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use feld_core::model::{RecordKind, Schedule};

use super::Store;
use super::actor::{live_actor, read_actor_rows};
use super::sql::Text;

#[derive(Clone, Debug)]
pub struct ScheduledActor {
    pub workspace: String,
    pub actor: String,
    pub schedule: Schedule,
    /// When the Actor, and so its Schedule, last changed.
    pub updated_at: String,
}

impl Store {
    pub fn get_schedule(&self, workspace: &str, name: &str) -> Result<Option<Schedule>> {
        self.read(
            |connection| match live_actor(connection, workspace, name)? {
                Some((id, _)) => get_schedule(connection, &id),
                None => Ok(None),
            },
        )
    }

    /// Whether the tick of the Actor planned at `planned` is settled: it fired or was recorded
    /// as missed.
    pub fn tick_recorded(&self, actor_id: &str, planned: &str) -> Result<bool> {
        self.read(|connection| tick_recorded(connection, actor_id, planned))
    }

    pub fn list_enabled_schedules(&self) -> Result<Vec<ScheduledActor>> {
        self.read(|connection| {
            Ok(read_actor_rows(
                connection,
                "WHERE schedule_enabled = 1 AND deleted_at IS NULL ORDER BY workspace_key, name",
                [],
            )?
            .into_iter()
            .filter_map(|row| {
                Some(ScheduledActor {
                    workspace: row.workspace,
                    actor: row.name,
                    schedule: row.schedule?,
                    updated_at: row.updated_at,
                })
            })
            .collect())
        })
    }
}

/// Whether the tick planned at `planned` is settled: it started a Session, admission refused
/// it, or it was recorded as missed.
pub(super) fn tick_recorded(
    connection: &Connection,
    actor_id: &str,
    planned: &str,
) -> Result<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM sessions WHERE actor_id = ?1 AND planned = ?2
             UNION ALL
             SELECT 1 FROM records WHERE actor_id = ?1 AND kind IN (?3, ?4)
               AND json_extract(data, '$.planned') = ?2
             LIMIT 1",
            params![
                actor_id,
                planned,
                Text(RecordKind::ScheduleSkipped),
                Text(RecordKind::ScheduleMissed)
            ],
            |_| Ok(()),
        )
        .optional()
        .context("check scheduled tick idempotency")?
        .is_some())
}

pub(super) fn get_schedule(connection: &Connection, actor_id: &str) -> Result<Option<Schedule>> {
    Ok(read_actor_rows(connection, "WHERE id = ?1", [actor_id])?
        .into_iter()
        .next()
        .and_then(|row| row.schedule))
}

pub(super) fn write_schedule(
    transaction: &rusqlite::Transaction<'_>,
    actor_id: &str,
    schedule: Option<&Schedule>,
) -> Result<()> {
    transaction
        .execute(
            "UPDATE actors SET schedule_cron = ?2, schedule_timezone = ?3, schedule_enabled = ?4,
                    schedule_created_at = ?5, schedule_paused_at = ?6, schedule_enabled_at = ?7,
                    schedule_revision = schedule_revision + 1
             WHERE id = ?1",
            params![
                actor_id,
                schedule.map(|schedule| &schedule.cron),
                schedule.map(|schedule| &schedule.timezone),
                schedule.map(|schedule| schedule.enabled),
                schedule.map(|schedule| &schedule.created_at),
                schedule.and_then(|schedule| schedule.paused_at.as_ref()),
                schedule.and_then(|schedule| schedule.enabled_at.as_ref()),
            ],
        )
        .context("write Schedule")?;
    Ok(())
}
