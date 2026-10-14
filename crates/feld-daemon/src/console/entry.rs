use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What Feld found where a Session's transcript file should be. `pending` is core's own: the
/// Runtime identity that names the entries has not arrived. The other three are the
/// Adapter's answer.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConsoleState {
    Pending,
    Missing,
    Lost,
    Readable,
}

/// The kinds of the common transcript entry shape. An Adapter that answers with a kind this Feld
/// does not know reads as `other`, which the All events view shows and the Conversation
/// view and the turn skip.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(from = "String", into = "String")]
pub enum EntryKind {
    Input,
    Notification,
    Response,
    Thinking,
    ToolCall,
    ToolResult,
    System,
    Other,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Notification => "notification",
            Self::Response => "response",
            Self::Thinking => "thinking",
            Self::ToolCall => "tool-call",
            Self::ToolResult => "tool-result",
            Self::System => "system",
            Self::Other => "other",
        }
    }

    /// Whether the entry counts towards the [turn]: the Runtime's own lines and the
    /// entries only the All events view shows do not.
    ///
    /// [turn]: https://github.com/crestai-labs/feld/blob/main/docs/spec/concepts/glossary.md#turn
    pub fn carries_turn(self) -> bool {
        !matches!(self, Self::System | Self::Other)
    }
}

impl From<String> for EntryKind {
    fn from(value: String) -> Self {
        match value.as_str() {
            "input" => Self::Input,
            "notification" => Self::Notification,
            "response" => Self::Response,
            "thinking" => Self::Thinking,
            "tool-call" => Self::ToolCall,
            "tool-result" => Self::ToolResult,
            "system" => Self::System,
            _ => Self::Other,
        }
    }
}

impl From<EntryKind> for String {
    fn from(value: EntryKind) -> Self {
        value.as_str().to_owned()
    }
}

/// Whether what an entry stands for can be read. An entry the Adapter left without one is
/// readable; `lost` stands for entries Feld cannot reach, which is how a Sub Agent whose
/// transcript file cannot be read still appears as a Sub Agent Session.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(from = "String", into = "String")]
pub enum EntryState {
    #[default]
    Readable,
    Lost,
}

impl EntryState {
    pub fn is_readable(&self) -> bool {
        matches!(self, Self::Readable)
    }
}

impl From<String> for EntryState {
    fn from(value: String) -> Self {
        match value.as_str() {
            "lost" => Self::Lost,
            _ => Self::Readable,
        }
    }
}

impl From<EntryState> for String {
    fn from(value: EntryState) -> Self {
        match value {
            EntryState::Readable => "readable".to_owned(),
            EntryState::Lost => "lost".to_owned(),
        }
    }
}

/// One transcript entry in the common shape.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntry {
    pub cursor: String,
    #[serde(default)]
    pub at: Option<String>,
    pub kind: EntryKind,
    #[serde(default)]
    pub text: String,
    /// Whether a tool result reports a failure.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub error: bool,
    #[serde(default, skip_serializing_if = "EntryState::is_readable")]
    pub state: EntryState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_agent_ended: Option<String>,
    #[serde(default)]
    pub raw: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptPage {
    pub session: String,
    pub state: ConsoleState,
    pub detail: Option<String>,
    pub after: Option<String>,
    pub next_after: Option<String>,
    pub previous_before: Option<String>,
    pub has_older: bool,
    pub records: Vec<TranscriptEntry>,
}

/// A read whose `after` or `before` names no entry of the transcript.
#[derive(Debug)]
pub struct UnknownCursor(pub String);

impl std::fmt::Display for UnknownCursor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "transcript cursor '{}' names no entry of this transcript",
            self.0
        )
    }
}

impl std::error::Error for UnknownCursor {}
