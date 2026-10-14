use std::path::Path;
use std::sync::Arc;

use tokio::sync::watch;

use crate::store::{
    ObservationUpdate, PendingRestart, RequestIdentity, ScheduledAdmission,
    ScheduledAdmissionRequest, SessionWriteError,
};
use feld_core::home::{
    FELD_ACTOR_ENV, FELD_API_ENV, FELD_HOME_ENV, FELD_SESSION_ID_ENV, FELD_SESSION_TOKEN_ENV,
    FELD_WORKSPACE_ENV,
};
use feld_core::model::{Execution, ObservationKind, Placement, Requester, Schedule, Session};
use feld_core::plugin::PluginKind;
use feld_core::plugin::protocol::{PlaceRequest, ProviderLaunchRequest, RuntimeLaunchRequest};
use feld_core::time::now;

use super::capture::capture_wrapper;
use super::observe::observation_update;
use super::{ControlError, ScheduleTickResult, SessionController, StartedSession};
use crate::bundled;

pub(super) enum StartSource<'a> {
    Start {
        requester: &'a Requester,
        requester_session_id: Option<&'a str>,
    },
    Restart(&'a PendingRestart),
    Schedule {
        schedule: &'a Schedule,
        planned: &'a str,
        accepted: &'a str,
        shutdown: &'a watch::Receiver<bool>,
    },
}

impl SessionController {
    pub async fn start(
        self: &Arc<Self>,
        workspace: &str,
        actor: &str,
        requester: &Requester,
        requester_session_id: Option<&str>,
    ) -> std::result::Result<StartedSession, ControlError> {
        let (workspace, actor, requester) =
            (workspace.to_owned(), actor.to_owned(), requester.clone());
        let requester_session_id = requester_session_id.map(str::to_owned);
        let started = self
            .operate(move |controller| {
                controller.start_locked(
                    &workspace,
                    &actor,
                    StartSource::Start {
                        requester: &requester,
                        requester_session_id: requester_session_id.as_deref(),
                    },
                )
            })
            .await?;
        match started {
            ScheduleTickResult::Started(started) => Ok(*started),
            ScheduleTickResult::Skipped(_)
            | ScheduleTickResult::Already
            | ScheduleTickResult::Stale
            | ScheduleTickResult::Cancelled => Err(ControlError::Internal(anyhow::anyhow!(
                "manual start returned a scheduled tick result"
            ))),
        }
    }

    pub async fn schedule_tick(
        self: &Arc<Self>,
        workspace: &str,
        actor: &str,
        schedule: &Schedule,
        planned: &str,
        accepted: &str,
        shutdown: &watch::Receiver<bool>,
    ) -> std::result::Result<ScheduleTickResult, ControlError> {
        let (workspace, actor, schedule) =
            (workspace.to_owned(), actor.to_owned(), schedule.clone());
        let (planned, accepted, shutdown) =
            (planned.to_owned(), accepted.to_owned(), shutdown.clone());
        self.operate(move |controller| {
            if *shutdown.borrow() {
                return Ok(ScheduleTickResult::Cancelled);
            }
            controller.start_locked(
                &workspace,
                &actor,
                StartSource::Schedule {
                    schedule: &schedule,
                    planned: &planned,
                    accepted: &accepted,
                    shutdown: &shutdown,
                },
            )
        })
        .await
    }

    pub(super) fn start_locked(
        &self,
        workspace: &str,
        actor: &str,
        source: StartSource<'_>,
    ) -> std::result::Result<ScheduleTickResult, ControlError> {
        for _ in 0..3 {
            let actor_record = self.store.get_actor(workspace, actor)?.ok_or_else(|| {
                ControlError::ActorNotFound {
                    workspace: workspace.to_owned(),
                    actor: actor.to_owned(),
                }
            })?;
            let launch = actor_record
                .launch_spec
                .as_ref()
                .filter(|launch| launch.supports(actor_record.kind))
                .ok_or_else(|| ControlError::NotProgramJob(actor.to_owned()))?;
            let provider_name = launch
                .provider
                .clone()
                .expect("validated launchSpec has provider");
            let provider = self.provider(&provider_name)?;
            product_on_path(PluginKind::Provider, &provider_name)?;
            let runtime = match launch.runtime.as_deref() {
                Some(name) => {
                    let adapter = self.runtime(name)?;
                    product_on_path(PluginKind::Runtime, name)?;
                    Some((name.to_owned(), adapter))
                }
                None => None,
            };
            let workdir = launch
                .workdir
                .clone()
                .expect("validated launchSpec has workdir");
            let node = launch.node.clone().expect("validated launchSpec has node");
            // The Provider places only a start that admission will take. Under the operation
            // lock admission judges the same way when it writes, so a start judged here as
            // not admissible reaches admission unplaced and is refused, skipped or dropped.
            let refused = !match &source {
                StartSource::Start {
                    requester,
                    requester_session_id,
                } => self.store.start_admissible(
                    workspace,
                    actor,
                    launch,
                    RequestIdentity {
                        requester,
                        session_id: *requester_session_id,
                    },
                ),
                StartSource::Restart(restart) => self.store.restart_admissible(restart, launch),
                StartSource::Schedule {
                    schedule,
                    planned,
                    accepted,
                    ..
                } => self
                    .store
                    .tick_admissible(workspace, actor, launch, schedule, planned, accepted),
            };
            let placed = if refused {
                None
            } else {
                let placed = provider
                    .place(&PlaceRequest {
                        options: launch.provider_options.clone().unwrap_or_default(),
                        node: node.clone(),
                        workdir: workdir.clone(),
                    })
                    .map_err(|error| {
                        ControlError::Placement(format!(
                            "{provider_name} could not place the launch: {error:#}"
                        ))
                    })?;
                Some(placed.placement)
            };
            let placement = Placement {
                provider: provider_name.clone(),
                node,
                workdir: workdir.clone(),
                placement: placed.unwrap_or_default(),
            };
            let collector_endpoint = self.collector_endpoint();
            let execution = match &runtime {
                Some((name, adapter)) => Execution {
                    execution_type: "agent-runtime".to_owned(),
                    runtime: Some(name.clone()),
                    label: adapter.manifest().label.clone(),
                    command: None,
                    ai: true,
                    telemetry_collector_available: Some(collector_endpoint.is_some()),
                    telemetry_runtime_prepared: Some(true),
                },
                None => {
                    let command = launch.command.as_ref().expect("program command checked");
                    Execution {
                        execution_type: "program".to_owned(),
                        runtime: None,
                        label: program_label(command),
                        command: Some(command.clone()),
                        ai: false,
                        telemetry_collector_available: None,
                        telemetry_runtime_prepared: None,
                    }
                }
            };
            let pending = match &source {
                StartSource::Restart(restart) => match self
                    .store
                    .admit_restart(restart, launch, &execution, &placement)
                {
                    Err(SessionWriteError::LaunchChanged(_)) => continue,
                    Ok(None) => return Ok(ScheduleTickResult::Stale),
                    result => result?.expect("admitted restart"),
                },
                StartSource::Start {
                    requester,
                    requester_session_id,
                } => match self.store.admit_start(
                    workspace,
                    actor,
                    launch,
                    &execution,
                    &placement,
                    RequestIdentity {
                        requester,
                        session_id: *requester_session_id,
                    },
                ) {
                    Err(SessionWriteError::LaunchChanged(_)) => continue,
                    result => result?,
                },
                StartSource::Schedule {
                    schedule,
                    planned,
                    accepted,
                    shutdown,
                } => {
                    if *shutdown.borrow() {
                        return Ok(ScheduleTickResult::Cancelled);
                    }
                    match self.store.admit_scheduled(
                        workspace,
                        actor,
                        ScheduledAdmissionRequest {
                            launch,
                            schedule,
                            planned,
                            accepted,
                            execution: &execution,
                            placement: &placement,
                        },
                    ) {
                        Err(SessionWriteError::LaunchChanged(_)) => continue,
                        Err(error) => return Err(error.into()),
                        Ok(ScheduledAdmission::Started(pending)) => *pending,
                        Ok(ScheduledAdmission::Skipped(skip)) => {
                            return Ok(ScheduleTickResult::Skipped(skip));
                        }
                        Ok(ScheduledAdmission::Already) => return Ok(ScheduleTickResult::Already),
                        Ok(ScheduledAdmission::Stale) => return Ok(ScheduleTickResult::Stale),
                    }
                }
            };
            // A start replaces the previous generation, whether or not admission had to close
            // it first.
            let replaced = match &pending.replaced_session {
                Some(session) => Some(session.id.clone()),
                None if pending.session.generation > 1 => self.store.session_of_generation(
                    &pending.session.actor_id,
                    pending.session.generation - 1,
                )?,
                None => None,
            };
            if refused {
                return Ok(started(
                    self.store.record_launch_failure(
                        &pending.session.id,
                        &gone(format!(
                            "{provider_name} was not asked to place the launch: admission \
                             changed while the start was prepared"
                        )),
                    )?,
                    replaced,
                ));
            }
            // A sandbox the Session runs in writes its requests into this directory alone.
            if let Err(error) = self
                .home
                .create_owner_dir(&self.home.session_spool_dir(&pending.session.id))
            {
                return Ok(started(
                    self.store.record_launch_failure(
                        &pending.session.id,
                        &gone(format!(
                            "the spool of the Session was not created: {error:#}"
                        )),
                    )?,
                    replaced,
                ));
            }
            let (command, env) = match &runtime {
                Some((_, adapter)) => {
                    match adapter.launch(&RuntimeLaunchRequest {
                        session: pending.session.id.clone(),
                        actor: actor.to_owned(),
                        workspace: workspace.to_owned(),
                        workdir: workdir.clone(),
                        runtime_settings: launch.runtime_settings.clone(),
                        trust_workdir: launch.trust_workdir.unwrap_or(false),
                        env: launch.env.clone().unwrap_or_default(),
                        session_dir: self.home.session_runtime_dir(&pending.session.id),
                        token: pending.token.clone(),
                        collector: collector_endpoint.clone(),
                    }) {
                        Ok(prepared) => (prepared.command, prepared.env),
                        Err(error) => {
                            return Ok(started(
                                self.store.record_launch_failure(
                                    &pending.session.id,
                                    &gone(format!(
                                        "the {} Runtime could not be prepared: {error:#}",
                                        execution.label
                                    )),
                                )?,
                                replaced,
                            ));
                        }
                    }
                }
                None => (
                    launch.command.clone().expect("program command checked"),
                    launch
                        .env
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .chain(
                            [
                                (FELD_API_ENV, self.api.clone()),
                                (FELD_SESSION_TOKEN_ENV, pending.token.clone()),
                                (FELD_SESSION_ID_ENV, pending.session.id.clone()),
                                (FELD_ACTOR_ENV, actor.to_owned()),
                                (FELD_WORKSPACE_ENV, workspace.to_owned()),
                                (FELD_HOME_ENV, self.home.root().display().to_string()),
                            ]
                            .into_iter()
                            .map(|(name, value)| (name.to_owned(), value)),
                        )
                        .collect(),
                ),
            };
            // The Console's log view of a `program` comes from core, which wraps the command
            // in its capture helper before the Provider sees it.
            let command = if runtime.is_some() {
                command
            } else {
                match self.prepare_output_capture(&pending.session.id) {
                    Ok(capture) => capture_wrapper(&capture, &command),
                    Err(error) => {
                        return Ok(started(
                            self.store.record_launch_failure(
                                &pending.session.id,
                                &gone(format!(
                                    "program output capture could not be prepared: {error:#}"
                                )),
                            )?,
                            replaced,
                        ));
                    }
                }
            };
            let launched = provider.launch(&ProviderLaunchRequest {
                placement: pending.session.placement.placement.clone(),
                name: format!("feld-{workspace}-{actor}-{}", pending.session.generation),
                command,
                env,
                workdir,
            });
            return match launched {
                Ok(result) => {
                    let placement = result.placement.map(|blob| Placement {
                        provider: provider_name,
                        node: pending.session.placement.node.clone(),
                        workdir: pending.session.placement.workdir.clone(),
                        placement: blob,
                    });
                    let observation = observation_update(&result.observation);
                    // `gone` with no Placement is the Provider saying nothing it created is
                    // left: the start is proven never to have begun, which is a different
                    // reading from an execution that ended. Any other kind leaves the
                    // execution unproven, so the Session keeps its ordinary reading.
                    let session =
                        if placement.is_none() && observation.kind == ObservationKind::Gone {
                            self.store
                                .record_launch_failure(&pending.session.id, &observation)?
                        } else {
                            self.store.record_observation(
                                &pending.session.id,
                                &observation,
                                placement.as_ref(),
                            )?
                        };
                    if observation.kind == ObservationKind::Gone {
                        self.clean_up(provider.as_ref(), &session);
                    }
                    Ok(started(session, replaced))
                }
                // The Provider could not answer, so whether it left an execution behind is
                // unknown.
                Err(error) => Ok(started(
                    self.store.record_observation(
                        &pending.session.id,
                        &ObservationUpdate {
                            kind: ObservationKind::Unknown,
                            at: now(),
                            detail: Some(format!("{provider_name} could not launch: {error:#}")),
                            exit_code: None,
                        },
                        None,
                    )?,
                    replaced,
                )),
            };
        }
        Err(ControlError::LaunchKeptChanging)
    }
}

/// Refuses a start on a shipped Plugin whose product is not on the daemon's `PATH`, which
/// the Session runs with.
fn product_on_path(kind: PluginKind, name: &str) -> Result<(), ControlError> {
    match bundled::find(kind, name) {
        Some(plugin) if !plugin.is_on_path() => Err(ControlError::ProductMissing {
            command: plugin.command.to_owned(),
            next: match kind {
                PluginKind::Provider => format!(
                    "{}, or give the Actor another Provider with --provider",
                    plugin.install_next()
                ),
                PluginKind::Runtime => plugin.install_next(),
            },
        }),
        _ => Ok(()),
    }
}

fn started(session: Session, replaced: Option<String>) -> ScheduleTickResult {
    ScheduleTickResult::Started(Box::new(StartedSession { session, replaced }))
}

/// A start that never reached the Provider.
fn gone(detail: String) -> ObservationUpdate {
    ObservationUpdate {
        kind: ObservationKind::Gone,
        at: now(),
        detail: Some(detail),
        exit_code: None,
    }
}

fn program_label(command: &[String]) -> String {
    command
        .first()
        .and_then(|program| Path::new(program).file_name())
        .and_then(|name| name.to_str())
        .map(|name| format!("{name} program"))
        .unwrap_or_else(|| "Program".to_owned())
}
