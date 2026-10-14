use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use feld_core::id::{fresh_id, fresh_token};
use feld_core::model::{
    ActorKind, Admission, EndKind, Execution, LaunchSpec, ObservationKind, Ownership, Placement,
    RecordKind, Requester, Schedule, ScheduleSkip, Session, StartKind, admission,
};
use feld_core::time::now;

use super::actor::{live_actor, read_actor_rows};
use super::error::SessionWriteError;
use super::lifecycle::{PendingRestart, close_exited, close_unknown};
use super::record::append_record;
use super::schedule::tick_recorded;
use super::session::get_session;
use super::sql::Text;
use super::workspace::live_workspace_id;
use super::{Notices, Store};

#[derive(Clone, Debug)]
pub struct PendingSession {
    pub session: Session,
    pub token: String,
    pub replaced_session: Option<Session>,
}

/// What an attach declares about the Runtime it found, which the Session's `execution`
/// records. The label is the Runtime Adapter manifest's.
#[derive(Clone, Copy, Debug)]
pub struct AttachedExecution<'a> {
    pub runtime: &'a str,
    pub label: &'a str,
}

#[derive(Clone, Debug)]
pub struct AttachAdmission {
    pub pending: PendingSession,
    pub reattached: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct RequestIdentity<'a> {
    pub requester: &'a Requester,
    pub session_id: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub enum ScheduledAdmission {
    Started(Box<PendingSession>),
    Skipped(ScheduleSkip),
    Already,
    Stale,
}

pub struct ScheduledAdmissionRequest<'a> {
    pub launch: &'a LaunchSpec,
    pub schedule: &'a Schedule,
    pub planned: &'a str,
    pub accepted: &'a str,
    pub execution: &'a Execution,
    pub placement: &'a Placement,
}

enum OwnedStart<'a> {
    Start(RequestIdentity<'a>),
    /// The next generation of a restart, whose request was accepted when the stop was.
    Restart {
        identity: RequestIdentity<'a>,
        stopped: &'a str,
    },
    Schedule {
        expected: &'a Schedule,
        planned: &'a str,
        accepted: &'a str,
    },
}

impl Store {
    /// Whether admission would start a Session for this start request, judged without
    /// writing anything. The controller asks before it has the Provider place the start, so
    /// that only a start admission takes is placed; admission judges again when it writes.
    pub fn start_admissible(
        &self,
        workspace: &str,
        actor: &str,
        expected_launch: &LaunchSpec,
        identity: RequestIdentity<'_>,
    ) -> bool {
        self.admissible(
            workspace,
            actor,
            expected_launch,
            &OwnedStart::Start(identity),
        )
    }

    /// [`Self::start_admissible`] for the next generation of a restart.
    pub fn restart_admissible(
        &self,
        restart: &PendingRestart,
        expected_launch: &LaunchSpec,
    ) -> bool {
        self.admissible(
            &restart.workspace,
            &restart.actor,
            expected_launch,
            &OwnedStart::Restart {
                identity: RequestIdentity {
                    requester: &restart.spawner,
                    session_id: restart.spawner_session_id.as_deref(),
                },
                stopped: &restart.session_id,
            },
        )
    }

    /// [`Self::start_admissible`] for a Schedule tick.
    pub fn tick_admissible(
        &self,
        workspace: &str,
        actor: &str,
        expected_launch: &LaunchSpec,
        schedule: &Schedule,
        planned: &str,
        accepted: &str,
    ) -> bool {
        self.admissible(
            workspace,
            actor,
            expected_launch,
            &OwnedStart::Schedule {
                expected: schedule,
                planned,
                accepted,
            },
        )
    }

    /// A refusal or a failed read counts as not admissible: the start goes on to admission
    /// unplaced, and admission answers it.
    fn admissible(
        &self,
        workspace: &str,
        actor: &str,
        expected_launch: &LaunchSpec,
        source: &OwnedStart<'_>,
    ) -> bool {
        self.read(|connection| judge_owned(connection, workspace, actor, expected_launch, source))
            .is_ok_and(|judgement| matches!(judgement, Judgement::Admit { .. }))
    }

    pub fn admit_start(
        &self,
        workspace: &str,
        actor: &str,
        expected_launch: &LaunchSpec,
        execution: &Execution,
        placement: &Placement,
        identity: RequestIdentity<'_>,
    ) -> std::result::Result<PendingSession, SessionWriteError> {
        match self.admit_requested(
            workspace,
            actor,
            expected_launch,
            execution,
            placement,
            OwnedStart::Start(identity),
        )? {
            Some(pending) => Ok(pending),
            None => Err(SessionWriteError::LaunchChanged(actor.to_owned())),
        }
    }

    /// Admits the next generation of a restart whose stopped Session has closed. `None`
    /// means the restart no longer applies: another generation followed the stopped one,
    /// and the restart is dropped.
    pub fn admit_restart(
        &self,
        restart: &PendingRestart,
        expected_launch: &LaunchSpec,
        execution: &Execution,
        placement: &Placement,
    ) -> std::result::Result<Option<PendingSession>, SessionWriteError> {
        self.admit_requested(
            &restart.workspace,
            &restart.actor,
            expected_launch,
            execution,
            placement,
            OwnedStart::Restart {
                identity: RequestIdentity {
                    requester: &restart.spawner,
                    session_id: restart.spawner_session_id.as_deref(),
                },
                stopped: &restart.session_id,
            },
        )
    }

    fn admit_requested(
        &self,
        workspace: &str,
        actor: &str,
        expected_launch: &LaunchSpec,
        execution: &Execution,
        placement: &Placement,
        source: OwnedStart<'_>,
    ) -> std::result::Result<Option<PendingSession>, SessionWriteError> {
        match self.admit_owned(
            workspace,
            actor,
            expected_launch,
            execution,
            placement,
            source,
        )? {
            ScheduledAdmission::Started(pending) => Ok(Some(*pending)),
            ScheduledAdmission::Stale => Ok(None),
            ScheduledAdmission::Skipped(_) | ScheduledAdmission::Already => {
                Err(SessionWriteError::LaunchChanged(actor.to_owned()))
            }
        }
    }

    pub fn admit_scheduled(
        &self,
        workspace: &str,
        actor: &str,
        request: ScheduledAdmissionRequest<'_>,
    ) -> std::result::Result<ScheduledAdmission, SessionWriteError> {
        self.admit_owned(
            workspace,
            actor,
            request.launch,
            request.execution,
            request.placement,
            OwnedStart::Schedule {
                expected: request.schedule,
                planned: request.planned,
                accepted: request.accepted,
            },
        )
    }

    fn admit_owned(
        &self,
        workspace: &str,
        actor: &str,
        expected_launch: &LaunchSpec,
        execution: &Execution,
        placement: &Placement,
        source: OwnedStart<'_>,
    ) -> std::result::Result<ScheduledAdmission, SessionWriteError> {
        self.write("Session admission", |transaction, notices| {
            let admitted = admit_owned_in(
                transaction,
                workspace,
                actor,
                expected_launch,
                execution,
                placement,
                source,
            )?;
            match &admitted {
                ScheduledAdmission::Started(pending) => {
                    notice_started(notices, workspace, actor, pending);
                }
                ScheduledAdmission::Skipped(skip) => {
                    notices.push("actor", actor, Some(workspace));
                    notices.push("session", &skip.current_session_id, Some(workspace));
                    notices.push("record", &skip.current_session_id, Some(workspace));
                }
                ScheduledAdmission::Already | ScheduledAdmission::Stale => {}
            }
            Ok(admitted)
        })
        .inspect(|admitted| {
            if let ScheduledAdmission::Started(pending) = admitted {
                self.keep_secret(&pending.token);
            }
        })
    }

    pub fn admit_attach(
        &self,
        workspace: &str,
        actor: &str,
        placement: &Placement,
        execution: AttachedExecution<'_>,
        telemetry_collector_available: bool,
        telemetry_runtime_prepared: bool,
    ) -> std::result::Result<AttachAdmission, SessionWriteError> {
        let runtime = placement
            .process()
            .context("attached Placement names no Runtime process")?;
        self.write("attach admission", |transaction, notices| {
            let Some(workspace_id) = live_workspace_id(transaction, workspace)? else {
                return Err(SessionWriteError::WorkspaceNotFound(workspace.to_owned()));
            };

            let actor_id = match live_actor(transaction, workspace, actor)? {
                Some((id, ActorKind::Agent)) => id,
                Some(_) => return Err(SessionWriteError::AttachToJob(actor.to_owned())),
                None => {
                    let id = fresh_id("actor");
                    transaction
                        .execute(
                            "INSERT INTO actors (id, workspace_id, workspace_key, name, kind,
                                                 launch_spec, created_at, updated_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?6)",
                            params![
                                id,
                                workspace_id,
                                workspace,
                                actor,
                                Text(ActorKind::Agent),
                                now()
                            ],
                        )
                        .context("create attached Actor")?;
                    id
                }
            };

            let mut statement = transaction
                .prepare(
                    "SELECT s.id, s.actor_id, a.name, s.placement
                     FROM sessions s JOIN actors a ON a.id = s.actor_id WHERE s.end IS NULL",
                )
                .context("prepare attached Runtime lookup")?;
            let open = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .context("query attached Runtime lookup")?
                .collect::<rusqlite::Result<Vec<_>>>()
                .context("read attached Runtime lookup")?;
            drop(statement);
            for (session_id, session_actor_id, session_actor, encoded) in open {
                let stored: Placement =
                    serde_json::from_str(&encoded).context("decode Placement")?;
                if stored.runtime_process().as_ref() == Some(&runtime) {
                    if session_actor_id != actor_id {
                        return Err(SessionWriteError::RuntimeAlreadyAttached {
                            actor: session_actor,
                            session: session_id,
                        });
                    }
                    // An `owned` Session keeps the token Feld started it with, and admission
                    // refuses its running Session below.
                    if stored.process().is_none() {
                        break;
                    }
                    let token = fresh_token();
                    transaction
                        .execute(
                            "INSERT OR IGNORE INTO session_tokens (session_id, token)
                             SELECT id, token FROM sessions WHERE id = ?1",
                            [&session_id],
                        )
                        .context("retain previous attached Session token")?;
                    transaction
                        .execute(
                            "INSERT OR IGNORE INTO session_tokens (session_id, token)
                             VALUES (?1, ?2)",
                            params![session_id, token],
                        )
                        .context("retain rotated attached Session token")?;
                    // The Runtime identity goes with the old token: the Runtime may have
                    // moved to another of its own sessions since (a new Codex thread), and the
                    // identity delivered with the new token is the one it runs now.
                    transaction
                        .execute(
                            "UPDATE sessions
                             SET token = ?2,
                                 telemetry_runtime_prepared = MIN(telemetry_runtime_prepared, ?3),
                                 runtime_identity = NULL
                             WHERE id = ?1",
                            params![session_id, token, telemetry_runtime_prepared],
                        )
                        .context("rotate attached Session token")?;
                    let session = get_session(transaction, &session_id)?
                        .context("reattached Session disappeared")?;
                    notices.push("actor", actor, Some(workspace));
                    notices.push("session", &session.id, Some(workspace));
                    return Ok(AttachAdmission {
                        pending: PendingSession {
                            session,
                            token,
                            replaced_session: None,
                        },
                        reattached: true,
                    });
                }
            }

            let accepted = now();
            let replaced_session = replace_current(transaction, actor, &actor_id, &accepted)?;
            let generation = next_generation(transaction, &actor_id)?;
            let session_id = fresh_id("session");
            let token = fresh_token();
            let execution = serde_json::to_string(&Execution {
                execution_type: "agent-runtime".to_owned(),
                runtime: Some(execution.runtime.to_owned()),
                label: execution.label.to_owned(),
                command: None,
                ai: true,
                telemetry_collector_available: Some(telemetry_collector_available),
                telemetry_runtime_prepared: Some(telemetry_runtime_prepared),
            })
            .context("encode attached execution")?;
            let encoded_placement =
                serde_json::to_string(placement).context("encode attached Placement")?;
            let observation = ObservationKind::Running;
            let detail = "the attached Runtime incarnation is running";
            transaction
                .execute(
                    "INSERT INTO sessions
                     (id, actor_id, generation, start, start_kind, ownership,
                      end_kind, execution, placement, token, start_settled,
                      observation_kind, observation_at, observation_since, observation_detail,
                      telemetry_collector_available, telemetry_runtime_prepared)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?4, ?11, ?4, ?4, ?12, ?13, ?14)",
                    params![
                        session_id,
                        actor_id,
                        generation,
                        accepted,
                        Text(StartKind::Attach),
                        Text(Ownership::Attached),
                        Text(EndKind::Running),
                        execution,
                        encoded_placement,
                        token,
                        Text(observation),
                        detail,
                        telemetry_collector_available,
                        telemetry_runtime_prepared,
                    ],
                )
                .context("create attached Session")?;
            append_record(
                transaction,
                &session_id,
                &accepted,
                RecordKind::SessionStarted,
                &serde_json::json!({ "startKind": StartKind::Attach }),
            )?;
            append_record(
                transaction,
                &session_id,
                &accepted,
                RecordKind::Observation,
                &serde_json::json!({
                    "kind": observation,
                    "detail": detail,
                    "exitCode": null,
                }),
            )?;
            let session =
                get_session(transaction, &session_id)?.context("attached Session disappeared")?;
            let pending = PendingSession {
                session,
                token,
                replaced_session,
            };
            notice_started(notices, workspace, actor, &pending);
            Ok(AttachAdmission {
                pending,
                reattached: false,
            })
        })
        .inspect(|admission| self.keep_secret(&admission.pending.token))
    }
}

/// The events of a Session that admission started, and of the one it replaced.
fn notice_started(notices: &mut Notices, workspace: &str, actor: &str, pending: &PendingSession) {
    notices.push("actor", actor, Some(workspace));
    if let Some(replaced) = pending.replaced_session.as_ref() {
        notices.push("session", &replaced.id, Some(&replaced.workspace));
        notices.push("record", &replaced.id, Some(&replaced.workspace));
    }
    notices.push("session", &pending.session.id, Some(workspace));
    notices.push("record", &pending.session.id, Some(workspace));
}

/// Closes the Actor's current Session that admission may replace, and answers it closed.
/// A current Session that may still run refuses the admission.
fn replace_current(
    transaction: &Transaction<'_>,
    actor: &str,
    actor_id: &str,
    accepted: &str,
) -> std::result::Result<Option<Session>, SessionWriteError> {
    let Some((current_id, observation)) = current_session(transaction, actor_id)? else {
        return Ok(None);
    };
    match admission(observation) {
        Admission::CloseUnknown => close_unknown(transaction, &current_id, accepted)?,
        Admission::CloseExited => close_exited(transaction, &current_id, accepted)?,
        Admission::Refuse => return Err(refusal(actor, current_id, observation)),
    }
    Ok(get_session(transaction, &current_id)?)
}

/// The Actor's current Session and its latest observation.
fn current_session(
    connection: &Connection,
    actor_id: &str,
) -> anyhow::Result<Option<(String, Option<ObservationKind>)>> {
    connection
        .query_row(
            "SELECT id, observation_kind FROM sessions WHERE actor_id = ?1 AND end IS NULL",
            [actor_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, Option<Text<_>>>(1)?.map(|Text(kind)| kind),
                ))
            },
        )
        .optional()
        .context("read Actor current Session for admission")
}

fn refusal(
    actor: &str,
    session: String,
    observation: Option<ObservationKind>,
) -> SessionWriteError {
    SessionWriteError::Admission {
        actor: actor.to_owned(),
        session,
        observation: observation
            .map_or("unavailable", ObservationKind::as_str)
            .to_owned(),
    }
}

fn next_generation(transaction: &Transaction<'_>, actor_id: &str) -> anyhow::Result<u64> {
    transaction
        .query_row(
            "SELECT COALESCE(MAX(generation), 0) + 1 FROM sessions WHERE actor_id = ?1",
            [actor_id],
            |row| row.get(0),
        )
        .context("allocate Session generation")
}

/// What admission decides for an owned start, read without writing anything, so that the
/// same decision can be asked before the Provider places the start and again inside the
/// admission that records it.
enum Judgement {
    Admit {
        actor_id: String,
    },
    /// A scheduled tick that meets a current Session that may still run.
    Skip {
        current_id: String,
    },
    Closed(ScheduledAdmission),
}

fn judge_owned(
    connection: &Connection,
    workspace: &str,
    actor: &str,
    expected_launch: &LaunchSpec,
    source: &OwnedStart<'_>,
) -> std::result::Result<Judgement, SessionWriteError> {
    let actor_record = read_actor_rows(
        connection,
        "WHERE workspace_key = ?1 AND name = ?2 AND deleted_at IS NULL",
        params![workspace, actor],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| SessionWriteError::ActorNotFound {
        workspace: workspace.to_owned(),
        actor: actor.to_owned(),
    })?;
    let actor_id = actor_record.id;
    let kind = actor_record.kind;
    let stored_launch: Option<LaunchSpec> = actor_record
        .launch_spec
        .map(|value| serde_json::from_str(&value).context("decode Actor launchSpec"))
        .transpose()?
        .flatten();
    let Some(stored_launch) = stored_launch.filter(|launch| launch.supports(kind)) else {
        return Err(SessionWriteError::NotProgramJob(actor.to_owned()));
    };
    if &stored_launch != expected_launch {
        return Err(SessionWriteError::LaunchChanged(actor.to_owned()));
    }
    if let OwnedStart::Start(identity) = source
        && let (Requester::Actor { id, .. }, Some(session_id)) =
            (identity.requester, identity.session_id)
    {
        let valid = connection
            .query_row(
                "SELECT 1 FROM sessions WHERE id = ?1 AND actor_id = ?2 AND end IS NULL",
                params![session_id, id],
                |_| Ok(()),
            )
            .optional()
            .context("revalidate spawning Session")?
            .is_some();
        if !valid {
            return Err(SessionWriteError::SessionClosed(session_id.to_owned()));
        }
    }
    if let OwnedStart::Restart { stopped, .. } = source {
        let requested = connection
            .query_row(
                "SELECT 1 FROM restarts WHERE session_id = ?1",
                [stopped],
                |_| Ok(()),
            )
            .optional()
            .context("read restart request")?
            .is_some();
        let latest = connection
            .query_row(
                "SELECT id FROM sessions WHERE actor_id = ?1 ORDER BY generation DESC LIMIT 1",
                [&actor_id],
                |row| row.get::<_, String>(0),
            )
            .context("read Actor latest Session for restart")?;
        if !requested || latest != *stopped {
            return Ok(Judgement::Closed(ScheduledAdmission::Stale));
        }
    }
    if let OwnedStart::Schedule {
        expected, planned, ..
    } = source
    {
        if kind != ActorKind::Job {
            return Err(SessionWriteError::NotProgramJob(actor.to_owned()));
        }
        if !actor_record
            .schedule
            .is_some_and(|schedule| schedule.fires_as(expected))
        {
            return Ok(Judgement::Closed(ScheduledAdmission::Stale));
        }
        if tick_recorded(connection, &actor_id, planned)? {
            return Ok(Judgement::Closed(ScheduledAdmission::Already));
        }
    }
    if let Some((current_id, observation)) = current_session(connection, &actor_id)?
        && admission(observation) == Admission::Refuse
    {
        // A scheduled tick that meets a current Session that may still run is recorded as
        // skipped instead of refused.
        return match source {
            OwnedStart::Schedule { .. } => Ok(Judgement::Skip { current_id }),
            OwnedStart::Start(_) | OwnedStart::Restart { .. } => {
                Err(refusal(actor, current_id, observation))
            }
        };
    }
    Ok(Judgement::Admit { actor_id })
}

fn admit_owned_in(
    transaction: &Transaction<'_>,
    workspace: &str,
    actor: &str,
    expected_launch: &LaunchSpec,
    execution: &Execution,
    placement: &Placement,
    source: OwnedStart<'_>,
) -> std::result::Result<ScheduledAdmission, SessionWriteError> {
    let judgement = judge_owned(transaction, workspace, actor, expected_launch, &source)?;
    // A restart is taken once, whether its next generation starts or no longer applies.
    if let OwnedStart::Restart { stopped, .. } = &source {
        transaction
            .execute("DELETE FROM restarts WHERE session_id = ?1", [stopped])
            .context("take restart request")?;
    }
    let accepted = match &source {
        OwnedStart::Start(_) | OwnedStart::Restart { .. } => now(),
        OwnedStart::Schedule { accepted, .. } => (*accepted).to_owned(),
    };
    let actor_id = match judgement {
        Judgement::Admit { actor_id } => actor_id,
        Judgement::Closed(closed) => return Ok(closed),
        Judgement::Skip { current_id } => {
            let OwnedStart::Schedule { planned, .. } = &source else {
                unreachable!("only a scheduled tick is skipped");
            };
            append_record(
                transaction,
                &current_id,
                &accepted,
                RecordKind::ScheduleSkipped,
                &serde_json::json!({
                    "planned": planned,
                    "currentSessionId": current_id,
                }),
            )?;
            return Ok(ScheduledAdmission::Skipped(ScheduleSkip {
                planned: (*planned).to_owned(),
                current_session_id: current_id,
                at: accepted,
            }));
        }
    };
    let replaced_session = replace_current(transaction, actor, &actor_id, &accepted)?;
    let generation = next_generation(transaction, &actor_id)?;
    let (start_kind, planned) = match &source {
        OwnedStart::Schedule { planned, .. } => (StartKind::Schedule, Some(*planned)),
        OwnedStart::Start(_) => (StartKind::for_generation(generation), None),
        OwnedStart::Restart { .. } => (StartKind::Replacement, None),
    };
    let session_id = fresh_id("session");
    let token = fresh_token();
    let telemetry_collector_available = execution.telemetry_collector_available.unwrap_or(true);
    let execution = serde_json::to_string(execution).context("encode Session execution")?;
    let placement = serde_json::to_string(placement).context("encode Session placement")?;
    transaction
        .execute(
            "INSERT INTO sessions
             (id, actor_id, generation, start, start_kind, ownership,
              end_kind, planned, execution, placement, token, telemetry_collector_available)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                session_id,
                actor_id,
                generation,
                accepted,
                Text(start_kind),
                Text(Ownership::Owned),
                Text(EndKind::Running),
                planned,
                execution,
                placement,
                token,
                telemetry_collector_available,
            ],
        )
        .context("create Session")?;
    if let OwnedStart::Start(identity) | OwnedStart::Restart { identity, .. } = &source {
        append_record(
            transaction,
            &session_id,
            &accepted,
            RecordKind::Spawned,
            &serde_json::json!({
                "spawner": identity.requester,
                "spawnerSessionId": identity.session_id,
            }),
        )?;
    }
    append_record(
        transaction,
        &session_id,
        &accepted,
        RecordKind::SessionStarted,
        &serde_json::json!({ "startKind": start_kind, "planned": planned }),
    )?;
    let session = get_session(transaction, &session_id)?
        .ok_or_else(|| anyhow::anyhow!("admitted Session was not found"))?;
    Ok(ScheduledAdmission::Started(Box::new(PendingSession {
        session,
        token,
        replaced_session,
    })))
}
