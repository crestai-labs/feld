use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::handler::Handler;
use axum::http::Method;
use axum::routing::{MethodFilter, on};

use super::AppState;
use super::actors::{
    attach_actor, create_actor, delete_actor, get_actor, get_actor_by_id, list_actors,
    restart_actor, start_actor, update_actor,
};
use super::daemon::{daemon_restart, daemon_shutdown, daemon_status, events};
use super::messages::{
    delete_message, get_mailbox, get_message, list_messages, queue_mailbox, read_mailbox,
    send_message, unqueue_mailbox,
};
use super::pairing::{
    create_pairing_code, list_browsers, pair_browser, revoke_browser, unpair_browser,
};
use super::runtime::{
    check_telemetry_token, get_runtime_metrics, list_providers, list_runtimes, record_runtime,
    record_telemetry,
};
use super::sessions::{
    get_output, get_self, get_session, get_session_metrics, get_transcript, kill_session,
    list_records, list_sessions, observe_session, stop_session,
};
use super::workspaces::{create_workspace, delete_workspace, get_workspace, list_workspaces};

/// A Router built together with the table of its routes, so a test compares the table with
/// `docs/guide/daemon-api.openapi.yaml`.
pub struct Routes<S> {
    router: Router<S>,
    table: Vec<(Method, &'static str)>,
}

impl<S> Routes<S>
where
    S: Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            table: Vec::new(),
        }
    }

    pub fn route<H, T>(mut self, method: Method, path: &'static str, handler: H) -> Self
    where
        H: Handler<T, S>,
        T: 'static,
    {
        let filter = MethodFilter::try_from(method.clone()).expect("a routable method");
        self.router = self.router.route(path, on(filter, handler));
        self.table.push((method, path));
        self
    }

    pub fn merge(mut self, other: Routes<S>) -> Self {
        self.router = self.router.merge(other.router);
        self.table.extend(other.table);
        self
    }

    pub fn into_router(self) -> Router<S> {
        self.router
    }

    pub fn table(&self) -> &[(Method, &'static str)] {
        &self.table
    }
}

impl<S> Default for Routes<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

/// The routes of the API, `/v1/...`.
pub(super) fn api_routes() -> Routes<AppState> {
    Routes::new()
        .route(Method::GET, "/v1/daemon/status", daemon_status)
        .route(Method::POST, "/v1/daemon/shutdown", daemon_shutdown)
        .route(Method::POST, "/v1/daemon/restart", daemon_restart)
        .route(Method::GET, "/v1/runtimes", list_runtimes)
        .route(Method::GET, "/v1/providers", list_providers)
        .route(
            Method::GET,
            "/v1/runtimes/{runtime}/metrics",
            get_runtime_metrics,
        )
        .route(Method::GET, "/v1/workspaces", list_workspaces)
        .route(Method::POST, "/v1/workspaces", create_workspace)
        .route(Method::GET, "/v1/workspaces/{workspace}", get_workspace)
        .route(
            Method::DELETE,
            "/v1/workspaces/{workspace}",
            delete_workspace,
        )
        .route(
            Method::GET,
            "/v1/workspaces/{workspace}/actors",
            list_actors,
        )
        .route(
            Method::POST,
            "/v1/workspaces/{workspace}/actors",
            create_actor,
        )
        .route(
            Method::GET,
            "/v1/workspaces/{workspace}/actors/{actor}",
            get_actor,
        )
        .route(
            Method::PATCH,
            "/v1/workspaces/{workspace}/actors/{actor}",
            update_actor,
        )
        .route(
            Method::DELETE,
            "/v1/workspaces/{workspace}/actors/{actor}",
            delete_actor,
        )
        .route(Method::GET, "/v1/actors/{actor}", get_actor_by_id)
        .route(
            Method::POST,
            "/v1/workspaces/{workspace}/actors/{actor}/start",
            start_actor,
        )
        .route(
            Method::POST,
            "/v1/workspaces/{workspace}/actors/{actor}/attach",
            attach_actor,
        )
        .route(
            Method::POST,
            "/v1/workspaces/{workspace}/actors/{actor}/restart",
            restart_actor,
        )
        .route(Method::GET, "/v1/sessions", list_sessions)
        .route(Method::GET, "/v1/sessions/{session}", get_session)
        .route(Method::POST, "/v1/sessions/{session}/stop", stop_session)
        .route(Method::POST, "/v1/sessions/{session}/kill", kill_session)
        .route(
            Method::POST,
            "/v1/sessions/{session}/observe",
            observe_session,
        )
        .route(
            Method::GET,
            "/v1/sessions/{session}/transcript",
            get_transcript,
        )
        .route(Method::GET, "/v1/sessions/{session}/output", get_output)
        .route(
            Method::GET,
            "/v1/sessions/{session}/metrics",
            get_session_metrics,
        )
        .route(Method::GET, "/v1/records", list_records)
        .route(Method::GET, "/v1/messages", list_messages)
        .route(Method::GET, "/v1/messages/{message}", get_message)
        .route(Method::DELETE, "/v1/messages/{message}", delete_message)
        .route(Method::GET, "/v1/self", get_self)
        .route(Method::GET, "/v1/self/messages", get_mailbox)
        .route(Method::POST, "/v1/self/messages/queued", queue_mailbox)
        .route(Method::POST, "/v1/self/messages/read", read_mailbox)
        .route(
            Method::DELETE,
            "/v1/self/messages/{message}/queued",
            unqueue_mailbox,
        )
        .route(Method::POST, "/v1/self/runtime", record_runtime)
        .route(
            Method::POST,
            "/v1/self/telemetry",
            record_telemetry.layer(DefaultBodyLimit::max(crate::collector::MAX_ENVELOPE_BYTES)),
        )
        .route(
            Method::POST,
            "/v1/self/telemetry/token",
            check_telemetry_token,
        )
        .route(Method::GET, "/v1/events", events)
        .route(Method::POST, "/v1/ui/pairing-codes", create_pairing_code)
        .route(Method::POST, "/v1/ui/pair", pair_browser)
        .route(Method::POST, "/v1/ui/unpair", unpair_browser)
        .route(Method::GET, "/v1/ui/browsers", list_browsers)
        .route(
            Method::DELETE,
            "/v1/ui/browsers/{browserId}",
            revoke_browser,
        )
        .route(
            Method::POST,
            "/v1/workspaces/{workspace}/actors/{actor}/messages",
            send_message,
        )
}
