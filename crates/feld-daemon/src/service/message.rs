use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Notify, watch};

use crate::console::{Secrets, redact_text};
use crate::store::{MessageFilter, MessageWriteError, RequestIdentity, Store};
use feld_core::id::valid_key;
use feld_core::model::{Message, MessageQueued, MessageRead, MessageSummary, message_preview};

/// How long a mailbox read that waits sleeps before it reads again without being woken.
const MAILBOX_POLL: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    #[error("Message body is empty")]
    EmptyBody,
    #[error("Message labels must match ^[a-z0-9][a-z0-9-]*$")]
    InvalidLabel,
    #[error(transparent)]
    Write(#[from] MessageWriteError),
}

#[derive(Debug, thiserror::Error)]
pub enum MailboxError {
    #[error(transparent)]
    Write(#[from] MessageWriteError),
    #[error("daemon is shutting down")]
    ShuttingDown,
}

/// Messages and the mailboxes they wait in. A send wakes every mailbox read that waits.
pub struct MessageService {
    store: Arc<Store>,
    sent: Notify,
    shutdown: watch::Sender<bool>,
}

impl MessageService {
    pub fn new(store: Arc<Store>, shutdown: watch::Sender<bool>) -> Self {
        Self {
            store,
            sent: Notify::new(),
            shutdown,
        }
    }

    pub fn send(
        &self,
        identity: RequestIdentity<'_>,
        token: Option<&str>,
        workspace: &str,
        actor: &str,
        label: Option<&str>,
        body: &str,
    ) -> Result<Message, MessageError> {
        if body.trim().is_empty() {
            return Err(MessageError::EmptyBody);
        }
        if label.is_some_and(|label| !valid_key(label)) {
            return Err(MessageError::InvalidLabel);
        }
        let message = self
            .store
            .send_message(identity, token, workspace, actor, label, body)?;
        self.sent.notify_waiters();
        Ok(message)
    }

    pub fn get(&self, id: &str) -> anyhow::Result<Option<Message>> {
        self.store.get_message(id)
    }

    pub fn delete(&self, id: &str) -> Result<Message, MessageWriteError> {
        self.store.delete_message(id)
    }

    /// The Messages the filter selects, with `secrets` taken out of each body. The preview
    /// is cut from the body, so it is cut again from the redacted body.
    pub fn summaries(
        &self,
        filter: &MessageFilter<'_>,
        secrets: &Secrets,
    ) -> anyhow::Result<Vec<MessageSummary>> {
        Ok(self
            .store
            .list_messages(filter)?
            .into_iter()
            .map(|mut message| {
                redact_text(&mut message.body, secrets);
                message.preview = message_preview(&message.body);
                MessageSummary::from(message)
            })
            .collect())
    }

    /// The unread Messages of the mailbox the Session token names. With `wait`, an empty
    /// mailbox is read again whenever a Message is sent, until one arrives or the daemon
    /// shuts down.
    pub async fn mailbox(
        &self,
        session_id: &str,
        token: &str,
        limit: Option<u64>,
        wait: bool,
    ) -> Result<Vec<Message>, MailboxError> {
        loop {
            let sent = self.sent.notified();
            tokio::pin!(sent);
            sent.as_mut().enable();
            let messages = self.store.mailbox(session_id, token, limit)?;
            if !messages.is_empty() || !wait {
                return Ok(messages);
            }
            let mut shutdown = self.shutdown.subscribe();
            if *shutdown.borrow() {
                return Err(MailboxError::ShuttingDown);
            }
            tokio::select! {
                _ = &mut sent => {}
                _ = tokio::time::sleep(MAILBOX_POLL) => {}
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Err(MailboxError::ShuttingDown);
                    }
                }
            }
        }
    }

    pub fn queue_mailbox(
        &self,
        session_id: &str,
        token: &str,
        message_ids: &[String],
    ) -> Result<Vec<MessageQueued>, MessageWriteError> {
        self.store.queue_mailbox(session_id, token, message_ids)
    }

    pub fn unqueue_mailbox(
        &self,
        session_id: &str,
        token: &str,
        message_id: &str,
    ) -> Result<Message, MessageWriteError> {
        self.store.unqueue_mailbox(session_id, token, message_id)
    }

    pub fn read_mailbox(
        &self,
        session_id: &str,
        token: &str,
        message_ids: &[String],
    ) -> Result<Vec<MessageRead>, MessageWriteError> {
        self.store.read_mailbox(session_id, token, message_ids)
    }
}
