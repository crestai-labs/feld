use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::watch;

use crate::store::{ObservationUpdate, SessionWriteError};
use feld_core::model::{ObservationKind, Placement, Session};
use feld_core::plugin::protocol::{ObservationReport, ObserveRequest, ObserveResult};
use feld_core::plugin::{PluginKind, not_installed};
use feld_core::time::now;

use super::{ControlError, SessionController};

const OBSERVATION_INTERVAL: Duration = Duration::from_millis(250);

impl SessionController {
    pub async fn observe(
        self: &Arc<Self>,
        session_id: &str,
    ) -> std::result::Result<Session, ControlError> {
        let session_id = session_id.to_owned();
        self.operate(move |controller| controller.observe_locked(&session_id))
            .await
    }

    /// Recovers every open Session: carries out its pending lifecycle action or observes it,
    /// then starts the restarts whose stopped Session has closed. A Provider or Session that
    /// fails leaves the rest to go on, and what failed comes back as one line each.
    pub async fn recover(self: &Arc<Self>) -> Result<Vec<String>> {
        self.operate(Self::recover_locked).await
    }

    fn recover_locked(&self) -> Result<Vec<String>> {
        let mut problems = Vec::new();
        let mut open = HashSet::new();
        let mut waiting: HashMap<String, Vec<Session>> = HashMap::new();
        for session in self.store.list_open_sessions()? {
            open.insert(session.id.clone());
            if let Some(action) = session.lifecycle.pending() {
                let id = session.id.clone();
                if let Err(error) = self.execute_lifecycle_locked(session, action, true) {
                    problems.push(format!("Session '{id}' was not recovered: {error:#}"));
                }
            } else {
                waiting
                    .entry(session.placement.provider.clone())
                    .or_default()
                    .push(session);
            }
        }
        // The periodic observation hands each Provider the Placements of all its open
        // Sessions at once.
        for (provider, sessions) in waiting {
            match self.observe_group(&provider, &sessions) {
                Ok(recorded) => {
                    for (session, recorded) in sessions.iter().zip(recorded) {
                        if let Err(error) = recorded {
                            problems.push(format!(
                                "Session '{}' was not observed: {error:#}",
                                session.id
                            ));
                        }
                    }
                }
                Err(error) => problems.push(format!(
                    "the Sessions of Provider '{provider}' were not observed: {error:#}"
                )),
            }
        }
        if let Err(error) = self.start_restarts_locked() {
            problems.push(format!("restarts were not started: {error:#}"));
        }
        self.forget_closed_console_cursors(&open);
        Ok(problems)
    }

    /// Recovers the open Sessions at every interval. `reported` is what the daemon already
    /// wrote to its log, and a problem that persists is written once, when it first appears.
    pub async fn run_periodic(
        self: Arc<Self>,
        reported: Vec<String>,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let mut interval = tokio::time::interval(OBSERVATION_INTERVAL);
        interval.tick().await;
        let mut reported: HashSet<String> = reported.into_iter().collect();
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let problems = match self.recover().await {
                        Ok(problems) => problems,
                        Err(error) => vec![format!("open Sessions were not read: {error:#}")],
                    };
                    for problem in &problems {
                        if !reported.contains(problem) {
                            eprintln!("Feld observe: {problem}");
                        }
                    }
                    reported = problems.into_iter().collect();
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    }

    fn observe_locked(&self, session_id: &str) -> std::result::Result<Session, ControlError> {
        let session = self
            .store
            .get_session(session_id)?
            .ok_or_else(|| SessionWriteError::SessionNotFound(session_id.to_owned()))?;
        if session.end.is_some() {
            return Err(SessionWriteError::SessionClosed(session_id.to_owned()).into());
        }
        let provider = session.placement.provider.clone();
        let recorded = self.observe_group(&provider, std::slice::from_ref(&session))?;
        let recorded = recorded
            .into_iter()
            .next()
            .ok_or_else(|| SessionWriteError::SessionNotFound(session_id.to_owned()))?;
        Ok(recorded?)
    }

    /// Observes every Session of one Provider in a single call and records what came back,
    /// with one record per Session.
    fn observe_group(
        &self,
        provider: &str,
        sessions: &[Session],
    ) -> std::result::Result<Vec<std::result::Result<Session, SessionWriteError>>, ControlError>
    {
        if sessions.is_empty() {
            return Ok(Vec::new());
        }
        let implementation = self.plugins.provider(provider);
        let observed = implementation.as_ref().map(|implementation| {
            implementation.observe(&ObserveRequest {
                placements: sessions
                    .iter()
                    .map(|session| session.placement.placement.clone())
                    .collect(),
            })
        });
        let results = match observed {
            Some(Ok(response)) if response.results.len() == sessions.len() => response.results,
            Some(Ok(response)) => {
                return Err(ControlError::ObserveMismatch {
                    provider: provider.to_owned(),
                    answered: response.results.len(),
                });
            }
            Some(Err(error)) => {
                return Ok(self.record_each(
                    sessions,
                    &ObservationUpdate {
                        kind: ObservationKind::Unknown,
                        at: now(),
                        detail: Some(format!("{provider} could not observe: {error:#}")),
                        exit_code: None,
                    },
                ));
            }
            None => {
                return Ok(self.record_each(
                    sessions,
                    &ObservationUpdate {
                        kind: ObservationKind::Lost,
                        at: now(),
                        detail: Some(not_installed(PluginKind::Provider, provider)),
                        exit_code: None,
                    },
                ));
            }
        };
        let implementation = implementation.expect("the Provider answered");
        let mut recorded = Vec::with_capacity(sessions.len());
        for (session, result) in sessions.iter().zip(results) {
            let observation = observation_update(&result.observation);
            let refreshed = refreshed_placement(session, &result);
            let session =
                match self
                    .store
                    .record_observation(&session.id, &observation, refreshed.as_ref())
                {
                    Ok(session) => session,
                    Err(error) => {
                        recorded.push(Err(error));
                        continue;
                    }
                };
            if observation.kind == ObservationKind::Gone {
                self.clean_up(implementation.as_ref(), &session);
            }
            self.notify_console_growth(&session);
            recorded.push(Ok(session));
        }
        Ok(recorded)
    }

    fn record_each(
        &self,
        sessions: &[Session],
        observation: &ObservationUpdate,
    ) -> Vec<std::result::Result<Session, SessionWriteError>> {
        sessions
            .iter()
            .map(|session| {
                self.store
                    .record_observation(&session.id, observation, None)
            })
            .collect()
    }
}

/// How the Provider read its resource this time, when that differs from what the Placement
/// says. Every listing shows the Placement's `summary` and `attachCommand`, so an
/// observation that resolved a new name is what the next read of the Session shows.
fn refreshed_placement(session: &Session, observed: &ObserveResult) -> Option<Placement> {
    let stored = &session.placement.placement;
    let summary = serde_json::Value::String(observed.summary.clone());
    let attach_command = observed
        .attach_command
        .clone()
        .map_or(serde_json::Value::Null, serde_json::Value::String);
    if stored.get("summary") == Some(&summary)
        && stored.get("attachCommand") == Some(&attach_command)
    {
        return None;
    }
    let mut refreshed = session.placement.clone();
    let object = refreshed.placement.as_object_mut()?;
    object.insert("summary".to_owned(), summary);
    object.insert("attachCommand".to_owned(), attach_command);
    Some(refreshed)
}

pub(super) fn observation_update(observation: &ObservationReport) -> ObservationUpdate {
    ObservationUpdate {
        kind: observation.kind,
        at: now(),
        detail: observation.detail.clone(),
        exit_code: observation.exit_code,
    }
}
