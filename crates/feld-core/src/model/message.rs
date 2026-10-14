use serde::{Deserialize, Serialize};

use super::{ActorReference, Requester};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRead {
    pub session_id: String,
    pub at: String,
    pub read_id: String,
    pub fifo: u64,
}

/// That a Runtime holds a Message in a queue of its own, which a later input of the model
/// takes: the Message is no longer Feld's to delete, and has no read yet.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageQueued {
    pub session_id: String,
    pub at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub fifo: u64,
    pub from: Requester,
    pub from_session_id: Option<String>,
    pub to: ActorReference,
    pub to_session_id: Option<String>,
    pub accepted: String,
    /// A name the sender gives the kind of the Message, in the grammar of Actor names.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub queued: Option<MessageQueued>,
    pub read: Option<MessageRead>,
    pub body: String,
    pub preview: String,
    pub anchor: String,
    pub waiting: bool,
    pub deleted_at: Option<String>,
}

/// The mailbox a Session reads, bodies included.
#[derive(Debug, Deserialize, Serialize)]
pub struct MessageList {
    pub messages: Vec<Message>,
}

/// A Message as a list shows it: its `preview` in place of the body.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: String,
    pub fifo: u64,
    pub from: Requester,
    pub from_session_id: Option<String>,
    pub to: ActorReference,
    pub to_session_id: Option<String>,
    pub accepted: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub queued: Option<MessageQueued>,
    pub read: Option<MessageRead>,
    pub preview: String,
    pub anchor: String,
    pub waiting: bool,
    pub deleted_at: Option<String>,
}

impl From<Message> for MessageSummary {
    fn from(message: Message) -> Self {
        Self {
            id: message.id,
            fifo: message.fifo,
            from: message.from,
            from_session_id: message.from_session_id,
            to: message.to,
            to_session_id: message.to_session_id,
            accepted: message.accepted,
            label: message.label,
            queued: message.queued,
            read: message.read,
            preview: message.preview,
            anchor: message.anchor,
            waiting: message.waiting,
            deleted_at: message.deleted_at,
        }
    }
}

/// The excerpt of a Message body that lists show.
pub fn message_preview(body: &str) -> String {
    let mut preview = body.chars().take(48).collect::<String>();
    if body.chars().count() > 48 {
        preview.push('…');
    }
    preview
}
