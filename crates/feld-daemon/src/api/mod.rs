use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use axum::Router;
use axum::http::StatusCode;
use axum::middleware;
use tokio::sync::watch;

use crate::control::SessionController;
use crate::scheduler::SchedulerStatus;
use crate::service::{
    ActorService, BrowserService, MessageService, SessionService, TokenService, WorkspaceService,
};
use feld_core::home::FeldHome;
use feld_core::plugin::Registry;

mod access;
mod actors;
mod auth;
mod daemon;
mod error;
mod host;
mod messages;
mod pairing;
mod query;
mod routes;
mod runtime;
mod sessions;
mod spool;
mod types;
mod web;
mod workspaces;

pub use host::{AllowedHosts, allowed_host_name};
pub use spool::serve as serve_spool;
pub use types::{
    ActorList, ActorResponse, AttachActorRequest, AttachResponse, BrowserList, BrowserResponse,
    CreateActorRequest, CreateScheduleRequest, CreateWorkspaceRequest, DaemonStatus,
    LifecycleResponse, MessageList, MessageReadList, MessageResponse, MessageSummaryList,
    ProviderEntry, ProviderList, RecordList, RestartResponse, RestoredCounts, RuntimeEntry,
    RuntimeList, SendMessageRequest, SessionList, SessionResponse, StartResponse,
    SubAgentSessionResponse, UpdateActorRequest, UpdateScheduleRequest, WorkspaceList,
    WorkspaceResponse,
};

use auth::redact_response;
use error::error;
use host::check_host;
use routes::{Routes, api_routes};

/// The way a request reached the daemon, which travels with the request to its handler.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Route {
    /// The API listener.
    #[default]
    Tcp,
    /// The spool, from a CLI inside a Runtime's sandbox.
    Spool,
}

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    pub internal_token: String,
    pub status: DaemonStatus,
    /// The installed Plugins, which the launchSpec checks read.
    pub registry: Arc<Registry>,
    pub collector: Arc<RwLock<Option<String>>>,
    pub workspaces: Arc<WorkspaceService>,
    pub actors: Arc<ActorService>,
    pub messages: Arc<MessageService>,
    pub sessions: Arc<SessionService>,
    pub tokens: Arc<TokenService>,
    pub browsers: Arc<BrowserService>,
    /// The host names the daemon answers.
    pub hosts: Arc<AllowedHosts>,
    pub controller: Arc<SessionController>,
    pub scheduler: Arc<Mutex<SchedulerStatus>>,
    pub shutdown: watch::Sender<bool>,
    /// Whether a restart is already under way, so that the requests made meanwhile join it.
    pub restarting: Arc<AtomicBool>,
    pub home: FeldHome,
}

pub fn router(state: AppState) -> Router {
    api_routes()
        .into_router()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            redact_response,
        ))
        .merge(web::routes().into_router())
        .fallback(|| async { error(StatusCode::NOT_FOUND, "no such API path") })
        .method_not_allowed_fallback(|| async {
            error(
                StatusCode::METHOD_NOT_ALLOWED,
                "this API path does not take this method",
            )
        })
        .layer(middleware::from_fn_with_state(state.clone(), check_host))
        .with_state(state)
}

/// Every route of the daemon: the API and the Web UI.
pub fn routes() -> Routes<AppState> {
    api_routes().merge(web::routes())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::routes;

    /// `(method, path)` of every operation in the `paths` of the OpenAPI document. The document
    /// keeps a path at two spaces of indent and its methods at four, as YAML block mappings.
    fn openapi_operations(document: &str) -> BTreeSet<(String, String)> {
        let mut operations = BTreeSet::new();
        let mut in_paths = false;
        let mut path = None;
        for line in document.lines() {
            if line.trim_start().starts_with('#') {
                continue;
            }
            if !line.starts_with(' ') && !line.is_empty() {
                in_paths = line == "paths:";
                continue;
            }
            if !in_paths {
                continue;
            }
            if let Some(key) = line
                .strip_prefix("  ")
                .and_then(|rest| rest.strip_suffix(':'))
                && key.starts_with('/')
            {
                path = Some(key.to_owned());
            } else if let Some(key) = line
                .strip_prefix("    ")
                .and_then(|rest| rest.strip_suffix(':'))
                && matches!(
                    key,
                    "get" | "put" | "post" | "delete" | "options" | "head" | "patch" | "trace"
                )
                && let Some(path) = &path
            {
                operations.insert((key.to_uppercase(), path.clone()));
            }
        }
        operations
    }

    #[test]
    fn openapi_describes_every_route() {
        let document = include_str!("../../../../docs/guide/daemon-api.openapi.yaml");
        let documented = openapi_operations(document);
        let routed: BTreeSet<_> = routes()
            .table()
            .iter()
            // A wildcard `{*path}` of the router is the parameter `{path}` of OpenAPI.
            .map(|(method, path)| (method.to_string(), path.replace("{*", "{")))
            .collect();
        assert_eq!(
            routed.difference(&documented).collect::<Vec<_>>(),
            Vec::<&(String, String)>::new(),
            "routes missing from docs/guide/daemon-api.openapi.yaml"
        );
        assert_eq!(
            documented.difference(&routed).collect::<Vec<_>>(),
            Vec::<&(String, String)>::new(),
            "operations of docs/guide/daemon-api.openapi.yaml without a route"
        );
    }
}
