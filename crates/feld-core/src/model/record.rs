use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ActorReference;

/// The kinds of the facts Feld itself records.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RecordKind {
    #[serde(rename = "session.started")]
    SessionStarted,
    #[serde(rename = "session.ended")]
    SessionEnded,
    #[serde(rename = "spawned")]
    Spawned,
    #[serde(rename = "message.accepted")]
    MessageAccepted,
    #[serde(rename = "message.sent")]
    MessageSent,
    #[serde(rename = "message.read")]
    MessageRead,
    #[serde(rename = "stop.requested")]
    StopRequested,
    #[serde(rename = "kill.requested")]
    KillRequested,
    #[serde(rename = "observation")]
    Observation,
    #[serde(rename = "schedule.skipped")]
    ScheduleSkipped,
    #[serde(rename = "schedule.missed")]
    ScheduleMissed,
}

impl RecordKind {
    pub const ALL: [Self; 11] = [
        Self::SessionStarted,
        Self::SessionEnded,
        Self::Spawned,
        Self::MessageAccepted,
        Self::MessageSent,
        Self::MessageRead,
        Self::StopRequested,
        Self::KillRequested,
        Self::Observation,
        Self::ScheduleSkipped,
        Self::ScheduleMissed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionStarted => "session.started",
            Self::SessionEnded => "session.ended",
            Self::Spawned => "spawned",
            Self::MessageAccepted => "message.accepted",
            Self::MessageSent => "message.sent",
            Self::MessageRead => "message.read",
            Self::StopRequested => "stop.requested",
            Self::KillRequested => "kill.requested",
            Self::Observation => "observation",
            Self::ScheduleSkipped => "schedule.skipped",
            Self::ScheduleMissed => "schedule.missed",
        }
    }
}

impl FromStr for RecordKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| format!("unknown record kind '{value}'"))
    }
}

impl fmt::Display for RecordKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One fact Feld itself recorded, in the global order of `sequence`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub sequence: u64,
    pub at: String,
    pub kind: RecordKind,
    pub actor: ActorReference,
    pub session_id: Option<String>,
    pub data: Value,
}
