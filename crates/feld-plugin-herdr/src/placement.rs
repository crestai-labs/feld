//! The shape of a Placement on herdr: its endpoint and axes, the request that prepares it,
//! the names it answers, and the errors the Provider reports.

use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use feld_core::plugin::sdk::provider::{self, CommandFailed, ResourceOrigin, names_a_path};
use feld_core::process::ProcessIncarnation;

/// The herdr server a Placement talks to.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum HerdrEndpoint {
    Session(String),
    Path(PathBuf),
}

impl HerdrEndpoint {
    /// Points one `herdr` invocation at this server. herdr reads `--session` before
    /// `HERDR_SOCKET_PATH` before `HERDR_SESSION`, and the inherited environment holds
    /// those of the caller's own pane, so each form clears the one it does not use.
    pub(super) fn append_args(&self, command: &mut Command) {
        match self {
            Self::Session(name) => {
                command.arg("--session").arg(name);
                command.env_remove("HERDR_SOCKET_PATH");
            }
            Self::Path(path) => {
                command.env("HERDR_SOCKET_PATH", path);
                command.env_remove("HERDR_SESSION");
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HerdrUnit {
    Workspace,
    Tab,
    Pane,
}

impl HerdrUnit {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Tab => "tab",
            Self::Pane => "pane",
        }
    }
}

/// The direction a `pane` unit splits its borrowed pane in.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HerdrSplit {
    Right,
    Down,
}

impl HerdrSplit {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "down",
        }
    }
}

/// The existing resource a `tab` or `pane` launch borrows, as herdr resolved it: the
/// selector a person wrote, and the IDs the launch re-checks before it creates anything.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Borrowed {
    pub selector: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
}

/// The Placement core stores for a herdr Session.
///
/// `place` fills the three axes and the borrowed resource; the resources of the Session and
/// the incarnation that tells them from a restored namesake arrive with `launch` and
/// `attach`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HerdrPlacement {
    pub endpoint: HerdrEndpoint,
    pub unit: HerdrUnit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split: Option<HerdrSplit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub borrowed: Option<Borrowed>,
    pub workdir: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_origin: Option<ResourceOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_origin: Option<ResourceOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_origin: Option<ResourceOrigin>,
    /// The id herdr gives the pane's real terminal. A restored pane keeps its ID and takes
    /// a new one, which is how a server restart shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
    /// The pane's own process (`pane.process_info`'s `shell_pid`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_process: Option<ProcessIncarnation>,
    /// The process of the `<command>` an `owned` Session runs in the pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<ProcessIncarnation>,
    /// The Runtime process this Placement took, under the key core reads for every
    /// Provider ([`feld_core::model::PlacementProcess`]). `owned` Sessions leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<ProcessIncarnation>,
}

/// What `place` resolves before anything is created.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareRequest {
    pub endpoint: HerdrEndpoint,
    pub unit: HerdrUnit,
    pub split: Option<HerdrSplit>,
    pub borrowed: Option<String>,
    pub workdir: PathBuf,
}

/// The labels herdr answers for a Placement right now, which a reading adds to its
/// [`summary`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentNames {
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
    pub workspace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_number: Option<u32>,
}

impl CurrentNames {
    /// The ID of the unit that holds the pane right now.
    pub(super) fn id_of(&self, unit: &str) -> Option<&str> {
        match unit {
            "workspace" => Some(&self.workspace_id),
            "tab" => Some(&self.tab_id),
            "pane" => Some(&self.pane_id),
            _ => None,
        }
    }

    /// The name herdr answers for one unit right now: the workspace label, with the number
    /// of the tab for a tab or a pane (`feldtest:2`).
    pub(super) fn name_of(&self, unit: &str) -> String {
        match (unit, self.tab_number) {
            ("workspace", _) | (_, None) => self.workspace.clone(),
            (_, Some(number)) => format!("{}:{number}", self.workspace),
        }
    }
}

pub type ObservationResult = provider::ObservationResult<CurrentNames>;
pub type LaunchRequest = provider::LaunchRequest<HerdrPlacement>;
pub type LaunchResult = provider::LaunchResult<HerdrPlacement, CurrentNames>;
pub type LaunchFailure = provider::LaunchFailure<HerdrPlacement, CurrentNames>;
pub type AttachedPane = provider::AttachedPane<HerdrPlacement, CurrentNames>;
pub type AttachRequest = provider::AttachRequest<HerdrEndpoint>;
pub type ControlResult = provider::ControlResult<CurrentNames>;

#[derive(Debug, Error)]
pub enum HerdrProviderError {
    #[error("invalid herdr provider request: {0}")]
    InvalidRequest(String),
    #[error(transparent)]
    Io(#[from] CommandFailed),
    #[error("herdr command failed ({command}): {code}: {message}")]
    Herdr {
        command: String,
        code: String,
        message: String,
    },
    #[error("unexpected herdr response: {0}")]
    Protocol(String),
    #[error("the launch environment could not be handed over: {0}")]
    LaunchEnv(std::io::Error),
}

pub type Result<T> = std::result::Result<T, HerdrProviderError>;

/// The server a launch takes when neither the caller nor the options name one.
pub const DEFAULT_SESSION: &str = "default";

/// The selector the three variables herdr puts in a pane process make up.
pub fn selector_of(
    herdr_env: Option<&str>,
    socket: Option<&str>,
    pane: Option<&str>,
) -> Option<Value> {
    if herdr_env? != "1" {
        return None;
    }
    let socket = socket.filter(|socket| !socket.is_empty())?;
    let pane = pane.filter(|pane| !pane.is_empty())?;
    Some(pane_selector(socket, pane))
}

/// A pane as `self` answers it from inside: the socket path and the pane ID.
pub fn pane_selector(socket: &str, pane: &str) -> Value {
    Value::Object(Map::from_iter([
        ("pane".to_owned(), Value::String(pane.to_owned())),
        ("session".to_owned(), Value::String(socket.to_owned())),
    ]))
}

/// The endpoint a session selector names: a socket path when it looks like one, a session
/// name otherwise.
pub fn endpoint(session: &str) -> HerdrEndpoint {
    if names_a_path(session) {
        HerdrEndpoint::Path(PathBuf::from(session))
    } else {
        HerdrEndpoint::Session(session.to_owned())
    }
}
