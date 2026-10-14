//! What herdr answers: the panes, tabs and workspaces one reading holds, and the pieces
//! of one command's reply.

use std::collections::HashMap;
use std::process::Output;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use feld_core::process::ProcessIncarnation;

use crate::placement::*;

/// One pane, as herdr answers it. The names are herdr's own.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PaneInfo {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    pub terminal_id: String,
    /// Where the pane itself sits, and where the program in front of it does. An `attach`
    /// takes the workdir of the Runtime it finds, which is the foreground one.
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground_cwd: Option<String>,
    /// What the pane's program last wrote as its terminal title (OSC 2). The launch script
    /// writes `feld exit <n>` there, which is where the exit code of an `owned` Session
    /// comes from.
    #[serde(default)]
    pub terminal_title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TabInfo {
    pub tab_id: String,
    pub number: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceInfo {
    pub workspace_id: String,
    pub label: String,
}

/// One reading of a herdr server. `session.snapshot` answers every pane, tab, and
/// workspace at once, so a whole `observe` costs the endpoint one call however many
/// Placements it holds.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct HerdrReading {
    #[serde(default)]
    pub panes: Vec<PaneInfo>,
    #[serde(default)]
    pub tabs: Vec<TabInfo>,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceInfo>,
}

impl HerdrReading {
    pub(super) fn pane(&self, pane_id: &str) -> Option<&PaneInfo> {
        self.panes.iter().find(|pane| pane.pane_id == pane_id)
    }

    /// The pane that holds one terminal. herdr renames a pane that moves to another
    /// workspace and keeps the old ID as an alias, so the terminal id is what follows the
    /// recorded pane wherever it goes.
    pub(super) fn pane_of_terminal(&self, terminal_id: &str) -> Option<&PaneInfo> {
        self.panes
            .iter()
            .find(|pane| pane.terminal_id == terminal_id)
    }

    /// Whether this reading still holds one resource of a unit, by ID.
    pub(super) fn holds(&self, unit: HerdrUnit, id: &str) -> bool {
        match unit {
            HerdrUnit::Workspace => self
                .workspaces
                .iter()
                .any(|workspace| workspace.workspace_id == id),
            HerdrUnit::Tab => self.tabs.iter().any(|tab| tab.tab_id == id),
            HerdrUnit::Pane => self.panes.iter().any(|pane| pane.pane_id == id),
        }
    }

    /// The labels herdr answers for the pane's own tab and workspace right now.
    pub(super) fn current_names(&self, pane: &PaneInfo) -> CurrentNames {
        CurrentNames {
            workspace_id: pane.workspace_id.clone(),
            tab_id: pane.tab_id.clone(),
            pane_id: pane.pane_id.clone(),
            workspace: self
                .workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == pane.workspace_id)
                .map(|workspace| workspace.label.clone())
                .unwrap_or_else(|| pane.workspace_id.clone()),
            tab_number: self
                .tabs
                .iter()
                .find(|tab| tab.tab_id == pane.tab_id)
                .map(|tab| tab.number),
        }
    }
}

/// Why a reading of one endpoint did not arrive. A server that is not running is evidence
/// about the pane; anything else is only uncertainty.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ReadFailure {
    ServerDown(String),
    Unreadable(String),
}

/// What one `observe` read of herdr and the process table, so that a sequence of
/// Placements is judged against one consistent reading instead of one reading each.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub(super) readings: HashMap<String, std::result::Result<HerdrReading, ReadFailure>>,
    pub(super) processes: HashMap<u32, Option<ProcessIncarnation>>,
}

/// The `result` of one herdr answer, or the error it reported.
pub(super) fn answer(command: &str, output: &Output) -> Result<Value> {
    reply(command, output)?
        .as_ref()
        .and_then(|reply| reply.get("result").cloned())
        .ok_or_else(|| {
            HerdrProviderError::Protocol(format!("herdr {command} answered without a result"))
        })
}

/// One herdr reply, or None when the command answered nothing at all, which `pane run`
/// and the `close` commands do when they succeed. herdr prints the reply of a command
/// that succeeded on stdout and the reply of one that failed on stderr, so both are read
/// for it.
pub(super) fn reply(command: &str, output: &Output) -> Result<Option<Value>> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let answered = [&stdout, &stderr]
        .into_iter()
        .flat_map(|printed| printed.lines())
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .find(|reply| reply.get("id").is_some());
    let Some(answered) = answered else {
        if output.status.success() {
            return Ok(None);
        }
        return Err(HerdrProviderError::Protocol(format!(
            "herdr {command} answered no JSON (status {}): {}",
            output.status,
            stderr.trim()
        )));
    };
    if let Some(error) = answered.get("error") {
        return Err(HerdrProviderError::Herdr {
            command: command.to_owned(),
            code: error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        });
    }
    Ok(Some(answered))
}

/// One string a herdr answer holds at a path of keys.
pub(super) fn text(answer: &Value, path: &[&str]) -> Result<String> {
    let mut at = answer;
    for key in path {
        at = at.get(*key).ok_or_else(|| {
            HerdrProviderError::Protocol(format!("herdr answered without '{}'", path.join(".")))
        })?;
    }
    at.as_str().map(str::to_owned).ok_or_else(|| {
        HerdrProviderError::Protocol(format!("herdr answered a non-string '{}'", path.join(".")))
    })
}

/// One pane out of a herdr answer, under whichever key the command that answered puts it.
fn pane_at(answer: &Value, keys: &[&str]) -> Result<PaneInfo> {
    let pane = keys
        .iter()
        .find_map(|key| answer.get(*key))
        .ok_or_else(|| HerdrProviderError::Protocol("herdr answered without a pane".to_owned()))?;
    serde_json::from_value(pane.clone()).map_err(|error| {
        HerdrProviderError::Protocol(format!("herdr answered an unreadable pane: {error}"))
    })
}

pub(super) fn pane_of(answer: &Value) -> Result<PaneInfo> {
    pane_at(answer, &["pane"])
}

pub(super) fn created_pane(answer: &Value) -> Result<PaneInfo> {
    pane_at(answer, &["root_pane", "pane"])
}

/// One number a herdr answer holds at a path of keys.
pub(super) fn number(answer: &Value, path: &[&str]) -> Result<u32> {
    let mut at = answer;
    for key in path {
        at = at.get(*key).ok_or_else(|| {
            HerdrProviderError::Protocol(format!("herdr answered without '{}'", path.join(".")))
        })?;
    }
    at.as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| {
            HerdrProviderError::Protocol(format!(
                "herdr answered a non-numeric '{}'",
                path.join(".")
            ))
        })
}
