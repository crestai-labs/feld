use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Params, params};

use feld_core::model::{
    ActorKind, EndKind, Execution, Observation, ObservationKind, Outcome, Ownership, Requester,
    Session, SessionLifecycle, SpawnOrigin, StartKind,
};
use feld_core::plugin::protocol::RuntimeIdentity;
use feld_core::time;

use super::Store;
use super::error::SessionWriteError;
use super::sql::Text;

/// What `Store::query_sessions` narrows to; each `None` leaves that side open.
#[derive(Default)]
pub struct SessionFilter<'a> {
    pub workspace: Option<&'a str>,
    pub actor: Option<&'a str>,
    pub kind: Option<ActorKind>,
    pub current: bool,
    pub start: Option<&'a str>,
    pub end: Option<&'a str>,
}

impl Store {
    /// The Session an Actor ran as one generation, without reading its history.
    pub fn session_of_generation(&self, actor_id: &str, generation: u64) -> Result<Option<String>> {
        self.read(|connection| {
            connection
                .query_row(
                    "SELECT id FROM sessions WHERE actor_id = ?1 AND generation = ?2",
                    params![actor_id, generation],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .context("read Session of generation")
        })
    }

    pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
        self.read(|connection| get_session(connection, id))
    }

    pub fn runtime_identity(&self, id: &str) -> Result<Option<RuntimeIdentity>> {
        self.read(|connection| {
            connection
                .query_row(
                    "SELECT runtime_identity FROM sessions WHERE id = ?1",
                    [id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()
                .context("read Runtime identity")?
                .flatten()
                .map(|value| serde_json::from_str(&value).context("decode Runtime identity"))
                .transpose()
        })
    }

    pub fn list_sessions(&self, workspace: Option<&str>) -> Result<Vec<Session>> {
        self.read(|connection| {
            read_session_rows(
                connection,
                &format!("WHERE {IN_WORKSPACE} ORDER BY s.start, s.id"),
                [workspace],
            )
        })
    }

    /// The Sessions the filter picks, newest first. `actor` names a live Actor, and a range
    /// keeps the Sessions that overlap it.
    pub fn query_sessions(&self, filter: &SessionFilter<'_>) -> Result<Vec<Session>> {
        let sessions = self.read(|connection| {
            read_session_rows(
                connection,
                &format!(
                    "WHERE {IN_WORKSPACE}
                       AND (?2 IS NULL OR (a.name = ?2 AND a.deleted_at IS NULL))
                       AND (?3 IS NULL OR a.kind = ?3)
                       AND (?4 = 0 OR s.end IS NULL)
                     ORDER BY s.start DESC, s.id DESC"
                ),
                params![
                    filter.workspace,
                    filter.actor,
                    filter.kind.map(Text),
                    filter.current
                ],
            )
        })?;
        Ok(sessions
            .into_iter()
            .filter(|session| {
                session_overlaps(
                    &session.start,
                    session.end.as_deref(),
                    filter.start,
                    filter.end,
                )
            })
            .collect())
    }

    pub fn list_open_sessions(&self) -> Result<Vec<Session>> {
        self.read(|connection| {
            read_session_rows(connection, "WHERE s.end IS NULL ORDER BY s.start", [])
        })
    }

    pub fn record_runtime_identity(
        &self,
        session_id: &str,
        token: &str,
        identity: &RuntimeIdentity,
    ) -> std::result::Result<Session, SessionWriteError> {
        self.write("Runtime identity record", |transaction, notices| {
            let existing = transaction
                .query_row(
                    "SELECT runtime_identity, end, token, execution
                     FROM sessions WHERE id = ?1",
                    [session_id],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()
                .context("read Runtime identity")?
                .ok_or_else(|| SessionWriteError::SessionNotFound(session_id.to_owned()))?;
            if existing.1.is_some() || existing.2 != token {
                return Err(SessionWriteError::InvalidToken);
            }
            let execution: Execution =
                serde_json::from_str(&existing.3).context("decode Session execution")?;
            if execution.execution_type != "agent-runtime" {
                return Err(SessionWriteError::NotAgentRuntime(session_id.to_owned()));
            }
            // The session id and the transcript file location identify the Runtime and stay fixed.
            // `model` is the Runtime's current fact: a deliverer that does not know it (an
            // attach before the Runtime's first response) leaves it null, and the hook fills it
            // in or moves it later.
            let recorded = match existing.0 {
                Some(value) => {
                    let recorded: RuntimeIdentity =
                        serde_json::from_str(&value).context("decode recorded Runtime identity")?;
                    if recorded.session_id != identity.session_id
                        || recorded.transcript_path != identity.transcript_path
                    {
                        return Err(SessionWriteError::RuntimeIdentityConflict {
                            session: session_id.to_owned(),
                            recorded_session_id: recorded.session_id,
                            recorded_transcript_path: recorded.transcript_path,
                            requested_session_id: identity.session_id.clone(),
                            requested_transcript_path: identity.transcript_path.clone(),
                        });
                    }
                    if identity.model.is_null() || recorded.model == identity.model {
                        None
                    } else {
                        Some(identity.clone())
                    }
                }
                None => Some(identity.clone()),
            };
            if let Some(identity) = recorded {
                let encoded =
                    serde_json::to_string(&identity).context("encode Runtime identity")?;
                transaction
                    .execute(
                        "UPDATE sessions SET runtime_identity = ?2 WHERE id = ?1",
                        params![session_id, encoded],
                    )
                    .context("record Runtime identity")?;
            }
            let session = get_session(transaction, session_id)?
                .ok_or_else(|| SessionWriteError::SessionNotFound(session_id.to_owned()))?;
            notices.push("session", &session.id, Some(&session.workspace));
            notices.push("log", &session.id, Some(&session.workspace));
            Ok(session)
        })
    }
}

/// Whether a Session from `session_start` to `session_end` (open when `None`) overlaps the
/// half-open range, each side open when `None`.
pub fn session_overlaps(
    session_start: &str,
    session_end: Option<&str>,
    range_start: Option<&str>,
    range_end: Option<&str>,
) -> bool {
    let before = |left: &str, right: &str| match (time::parse(left), time::parse(right)) {
        (Ok(left), Ok(right)) => left < right,
        _ => false,
    };
    range_end.is_none_or(|end| before(session_start, end))
        && range_start.is_none_or(|start| session_end.is_none_or(|end| before(start, end)))
}

/// Keeps the Sessions of the Actors of one Workspace, or of every Workspace when `?1` is null.
const IN_WORKSPACE: &str = "(?1 IS NULL OR a.workspace_id = (SELECT id FROM workspaces WHERE key = ?1 AND deleted_at IS NULL))";

/// Reads Sessions with their Actor, as `s` and `a`, and the record that spawned them.
const SESSION_SELECT: &str =
    "SELECT s.id, a.workspace_key, s.actor_id, a.name, a.kind, s.generation,
        s.start, s.end, s.start_kind, s.ownership, s.end_kind,
        s.exit_code, s.planned, s.execution, s.placement, s.start_settled,
        s.start_failed, s.stop_record, s.kill_record,
        s.observation_kind, s.observation_at,
        s.observation_detail, s.observation_exit_code, s.runtime_identity,
        s.telemetry_collector_available, s.telemetry_runtime_prepared,
        sp.session_id, spa.id, spa.workspace_key, spa.name,
        json_extract(sp.data, '$.spawnerSessionId'), s.observation_since
     FROM sessions s
     JOIN actors a ON a.id = s.actor_id
     LEFT JOIN records sp ON sp.session_id = s.id AND sp.kind = 'spawned'
     LEFT JOIN actors spa ON spa.id = json_extract(sp.data, '$.spawner.id')";

struct StoredSessionRow {
    id: String,
    workspace: String,
    actor_id: String,
    actor: String,
    actor_kind: ActorKind,
    generation: u64,
    start: String,
    end: Option<String>,
    start_kind: StartKind,
    ownership: Ownership,
    end_kind: EndKind,
    exit_code: Option<i32>,
    planned: Option<String>,
    execution: String,
    placement: String,
    start_settled: Option<String>,
    start_failed: Option<String>,
    stop_record: Option<String>,
    kill_record: Option<String>,
    observation_kind: Option<ObservationKind>,
    observation_at: Option<String>,
    observation_since: Option<String>,
    observation_detail: Option<String>,
    observation_exit_code: Option<i32>,
    runtime_identity: Option<String>,
    telemetry_collector_available: bool,
    telemetry_runtime_prepared: bool,
    spawn_session_id: Option<String>,
    spawner_actor_id: Option<String>,
    spawner_workspace: Option<String>,
    spawner_name: Option<String>,
    spawner_session_id: Option<String>,
}

fn stored_session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredSessionRow> {
    Ok(StoredSessionRow {
        id: row.get(0)?,
        workspace: row.get(1)?,
        actor_id: row.get(2)?,
        actor: row.get(3)?,
        actor_kind: row.get::<_, Text<_>>(4)?.0,
        generation: row.get(5)?,
        start: row.get(6)?,
        end: row.get(7)?,
        start_kind: row.get::<_, Text<_>>(8)?.0,
        ownership: row.get::<_, Text<_>>(9)?.0,
        end_kind: row.get::<_, Text<_>>(10)?.0,
        exit_code: row.get(11)?,
        planned: row.get(12)?,
        execution: row.get(13)?,
        placement: row.get(14)?,
        start_settled: row.get(15)?,
        start_failed: row.get(16)?,
        stop_record: row.get(17)?,
        kill_record: row.get(18)?,
        observation_kind: row.get::<_, Option<Text<_>>>(19)?.map(|Text(kind)| kind),
        observation_at: row.get(20)?,
        observation_since: row.get(31)?,
        observation_detail: row.get(21)?,
        observation_exit_code: row.get(22)?,
        runtime_identity: row.get(23)?,
        telemetry_collector_available: row.get(24)?,
        telemetry_runtime_prepared: row.get(25)?,
        spawn_session_id: row.get(26)?,
        spawner_actor_id: row.get(27)?,
        spawner_workspace: row.get(28)?,
        spawner_name: row.get(29)?,
        spawner_session_id: row.get(30)?,
    })
}

/// The Sessions `suffix` picks from [`SESSION_SELECT`].
pub(super) fn read_session_rows<P: Params>(
    connection: &Connection,
    suffix: &str,
    params: P,
) -> Result<Vec<Session>> {
    connection
        .prepare(&format!("{SESSION_SELECT} {suffix}"))
        .context("prepare Session read")?
        .query_map(params, stored_session_row)
        .context("query Sessions")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("read Sessions")?
        .into_iter()
        .map(session_from_stored)
        .collect()
}

pub(super) fn get_session(connection: &Connection, id: &str) -> Result<Option<Session>> {
    Ok(read_session_rows(connection, "WHERE s.id = ?1", [id])?
        .into_iter()
        .next())
}

fn session_from_stored(row: StoredSessionRow) -> Result<Session> {
    let execution = {
        let mut execution: Execution =
            serde_json::from_str(&row.execution).context("decode Session execution")?;
        execution.telemetry_collector_available = Some(row.telemetry_collector_available);
        execution.telemetry_runtime_prepared = Some(row.telemetry_runtime_prepared);
        execution
    };
    let observation = match (row.observation_kind, row.observation_at) {
        (Some(kind), Some(at)) => Some(Observation {
            kind,
            at,
            since: row.observation_since,
            detail: row.observation_detail,
            exit_code: row.observation_exit_code,
        }),
        _ => None,
    };
    let mut session = Session {
        id: row.id,
        workspace: row.workspace,
        actor_id: row.actor_id,
        actor: row.actor,
        actor_kind: row.actor_kind,
        generation: row.generation,
        start: row.start,
        end: row.end,
        start_kind: row.start_kind,
        ownership: row.ownership,
        end_kind: row.end_kind,
        exit_code: row.exit_code,
        planned: row.planned,
        execution: execution.clone(),
        placement: serde_json::from_str(&row.placement).context("decode Session placement")?,
        lifecycle: SessionLifecycle {
            start_settled: row.start_settled,
            start_failed: row.start_failed,
            stop: row
                .stop_record
                .map(|value| serde_json::from_str(&value).context("decode Session stop record"))
                .transpose()?,
            kill: row
                .kill_record
                .map(|value| serde_json::from_str(&value).context("decode Session kill record"))
                .transpose()?,
        },
        observation,
        model: row
            .runtime_identity
            .map(|value| {
                serde_json::from_str::<RuntimeIdentity>(&value)
                    // The display name of the model is the Runtime's own label, which the
                    // Runtime Adapter's manifest gave the Session.
                    .map(|identity| serde_json::json!([execution.label, identity.model]))
                    .context("decode Runtime identity")
            })
            .transpose()?,
        spawned_by: row.spawn_session_id.map(|_| SpawnOrigin {
            spawner: requester(
                row.spawner_actor_id,
                row.spawner_workspace,
                row.spawner_name,
            ),
            spawner_session_id: row.spawner_session_id,
        }),
        outcome: Outcome::Running,
        turn: None,
        last_entry_at: None,
    };
    session.derive();
    Ok(session)
}

pub(super) fn list_sessions_for_actor(
    connection: &Connection,
    actor_id: &str,
) -> Result<Vec<Session>> {
    read_session_rows(
        connection,
        "WHERE s.actor_id = ?1 ORDER BY s.generation DESC",
        [actor_id],
    )
}

/// A requester read back from the Actor columns a record keeps, which name the Admin
/// when they are empty.
pub(super) fn requester(
    id: Option<String>,
    workspace: Option<String>,
    name: Option<String>,
) -> Requester {
    match (id, workspace, name) {
        (Some(id), Some(workspace), Some(name)) => Requester::Actor {
            id,
            workspace,
            name,
        },
        _ => Requester::Admin,
    }
}

#[cfg(test)]
mod tests {
    use super::session_overlaps;

    #[test]
    fn session_overlap_is_half_open() {
        let session_start = "2026-09-15T01:00:02Z";
        let session_end = "2026-09-15T01:00:04Z";
        assert!(session_overlaps(
            session_start,
            Some(session_end),
            Some("2026-09-15T01:00:01.000Z"),
            Some("2026-09-15T01:00:03.000Z")
        ));
        assert!(!session_overlaps(
            session_start,
            Some(session_end),
            None,
            Some(session_start)
        ));
        assert!(!session_overlaps(
            session_start,
            Some(session_end),
            Some(session_end),
            None
        ));
    }
}
