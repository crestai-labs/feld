//! The shape of a Placement on tmux: its endpoint and axes, the request that prepares it,
//! the names it answers, and the errors the Provider reports.

use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use feld_core::plugin::sdk::provider::{self, CommandFailed, ResourceOrigin, names_a_path};
use feld_core::process::ProcessIncarnation;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum TmuxEndpoint {
    Name(String),
    Path(PathBuf),
}

impl TmuxEndpoint {
    /// The socket as the caller named it: a name for `-L`, a path for `-S`.
    pub(super) fn label(&self) -> String {
        match self {
            Self::Name(name) => name.clone(),
            Self::Path(path) => path.display().to_string(),
        }
    }

    pub(super) fn append_args(&self, command: &mut Command) {
        match self {
            Self::Name(name) => {
                command.arg("-L").arg(name);
            }
            Self::Path(path) => {
                command.arg("-S").arg(path);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TmuxUnit {
    Session,
    Window,
    Pane,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedSession {
    pub stable_id: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedLaunch {
    pub endpoint: TmuxEndpoint,
    pub unit: TmuxUnit,
    pub borrowed: Option<String>,
    pub workdir: PathBuf,
    pub server: Option<ProcessIncarnation>,
    pub session: Option<PreparedSession>,
    pub window_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareRequest {
    pub endpoint: TmuxEndpoint,
    pub unit: TmuxUnit,
    pub borrowed: Option<String>,
    pub workdir: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmuxPlacement {
    pub endpoint: TmuxEndpoint,
    pub unit: TmuxUnit,
    pub borrowed: Option<String>,
    pub workdir: PathBuf,
    pub server: ProcessIncarnation,
    pub session_id: String,
    pub session_created_at: i64,
    pub session_origin: ResourceOrigin,
    pub window_id: String,
    pub window_origin: ResourceOrigin,
    pub pane_id: String,
    pub pane_origin: ResourceOrigin,
    pub pane_process: ProcessIncarnation,
    /// The process of the `<command>` an `owned` Session runs, under the key core reads for
    /// every Provider ([`feld_core::model::Placement::program`]). `<command>` takes the
    /// pane's own process by `exec`, so it is the pane's process. `attached` Sessions leave
    /// it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<ProcessIncarnation>,
    /// The Runtime process this Placement took, under the key core reads for every
    /// Provider ([`feld_core::model::PlacementProcess`]). `owned` Sessions leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<ProcessIncarnation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentNames {
    pub session_id: String,
    pub window_id: String,
    pub pane_id: String,
    pub session: String,
    pub window: String,
    pub window_index: u32,
    pub pane_index: u32,
}

impl CurrentNames {
    /// The stable ID of the unit that holds the pane right now.
    pub(super) fn id_of(&self, unit: &str) -> Option<&str> {
        match unit {
            "session" => Some(&self.session_id),
            "window" => Some(&self.window_id),
            "pane" => Some(&self.pane_id),
            _ => None,
        }
    }

    /// The name tmux answers for one unit right now: the session it is in, with the index
    /// of the window for a window or a pane (`main:1`).
    pub(super) fn name_of(&self, unit: &str) -> String {
        match unit {
            "session" => self.session.clone(),
            _ => format!("{}:{}", self.session, self.window_index),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialTmuxPlacement {
    pub endpoint: TmuxEndpoint,
    pub unit: TmuxUnit,
    pub borrowed: Option<String>,
    pub workdir: PathBuf,
    pub server_pid: u32,
    pub server: Option<ProcessIncarnation>,
    pub session_id: String,
    pub session_created_at: i64,
    pub session_origin: ResourceOrigin,
    pub window_id: String,
    pub window_origin: ResourceOrigin,
    pub pane_id: String,
    pub pane_origin: ResourceOrigin,
    pub waiting_pane_pid: u32,
}

pub type ObservationResult = provider::ObservationResult<CurrentNames>;
pub type LaunchRequest = provider::LaunchRequest<PreparedLaunch>;
pub type LaunchResult = provider::LaunchResult<TmuxPlacement, CurrentNames>;
pub type LaunchFailure = provider::LaunchFailure<PartialTmuxPlacement, CurrentNames>;
pub type AttachedPane = provider::AttachedPane<TmuxPlacement, CurrentNames>;
pub type AttachRequest = provider::AttachRequest<TmuxEndpoint>;
pub type ControlResult = provider::ControlResult<CurrentNames>;

#[derive(Debug, Error)]
pub enum TmuxProviderError {
    #[error("invalid tmux provider request: {0}")]
    InvalidRequest(String),
    #[error(transparent)]
    Io(#[from] CommandFailed),
    #[error("tmux command failed ({command}): {stderr}")]
    Tmux {
        command: String,
        status: Option<i32>,
        stderr: String,
    },
    #[error("unexpected tmux response: {0}")]
    Protocol(String),
    #[error("{0}")]
    NotFound(String),
    #[error("the launch environment could not be handed over: {0}")]
    LaunchEnv(std::io::Error),
}

pub type Result<T> = std::result::Result<T, TmuxProviderError>;

/// The socket a launch takes when neither the caller nor the options name one.
pub const DEFAULT_SOCKET: &str = "feld";

/// The socket path a `$TMUX` value names.
pub fn socket_path(value: &str) -> Option<String> {
    let mut fields = value.rsplitn(3, ',');
    fields.next()?.parse::<u64>().ok()?;
    fields.next()?.parse::<u32>().ok()?;
    let socket = fields.next()?;
    (!socket.is_empty()).then(|| socket.to_owned())
}

/// The endpoint a socket selector names: a path when it looks like one, a name otherwise.
pub fn endpoint(socket: &str) -> TmuxEndpoint {
    if names_a_path(socket) {
        TmuxEndpoint::Path(PathBuf::from(socket))
    } else {
        TmuxEndpoint::Name(socket.to_owned())
    }
}
