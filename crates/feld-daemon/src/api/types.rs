use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::console::SubAgentSession;
use crate::scheduler::SchedulerStatus;
use crate::store::Workspace;
use feld_core::model::{
    Actor, ActorKind, Browser, Grant, LaunchSpec, LaunchSpecPatch, Message, MessageQueued,
    MessageRead, MessageSummary, Placement, Record, Session,
};
use feld_core::plugin::PluginSummary;

pub use feld_core::model::MessageList;

/// An optional JSON object, which refuses every other JSON type.
fn optional_object<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Option::<serde_json::Map<String, Value>>::deserialize(deserializer)
        .map(|object| object.map(Value::Object))
}

/// A set that a request may leave out but never gives as `null`, so that a request that names
/// the field always carries the set.
fn present_set<'de, D, T>(deserializer: D) -> Result<Option<BTreeSet<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Ord,
{
    BTreeSet::deserialize(deserializer).map(Some)
}

/// A field that is present, null or not, as against one left out.
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    pub state: String,
    /// The version `feld --version` prints, of the executable this daemon runs.
    #[serde(default)]
    pub version: String,
    pub instance_id: String,
    pub pid: u32,
    pub home: String,
    pub api: String,
    pub started_at: String,
    pub server_time: String,
    pub history_start: Option<String>,
    pub timezone: String,
    #[serde(default)]
    pub runtimes: Vec<PluginSummary>,
    pub providers: Vec<PluginSummary>,
    pub nodes: Vec<String>,
    pub web_ui: Option<String>,
    /// The host names allowed with `--allow-host`, besides the loopback names.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    pub otlp: Option<String>,
    #[serde(default)]
    pub restored: RestoredCounts,
    #[serde(default)]
    pub scheduler: SchedulerStatus,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredCounts {
    pub actors: u64,
    pub running_sessions: u64,
    pub schedules: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkspaceList {
    pub workspaces: Vec<Workspace>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateWorkspaceRequest {
    pub key: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkspaceResponse {
    pub workspace: Workspace,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateActorRequest {
    pub name: String,
    pub kind: ActorKind,
    pub launch_spec: LaunchSpec,
    pub schedule: Option<CreateScheduleRequest>,
    /// The grants the declaration gives, which only an Admin token sets. A replaced
    /// declaration keeps its grants without it.
    #[serde(
        default,
        deserialize_with = "present_set",
        skip_serializing_if = "Option::is_none"
    )]
    pub grants: Option<BTreeSet<Grant>>,
    /// The names of the Actors whose Messages the Actor accepts. A replaced declaration keeps
    /// them without it.
    #[serde(
        default,
        deserialize_with = "present_set",
        skip_serializing_if = "Option::is_none"
    )]
    pub accept_from: Option<BTreeSet<String>>,
    /// Where the creating CLI sits on the Provider, as its `self` answered. The Provider's
    /// `resolve` fills the omitted `providerOptions` from it.
    #[serde(default, deserialize_with = "optional_object")]
    pub caller: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateScheduleRequest {
    pub cron: String,
    pub timezone: Option<String>,
}

/// A partial update of an Actor: each top-level field of `launchSpec` and `schedule` that
/// the request carries is replaced, and the rest is kept.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateActorRequest {
    /// The name the Actor takes, keeping its `id`, Sessions and mailbox.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_spec: Option<LaunchSpecPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<UpdateScheduleRequest>,
    /// The grants that replace the Actor's, which only an Admin token sets.
    #[serde(
        default,
        deserialize_with = "present_set",
        skip_serializing_if = "Option::is_none"
    )]
    pub grants: Option<BTreeSet<Grant>>,
    /// The names of the Actors whose Messages the Actor accepts, which replace its own; null
    /// accepts every Actor of the Workspace again.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub accept_from: Option<Option<BTreeSet<String>>>,
    #[serde(
        default,
        deserialize_with = "optional_object",
        skip_serializing_if = "Option::is_none"
    )]
    pub caller: Option<Value>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateScheduleRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ActorResponse {
    pub actor: Actor,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ActorList {
    pub workspace: String,
    pub actors: Vec<Actor>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SessionResponse {
    pub session: Session,
    #[serde(default, rename = "subAgents")]
    pub sub_agents: Vec<SubAgentSession>,
}

/// A Sub Agent Session read by its own id.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentSessionResponse {
    pub sub_agent_session: SubAgentSession,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResponse {
    pub session: Session,
    /// The previous generation this start replaced, when there was one.
    #[serde(default)]
    pub replaced_session: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartResponse {
    /// The current Session the restart asked to stop.
    pub stopped: Session,
    /// The next generation, when the stopped Session closed before the response; otherwise
    /// it starts once that Session closes.
    #[serde(default)]
    pub session: Option<Session>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachResponse {
    pub session: Session,
    pub token: String,
    pub reattached: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachActorRequest {
    pub placement: Placement,
    pub runtime: String,
    /// Whether the Runtime process started with the telemetry environment the Runtime
    /// Adapter prepares for attached Sessions; the attaching CLI knows, the daemon records.
    #[serde(default = "default_true")]
    pub runtime_telemetry_prepared: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleResponse {
    pub session: Session,
    pub already_requested: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordList {
    pub records: Vec<Record>,
    /// The sequence of the last record listed, else the `after` asked for.
    pub next_after: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RuntimeList {
    pub runtimes: Vec<RuntimeEntry>,
}

/// An installed Runtime with the number of current Sessions that run it.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEntry {
    #[serde(flatten)]
    pub plugin: PluginSummary,
    pub running_sessions: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProviderList {
    pub providers: Vec<ProviderEntry>,
}

/// An installed Provider with the daemon's Nodes and the number of current Sessions placed on
/// it.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    #[serde(flatten)]
    pub plugin: PluginSummary,
    pub nodes: Vec<String>,
    pub running_sessions: usize,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SendMessageRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub body: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MessageResponse {
    pub message: Message,
}

/// The Messages `GET /v1/messages` lists, each with its preview in place of the body.
#[derive(Debug, Deserialize, Serialize)]
pub struct MessageSummaryList {
    pub messages: Vec<MessageSummary>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TelemetryRequest {
    pub(super) request_id: String,
    pub(super) token: String,
    pub(super) signal: String,
    pub(super) content_type: String,
    pub(super) payload: String,
}

/// The telemetry token the Collector asks the daemon about.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TelemetryTokenRequest {
    pub(super) token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MessageQueuedList {
    pub queued: Vec<MessageQueued>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MessageReadList {
    pub reads: Vec<MessageRead>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SessionList {
    pub sessions: Vec<Session>,
    /// The Sub Agent Sessions of the listed Sessions, when the listing asked for them.
    #[serde(default, rename = "subAgents", skip_serializing_if = "Option::is_none")]
    pub sub_agents: Option<Vec<SubAgentSession>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BrowserList {
    pub browsers: Vec<Browser>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BrowserResponse {
    pub browser: Browser,
}

/// The answer to a pairing: the browser token's record and the key the page sends beside
/// its cookie.
#[derive(Debug, Deserialize, Serialize)]
pub struct PairResponse {
    pub browser: Browser,
    pub key: String,
}
