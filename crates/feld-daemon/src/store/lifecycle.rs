use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};

use feld_core::model::{
    EndKind, LifecycleAcceptance, LifecycleAction, LifecycleActionKind, Observation,
    ObservationKind, Ownership, Placement, RecordKind, Requester, Session, end_kind_for_gone,
};
use feld_core::time::now;

use super::actor::live_actor;
use super::admission::RequestIdentity;
use super::error::SessionWriteError;
use super::message::unqueue_closed_session;
use super::record::append_record;
use super::session::{get_session, requester};
use super::sql::Text;
use super::{Notices, Store};

/// A restart whose stopped Session has closed, waiting for its next generation.
#[derive(Clone, Debug)]
pub struct PendingRestart {
    pub session_id: String,
    pub workspace: String,
    pub actor: String,
    pub spawner: Requester,
    pub spawner_session_id: Option<String>,
}

#[derive(Clone, Debug)]
pub enum AcceptedLifecycleAction {
    Already(Session),
    Resume(Session),
    NeedsProvider(Session),
    Completed(Session),
}

#[derive(Clone, Debug)]
pub struct ObservationUpdate {
    pub kind: ObservationKind,
    pub at: String,
    pub detail: Option<String>,
    pub exit_code: Option<i32>,
}

impl Store {
    pub fn record_observation(
        &self,
        session_id: &str,
        update: &ObservationUpdate,
        placement: Option<&Placement>,
    ) -> std::result::Result<Session, SessionWriteError> {
        self.write("observation record", |transaction, notices| {
            if let Some(placement) = placement {
                transaction
                    .execute(
                        "UPDATE sessions SET placement = ?2 WHERE id = ?1",
                        params![
                            session_id,
                            serde_json::to_string(placement).context("encode Session placement")?
                        ],
                    )
                    .context("update Session placement")?;
            }
            let recorded = record_observation(transaction, session_id, update)?;
            let session = get_session(transaction, session_id)?
                .ok_or_else(|| SessionWriteError::SessionNotFound(session_id.to_owned()))?;
            if recorded || placement.is_some() {
                notices.push("session", &session.id, Some(&session.workspace));
            }
            if recorded {
                notices.push("record", &session.id, Some(&session.workspace));
            }
            Ok(session)
        })
    }

    /// The launch left no execution behind: the observation is written, and the instant it
    /// proved the Session never began is kept beside `startSettled`.
    pub fn record_launch_failure(
        &self,
        session_id: &str,
        update: &ObservationUpdate,
    ) -> std::result::Result<Session, SessionWriteError> {
        self.write("launch failure record", |transaction, notices| {
            record_observation(transaction, session_id, update)?;
            transaction
                .execute(
                    "UPDATE sessions SET start_failed = ?2
                     WHERE id = ?1 AND start_settled IS NULL AND start_failed IS NULL",
                    params![session_id, update.at],
                )
                .context("record that the start never began")?;
            let session = get_session(transaction, session_id)?
                .ok_or_else(|| SessionWriteError::SessionNotFound(session_id.to_owned()))?;
            notices.push("session", &session.id, Some(&session.workspace));
            notices.push("record", &session.id, Some(&session.workspace));
            Ok(session)
        })
    }

    pub fn accept_lifecycle_action(
        &self,
        session_id: &str,
        action_kind: LifecycleActionKind,
        requester_session_id: Option<&str>,
    ) -> std::result::Result<AcceptedLifecycleAction, SessionWriteError> {
        self.write("lifecycle action", |transaction, notices| {
            let accepted =
                accept_lifecycle_in(transaction, session_id, action_kind, requester_session_id)?;
            notice_lifecycle(notices, &accepted);
            Ok(accepted)
        })
    }

    /// Accepts a restart: the stop of the Actor's current Session, and the request that
    /// starts its next generation once that Session has closed.
    pub fn accept_restart(
        &self,
        workspace: &str,
        actor: &str,
        identity: RequestIdentity<'_>,
    ) -> std::result::Result<AcceptedLifecycleAction, SessionWriteError> {
        self.write("restart", |transaction, notices| {
            let (actor_id, _) = live_actor(transaction, workspace, actor)?.ok_or_else(|| {
                SessionWriteError::ActorNotFound {
                    workspace: workspace.to_owned(),
                    actor: actor.to_owned(),
                }
            })?;
            let (current, ownership): (String, Ownership) = transaction
                .query_row(
                    "SELECT id, ownership FROM sessions WHERE actor_id = ?1 AND end IS NULL",
                    [&actor_id],
                    |row| Ok((row.get(0)?, row.get::<_, Text<_>>(1)?.0)),
                )
                .optional()
                .context("read Actor current Session for restart")?
                .ok_or_else(|| SessionWriteError::NotRunning(actor.to_owned()))?;
            if ownership != Ownership::Owned {
                return Err(SessionWriteError::NotOwned(current));
            }
            let accepted = accept_lifecycle_in(
                transaction,
                &current,
                LifecycleActionKind::Stop,
                identity.session_id,
            )?;
            transaction
                .execute(
                    "INSERT OR IGNORE INTO restarts
                     (session_id, spawner_actor_id, spawner_session_id, at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        current,
                        identity.requester.actor_id(),
                        identity.session_id,
                        now(),
                    ],
                )
                .context("record restart request")?;
            notice_lifecycle(notices, &accepted);
            Ok(accepted)
        })
    }

    /// The restarts whose stopped Session has closed, oldest first.
    pub fn pending_restarts(&self) -> Result<Vec<PendingRestart>> {
        self.read(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT r.session_id, a.workspace_key, a.name,
                            r.spawner_actor_id, spa.workspace_key, spa.name, r.spawner_session_id
                     FROM restarts r
                     JOIN sessions s ON s.id = r.session_id
                     JOIN actors a ON a.id = s.actor_id
                     LEFT JOIN actors spa ON spa.id = r.spawner_actor_id
                     WHERE s.end IS NOT NULL
                     ORDER BY r.at",
                )
                .context("prepare pending restarts")?;
            statement
                .query_map([], |row| {
                    Ok(PendingRestart {
                        session_id: row.get(0)?,
                        workspace: row.get(1)?,
                        actor: row.get(2)?,
                        spawner: requester(row.get(3)?, row.get(4)?, row.get(5)?),
                        spawner_session_id: row.get(6)?,
                    })
                })
                .context("query pending restarts")?
                .collect::<rusqlite::Result<Vec<_>>>()
                .context("read pending restarts")
        })
    }

    /// Drops a restart whose next generation cannot start.
    pub fn drop_restart(&self, session_id: &str) -> Result<()> {
        self.write("restart drop", |transaction, _| {
            transaction
                .execute("DELETE FROM restarts WHERE session_id = ?1", [session_id])
                .context("drop restart request")?;
            Ok(())
        })
    }

    pub fn finish_lifecycle_action(
        &self,
        session_id: &str,
        action_kind: LifecycleActionKind,
        attempt: Option<&str>,
        observation: &ObservationUpdate,
    ) -> std::result::Result<Session, SessionWriteError> {
        self.write("lifecycle result", |transaction, notices| {
            let mut action = read_lifecycle_action(transaction, session_id, action_kind)?
                .ok_or_else(|| anyhow::anyhow!("lifecycle action was not accepted"))?;
            action.attempt = attempt.map(str::to_owned);
            action.observed = Some(Observation {
                kind: observation.kind,
                at: observation.at.clone(),
                since: None,
                detail: observation.detail.clone(),
                exit_code: observation.exit_code,
            });
            write_lifecycle_action(transaction, session_id, action_kind, &action)?;
            record_observation(transaction, session_id, observation)?;
            if observation.kind.is_unavailable() {
                close_unknown(transaction, session_id, &observation.at)?;
            }
            let session = get_session(transaction, session_id)?
                .ok_or_else(|| SessionWriteError::SessionNotFound(session_id.to_owned()))?;
            notices.push("session", &session.id, Some(&session.workspace));
            notices.push("record", &session.id, Some(&session.workspace));
            Ok(session)
        })
    }
}

fn notice_lifecycle(notices: &mut Notices, accepted: &AcceptedLifecycleAction) {
    let session = match accepted {
        AcceptedLifecycleAction::Already(_) | AcceptedLifecycleAction::Resume(_) => return,
        AcceptedLifecycleAction::NeedsProvider(session)
        | AcceptedLifecycleAction::Completed(session) => session,
    };
    notices.push("session", &session.id, Some(&session.workspace));
    notices.push("record", &session.id, Some(&session.workspace));
}

/// The column that keeps one kind of lifecycle action.
fn action_column(kind: LifecycleActionKind) -> &'static str {
    match kind {
        LifecycleActionKind::Stop => "stop_record",
        LifecycleActionKind::Kill => "kill_record",
    }
}

/// Writes the latest observation and returns whether it committed a record.
/// A re-observation whose kind repeats the last one only advances
/// `observation.at`, which is not a record and must not wake readers.
fn record_observation(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    update: &ObservationUpdate,
) -> Result<bool> {
    let (last_kind, end): (Option<ObservationKind>, Option<String>) = transaction
        .query_row(
            "SELECT observation_kind, end FROM sessions WHERE id = ?1",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, Option<Text<_>>>(0)?.map(|Text(kind)| kind),
                    row.get(1)?,
                ))
            },
        )
        .optional()
        .context("read Session before observation")?
        .ok_or_else(|| anyhow::anyhow!("Session '{session_id}' was not found"))?;
    let mut recorded = false;
    if last_kind != Some(update.kind) {
        recorded = true;
        append_record(
            transaction,
            session_id,
            &update.at,
            RecordKind::Observation,
            &serde_json::json!({
                "kind": update.kind,
                "detail": update.detail,
                "exitCode": update.exit_code,
            }),
        )?;
    }
    transaction
        .execute(
            "UPDATE sessions SET observation_kind = ?2, observation_at = ?3,
                    observation_since = CASE WHEN ?7 THEN ?3 ELSE observation_since END,
                    observation_detail = ?4, observation_exit_code = ?5,
                    start_settled = CASE
                      WHEN start_settled IS NULL AND ?6 THEN ?3
                      ELSE start_settled
                    END
             WHERE id = ?1",
            params![
                session_id,
                Text(update.kind),
                update.at,
                update.detail,
                update.exit_code,
                update.kind.settles_start(),
                recorded,
            ],
        )
        .context("update latest observation")?;
    if update.kind.ends_execution() && end.is_none() {
        recorded = true;
        close_gone(transaction, session_id, &update.at, update.exit_code)?;
    }
    Ok(recorded)
}

/// Accepts a stop or kill of one Session inside the caller's transaction.
fn accept_lifecycle_in(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    action_kind: LifecycleActionKind,
    requester_session_id: Option<&str>,
) -> std::result::Result<AcceptedLifecycleAction, SessionWriteError> {
    if let Some(requester) = requester_session_id {
        let requester_is_open = transaction
            .query_row(
                "SELECT 1 FROM sessions WHERE id = ?1 AND end IS NULL",
                [requester],
                |_| Ok(()),
            )
            .optional()
            .context("revalidate lifecycle requester")?
            .is_some();
        if !requester_is_open {
            return Err(SessionWriteError::SessionClosed(requester.to_owned()));
        }
    }
    let current = get_session(transaction, session_id)?
        .ok_or_else(|| SessionWriteError::SessionNotFound(session_id.to_owned()))?;
    if current.end.is_some() {
        return Err(SessionWriteError::SessionClosed(session_id.to_owned()));
    }
    match current.lifecycle.accept(action_kind) {
        LifecycleAcceptance::Already => return Ok(AcceptedLifecycleAction::Already(current)),
        LifecycleAcceptance::Resume => return Ok(AcceptedLifecycleAction::Resume(current)),
        LifecycleAcceptance::Accept => {}
    }
    if current.ownership != Ownership::Owned {
        return Err(SessionWriteError::NotOwned(session_id.to_owned()));
    }
    let action =
        LifecycleAction::accept(&now(), requester_session_id, current.observation.as_ref());
    write_lifecycle_action(transaction, session_id, action_kind, &action)?;
    append_record(
        transaction,
        session_id,
        &action.accepted,
        action_kind.record_kind(),
        &serde_json::json!({ "requesterSessionId": requester_session_id }),
    )?;
    // An action settled on acceptance found nothing to reach, so the Session closes.
    if !action.is_pending() {
        close_unknown(transaction, session_id, &action.accepted)?;
    }
    let session = get_session(transaction, session_id)?
        .ok_or_else(|| anyhow::anyhow!("accepted Session was not found"))?;
    Ok(if action.is_pending() {
        AcceptedLifecycleAction::NeedsProvider(session)
    } else {
        AcceptedLifecycleAction::Completed(session)
    })
}

/// Closes an open Session, recording how it ended.
fn close(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    at: &str,
    end_kind: EndKind,
    exit_code: Option<i32>,
) -> Result<()> {
    let changed = transaction
        .execute(
            "UPDATE sessions SET end = ?2, end_kind = ?3, exit_code = ?4
             WHERE id = ?1 AND end IS NULL",
            params![session_id, at, Text(end_kind), exit_code],
        )
        .context("close Session")?;
    if changed == 1 {
        // A Session closed as `unknown` may leave its Runtime running, queue and all, so only
        // one whose execution was seen gone gives its queue up.
        if end_kind != EndKind::Unknown {
            unqueue_closed_session(transaction, session_id)?;
        }
        append_record(
            transaction,
            session_id,
            at,
            RecordKind::SessionEnded,
            &serde_json::json!({ "endKind": end_kind, "exitCode": exit_code }),
        )?;
    }
    Ok(())
}

pub(super) fn close_unknown(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    at: &str,
) -> Result<()> {
    close(transaction, session_id, at, EndKind::Unknown, None)
}

/// Closes a Session whose execution is gone with the exit code last observed.
pub(super) fn close_exited(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    at: &str,
) -> Result<()> {
    let exit_code = transaction
        .query_row(
            "SELECT observation_exit_code FROM sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .context("read gone Session before admission")?;
    close_gone(transaction, session_id, at, exit_code)
}

fn close_gone(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    at: &str,
    exit_code: Option<i32>,
) -> Result<()> {
    let requested = transaction
        .query_row(
            "SELECT stop_record IS NOT NULL OR kill_record IS NOT NULL
             FROM sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .context("read Session lifecycle before close")?;
    close(
        transaction,
        session_id,
        at,
        end_kind_for_gone(requested),
        exit_code,
    )
}

fn read_lifecycle_action(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    action_kind: LifecycleActionKind,
) -> Result<Option<LifecycleAction>> {
    let statement = format!(
        "SELECT {} FROM sessions WHERE id = ?1",
        action_column(action_kind)
    );
    let record = transaction
        .query_row(&statement, [session_id], |row| {
            row.get::<_, Option<String>>(0)
        })
        .optional()
        .context("read lifecycle action")?
        .flatten();
    record
        .map(|value| serde_json::from_str(&value).context("decode lifecycle action"))
        .transpose()
}

fn write_lifecycle_action(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    action_kind: LifecycleActionKind,
    action: &LifecycleAction,
) -> Result<()> {
    let statement = format!(
        "UPDATE sessions SET {} = ?2 WHERE id = ?1",
        action_column(action_kind)
    );
    let changed = transaction
        .execute(
            &statement,
            params![
                session_id,
                serde_json::to_string(action).context("encode lifecycle action")?
            ],
        )
        .context("write lifecycle action")?;
    if changed != 1 {
        bail!("Session '{session_id}' was not found");
    }
    Ok(())
}
