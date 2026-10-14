//! Plugins: how core places, finds, and calls the parts that know a product.
//!
//! A Plugin is a directory under `$FELD_HOME/plugins/<kind>/<name>/` with a manifest and
//! one executable core runs per operation (see [`invoke`]). Core reaches every Runtime
//! Adapter and Execution Provider through the two traits below, which each installed Plugin
//! directory answers.
//!
//! # The packaging of the Plugins Feld ships
//!
//! The Plugins Feld ships follow one shape, which `feld init` writes into
//! `$FELD_HOME/plugins/`:
//!
//! - the executable is the binary of the workspace crate `crates/feld-plugin-<name>`, so
//!   `cargo build` puts it beside `feld`
//! - the static files of the Plugin directory (`plugin.json`, hook and monitor
//!   declarations, `SKILL.md`, helper shims) live in the repository under
//!   `plugins/<kind>/<name>/` and are embedded in the `feld` binary with `include_str!`.
//!   `feld init` writes them out and copies the `feld-plugin-<name>` beside
//!   `current_exe()` to the Plugin directory's `plugin`
//! - the only Feld crate a Plugin crate depends on is this one: what every Plugin
//!   executable shares ([`sdk`]), the parts beside a Runtime ([`crate::self_api`]), and the
//!   operation types ([`protocol`])
//!
//! Every Runtime Adapter and Execution Provider is a Plugin directory: core carries no
//! implementation of its own.

mod choice;
pub mod invoke;
pub mod manifest;
pub mod protocol;
mod refusal;
pub mod registry;
pub mod remote;
pub mod sdk;
mod self_index;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use choice::{ProviderChoiceError, choose_provider};
pub use manifest::{Manifest, PluginKind};
pub use refusal::{
    default_init, none_installed, not_installed, provider_not_installed, runtime_not_installed,
};
pub use registry::{Plugins, Registry};
pub use self_index::{runtime_self_index_key, self_index_key};

use protocol::{
    AttachedRequest, AttachedResponse, CandidatesRequest, CandidatesResponse, ControlResponse,
    IdentityRequest, IdentityResponse, MatchRequest, MatchResponse, ObserveRequest,
    ObserveResponse, PlaceRequest, PlaceResponse, PlacementRequest, PrepareRequest,
    PrepareResponse, ProviderAttachRequest, ProviderAttachResponse, ProviderLaunchRequest,
    ProviderLaunchResponse, ResolveRequest, ResolveResponse, RestoreRequest, RestoreResponse,
    RuntimeLaunchRequest, RuntimeLaunchResponse, SelfResponse, StopRequest, TranscriptRequest,
    TranscriptResponse,
};

/// The Nodes a Placement can name. Every installed Provider places on the local Node until
/// a Provider reports the Nodes it reaches.
pub const NODES: &[&str] = &["local"];

/// The default shipped Plugins of each kind, which the `feld init` a missing kind is
/// pointed to places.
pub const DEFAULTS: &[(PluginKind, &str)] = &[
    (PluginKind::Runtime, "claude-code"),
    (PluginKind::Provider, "tmux"),
];

/// One installed Plugin as `GET /v1/daemon/status` lists it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PluginSummary {
    pub name: String,
    pub label: String,
}

/// The Plugin that connects Feld to one Coding Agent Runtime.
pub trait RuntimeAdapter: Send + Sync {
    fn manifest(&self) -> &Manifest;

    /// What to run to start the Runtime, and the environment it runs in.
    fn launch(&self, request: &RuntimeLaunchRequest) -> Result<RuntimeLaunchResponse>;

    /// Whether a process is this Runtime.
    fn match_process(&self, request: &MatchRequest) -> Result<MatchResponse>;

    /// The Runtime identity the caller's own environment tells, or the one the process an
    /// attach from outside took tells, when it tells one.
    fn identity(&self, request: &IdentityRequest) -> Result<IdentityResponse>;

    /// Starts what runs beside an attached Runtime and waits for no event of the Runtime to
    /// start, with the token the attach received.
    fn attached(&self, _request: &AttachedRequest) -> Result<AttachedResponse> {
        Ok(AttachedResponse::default())
    }

    /// Which conversation of this Runtime the caller runs in (`self`), from the caller's
    /// environment alone. Whether a process of this Runtime is in the caller's parent chain
    /// is for core to check with `match`.
    fn current(&self) -> Result<SelfResponse> {
        Ok(SelfResponse::default())
    }

    /// What a Runtime a person starts themselves needs to be attached, written when `apply`.
    fn prepare(&self, request: &PrepareRequest) -> Result<PrepareResponse>;

    /// What `prepare` wrote in a Runtime a person starts themselves, taken back when `apply`.
    fn restore(&self, request: &RestoreRequest) -> Result<RestoreResponse>;

    /// The Runtime's transcript entries, projected onto the common transcript entry shape.
    fn transcript(&self, request: &TranscriptRequest) -> Result<TranscriptResponse>;
}

/// The Plugin that connects Feld to one execution substrate.
pub trait ExecutionProvider: Send + Sync {
    fn manifest(&self) -> &Manifest;

    /// Where the caller's own environment sits inside this Provider (`self`).
    fn current(&self) -> Result<SelfResponse>;

    /// Fills in the options a declaration left out.
    fn resolve(&self, request: &ResolveRequest) -> Result<ResolveResponse>;

    /// The Placement an admitted start will hold, without an incarnation yet.
    fn place(&self, request: &PlaceRequest) -> Result<PlaceResponse>;

    fn launch(&self, request: &ProviderLaunchRequest) -> Result<ProviderLaunchResponse>;

    fn observe(&self, request: &ObserveRequest) -> Result<ObserveResponse>;

    fn stop(&self, request: &StopRequest) -> Result<ControlResponse>;

    fn kill(&self, request: &PlacementRequest) -> Result<ControlResponse>;

    fn cleanup(&self, request: &PlacementRequest) -> Result<()>;

    /// Identifies an execution that is already running (`feld actor attach`).
    fn attach(&self, request: &ProviderAttachRequest) -> Result<ProviderAttachResponse>;

    /// The existing resources an attach could take, each with the process in front of it.
    fn candidates(&self, _request: &CandidatesRequest) -> Result<CandidatesResponse> {
        anyhow::bail!("{} lists no candidates", self.manifest().label)
    }

    /// How a stored Placement reads (`window @7 (main:1, created)`). Every listing shows it,
    /// so it is answered from the Placement rather than by running an operation: a Plugin
    /// puts the `summary` its operations returned into the Placement it hands back, and
    /// core rewrites it whenever an `observe` resolves a new one.
    fn summary(&self, placement: &Value) -> String {
        placement
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    }

    /// The command a person runs to reach the execution, answered from the Placement for
    /// the same reason as [`ExecutionProvider::summary`].
    fn attach_command(&self, placement: &Value) -> Option<String> {
        Some(placement.get("attachCommand")?.as_str()?.to_owned())
    }
}
