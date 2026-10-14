use std::sync::Arc;

use crate::store::{AcceptedLifecycleAction, LifecycleActionKind, RequestIdentity};
use feld_core::model::{ObservationKind, Requester, Session};
use feld_core::plugin::ExecutionProvider;
use feld_core::plugin::protocol::{ObservationReport, PlacementRequest, StopRequest};

use super::observe::observation_update;
use super::start::StartSource;
use super::{
    ControlError, LifecycleResult, RestartResult, ScheduleTickResult, SessionController,
    StartedSession,
};

impl SessionController {
    /// The inputs `feld session stop` delivers to this Session's Runtime, when its Adapter
    /// declares them.
    fn stop_input(&self, session: &Session) -> Option<Vec<String>> {
        self.plugins
            .runtime(session.execution.runtime.as_deref()?)?
            .manifest()
            .stop_input
            .clone()
            .filter(|inputs| !inputs.is_empty())
    }

    pub async fn lifecycle_action(
        self: &Arc<Self>,
        session_id: &str,
        action_kind: LifecycleActionKind,
        requester_session_id: Option<&str>,
    ) -> std::result::Result<LifecycleResult, ControlError> {
        let session_id = session_id.to_owned();
        let requester_session_id = requester_session_id.map(str::to_owned);
        self.operate(move |controller| {
            controller.lifecycle_action_locked(
                &session_id,
                action_kind,
                requester_session_id.as_deref(),
            )
        })
        .await
    }

    fn lifecycle_action_locked(
        &self,
        session_id: &str,
        action_kind: LifecycleActionKind,
        requester_session_id: Option<&str>,
    ) -> std::result::Result<LifecycleResult, ControlError> {
        match self
            .store
            .accept_lifecycle_action(session_id, action_kind, requester_session_id)?
        {
            AcceptedLifecycleAction::Already(session) => Ok(LifecycleResult {
                session,
                already_requested: true,
            }),
            AcceptedLifecycleAction::Resume(session) => {
                self.execute_lifecycle_locked(session, action_kind, true)
            }
            AcceptedLifecycleAction::Completed(session) => Ok(LifecycleResult {
                session,
                already_requested: false,
            }),
            AcceptedLifecycleAction::NeedsProvider(session) => {
                self.execute_lifecycle_locked(session, action_kind, false)
            }
        }
    }

    pub async fn restart(
        self: &Arc<Self>,
        workspace: &str,
        actor: &str,
        requester: &Requester,
        requester_session_id: Option<&str>,
    ) -> std::result::Result<RestartResult, ControlError> {
        let (workspace, actor, requester) =
            (workspace.to_owned(), actor.to_owned(), requester.clone());
        let requester_session_id = requester_session_id.map(str::to_owned);
        self.operate(move |controller| {
            controller.restart_locked(
                &workspace,
                &actor,
                &requester,
                requester_session_id.as_deref(),
            )
        })
        .await
    }

    fn restart_locked(
        &self,
        workspace: &str,
        actor: &str,
        requester: &Requester,
        requester_session_id: Option<&str>,
    ) -> std::result::Result<RestartResult, ControlError> {
        let accepted = self.store.accept_restart(
            workspace,
            actor,
            RequestIdentity {
                requester,
                session_id: requester_session_id,
            },
        )?;
        let stopped = match accepted {
            AcceptedLifecycleAction::Already(session)
            | AcceptedLifecycleAction::Completed(session) => session,
            AcceptedLifecycleAction::Resume(session) => {
                self.execute_lifecycle_locked(session, LifecycleActionKind::Stop, true)?
                    .session
            }
            AcceptedLifecycleAction::NeedsProvider(session) => {
                self.execute_lifecycle_locked(session, LifecycleActionKind::Stop, false)?
                    .session
            }
        };
        let started = self
            .start_restarts_locked()?
            .into_iter()
            .find(|started| started.session.actor_id == stopped.actor_id);
        Ok(RestartResult { stopped, started })
    }

    /// Starts the next generation of every restart whose stopped Session has closed. A
    /// restart that cannot start is dropped, so that it is tried once.
    pub(super) fn start_restarts_locked(
        &self,
    ) -> std::result::Result<Vec<StartedSession>, ControlError> {
        let mut started = Vec::new();
        for restart in self.store.pending_restarts()? {
            match self.start_locked(
                &restart.workspace,
                &restart.actor,
                StartSource::Restart(&restart),
            ) {
                Ok(ScheduleTickResult::Started(session)) => started.push(*session),
                Ok(_) => {}
                Err(error) => {
                    eprintln!(
                        "restart of Actor '{}' in Workspace '{}' after Session '{}' was dropped: {error}",
                        restart.actor, restart.workspace, restart.session_id
                    );
                    self.store.drop_restart(&restart.session_id)?;
                }
            }
        }
        Ok(started)
    }

    pub(super) fn execute_lifecycle_locked(
        &self,
        session: Session,
        action_kind: LifecycleActionKind,
        already_requested: bool,
    ) -> std::result::Result<LifecycleResult, ControlError> {
        let placement = session.placement.placement.clone();
        let result = self
            .provider(&session.placement.provider)
            .map_err(anyhow::Error::msg)
            .and_then(|provider| match action_kind {
                // `feld session stop` asks the Runtime to end itself with the input its Adapter
                // declares, and signals anything else the way a `program` is signalled.
                LifecycleActionKind::Stop => provider.stop(&StopRequest {
                    placement,
                    input: self.stop_input(&session),
                }),
                LifecycleActionKind::Kill => provider.kill(&PlacementRequest { placement }),
            });
        // A Provider that could not answer leaves the execution as it was, so the Session stays
        // open with an unknown observation and the same request tries again.
        let (attempt, observation) = match result {
            Ok(response) => (
                Some(response.attempt),
                observation_update(&response.observation),
            ),
            Err(error) => (
                None,
                observation_update(&ObservationReport::new(
                    ObservationKind::Unknown,
                    format!("{error:#}"),
                )),
            ),
        };
        let session = self.store.finish_lifecycle_action(
            &session.id,
            action_kind,
            attempt.as_deref(),
            &observation,
        )?;
        if observation.kind == ObservationKind::Gone
            && let Ok(provider) = self.provider(&session.placement.provider)
        {
            self.clean_up(provider.as_ref(), &session);
        }
        Ok(LifecycleResult {
            session,
            already_requested,
        })
    }

    /// Releases what the Provider left for a Session that is gone, once its captured output
    /// is complete. The Session is closed, so nothing calls `cleanup` for it again, and a
    /// failure goes to the daemon log.
    pub(super) fn clean_up(&self, provider: &dyn ExecutionProvider, session: &Session) {
        self.await_output_capture(session);
        if let Err(error) = provider.cleanup(&PlacementRequest {
            placement: session.placement.placement.clone(),
        }) {
            eprintln!(
                "{}",
                cleanup_failure(&session.placement.provider, &session.id, &error)
            );
        }
    }
}

fn cleanup_failure(provider: &str, session: &str, error: &anyhow::Error) -> String {
    format!("{provider} could not clean up after Session '{session}': {error:#}")
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use super::cleanup_failure;

    #[test]
    fn cleanup_failure_names_the_provider_the_session_and_the_error() {
        let error = anyhow!("pane %3 is still open").context("tmux kill-pane failed");
        assert_eq!(
            cleanup_failure("tmux", "session_1", &error),
            "tmux could not clean up after Session 'session_1': tmux kill-pane failed: pane %3 is still open"
        );
    }
}
