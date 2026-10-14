//! What every Execution Provider executable shares: the operations of `plugin.md` over the
//! Placement a Provider knows, and the results the Provider answers them with.
//!
//! A Provider implements [`ProviderBackend`] and runs [`serve`]. The Placement core stores
//! is the Provider's own; `serve` decodes it, and writes back the `summary` and
//! `attachCommand` every listing shows.

use std::fmt::Display;
use std::path::Path;
use std::process::{Child, Command, ExitCode, Output, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use super::{io, launch_env};
use crate::home::{
    FELD_ACTOR_ENV, FELD_SESSION_ID_ENV, FELD_SESSION_TOKEN_ENV, FELD_WORKSPACE_ENV,
};
use crate::plugin::invoke::exchange;
use crate::plugin::protocol::{
    Candidate, CandidatesRequest, CandidatesResponse, ControlResponse, ObservationKind,
    ObservationReport, ObserveRequest, ObserveResponse, ObserveResult, PlaceRequest, PlaceResponse,
    PlacementRequest, ProviderAttachRequest, ProviderAttachResponse, ProviderLaunchRequest,
    ProviderLaunchResponse, ResolveRequest, ResolveResponse, SelfResponse, StopRequest,
};
use crate::process::ProcessIncarnation;

/// The command a pane waits on until the launch puts the Runtime's command in it.
pub const WAIT_COMMAND: &str = "exec sleep 2147483647";

/// How many times the whole `stopInput` column at most reaches a pane that stays. A Runtime
/// that is still starting drops what is typed into it, the Enter or the whole line, so its
/// exit comes a column late.
pub const STOP_INPUT_ROUNDS: usize = 3;

/// Whether a Placement created a resource or took one that was already there. Cleanup
/// removes only what it created.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceOrigin {
    Created,
    Existing,
}

/// What a Provider saw of one Placement, before core records the time, with the names the
/// Provider answers for it right now.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationResult<Current> {
    pub kind: ObservationKind,
    pub exit_code: Option<i32>,
    pub detail: String,
    pub current: Option<Current>,
}

impl<Current> ObservationResult<Current> {
    /// What core records of this observation.
    pub fn report(&self) -> ObservationReport {
        ObservationReport {
            kind: self.kind,
            exit_code: self.exit_code,
            detail: Some(self.detail.clone()),
        }
    }
}

pub fn observation<Current>(
    kind: ObservationKind,
    exit_code: Option<i32>,
    detail: String,
    current: Option<Current>,
) -> ObservationResult<Current> {
    ObservationResult {
        kind,
        exit_code,
        detail,
        current,
    }
}

/// What `launch` put in place, and what the first observation saw of it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult<Placement, Current> {
    pub placement: Placement,
    pub observation: ObservationResult<Current>,
}

/// What `launch` puts in the resource it creates: the argv core built, the environment the
/// Runtime Adapter asked for, and the name the created resource is labelled with.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRequest<Prepared> {
    pub prepared: Prepared,
    pub resource_name: String,
    pub command: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// A launch that did not reach a running program: the observation core records, the
/// Placement of whatever was created before it stopped, and what happened to that.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchFailure<Partial, Current> {
    pub observation: Box<ObservationResult<Current>>,
    pub partial: Option<Box<Partial>>,
    pub cleanup: Option<CleanupResult>,
    pub source: String,
}

/// What `launch` answers: the Placement it put in place, or what it left behind.
pub type Launched<Placement, Partial, Current> =
    Result<LaunchResult<Placement, Current>, LaunchFailure<Partial, Current>>;

/// The pane an `attach` takes: the server it sits on and the pane ID a person named.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachRequest<Endpoint> {
    pub endpoint: Endpoint,
    pub pane: String,
}

impl<Partial, Current> LaunchFailure<Partial, Current> {
    /// A launch that stopped on `error`, which the observation of `kind` carries.
    pub fn new(
        kind: ObservationKind,
        partial: Option<Partial>,
        cleanup: Option<CleanupResult>,
        error: impl Display,
    ) -> Self {
        let source = error.to_string();
        Self {
            observation: Box::new(observation(kind, None, source.clone(), None)),
            partial: partial.map(Box::new),
            cleanup,
            source,
        }
    }
}

/// What `attach` found: the pane as a Placement, how that pane reads right now, and the
/// selector `self` answers inside it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachedPane<Placement, Current> {
    pub placement: Placement,
    pub current: Current,
    pub selector: Value,
}

/// What one `stop` or `kill` tried and what it saw afterwards, which core records as
/// `lifecycle.stop` / `lifecycle.kill`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlResult<Current> {
    pub attempt: String,
    pub observed: ObservationResult<Current>,
}

impl<Current> ControlResult<Current> {
    fn response(&self) -> ControlResponse {
        ControlResponse {
            attempt: self.attempt.clone(),
            observation: self.observed.report(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub cleaned: bool,
    pub status: CleanupStatus,
    pub detail: String,
}

impl CleanupResult {
    /// A cleanup that could not tell what became of the resource.
    pub fn unknown(detail: impl Into<String>) -> Self {
        Self {
            cleaned: false,
            status: CleanupStatus::Unknown,
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CleanupStatus {
    Removed,
    AlreadyAbsent,
    Retained,
    Unknown,
}

pub fn retained(detail: &str) -> CleanupResult {
    CleanupResult {
        cleaned: false,
        status: CleanupStatus::Retained,
        detail: detail.to_owned(),
    }
}

pub fn already_cleaned(detail: &str) -> CleanupResult {
    CleanupResult {
        cleaned: false,
        status: CleanupStatus::AlreadyAbsent,
        detail: detail.to_owned(),
    }
}

/// A command the Provider could not run at all.
#[derive(Debug, Error)]
#[error("failed to run {program}: {source}")]
pub struct CommandFailed {
    pub program: String,
    #[source]
    pub source: std::io::Error,
}

impl CommandFailed {
    /// Whether the command started and was killed at [`RUN_DEADLINE`]. What it was asked
    /// may have taken effect.
    pub fn timed_out(&self) -> bool {
        self.source.kind() == std::io::ErrorKind::TimedOut
    }
}

/// The process table could not be read.
pub fn ps_failed(source: std::io::Error) -> CommandFailed {
    CommandFailed {
        program: "ps".to_owned(),
        source,
    }
}

/// How long one command of the product may run. A client whose server does not answer
/// waits on it, and is killed at this deadline well inside the one core gives the whole
/// operation.
pub const RUN_DEADLINE: Duration = Duration::from_secs(10);

/// Runs one command of the product a Provider drives, with nothing on its stdin and both
/// outputs read back once it exits. A command can start the product's server, which lives
/// on with whatever it inherits and shows it to every pane, so what names one Session stays
/// out. A command still running at [`RUN_DEADLINE`] is killed with what it started, and
/// fails with `TimedOut`: it may have done what it was asked before it stopped answering.
pub fn run(program: &Path, arguments: impl FnOnce(&mut Command)) -> Result<Output, CommandFailed> {
    run_within(program, arguments, RUN_DEADLINE)
}

fn run_within(
    program: &Path,
    arguments: impl FnOnce(&mut Command),
    limit: Duration,
) -> Result<Output, CommandFailed> {
    let mut command = crate::process::command(program);
    for name in [
        FELD_SESSION_TOKEN_ENV,
        FELD_SESSION_ID_ENV,
        FELD_ACTOR_ENV,
        FELD_WORKSPACE_ENV,
    ] {
        command.env_remove(name);
    }
    arguments(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let failed = |source| CommandFailed {
        program: program.display().to_string(),
        source,
    };
    let mut child = command.spawn().map_err(failed)?;
    let output = exchange(&mut child, &[], limit);
    if output.is_err() && matches!(child.try_wait(), Ok(None)) {
        kill_with_what_it_started(&mut child);
    }
    output.map_err(failed)
}

/// Kills a command that missed its deadline, and what it started in the Plugin's process
/// group. The command stays in that group, so core's kill at its own deadline reaches it
/// too. A process that left the group, such as a server that detached itself, stays.
fn kill_with_what_it_started(child: &mut Child) {
    let pid = child.id();
    let send = |pid: u32, signal| {
        // SAFETY: kill has no memory effects.
        unsafe { libc::kill(pid as libc::pid_t, signal) };
    };
    // A stopped command starts nothing more while its descendants are read.
    send(pid, libc::SIGSTOP);
    // SAFETY: getpgrp has no memory effects.
    let group = unsafe { libc::getpgrp() }.to_string();
    // core shows what the Plugin writes on stderr when the operation fails.
    let descendants = crate::process::descendants(pid).unwrap_or_else(|error| {
        eprintln!("the processes started by {pid} were left running: {error}");
        Vec::new()
    });
    for descendant in descendants {
        if descendant == pid {
            continue;
        }
        match crate::process::field(descendant, "pgid=") {
            Ok(Some(pgid)) if pgid.trim() == group => send(descendant, libc::SIGKILL),
            Ok(_) => {}
            Err(error) => eprintln!("process {descendant} was left running: {error}"),
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Sends `signal` to `pid`. A kill that ran and refused answers what it said.
pub fn signal(pid: u32, signal: &str) -> Result<Result<(), String>, CommandFailed> {
    let output = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .output()
        .map_err(|source| CommandFailed {
            program: "kill".to_owned(),
            source,
        })?;
    Ok(if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "kill -{signal} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    })
}

/// Whether the Provider may cause a side effect on what it just observed. Only a resource
/// that answered as the recorded incarnation is stopped, killed, or closed.
pub fn acts_on<Current>(observed: &ObservationResult<Current>) -> bool {
    matches!(
        observed.kind,
        ObservationKind::Running | ObservationKind::Relocated
    )
}

/// What a stop or a kill answers when there is nothing it may act on.
pub const NOTHING_TO_KILL: &str = "nothing to kill";

/// Whether a selector names a server by its socket path rather than by name.
pub fn names_a_path(selector: &str) -> bool {
    Path::new(selector).is_absolute() || selector.contains('/')
}

/// The value of one option, which is a non-empty string when it is given at all.
pub fn option_text(
    provider: &str,
    given: &Map<String, Value>,
    key: &str,
) -> std::result::Result<Option<String>, String> {
    match given.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.clone())),
        Some(_) => Err(format!(
            "{provider} option '{key}' must be a non-empty string"
        )),
    }
}

pub fn selector_text(selector: Option<&Value>, key: &str) -> Option<String> {
    selector?.get(key)?.as_str().map(str::to_owned)
}

/// Whether `name` is an environment variable a shell can assign.
pub fn valid_env_name(name: &str) -> bool {
    let mut characters = name.chars();
    matches!(characters.next(), Some('A'..='Z' | 'a'..='z' | '_'))
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// One Execution Provider, as [`serve`] runs its operations.
pub trait ProviderBackend: Default {
    /// The Provider name, which its messages and its executable carry.
    const NAME: &'static str;
    /// The server a Placement talks to.
    type Endpoint;
    /// The three axes `place` resolves.
    type PrepareRequest;
    /// What `place` resolves before anything is created, which `launch` takes back.
    type Prepared: Serialize + DeserializeOwned;
    /// The Placement core stores.
    type Placement: Clone + Serialize + DeserializeOwned;
    /// The Placement of what a failed launch created before it stopped.
    type Partial: Serialize + DeserializeOwned;
    /// The names the Provider answers for a Placement right now.
    type Current;
    /// One reading of the product and the process table, shared by the Placements one
    /// `observe` judges.
    type Snapshot: Default;
    type Error: Display;

    /// `self`: the selector of the resource the caller runs in, if any.
    fn self_selector() -> Option<Value>;
    /// `resolve`: fill the options the caller left out from its own position and refuse
    /// the combinations the product cannot place. The answer is what the launchSpec keeps.
    fn resolve_options(
        options: Option<&Value>,
        caller: Option<&Value>,
    ) -> std::result::Result<Value, String>;
    fn prepare_request(
        options: Option<&Value>,
        workdir: &Path,
    ) -> Result<Self::PrepareRequest, Self::Error>;
    fn attach_request(
        selector: &Value,
    ) -> std::result::Result<AttachRequest<Self::Endpoint>, String>;
    /// `summary`: the Placement as a person reads it.
    fn summary(placement: &Value, current: Option<&Self::Current>) -> String;
    /// `attachCommand`: the command that puts a terminal on the stored Placement, at where
    /// the reading found it when there was one.
    fn attach_command(placement: &Value, current: Option<&Self::Current>) -> Option<String>;
    fn workdir(placement: &Self::Placement) -> &Path;

    fn prepare(&self, request: Self::PrepareRequest) -> Result<Self::Prepared, Self::Error>;
    fn launch(
        &self,
        request: LaunchRequest<Self::Prepared>,
    ) -> Launched<Self::Placement, Self::Partial, Self::Current>;
    fn attach(
        &self,
        request: AttachRequest<Self::Endpoint>,
    ) -> Result<AttachedPane<Self::Placement, Self::Current>, Self::Error>;
    /// `candidates`: the existing resources on the server the caller's `self` answer names,
    /// each with the process in front of it.
    fn candidates(&self, selector: Option<&Value>) -> std::result::Result<Vec<Candidate>, String> {
        let _ = selector;
        Err(format!("{} has no operation 'candidates'", Self::NAME))
    }
    fn observe_checked(
        &self,
        placement: &Self::Placement,
        snapshot: &mut Self::Snapshot,
    ) -> Result<ObservationResult<Self::Current>, Self::Error>;
    fn stop(&self, placement: &Self::Placement) -> ControlResult<Self::Current>;
    fn kill(&self, placement: &Self::Placement) -> ControlResult<Self::Current>;
    fn cleanup_checked(&self, placement: &Self::Placement) -> Result<CleanupResult, Self::Error>;
    /// Types one line of the `stopInput` column into the Placement's pane.
    fn send_input(&self, placement: &Self::Placement, input: &str) -> Result<(), Self::Error>;
    /// Re-observes after a signal or an input until the Placement settles, or the wait is
    /// spent.
    fn observe_after_signal(&self, placement: &Self::Placement)
    -> ObservationResult<Self::Current>;
    /// The process a signal reaches.
    fn signalled(placement: &Self::Placement) -> Option<u32>;
    fn signal(&self, pid: u32, signal: &str) -> Result<(), Self::Error>;

    /// `stop` with a `stopInput`: the column typed into the pane, again while the Runtime
    /// stays, at most [`STOP_INPUT_ROUNDS`] times.
    fn graceful_stop(
        &self,
        placement: &Self::Placement,
        inputs: &[String],
    ) -> ControlResult<Self::Current> {
        let mut observed = self.observe(placement);
        if !acts_on(&observed) {
            return ControlResult {
                attempt: NOTHING_TO_KILL.to_owned(),
                observed,
            };
        }
        let attempt = format!(
            "{} sent {} to the pane",
            Self::NAME,
            inputs
                .iter()
                .filter(|input| !input.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" and ")
        );
        for input in inputs.iter().cycle().take(inputs.len() * STOP_INPUT_ROUNDS) {
            match self.send_input(placement, input) {
                Ok(()) => observed = self.observe_after_signal(placement),
                Err(error) => {
                    observed = observation(
                        ObservationKind::Unknown,
                        None,
                        error.to_string(),
                        observed.current,
                    );
                    break;
                }
            }
            if !acts_on(&observed) {
                break;
            }
        }
        ControlResult { attempt, observed }
    }

    /// Sends `signal` to the process of a Placement that answers as the recorded
    /// incarnation, and reports what it settled on.
    fn control(
        &self,
        placement: &Self::Placement,
        signal: &str,
        attempt: &str,
    ) -> ControlResult<Self::Current> {
        let before = self.observe(placement);
        let Some(pid) = acts_on(&before)
            .then(|| Self::signalled(placement))
            .flatten()
        else {
            return ControlResult {
                attempt: NOTHING_TO_KILL.to_owned(),
                observed: before,
            };
        };
        if let Err(error) = self.signal(pid, signal) {
            return ControlResult {
                attempt: attempt.to_owned(),
                observed: observation(
                    ObservationKind::Unknown,
                    None,
                    error.to_string(),
                    before.current,
                ),
            };
        }
        ControlResult {
            attempt: attempt.to_owned(),
            observed: self.observe_after_signal(placement),
        }
    }

    fn observe(&self, placement: &Self::Placement) -> ObservationResult<Self::Current> {
        self.observe_one(placement, &mut Self::Snapshot::default())
    }

    /// Observes a whole sequence of Placements against one reading of the product and the
    /// process table. The answers come back in the order they were given.
    fn observe_all(&self, placements: &[Self::Placement]) -> Vec<ObservationResult<Self::Current>> {
        let mut snapshot = Self::Snapshot::default();
        placements
            .iter()
            .map(|placement| self.observe_one(placement, &mut snapshot))
            .collect()
    }

    fn observe_one(
        &self,
        placement: &Self::Placement,
        snapshot: &mut Self::Snapshot,
    ) -> ObservationResult<Self::Current> {
        match self.observe_checked(placement, snapshot) {
            Ok(observation) => observation,
            Err(error) => observation(ObservationKind::Unknown, None, error.to_string(), None),
        }
    }

    fn cleanup(&self, placement: &Self::Placement) -> CleanupResult {
        match self.cleanup_checked(placement) {
            Ok(result) => result,
            Err(error) => CleanupResult::unknown(error.to_string()),
        }
    }
}

/// Runs the operation `feld-plugin-<name> <operation>` names: one JSON request from
/// stdin, one JSON response to stdout. Exit 0 means this Provider answered, whatever the
/// answer says; a non-zero exit means it could not answer and stderr holds the reason.
pub fn serve<B: ProviderBackend>() -> ExitCode {
    let Some(operation) = std::env::args().nth(1) else {
        eprintln!("usage: feld-plugin-{} <operation>", B::NAME);
        return ExitCode::from(2);
    };
    if operation == launch_env::OPERATION {
        return launch_env::receive(std::env::args().skip(2));
    }
    let provider = B::default();
    io::exit(match operation.as_str() {
        "--version" => io::version(B::NAME),
        "self" => io::encode(&SelfResponse {
            selector: B::self_selector(),
            notes: Vec::new(),
        }),
        "resolve" => io::answer(|request: ResolveRequest| {
            Ok(ResolveResponse {
                options: B::resolve_options(Some(&request.options), request.caller.as_ref())
                    .map_err(|message| anyhow!(message))?,
            })
        }),
        "place" => io::answer(|request: PlaceRequest| place(&provider, &request)),
        "launch" => io::answer(|request| launch(&provider, request)),
        "observe" => io::answer(|request: ObserveRequest| Ok(observe(&provider, &request))),
        "stop" => io::answer(|request: StopRequest| stop(&provider, &request)),
        "kill" => io::answer(|request: PlacementRequest| {
            Ok(provider
                .kill(&placement::<B>(&request.placement)?)
                .response())
        }),
        "cleanup" => io::answer(|request: PlacementRequest| {
            let cleaned = provider.cleanup(&placement::<B>(&request.placement)?);
            // Cleanup answers nothing, so a resource the Provider kept is said here: core
            // drops the stderr of a Plugin that answered, and a person running the
            // operation by hand reads the reason.
            if !cleaned.cleaned {
                eprintln!(
                    "{} kept the resource of this Placement: {}",
                    B::NAME,
                    cleaned.detail
                );
            }
            Ok(Map::new())
        }),
        "attach" => io::answer(|request| attach(&provider, &request)),
        "candidates" => io::answer(|request: CandidatesRequest| {
            Ok(CandidatesResponse {
                candidates: provider
                    .candidates(request.selector.as_ref())
                    .map_err(|message| anyhow!(message))?,
            })
        }),
        other => Err(anyhow!("{} has no operation '{other}'", B::NAME)),
    })
}

/// `place`: the three axes resolved to what the launch re-checks before it creates
/// anything.
fn place<B: ProviderBackend>(provider: &B, request: &PlaceRequest) -> Result<PlaceResponse> {
    let pending = B::prepare_request(Some(&request.options), Path::new(&request.workdir))
        .map_err(|error| anyhow!("{error}"))?;
    let prepared = provider
        .prepare(pending)
        .map_err(|error| anyhow!("{error}"))?;
    // Nothing is created yet, so the Provider answers no name for it.
    let placement = described::<B>(serde_json::to_value(&prepared)?, None);
    Ok(PlaceResponse {
        summary: B::summary(&placement, None),
        placement,
    })
}

/// `launch`: create the resource, put the command in it, and report what the first
/// observation saw. A failure answers with the observation it is sure of, so that core can
/// close the Session rather than treat the Provider as unreachable.
fn launch<B: ProviderBackend>(
    provider: &B,
    request: ProviderLaunchRequest,
) -> Result<ProviderLaunchResponse> {
    let prepared: B::Prepared = serde_json::from_value(request.placement)
        .with_context(|| format!("decode the placed {} Placement", B::NAME))?;
    let launched = provider.launch(LaunchRequest {
        prepared,
        resource_name: request.name,
        command: request.command,
        env: request.env.into_iter().collect(),
    });
    match launched {
        Ok(LaunchResult {
            placement,
            observation,
        }) => {
            let current = observation.current.as_ref();
            let placement = described::<B>(serde_json::to_value(&placement)?, current);
            Ok(ProviderLaunchResponse {
                summary: B::summary(&placement, current),
                attach_command: B::attach_command(&placement, current),
                placement: Some(placement),
                observation: observation.report(),
            })
        }
        Err(failure) => {
            let mut observation = failure.observation.report();
            if let Some(cleanup) = &failure.cleanup {
                observation.detail =
                    Some(format!("{}; cleanup: {}", failure.source, cleanup.detail));
            }
            // `gone` is the Provider sure that nothing it created is left, so the command
            // never began: answering no Placement is how core reads the start as failed.
            let partial = match observation.kind {
                ObservationKind::Gone => None,
                _ => failure.partial.as_deref(),
            };
            Ok(ProviderLaunchResponse {
                placement: partial
                    .map(serde_json::to_value)
                    .transpose()?
                    .map(|partial| described::<B>(partial, None)),
                observation,
                summary: String::new(),
                attach_command: None,
            })
        }
    }
}

/// `observe`: one answer per Placement, in the order they came. The Placements core hands
/// over at once are judged against one reading of the product and the process table.
pub fn observe<B: ProviderBackend>(provider: &B, request: &ObserveRequest) -> ObserveResponse {
    let decoded: Vec<_> = request.placements.iter().map(placement::<B>).collect();
    let readable: Vec<B::Placement> = decoded.iter().flatten().cloned().collect();
    let mut observed = provider.observe_all(&readable).into_iter();
    ObserveResponse {
        results: request
            .placements
            .iter()
            .zip(&decoded)
            .map(|(encoded, decoded)| match decoded {
                Ok(_) => {
                    let observation = observed.next().expect("one answer per Placement");
                    ObserveResult {
                        summary: B::summary(encoded, observation.current.as_ref()),
                        attach_command: B::attach_command(encoded, observation.current.as_ref()),
                        observation: observation.report(),
                    }
                }
                Err(error) => ObserveResult {
                    observation: ObservationReport::new(
                        ObservationKind::Lost,
                        unfinished::<B>(encoded).map_or_else(
                            || format!("{} incarnation is unavailable: {error:#}", B::NAME),
                            str::to_owned,
                        ),
                    ),
                    summary: String::new(),
                    attach_command: None,
                },
            })
            .collect(),
    }
}

/// `stop`: the `stopInput` column typed into the pane when there is one, a signal
/// otherwise.
pub fn stop<B: ProviderBackend>(provider: &B, request: &StopRequest) -> Result<ControlResponse> {
    let placement = placement::<B>(&request.placement)?;
    let result = match &request.input {
        Some(input) => provider.graceful_stop(&placement, input),
        None => provider.stop(&placement),
    };
    Ok(result.response())
}

/// `attach`: the existing pane a selector names, with the process in its foreground.
/// Whether that process is the Runtime the caller asked for is the Runtime Adapter's
/// `match`.
fn attach<B: ProviderBackend>(
    provider: &B,
    request: &ProviderAttachRequest,
) -> Result<ProviderAttachResponse> {
    let attach = B::attach_request(&request.selector).map_err(|message| anyhow!(message))?;
    let AttachedPane {
        placement,
        current,
        selector,
    } = provider
        .attach(attach)
        .map_err(|error| anyhow!("{error}"))?;
    let workdir = B::workdir(&placement).display().to_string();
    let placement = described::<B>(serde_json::to_value(&placement)?, Some(&current));
    // The Runtime process sits under the key core reads for every Provider.
    let process = placement
        .get("process")
        .and_then(|process| serde_json::from_value::<ProcessIncarnation>(process.clone()).ok())
        .map(|runtime| crate::process::snapshot(runtime.pid))
        .transpose()
        .with_context(|| format!("read the {} pane foreground process", B::NAME))?
        .flatten();
    Ok(ProviderAttachResponse {
        workdir,
        summary: B::summary(&placement, Some(&current)),
        attach_command: B::attach_command(&placement, Some(&current)),
        placement,
        process,
        selector,
    })
}

/// The Placement core stores, with the two things every listing shows written into it: core
/// reads them back without running an operation, and the next operation that reads the
/// product refreshes them.
fn described<B: ProviderBackend>(mut placement: Value, current: Option<&B::Current>) -> Value {
    let summary = B::summary(&placement, current);
    let attach_command = B::attach_command(&placement, current);
    if let Some(object) = placement.as_object_mut() {
        object.insert("summary".to_owned(), Value::String(summary));
        object.insert(
            "attachCommand".to_owned(),
            attach_command.map_or(Value::Null, Value::String),
        );
    }
    placement
}

fn placement<B: ProviderBackend>(placement: &Value) -> Result<B::Placement> {
    serde_json::from_value(placement.clone()).map_err(|error| match unfinished::<B>(placement) {
        Some(reason) => anyhow!(reason),
        None => anyhow::Error::new(error).context(format!("decode the {} Placement", B::NAME)),
    })
}

/// Why a Placement the launch left short of complete holds no incarnation: the Placement
/// of what a failed launch created, or the one `place` resolved before anything was.
fn unfinished<B: ProviderBackend>(placement: &Value) -> Option<&'static str> {
    if serde_json::from_value::<B::Partial>(placement.clone()).is_ok() {
        Some("the launch did not finish recording an incarnation")
    } else if serde_json::from_value::<B::Prepared>(placement.clone()).is_ok() {
        Some("the launch never recorded an incarnation")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_that_does_not_exit_by_the_deadline_is_killed_with_what_it_started() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("client");
        let pids = directory.path().join("pids");
        std::fs::write(
            &program,
            "#!/bin/sh\n[ -n \"$1\" ] || exit 0\nsleep 300 &\necho $$ $! > \"$1\"\nwait\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        // macOS inspects an executable the first time it runs, which takes seconds while the
        // machine builds and runs others. Running it once here keeps that out of the deadline.
        let _ = Command::new(&program).status();
        let error = run_within(
            &program,
            |command| {
                command.arg(&pids);
            },
            Duration::from_secs(3),
        )
        .unwrap_err();
        assert!(error.timed_out());
        // The child the command started is no longer anyone's, so whoever adopts it reaps
        // it a moment later.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        for pid in std::fs::read_to_string(&pids).unwrap().split_whitespace() {
            let pid: u32 = pid.parse().unwrap();
            while crate::process::field(pid, "pid=").unwrap().is_some() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "process {pid} is still there"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}
