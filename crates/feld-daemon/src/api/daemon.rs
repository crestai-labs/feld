use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use serde_json::json;
use tokio::sync::broadcast;

use crate::store::ResourceEvent;
use feld_core::id::valid_key;
use feld_core::model::Session;
use feld_core::time;

use super::AppState;
use super::auth::{AdminCaller, Caller, StreamCaller};
use super::error::{ApiError, error, internal};
use super::query::{ApiQuery, EventQuery};
use super::types::DaemonStatus;

/// How often a stream opened with a browser token or a Session token checks that the token
/// is still valid.
const TOKEN_CHECK: Duration = Duration::from_secs(60);

pub(super) async fn events(
    State(state): State<AppState>,
    StreamCaller {
        browser,
        session_token,
        workspace: own_workspace,
    }: StreamCaller,
    ApiQuery(query): ApiQuery<EventQuery>,
) -> Result<Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    if let Some(workspace) = query.workspace.as_deref()
        && !valid_key(workspace)
    {
        return Err(error(StatusCode::BAD_REQUEST, "invalid Workspace key"));
    }
    let mut receiver = state.sessions.subscribe_events();
    let mut shutdown = state.shutdown.subscribe();
    let mut revocations = state.browsers.revocations();
    // A revocation between the authentication and this subscription is checked at once.
    revocations.mark_changed();
    let mut expiry =
        tokio::time::interval_at(tokio::time::Instant::now() + TOKEN_CHECK, TOKEN_CHECK);
    let browsers = Arc::clone(&state.browsers);
    let browser_stream = browser.is_some();
    let browser_ended = move || {
        browser
            .as_deref()
            .is_some_and(|id| !browsers.valid(id).unwrap_or(false))
    };
    // A Session token stays valid while its Session is open and the token is the last one
    // issued to it; a stream opened with one closes when it is no longer.
    let tokens = Arc::clone(&state.tokens);
    let session_stream = session_token.is_some();
    let session_ended = move || {
        session_token
            .as_deref()
            .is_some_and(|token| !matches!(tokens.authenticate(token), Ok(Some(_))))
    };
    // A Session token reads the events of its own Workspace.
    let workspace = match (query.workspace, own_workspace.clone()) {
        (Some(asked), Some(own)) if asked != own => {
            return Err(error(
                StatusCode::FORBIDDEN,
                "a Session token reaches only the Workspace of its Session",
            ));
        }
        (asked, own) => asked.or(own),
    };
    let instance_id = state.status.instance_id.clone();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("resource").data(
            serde_json::json!({"resource":"daemon","id":instance_id}).to_string()
        ));
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        let event = Event::default().event("resource").data(
                            serde_json::to_string(&ResourceEvent {
                                resource: "daemon".to_owned(),
                                id: "shutdown".to_owned(),
                                workspace: None,
                            }).expect("resource event serializes")
                        );
                        yield Ok(event);
                        break;
                    }
                }
                // A browser token that is revoked or expires closes its stream; the browser's
                // reconnection is refused with 401.
                _ = revocations.changed(), if browser_stream => {
                    if browser_ended() {
                        break;
                    }
                }
                _ = expiry.tick(), if browser_stream || session_stream => {
                    if browser_ended() || session_ended() {
                        break;
                    }
                }
                received = receiver.recv() => match received {
                    Ok(event) if visible(&event, workspace.as_deref(), own_workspace.as_deref()) => {
                        if session_ended() {
                            break;
                        }
                        yield Ok(Event::default().event("resource").data(
                            serde_json::to_string(&event).expect("resource event serializes")
                        ));
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let id = workspace.as_deref().unwrap_or("*");
                        yield Ok(Event::default().event("resource").data(
                            serde_json::json!({"resource":"resync","id":id}).to_string()
                        ));
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

/// Whether a stream sends `event`: the events of the `selected` Workspace and the events that
/// belong to no Workspace. A Session token's stream (`own` is its Workspace) leaves out the
/// creation and deletion of other Workspaces.
fn visible(event: &ResourceEvent, selected: Option<&str>, own: Option<&str>) -> bool {
    match event.workspace.as_deref() {
        Some(scope) => selected.is_none_or(|selected| selected == scope),
        None => event.resource != "workspace" || own.is_none_or(|own| own == event.id),
    }
}

pub(super) async fn daemon_status(
    State(state): State<AppState>,
    _caller: Caller,
) -> Result<Json<DaemonStatus>, ApiError> {
    let mut status = state.status.clone();
    status.otlp = state
        .collector
        .read()
        .expect("Collector status lock poisoned")
        .clone();
    status.scheduler = state
        .scheduler
        .lock()
        .expect("Scheduler status lock poisoned")
        .clone();
    status.server_time = time::now();
    status.history_start = state.sessions.history_start().map_err(internal)?;
    Ok(Json(status))
}

/// The current Sessions of every Workspace, which the Runtime and Provider listings count.
pub(super) fn current_sessions(state: &AppState) -> Result<Vec<Session>, ApiError> {
    state.sessions.current().map_err(internal)
}

pub(super) async fn daemon_shutdown(
    State(state): State<AppState>,
    _caller: AdminCaller,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .shutdown
        .send(true)
        .map_err(|cause| internal(anyhow::Error::new(cause).context("daemon cannot shut down")))?;
    Ok(Json(json!({ "accepted": true, "pid": state.status.pid })))
}

pub(super) async fn daemon_restart(
    State(state): State<AppState>,
    _caller: AdminCaller,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !state.restarting.swap(true, Ordering::SeqCst)
        && let Err(restart_error) =
            crate::daemon::spawn_restart(&state.home, Arc::clone(&state.restarting))
    {
        state.restarting.store(false, Ordering::SeqCst);
        return Err(internal(restart_error.context("daemon cannot restart")));
    }
    Ok(Json(json!({
        "accepted": true,
        "pid": state.status.pid,
        "instanceId": state.status.instance_id,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(resource: &str, id: &str, workspace: Option<&str>) -> ResourceEvent {
        ResourceEvent {
            resource: resource.to_owned(),
            id: id.to_owned(),
            workspace: workspace.map(str::to_owned),
        }
    }

    #[test]
    fn a_session_token_stream_sees_only_its_own_workspace() {
        let own = Some("alpha");
        assert!(visible(&event("session", "s1", Some("alpha")), own, own));
        assert!(!visible(&event("session", "s2", Some("beta")), own, own));
        assert!(visible(&event("workspace", "alpha", None), own, own));
        assert!(!visible(&event("workspace", "beta", None), own, own));
        assert!(visible(&event("quota", "request", None), own, own));
    }

    #[test]
    fn an_admin_stream_sees_every_workspace_change() {
        assert!(visible(&event("workspace", "beta", None), None, None));
        assert!(visible(
            &event("workspace", "beta", None),
            Some("alpha"),
            None
        ));
        assert!(!visible(
            &event("session", "s2", Some("beta")),
            Some("alpha"),
            None
        ));
        assert!(visible(&event("session", "s2", Some("beta")), None, None));
    }
}
