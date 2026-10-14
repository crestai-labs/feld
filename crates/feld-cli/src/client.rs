//! The daemon API as the CLI speaks it: where it listens, the token the caller speaks with,
//! and the requests.

use anyhow::{Context as _, Result, anyhow};
use feld_core::plugin::Registry;
use feld_core::process;
use feld_core::session_token::{self, Found, session_token};
use feld_core::spool::{Credential, Source};
use feld_daemon::api::{ActorResponse, DaemonStatus, SessionResponse, SubAgentSessionResponse};
use feld_daemon::console::SubAgentSession;
use feld_daemon::daemon::{self, Presence};
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{CliResult, Context};
use crate::spool::Spool;

/// A running daemon, and the token a command speaks to it with.
pub struct Daemon {
    pub api: String,
    pub token: String,
    /// The Execution Providers the daemon has installed.
    pub providers: Vec<String>,
    client: reqwest::Client,
    /// The spool the requests go through instead of TCP, when a Runtime's sandbox denies TCP.
    spool: Option<Spool>,
}

/// Which Session of an Actor a Session target names.
#[derive(Clone, Copy)]
pub enum Target {
    /// The running current Session: the one stop, kill and observe act on.
    Current,
    /// The current Session, or with none the latest generation: the one a read shows.
    Latest,
}

impl Daemon {
    /// The daemon of the Feld home, spoken to with the caller's Session token when it has
    /// one, else with the Feld home's API token.
    pub async fn connect(ctx: &Context) -> CliResult<Self> {
        let Some(resolved) = resolve_here(ctx)? else {
            return Self::through_spool(ctx, admin_outside_sandbox).await;
        };
        let Some(status) = reach_over_tcp(ctx, resolved.as_deref()).await? else {
            return Self::through_spool(ctx, admin_outside_sandbox).await;
        };
        let token = match resolved {
            Some(token) => token,
            None => ctx.home.read_token()?,
        };
        Ok(Self::new(status, token, None))
    }

    /// The daemon of the Feld home, spoken to with the Feld home's API token.
    pub async fn connect_local(ctx: &Context) -> CliResult<Self> {
        Self::connect_local_or(ctx, admin_outside_sandbox).await
    }

    /// [`Self::connect_local`], with `outside` as the refusal of a caller inside a sandbox.
    pub async fn connect_local_or(
        ctx: &Context,
        outside: impl FnOnce() -> super::CliError,
    ) -> CliResult<Self> {
        let status = reach_over_tcp(ctx, None).await?.ok_or_else(outside)?;
        let token = ctx.home.read_token()?;
        Ok(Self::new(status, token, None))
    }

    /// The daemon of the Feld home, spoken to with the caller's own Session token. Inside a
    /// sandbox that denies TCP, the daemon checks the caller's candidates through the spool
    /// even when none resolves here, as in another PID namespace.
    pub async fn connect_self(ctx: &Context) -> CliResult<Self> {
        let Some(resolved) = resolve_here(ctx)? else {
            return Self::through_spool(ctx, no_session_token).await;
        };
        let Some(token) = resolved else {
            return match daemon::probe(&ctx.home).await {
                Err(error) if denied_by_sandbox(&error) => {
                    Self::through_spool(ctx, no_session_token).await
                }
                _ => Err(no_session_token()),
            };
        };
        let Some(status) = reach_over_tcp(ctx, Some(&token)).await? else {
            return Self::through_spool(ctx, no_session_token).await;
        };
        Ok(Self::new(status, token, None))
    }

    /// The daemon reached through the spool, which carries the caller's Session token
    /// candidates for the daemon to check against the caller (`spool.md`).
    /// `untokened` is the refusal of a caller with no Session token for the spool to carry.
    async fn through_spool(ctx: &Context, untokened: fn() -> super::CliError) -> CliResult<Self> {
        let credentials = spool_credentials(ctx)?;
        let Some(first) = credentials.first() else {
            return Err(untokened());
        };
        let token = first.token.clone();
        let spool = Spool::new(ctx.home.clone(), credentials);
        let presence = probe_spool(ctx, &spool, untokened).await?;
        let status = running(ctx, presence)?;
        Ok(Self::new(status, token, Some(spool)))
    }

    fn new(status: DaemonStatus, token: String, spool: Option<Spool>) -> Self {
        Self {
            providers: plugin_names(&status.providers),
            api: status.api,
            token,
            client: reqwest::Client::new(),
            spool,
        }
    }

    /// A request to `path` of the API, with the token this daemon is spoken to with.
    pub fn call(&self, method: Method, path: &str) -> RequestBuilder {
        self.call_as(method, path, &self.token)
    }

    /// [`Daemon::call`] with another token.
    pub fn call_as(&self, method: Method, path: &str, token: &str) -> RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.api))
            .bearer_auth(token)
    }

    /// Sends a request and reads the daemon's answer, over TCP or through the spool; a
    /// refusal carries the daemon's message.
    pub async fn send<T: DeserializeOwned>(&self, builder: RequestBuilder) -> CliResult<T> {
        let (status, body) = match &self.spool {
            None => {
                let response = builder.send().await.context("call daemon API")?;
                let status = response.status();
                let body = response.bytes().await.context("read daemon response")?;
                (status, body.to_vec())
            }
            Some(spool) => {
                let request = builder.build().context("build daemon API request")?;
                let response = spool_exchange(spool, &request).await?;
                let status = StatusCode::from_u16(response.status)
                    .context("read the status of the spool response")?;
                (status, response.body.unwrap_or_default().into_bytes())
            }
        };
        if !status.is_success() {
            let answer = serde_json::from_slice::<Value>(&body).ok();
            let field = |key: &str| {
                answer
                    .as_ref()
                    .and_then(|value| value.get(key)?.as_str().map(str::to_owned))
            };
            let message =
                field("message").unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
            let error = rejected!("daemon rejected the request ({status}): {message}");
            return Err(match field("next") {
                Some(next) => error.with_next(next),
                None => error,
            });
        }
        Ok(serde_json::from_slice(&body).context("decode daemon response")?)
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> CliResult<T> {
        self.send(self.call(Method::GET, path)).await
    }

    pub async fn get_with<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &(impl Serialize + ?Sized),
    ) -> CliResult<T> {
        self.send(self.call(Method::GET, path).query(query)).await
    }

    pub async fn post<T: DeserializeOwned>(&self, path: &str) -> CliResult<T> {
        self.send(self.call(Method::POST, path)).await
    }

    pub async fn post_json<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &(impl Serialize + ?Sized),
    ) -> CliResult<T> {
        self.send(self.call(Method::POST, path).json(body)).await
    }

    pub async fn patch<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &(impl Serialize + ?Sized),
    ) -> CliResult<T> {
        self.send(self.call(Method::PATCH, path).json(body)).await
    }

    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> CliResult<T> {
        self.send(self.call(Method::DELETE, path)).await
    }

    pub async fn get_actor(&self, workspace: &str, actor: &str) -> CliResult<ActorResponse> {
        self.get(&actor_path(workspace, actor)).await
    }

    pub async fn get_session(&self, session_id: &str) -> CliResult<SessionResponse> {
        self.get(&session_path(session_id)).await
    }

    pub async fn get_sub_agent_session(&self, session_id: &str) -> CliResult<SubAgentSession> {
        let response: SubAgentSessionResponse = self.get(&session_path(session_id)).await?;
        Ok(response.sub_agent_session)
    }

    /// The Session a target names: a Session ID as it is, or an Actor's Session.
    pub async fn resolve_session(
        &self,
        workspace: &str,
        target: &str,
        kind: Target,
    ) -> CliResult<String> {
        if target.starts_with("session_") {
            return Ok(target.to_owned());
        }
        let actor = self.get_actor(workspace, target).await?.actor;
        match kind {
            Target::Current => actor
                .current_session
                .ok_or_else(|| rejected!("Actor '{target}' has no current Session")),
            Target::Latest => actor
                .current_session
                .or(actor.latest_session)
                .ok_or_else(|| rejected!("Actor '{target}' has no Session")),
        }
        .map(|session| session.id)
    }
}

/// The caller's Session token as resolved here, or with none the Admin, and `None` when a
/// sandbox denies the process table the resolution checks the self index against.
fn resolve_here(ctx: &Context) -> CliResult<Option<Option<String>>> {
    match session_token(&ctx.home) {
        Ok(resolved) => Ok(Some(resolved.map(|resolved| resolved.token))),
        Err(error) if process::table_denied(&error) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Whether a daemon runs for the Feld home, over TCP or, when a Runtime's sandbox denies
/// TCP, through the spool with the caller's Session token. An unresponsive daemon is an
/// [`Operational`](super::CliError::Operational) error carrying [`daemon::Unresponsive`].
pub async fn probe(ctx: &Context) -> CliResult<Presence> {
    let token = resolve_here(ctx)?.flatten();
    match probe_over_tcp(ctx, token.as_deref()).await {
        Err(error) if denied_by_sandbox(&error) => {
            let credentials = spool_credentials(ctx)?;
            if credentials.is_empty() {
                return Err(admin_outside_sandbox());
            }
            let spool = Spool::new(ctx.home.clone(), credentials);
            probe_spool(ctx, &spool, admin_outside_sandbox).await
        }
        presence => Ok(presence?),
    }
}

/// [`daemon::probe`] with the status read through the spool. `untokened` is the refusal when
/// no Session token the spool carries belongs to the caller.
async fn probe_spool(
    ctx: &Context,
    spool: &Spool,
    untokened: fn() -> super::CliError,
) -> CliResult<Presence> {
    daemon::probe_with(&ctx.home, |_record| {
        let spool = spool.clone();
        async move { spool_status(&spool).await }
    })
    .await
    .map_err(|error| {
        // No candidate belongs to the caller, as when none resolves over TCP.
        if error.chain().any(|cause| cause.is::<Unverified>()) {
            untokened()
        } else {
            error.into()
        }
    })
}

/// The daemon of the Feld home over TCP, or `None` when a Runtime's sandbox denies TCP, or
/// the local token to a caller without a Session `token`.
async fn reach_over_tcp(ctx: &Context, token: Option<&str>) -> CliResult<Option<DaemonStatus>> {
    match probe_over_tcp(ctx, token).await {
        Err(error) if denied_by_sandbox(&error) => Ok(None),
        presence => running(ctx, presence?).map(Some),
    }
}

/// [`daemon::probe`] over TCP, asked with the local token, or with the caller's Session
/// `token` when a Runtime's sandbox denies the local token.
async fn probe_over_tcp(ctx: &Context, token: Option<&str>) -> anyhow::Result<Presence> {
    match (daemon::probe(&ctx.home).await, token) {
        (Err(error), Some(token)) if error.is::<daemon::LocalTokenDenied>() => {
            daemon::probe_with(&ctx.home, |record| async move {
                daemon::get_status_as(&record.api, token).await
            })
            .await
        }
        (presence, _) => presence,
    }
}

/// The status of a running daemon. The human-readable output reads times in the timezone the
/// daemon runs in from here on.
fn running(ctx: &Context, presence: Presence) -> CliResult<DaemonStatus> {
    match presence {
        Presence::Running(status) => {
            ctx.adopt_timezone(&status.timezone);
            warn_version_drift(ctx, &status);
            Ok(*status)
        }
        Presence::NotRunning => Err(super::CliError::from(anyhow!(
            "no Feld daemon is running for {}",
            ctx.home.root().display()
        ))
        .with_next(start_daemon(&ctx.home))),
    }
}

/// The version of the executable a daemon runs, as `feld --version` begins: `0.15.0`.
pub fn daemon_version(status: &DaemonStatus) -> &str {
    status
        .version
        .split_whitespace()
        .next()
        .unwrap_or("unknown")
}

/// Whether the daemon runs another version than this CLI.
pub fn version_drifts(status: &DaemonStatus) -> bool {
    daemon_version(status) != env!("CARGO_PKG_VERSION")
}

/// Tells on stderr, once a command, that the daemon runs another version than this CLI. The
/// command goes on as it is, and stdout keeps its output.
pub fn warn_version_drift(ctx: &Context, status: &DaemonStatus) {
    if version_drifts(status) {
        ctx.version_warned.call_once(|| {
            eprintln!(
                "warning: daemon {} differs from this CLI {} \u{b7} next: {VERSION_NEXT}",
                daemon_version(status),
                env!("CARGO_PKG_VERSION")
            );
        });
    }
}

/// The step that brings the daemon and the installed Plugins to the version of this CLI.
pub const VERSION_NEXT: &str = "feld init";

/// The command that starts the daemon of `home`, or `feld init` before there is a Feld home.
pub fn start_daemon(home: &feld_core::home::FeldHome) -> &'static str {
    if home.root().exists() {
        "feld daemon start"
    } else {
        "feld init"
    }
}

/// The Session tokens a request through the spool carries: the environment's, or the self
/// index entries the caller's keys name, unchecked.
fn spool_credentials(ctx: &Context) -> CliResult<Vec<Credential>> {
    let registry = Registry::load(&ctx.home, None);
    Ok(match session_token::found(&ctx.home, &registry)? {
        Found::Environment(resolved) => vec![Credential {
            source: Source::Environment,
            plugin: None,
            token: resolved.token,
        }],
        Found::Index(candidates) => candidates
            .into_iter()
            .map(|candidate| candidate.credential)
            .collect(),
    })
}

/// Whether TCP to the daemon was refused by a sandbox, which denies the connection itself
/// (`EPERM` / `EACCES`) rather than finding nobody listening, or denies the local token the
/// connection is made with.
pub fn denied_by_sandbox(error: &anyhow::Error) -> bool {
    if error.is::<daemon::LocalTokenDenied>() {
        return true;
    }
    let connecting = error.chain().any(|cause| {
        cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_connect)
    });
    connecting
        && error.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| matches!(io.raw_os_error(), Some(libc::EPERM | libc::EACCES)))
        })
}

/// The daemon checked no Session token of a spool request against its caller.
#[derive(Debug)]
struct Unverified;

impl std::fmt::Display for Unverified {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("no Session token of the spool request belongs to its caller")
    }
}

impl std::error::Error for Unverified {}

async fn spool_status(spool: &Spool) -> Result<DaemonStatus> {
    let response = spool
        .exchange("GET", "/v1/daemon/status", None, None)
        .await
        .map_err(anyhow::Error::from)?;
    let body = response.body.unwrap_or_default();
    if response.status == StatusCode::UNAUTHORIZED.as_u16() {
        return Err(Unverified.into());
    }
    if !(200..300).contains(&response.status) {
        anyhow::bail!(
            "daemon rejected status request ({}): {body}",
            response.status
        );
    }
    serde_json::from_str(&body).context("read daemon status")
}

async fn spool_exchange(
    spool: &Spool,
    request: &reqwest::Request,
) -> CliResult<feld_core::spool::Response> {
    let url = request.url();
    let path = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    let content_type = request
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = match request.body() {
        Some(body) => Some(
            String::from_utf8(body.as_bytes().unwrap_or_default().to_vec())
                .context("encode the request body for the spool")?,
        ),
        None => None,
    };
    spool
        .exchange(request.method().as_str(), &path, content_type, body)
        .await
}

/// The refusal of a call that has no Session token inside a sandbox: the spool carries the
/// calls of a Session only.
fn admin_outside_sandbox() -> super::CliError {
    rejected!("{ADMIN_OUTSIDE_SANDBOX}")
}

/// [`admin_outside_sandbox`] with the command to run outside the sandbox.
pub fn admin_outside_sandbox_as(command: &str) -> super::CliError {
    rejected!("{ADMIN_OUTSIDE_SANDBOX}:\n  {command}")
}

const ADMIN_OUTSIDE_SANDBOX: &str = "this command runs as the Admin, which the Feld spool does not carry; run it outside the sandbox";

/// The caller's Session token for the commands that use the Runtime API, which a
/// caller outside a Session does not have.
pub fn self_token(ctx: &Context) -> CliResult<String> {
    resolve_here(ctx)?
        .ok_or_else(|| {
            rejected!(
                "cannot verify the self index from inside a sandbox that denies reading the process table"
            )
        })?
        .ok_or_else(no_session_token)
}

fn no_session_token() -> super::CliError {
    rejected!("No Session token: run this inside an attached or owned Feld Session")
}

/// The names of the installed Plugins a daemon status lists.
pub fn plugin_names(plugins: &[feld_core::plugin::PluginSummary]) -> Vec<String> {
    plugins.iter().map(|plugin| plugin.name.clone()).collect()
}

/// The path of the Actors of a Workspace.
pub fn actors_path(workspace: &str) -> String {
    format!("/v1/workspaces/{}/actors", percent_encode(workspace))
}

/// The path of an Actor of a Workspace.
pub fn actor_path(workspace: &str, actor: &str) -> String {
    format!("{}/{}", actors_path(workspace), percent_encode(actor))
}

/// The path of a Session.
pub fn session_path(session_id: &str) -> String {
    format!("/v1/sessions/{}", percent_encode(session_id))
}

pub fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_keep_each_value_in_its_own_segment() {
        assert_eq!(
            actor_path("default", "a/b?c#d"),
            "/v1/workspaces/default/actors/a%2Fb%3Fc%23d"
        );
        assert_eq!(
            session_path("session_x/../../y"),
            "/v1/sessions/session_x%2F..%2F..%2Fy"
        );
    }
}
