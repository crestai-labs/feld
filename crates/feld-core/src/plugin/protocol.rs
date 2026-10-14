//! The request and response of every Plugin operation.
//!
//! These are the types the [`RuntimeAdapter`](super::RuntimeAdapter) and
//! [`ExecutionProvider`](super::ExecutionProvider) traits speak. A Plugin implementation
//! relays them as JSON to the Plugin's executable, so every field here is the field of the
//! same name in `plugin.md`. Items the tables do not name are the Plugin's own: they travel
//! inside the untyped values (`placement`, `selector`, `options`, `runtimeSettings`) and
//! core does not read them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::process::ProcessSnapshot;

pub use crate::model::{ObservationKind, RuntimeIdentity};

// ---------------------------------------------------------------- Runtime Adapter

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLaunchRequest {
    pub session: String,
    pub actor: String,
    pub workspace: String,
    pub workdir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_settings: Option<Value>,
    /// The launchSpec `trustWorkdir`: whether the Runtime may read the workdir's own settings
    /// without asking.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trust_workdir: bool,
    /// The launchSpec `env`, which the Runtime starts in beneath the Adapter's own variables.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    pub session_dir: PathBuf,
    pub token: String,
    pub collector: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLaunchResponse {
    pub command: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<RuntimeIdentity>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRequest {
    pub process: ProcessSnapshot,
    /// The OTLP endpoint of the daemon's Collector, when it has one.
    #[serde(default)]
    pub collector: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchResponse {
    pub matched: bool,
    /// Whether that process started in a shape that attributes its telemetry to the
    /// Session. A Runtime that always does leaves it out.
    #[serde(default)]
    pub telemetry_prepared: Option<bool>,
    /// Why `telemetry_prepared` is false, and what makes the telemetry arrive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityRequest {
    /// The Runtime process an attach from outside took, whose identity the Adapter reads
    /// from that process. None inside the Runtime, where the caller's environment tells it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<ProcessSnapshot>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityResponse {
    pub identity: Option<RuntimeIdentity>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachedRequest {
    pub session: String,
    pub token: String,
    /// The Runtime identity the attach delivered, or null when it delivered none.
    pub identity: Option<RuntimeIdentity>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachedResponse {
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareRequest {
    /// Whether to write the entries, or only to answer them.
    #[serde(default)]
    pub apply: bool,
    pub collector: Option<String>,
}

/// What `prepare` does with one entry of a Runtime's settings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PrepareAction {
    /// There is no value yet, and Feld's is written.
    Write,
    /// The value is Feld's, and stays.
    Keep,
    /// The value is carried by Feld's helper as `--chain`.
    Chain,
    /// The value is kept in the prepare record and Feld's replaces it.
    Replace,
    /// Feld's element joins the value, a list, beside the elements there.
    Add,
}

impl PrepareAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Keep => "keep",
            Self::Chain => "chain",
            Self::Replace => "replace",
            Self::Add => "add",
        }
    }

    /// Whether writing this entry takes the person's consent: it changes a value of theirs,
    /// or widens what the Runtime may do.
    pub fn needs_consent(self) -> bool {
        matches!(self, Self::Chain | Self::Replace | Self::Add)
    }
}

/// One thing `prepare` writes, or would write, in a Runtime's configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareEntry {
    pub path: String,
    pub key: String,
    #[serde(default)]
    pub current: Option<String>,
    #[serde(default)]
    pub next: Option<String>,
    pub action: PrepareAction,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareResponse {
    #[serde(default)]
    pub entries: Vec<PrepareEntry>,
    /// Whether every entry is `keep`.
    #[serde(default)]
    pub prepared: bool,
    #[serde(default)]
    pub notes: Vec<String>,
    /// Notes about the running daemon, which hold while it keeps the environment it started
    /// in; a daemon started from the caller's environment settles them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub daemon_notes: Vec<String>,
    /// Whether the Runtime this attach found can attribute its telemetry to the Session.
    #[serde(default)]
    pub telemetry_prepared: Option<bool>,
    /// Why `telemetry_prepared` is false, and what makes the telemetry arrive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreRequest {
    /// Whether to write the entries, or only to answer them.
    #[serde(default)]
    pub apply: bool,
    /// The prepare record the Feld home keeps for this Runtime, or null when there is none.
    #[serde(default)]
    pub record: Option<Value>,
}

/// What `restore` does with one entry `prepare` wrote. Each answers one [`PrepareAction`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RestoreAction {
    /// The entry is Feld's alone, and goes (`write`).
    Remove,
    /// The value `prepare` replaced comes back (`replace`).
    Restore,
    /// The command carried by `--chain` comes back, the rest of the value as it is (`chain`).
    Unchain,
    /// The entry changed after `prepare` wrote it, and stays.
    Keep,
}

impl RestoreAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Remove => "remove",
            Self::Restore => "restore",
            Self::Unchain => "unchain",
            Self::Keep => "keep",
        }
    }
}

/// One thing `restore` takes back, or would take back, in a Runtime's configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreEntry {
    pub path: String,
    pub key: String,
    #[serde(default)]
    pub current: Option<String>,
    #[serde(default)]
    pub next: Option<String>,
    pub action: RestoreAction,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResponse {
    #[serde(default)]
    pub entries: Vec<RestoreEntry>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRequest {
    pub identity: RuntimeIdentity,
    pub after: Option<String>,
    pub limit: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptState {
    Missing,
    Lost,
    Readable,
    /// `after` names no entry of the transcript.
    #[serde(rename = "invalid-cursor")]
    InvalidCursor,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptResponse {
    pub state: TranscriptState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub records: Vec<Value>,
    #[serde(default)]
    pub next: Option<String>,
}

// -------------------------------------------------------------- Execution Provider

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfResponse {
    /// Where the caller's own environment sits inside this Provider, or null when the
    /// caller is not inside it. A Runtime Adapter answers the conversation of its Runtime
    /// the caller belongs to.
    pub selector: Option<Value>,
    /// What a person can do so that the caller's place is told without them, which the CLI
    /// shows when it asks them for it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveRequest {
    pub options: Value,
    pub caller: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveResponse {
    pub options: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceRequest {
    pub options: Value,
    pub node: String,
    pub workdir: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceResponse {
    pub placement: Value,
    #[serde(default)]
    pub summary: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLaunchRequest {
    pub placement: Value,
    /// The name core gives the resource this launch holds. It follows the Session, which
    /// the Provider cannot know.
    pub name: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub workdir: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLaunchResponse {
    /// The Placement with its incarnation. A launch that could not reach one leaves it out
    /// and core keeps what `place` wrote.
    #[serde(default)]
    pub placement: Option<Value>,
    pub observation: ObservationReport,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub attach_command: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObserveRequest {
    pub placements: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObserveResponse {
    pub results: Vec<ObserveResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObserveResult {
    pub observation: ObservationReport,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub attach_command: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopRequest {
    pub placement: Value,
    /// The Runtime Adapter's `stopInput`, or null for a `program` and a Runtime without
    /// one.
    pub input: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementRequest {
    pub placement: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlResponse {
    pub attempt: String,
    pub observation: ObservationReport,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAttachRequest {
    pub selector: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAttachResponse {
    pub placement: Value,
    pub workdir: String,
    /// The Runtime candidate in front of the resource. The Runtime Adapter's `match`
    /// decides whether it is the Runtime the caller asked for.
    pub process: Option<ProcessSnapshot>,
    /// The resource in the form `self` answers inside it, which the self index key is
    /// made from.
    pub selector: Value,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub attach_command: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidatesRequest {
    /// What the caller's `self` answered, whose server the candidates are read from, or
    /// null when the caller's environment names none.
    #[serde(default)]
    pub selector: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidatesResponse {
    #[serde(default)]
    pub candidates: Vec<Candidate>,
}

/// One existing resource an attach could take, with the process in front of it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// The `providerOptions` that name this resource, which a person passes back as
    /// `--<provider>-<key> <value>`.
    pub options: Value,
    pub summary: String,
    pub workdir: String,
    pub process: ProcessSnapshot,
}

/// What a Provider saw of an execution. Core records the time.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationReport {
    pub kind: ObservationKind,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub detail: Option<String>,
}

impl ObservationReport {
    pub fn new(kind: ObservationKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            exit_code: None,
            detail: Some(detail.into()),
        }
    }
}
