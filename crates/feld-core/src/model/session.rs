use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{ActorKind, RecordKind, Requester};

/// What a Runtime Adapter knows about the Runtime it started or found: the Runtime's own
/// session id, where it writes its records, and the model it is running.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeIdentity {
    pub session_id: String,
    pub transcript_path: String,
    pub model: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StartKind {
    Start,
    Replacement,
    Attach,
    Schedule,
}

impl StartKind {
    /// How a requested start of `generation` begins: the first generation starts the Actor,
    /// and a later one replaces the one before it.
    pub fn for_generation(generation: u64) -> Self {
        if generation == 1 {
            Self::Start
        } else {
            Self::Replacement
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Replacement => "replacement",
            Self::Attach => "attach",
            Self::Schedule => "schedule",
        }
    }
}

impl FromStr for StartKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "start" => Ok(Self::Start),
            "replacement" => Ok(Self::Replacement),
            "attach" => Ok(Self::Attach),
            "schedule" => Ok(Self::Schedule),
            _ => Err(format!("unknown Session start kind '{value}'")),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Ownership {
    Owned,
    Attached,
}

impl Ownership {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owned => "owned",
            Self::Attached => "attached",
        }
    }
}

impl FromStr for Ownership {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "owned" => Ok(Self::Owned),
            "attached" => Ok(Self::Attached),
            _ => Err(format!("unknown Session ownership '{value}'")),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EndKind {
    Running,
    Stopped,
    Exited,
    Unknown,
}

impl EndKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Exited => "exited",
            Self::Unknown => "unknown",
        }
    }
}

/// How a Session whose execution is gone ended: stopped when a stop or kill was requested,
/// and exited on its own otherwise.
pub fn end_kind_for_gone(requested: bool) -> EndKind {
    if requested {
        EndKind::Stopped
    } else {
        EndKind::Exited
    }
}

impl FromStr for EndKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "running" => Ok(Self::Running),
            "stopped" => Ok(Self::Stopped),
            "exited" => Ok(Self::Exited),
            "unknown" => Ok(Self::Unknown),
            _ => Err(format!("unknown Session end kind '{value}'")),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Turn {
    Responding,
    WaitingInput,
    Unavailable,
}

impl Turn {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Responding => "responding",
            Self::WaitingInput => "waiting-input",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Running,
    Stopped,
    Succeeded,
    Failed,
    Exited,
    Ended,
    Unknown,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Exited => "exited",
            Self::Ended => "ended",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservationKind {
    Running,
    Relocated,
    Gone,
    Lost,
    Mismatched,
    Unknown,
}

impl ObservationKind {
    /// Whether the observation leaves Feld unable to reach the execution, so that nothing
    /// is left to stop.
    pub fn is_unavailable(self) -> bool {
        matches!(self, Self::Lost | Self::Mismatched)
    }

    /// Whether the observation shows the execution began, which settles the start.
    pub fn settles_start(self) -> bool {
        self == Self::Running
    }

    /// Whether the observation shows the execution ended, which closes an open Session.
    pub fn ends_execution(self) -> bool {
        self == Self::Gone
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Relocated => "relocated",
            Self::Gone => "gone",
            Self::Lost => "lost",
            Self::Mismatched => "mismatched",
            Self::Unknown => "unknown",
        }
    }
}

impl FromStr for ObservationKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "running" => Ok(Self::Running),
            "relocated" => Ok(Self::Relocated),
            "gone" => Ok(Self::Gone),
            "lost" => Ok(Self::Lost),
            "mismatched" => Ok(Self::Mismatched),
            "unknown" => Ok(Self::Unknown),
            _ => Err(format!("unknown observation kind '{value}'")),
        }
    }
}

/// What admission does with the current Session of an Actor that another Session would
/// replace, by the Session's latest observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    /// Nothing can be reached: the Session closes as `unknown`.
    CloseUnknown,
    /// The execution is gone: the Session closes as it ended.
    CloseExited,
    /// The Session may still run, so the new one is refused.
    Refuse,
}

pub fn admission(observation: Option<ObservationKind>) -> Admission {
    match observation {
        Some(kind) if kind.is_unavailable() => Admission::CloseUnknown,
        Some(ObservationKind::Gone) => Admission::CloseExited,
        _ => Admission::Refuse,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub kind: ObservationKind,
    pub at: String,
    /// The instant the latest kind was recorded; a Session's latest observation carries it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    #[serde(rename = "type")]
    pub execution_type: String,
    /// The Runtime Adapter this Session runs, for an `agent-runtime`. Core reads the
    /// Adapter's manifest through it, for the `stopInput` of `feld session stop` among others.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    pub label: String,
    pub command: Option<Vec<String>>,
    pub ai: bool,
    #[serde(skip)]
    pub telemetry_collector_available: Option<bool>,
    /// Whether the Runtime process started with the telemetry environment Feld prepares
    /// for it. Claude Code applies its settings `env` at startup, so an attached Claude Code
    /// that was running before `feld actor attach` wrote the settings exports no telemetry.
    #[serde(skip)]
    pub telemetry_runtime_prepared: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub provider: String,
    pub node: String,
    pub workdir: String,
    /// What the Execution Provider returned for this binding, stored as it came. The
    /// incarnation that names the running resource lives in here, and core passes the whole
    /// object back to the Provider.
    pub placement: Value,
}

/// The Runtime process a Placement names under the `process` key. The key is core's, so
/// that any Provider can say which process an `attached` Session took, and the shape is
/// the one core compares: one process, told apart from a later one that reuses its pid.
pub use crate::process::ProcessIncarnation as PlacementProcess;

impl Placement {
    pub fn process(&self) -> Option<PlacementProcess> {
        self.key("process")
    }

    /// The process of the `<command>` an `owned` Session runs, under the `program` key, which
    /// is core's in the same way as `process`.
    pub fn program(&self) -> Option<PlacementProcess> {
        self.key("program")
    }

    /// The process that runs this Session: the Runtime process an `attached` Session took,
    /// or the `<command>` an `owned` one runs. An attach compares a Runtime with it.
    pub fn runtime_process(&self) -> Option<PlacementProcess> {
        self.process().or_else(|| self.program())
    }

    fn key(&self, key: &str) -> Option<PlacementProcess> {
        serde_json::from_value(self.placement.get(key)?.clone()).ok()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleAction {
    pub accepted: String,
    pub requester_session_id: Option<String>,
    pub attempt: Option<String>,
    pub observed: Option<Observation>,
}

impl LifecycleAction {
    /// The action accepted at `at`. When the latest `observation` leaves nothing to reach,
    /// the action is settled on acceptance.
    pub fn accept(
        at: &str,
        requester_session_id: Option<&str>,
        observation: Option<&Observation>,
    ) -> Self {
        let unavailable = observation.filter(|observation| observation.kind.is_unavailable());
        Self {
            accepted: at.to_owned(),
            requester_session_id: requester_session_id.map(str::to_owned),
            attempt: unavailable.map(|_| "nothing to kill".to_owned()),
            observed: unavailable.cloned(),
        }
    }

    /// Whether the Execution Provider has yet to carry the action out.
    pub fn is_pending(&self) -> bool {
        self.attempt.is_none() && self.observed.is_none()
    }

    /// Whether the action is pending, or the Execution Provider could not tell what came of
    /// it, so that the same request carries it out again.
    pub fn is_unsettled(&self) -> bool {
        self.is_pending()
            || self
                .observed
                .as_ref()
                .is_some_and(|observed| observed.kind == ObservationKind::Unknown)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleActionKind {
    Stop,
    Kill,
}

impl LifecycleActionKind {
    pub fn record_kind(self) -> RecordKind {
        match self {
            Self::Stop => RecordKind::StopRequested,
            Self::Kill => RecordKind::KillRequested,
        }
    }
}

/// What a stop or kill request does to a Session's lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleAcceptance {
    /// An action that covers the request was accepted and carried out before.
    Already,
    /// An action that covers the request was accepted and is still pending, or its outcome
    /// is unknown.
    Resume,
    /// The request is a new action.
    Accept,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLifecycle {
    pub start_settled: Option<String>,
    pub start_failed: Option<String>,
    pub stop: Option<LifecycleAction>,
    pub kill: Option<LifecycleAction>,
}

impl SessionLifecycle {
    /// A kill covers a stop, so a stop after a kill is already done.
    pub fn accept(&self, kind: LifecycleActionKind) -> LifecycleAcceptance {
        let action = match kind {
            LifecycleActionKind::Stop if self.kill.is_some() => {
                return LifecycleAcceptance::Already;
            }
            LifecycleActionKind::Stop => self.stop.as_ref(),
            LifecycleActionKind::Kill => self.kill.as_ref(),
        };
        match action {
            None => LifecycleAcceptance::Accept,
            Some(action) if action.is_unsettled() => LifecycleAcceptance::Resume,
            Some(_) => LifecycleAcceptance::Already,
        }
    }

    /// The accepted action the Execution Provider has yet to carry out, a kill first.
    pub fn pending(&self) -> Option<LifecycleActionKind> {
        if self.kill.as_ref().is_some_and(LifecycleAction::is_pending) {
            Some(LifecycleActionKind::Kill)
        } else if self.stop.as_ref().is_some_and(LifecycleAction::is_pending) {
            Some(LifecycleActionKind::Stop)
        } else {
            None
        }
    }

    /// Whether a stop or kill was requested.
    pub fn requested(&self) -> bool {
        self.stop.is_some() || self.kill.is_some()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub workspace: String,
    /// The Actor's `id`; `actor` is its `name`.
    pub actor_id: String,
    pub actor: String,
    pub actor_kind: ActorKind,
    pub generation: u64,
    pub start: String,
    pub end: Option<String>,
    pub start_kind: StartKind,
    pub ownership: Ownership,
    pub end_kind: EndKind,
    pub exit_code: Option<i32>,
    pub planned: Option<String>,
    pub execution: Execution,
    pub placement: Placement,
    pub lifecycle: SessionLifecycle,
    pub observation: Option<Observation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawned_by: Option<SpawnOrigin>,
    pub outcome: Outcome,
    /// The [turn] of a current `agent-runtime` Session, which the reads derive from its
    /// transcript.
    ///
    /// [turn]: https://github.com/crestai-labs/feld/blob/main/docs/spec/concepts/glossary.md#turn
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<Turn>,
    /// The `at` of the transcript entry the turn is read from. A Session with a turn has
    /// it, null when that entry has no `at` or the turn is `unavailable`.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_entry_at: Option<Option<String>>,
}

/// A field that is present, null or not, as against one left out.
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// The spawned record that started this Session, projected onto the Session read.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnOrigin {
    pub spawner: Requester,
    pub spawner_session_id: Option<String>,
}

impl Session {
    pub fn derive(&mut self) {
        self.outcome = derive_outcome(self);
    }

    /// Whether this is an open `owned` agent-runtime Session its Runtime has not linked to
    /// yet: no Runtime identity has arrived, which `model` carries once one does. A Runtime
    /// that stays so is waiting for a person in its pane before its hooks run.
    pub fn is_unlinked(&self) -> bool {
        self.end.is_none()
            && self.ownership == Ownership::Owned
            && self.execution.execution_type == "agent-runtime"
            && self.model.is_none()
    }
}

pub fn derive_outcome(session: &Session) -> Outcome {
    if session.end.is_none() {
        return Outcome::Running;
    }
    match session.end_kind {
        EndKind::Unknown => Outcome::Unknown,
        EndKind::Stopped => Outcome::Stopped,
        EndKind::Running => Outcome::Running,
        EndKind::Exited if session.ownership == Ownership::Attached => Outcome::Ended,
        EndKind::Exited if session.actor_kind == ActorKind::Agent => Outcome::Exited,
        EndKind::Exited if session.exit_code == Some(0) => Outcome::Succeeded,
        EndKind::Exited if session.exit_code.is_some() => Outcome::Failed,
        EndKind::Exited => Outcome::Unknown,
    }
}
