use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Params, params};

use feld_core::id::fresh_id;
use feld_core::model::{
    ActorReference, Message, MessageQueued, MessageRead, RecordKind, Requester, message_preview,
};
use feld_core::time::now;

use super::Store;
use super::actor::{accepts, live_actor};
use super::admission::RequestIdentity;
use super::error::MessageWriteError;
use super::record::{append_actor_record, append_record};
use super::session::requester;

#[derive(Default)]
pub struct MessageFilter<'a> {
    pub workspace: Option<&'a str>,
    /// The name of a live Actor that sent the Message.
    pub from: Option<&'a str>,
    /// The name of a live Actor the Message is addressed to.
    pub to: Option<&'a str>,
    pub label: Option<&'a str>,
    /// Only the Messages without a read.
    pub waiting: bool,
    pub start: Option<&'a str>,
    pub end: Option<&'a str>,
}

impl Store {
    pub fn send_message(
        &self,
        identity: RequestIdentity<'_>,
        token: Option<&str>,
        workspace: &str,
        actor: &str,
        label: Option<&str>,
        body: &str,
    ) -> std::result::Result<Message, MessageWriteError> {
        self.write("Message send", |transaction, notices| {
            revalidate_message_identity(transaction, identity, token)?;
            // An Actor sends within its Workspace, before it learns whether the Actor exists.
            if let Requester::Actor {
                workspace: own_workspace,
                ..
            } = identity.requester
                && own_workspace != workspace
            {
                return Err(MessageWriteError::OtherWorkspace(workspace.to_owned()));
            }
            let (to_actor_id, _) = live_actor(transaction, workspace, actor)?.ok_or_else(|| {
                MessageWriteError::ActorNotFound {
                    workspace: workspace.to_owned(),
                    actor: actor.to_owned(),
                }
            })?;
            if let Requester::Actor { name: from, .. } = identity.requester
                && !accepts(transaction, &to_actor_id, from)?
            {
                return Err(MessageWriteError::NotAccepted {
                    actor: actor.to_owned(),
                    from: from.to_owned(),
                });
            }
            let to_session_id = transaction
                .query_row(
                    "SELECT id FROM sessions WHERE actor_id = ?1 AND end IS NULL",
                    [&to_actor_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .context("read Message target Session")?;
            let fifo: u64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(fifo), 0) + 1 FROM messages WHERE to_actor_id = ?1",
                    [&to_actor_id],
                    |row| row.get(0),
                )
                .context("allocate mailbox FIFO position")?;
            let id = fresh_id("message");
            let accepted = now();
            let anchor = format!("feld:{id}");
            transaction
                .execute(
                    "INSERT INTO messages
                     (id, fifo, from_actor_id, from_session_id, to_actor_id, to_session_id,
                      accepted, label, body, anchor)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        id,
                        fifo,
                        identity.requester.actor_id(),
                        identity.session_id,
                        to_actor_id,
                        to_session_id,
                        accepted,
                        label,
                        body,
                        anchor,
                    ],
                )
                .context("append Message")?;
            let from = serde_json::to_value(identity.requester).context("encode Message sender")?;
            if let Some(session_id) = identity.session_id {
                append_record(
                    transaction,
                    session_id,
                    &accepted,
                    RecordKind::MessageSent,
                    &serde_json::json!({
                        "messageId": id,
                        "to": {"id": to_actor_id, "workspace": workspace, "name": actor},
                        "toSessionId": to_session_id,
                        "anchor": anchor,
                    }),
                )?;
            }
            let accepted_data = serde_json::json!({
                "messageId": id,
                "from": from,
                "fromSessionId": identity.session_id,
                "waiting": true,
            });
            // Without a current Session the record belongs to the recipient Actor alone.
            match &to_session_id {
                Some(session_id) => append_record(
                    transaction,
                    session_id,
                    &accepted,
                    RecordKind::MessageAccepted,
                    &accepted_data,
                )?,
                None => append_actor_record(
                    transaction,
                    &to_actor_id,
                    &accepted,
                    RecordKind::MessageAccepted,
                    &accepted_data,
                )?,
            }
            let message = get_message(transaction, &id)?.context("sent Message disappeared")?;
            notices.push("message", &message.id, Some(&message.to.workspace));
            notices.push(
                "record",
                message.to_session_id.as_deref().unwrap_or(&message.to.name),
                Some(&message.to.workspace),
            );
            if let (Some(session_id), Requester::Actor { workspace, .. }) =
                (message.from_session_id.as_deref(), &message.from)
                && message.to_session_id.as_deref() != Some(session_id)
            {
                notices.push("record", session_id, Some(workspace));
            }
            Ok(message)
        })
    }

    pub fn get_message(&self, id: &str) -> Result<Option<Message>> {
        self.read(|connection| get_message(connection, id))
    }

    /// The Messages that are not deleted, oldest first.
    pub fn list_messages(&self, filter: &MessageFilter<'_>) -> Result<Vec<Message>> {
        self.read(|connection| {
            list_message_rows(
                connection,
                "WHERE m.deleted_at IS NULL
                   AND (?1 IS NULL OR ta.workspace_id = (SELECT id FROM workspaces WHERE key = ?1 AND deleted_at IS NULL))
                   AND (?2 IS NULL OR (fa.name = ?2 AND fa.deleted_at IS NULL
                                       AND (?1 IS NULL OR fa.workspace_key = ?1)))
                   AND (?3 IS NULL OR (ta.name = ?3 AND ta.deleted_at IS NULL))
                   AND (?4 = 0 OR m.read_id IS NULL)
                   AND (?5 IS NULL OR m.accepted >= ?5)
                   AND (?6 IS NULL OR m.accepted < ?6)
                   AND (?7 IS NULL OR m.label = ?7)
                 ORDER BY m.accepted, m.id",
                params![
                    filter.workspace,
                    filter.from,
                    filter.to,
                    filter.waiting,
                    filter.start,
                    filter.end,
                    filter.label
                ],
            )
        })
    }

    /// Records `deletedAt` on a Message that no Runtime has queued or read.
    pub fn delete_message(&self, id: &str) -> std::result::Result<Message, MessageWriteError> {
        self.write("Message delete", |transaction, notices| {
            let message = get_message(transaction, id)?
                .filter(|message| message.deleted_at.is_none())
                .ok_or_else(|| MessageWriteError::MessageNotFound(id.to_owned()))?;
            if message.read.is_some() {
                return Err(MessageWriteError::Read(id.to_owned()));
            }
            if message.queued.is_some() {
                return Err(MessageWriteError::Queued(id.to_owned()));
            }
            transaction
                .execute(
                    "UPDATE messages SET deleted_at = ?2 WHERE id = ?1",
                    params![id, now()],
                )
                .context("delete Message")?;
            let message = get_message(transaction, id)?.context("deleted Message disappeared")?;
            notices.push("message", &message.id, Some(&message.to.workspace));
            Ok(message)
        })
    }

    pub fn mailbox(
        &self,
        session_id: &str,
        token: &str,
        limit: Option<u64>,
    ) -> std::result::Result<Vec<Message>, MessageWriteError> {
        self.read(|connection| {
            let (actor_id, _) = mailbox_identity(connection, session_id, token)?;
            let limit = limit.unwrap_or(u64::MAX).min(i64::MAX as u64);
            list_message_rows(
                connection,
                "WHERE m.to_actor_id = ?1 AND m.read_id IS NULL AND m.deleted_at IS NULL
                 ORDER BY m.fifo LIMIT ?2",
                params![actor_id, limit],
            )
            .map_err(Into::into)
        })
    }

    /// Records that the Runtime of the Session holds each Message in a queue of its own. A
    /// Message queued or read before keeps what it has.
    pub fn queue_mailbox(
        &self,
        session_id: &str,
        token: &str,
        message_ids: &[String],
    ) -> std::result::Result<Vec<MessageQueued>, MessageWriteError> {
        self.write("mailbox queue", |transaction, notices| {
            let (actor_id, workspace) = mailbox_identity(transaction, session_id, token)?;
            let messages = mailbox_messages(transaction, &actor_id, message_ids)?;
            let at = now();
            let mut queued = Vec::with_capacity(messages.len());
            for message in messages {
                let entry = match message.queued {
                    Some(entry) => entry,
                    None => {
                        let entry = MessageQueued {
                            session_id: session_id.to_owned(),
                            at: at.clone(),
                        };
                        transaction
                            .execute(
                                "UPDATE messages SET queued_session_id = ?2, queued_at = ?3
                                 WHERE id = ?1 AND queued_at IS NULL",
                                params![message.id, entry.session_id, entry.at],
                            )
                            .context("record Message queue")?;
                        entry
                    }
                };
                notices.push("message", &message.id, Some(&workspace));
                queued.push(entry);
            }
            Ok(queued)
        })
    }

    /// Takes back the `queued` this Session recorded on a Message its Runtime did not take
    /// into its queue after all. A Message read, or queued by another Session, keeps what it
    /// has.
    pub fn unqueue_mailbox(
        &self,
        session_id: &str,
        token: &str,
        message_id: &str,
    ) -> std::result::Result<Message, MessageWriteError> {
        self.write("mailbox unqueue", |transaction, notices| {
            let (actor_id, workspace) = mailbox_identity(transaction, session_id, token)?;
            mailbox_messages(transaction, &actor_id, &[message_id.to_owned()])?;
            transaction
                .execute(
                    "UPDATE messages SET queued_session_id = NULL, queued_at = NULL
                     WHERE id = ?1 AND queued_session_id = ?2 AND read_id IS NULL",
                    params![message_id, session_id],
                )
                .context("take back Message queue")?;
            notices.push("message", message_id, Some(&workspace));
            Ok(get_message(transaction, message_id)?.context("unqueued Message disappeared")?)
        })
    }

    pub fn read_mailbox(
        &self,
        session_id: &str,
        token: &str,
        message_ids: &[String],
    ) -> std::result::Result<Vec<MessageRead>, MessageWriteError> {
        self.write("mailbox read", |transaction, notices| {
            let (actor_id, workspace) = mailbox_identity(transaction, session_id, token)?;
            mailbox_messages(transaction, &actor_id, message_ids)?;
            let at = now();
            let mut reads = Vec::with_capacity(message_ids.len());
            for id in message_ids {
                let message = get_message(transaction, id)?
                    .ok_or_else(|| MessageWriteError::MessageNotFound(id.clone()))?;
                let read = if let Some(read) = message.read {
                    read
                } else {
                    let read = MessageRead {
                        session_id: session_id.to_owned(),
                        at: at.clone(),
                        read_id: fresh_id("read"),
                        fifo: message.fifo,
                    };
                    transaction
                        .execute(
                            "UPDATE messages SET read_session_id = ?2, read_at = ?3, read_id = ?4
                             WHERE id = ?1 AND read_id IS NULL",
                            params![message.id, read.session_id, read.at, read.read_id],
                        )
                        .context("record Message read")?;
                    append_record(
                        transaction,
                        session_id,
                        &read.at,
                        RecordKind::MessageRead,
                        &serde_json::json!({
                            "messageId": message.id,
                            "readId": read.read_id,
                            "fifo": read.fifo,
                        }),
                    )?;
                    read
                };
                reads.push(read);
            }
            for id in message_ids {
                notices.push("message", id, Some(&workspace));
            }
            notices.push("record", session_id, Some(&workspace));
            Ok(reads)
        })
    }
}

/// The Messages of `message_ids`, each of them one of this Actor's mailbox that is not
/// deleted.
fn mailbox_messages(
    connection: &Connection,
    actor_id: &str,
    message_ids: &[String],
) -> std::result::Result<Vec<Message>, MessageWriteError> {
    message_ids
        .iter()
        .map(|id| {
            let message = get_message(connection, id)?
                .filter(|message| message.deleted_at.is_none())
                .ok_or_else(|| MessageWriteError::MessageNotFound(id.clone()))?;
            if message.to.id != actor_id {
                return Err(MessageWriteError::WrongMailbox(id.clone()));
            }
            Ok(message)
        })
        .collect()
}

/// Takes back the `queued` of the Messages a Session's Runtime held without reading them,
/// once its execution is gone: its queue ended with it, and the next Session reads them
/// from the mailbox.
pub(super) fn unqueue_closed_session(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
) -> Result<()> {
    transaction
        .execute(
            "UPDATE messages SET queued_session_id = NULL, queued_at = NULL
             WHERE queued_session_id = ?1 AND read_id IS NULL",
            [session_id],
        )
        .context("take back the queue of a closed Session")?;
    Ok(())
}

fn revalidate_message_identity(
    connection: &Connection,
    identity: RequestIdentity<'_>,
    token: Option<&str>,
) -> std::result::Result<(), MessageWriteError> {
    let Some(session_id) = identity.session_id else {
        return Ok(());
    };
    let Some(token) = token else {
        return Err(MessageWriteError::InvalidToken);
    };
    let actual = connection
        .query_row(
            "SELECT actor_id FROM sessions WHERE id = ?1 AND token = ?2 AND end IS NULL",
            params![session_id, token],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .context("revalidate Message sender token")?;
    match (actual, identity.requester.actor_id()) {
        (Some(actual), Some(expected)) if actual == expected => Ok(()),
        _ => Err(MessageWriteError::InvalidToken),
    }
}

/// The Actor whose mailbox a Session token reads, with its Workspace.
fn mailbox_identity(
    connection: &Connection,
    session_id: &str,
    token: &str,
) -> std::result::Result<(String, String), MessageWriteError> {
    connection
        .query_row(
            "SELECT s.actor_id, a.workspace_key FROM sessions s JOIN actors a ON a.id = s.actor_id
             WHERE s.id = ?1 AND s.token = ?2 AND s.end IS NULL",
            params![session_id, token],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .context("authenticate mailbox Session")?
        .ok_or(MessageWriteError::InvalidToken)
}

const MESSAGE_SELECT: &str = "SELECT m.id, m.fifo, m.from_actor_id, fa.workspace_key, fa.name,
     m.from_session_id, m.to_actor_id, ta.workspace_key, ta.name, m.to_session_id, m.accepted,
     m.body, m.anchor, m.read_session_id, m.read_at, m.read_id, m.deleted_at,
     m.queued_session_id, m.queued_at, m.label
     FROM messages m
     JOIN actors ta ON ta.id = m.to_actor_id
     LEFT JOIN actors fa ON fa.id = m.from_actor_id";

fn list_message_rows<P: Params>(
    connection: &Connection,
    suffix: &str,
    params: P,
) -> Result<Vec<Message>> {
    let mut statement = connection
        .prepare(&format!("{MESSAGE_SELECT} {suffix}"))
        .context("prepare Message listing")?;
    statement
        .query_map(params, message_row)
        .context("query Messages")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("read Messages")
}

pub(super) fn get_message(connection: &Connection, id: &str) -> Result<Option<Message>> {
    connection
        .query_row(
            &format!("{MESSAGE_SELECT} WHERE m.id = ?1"),
            [id],
            message_row,
        )
        .optional()
        .context("read Message")
}

fn message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let fifo = row.get(1)?;
    let read_session_id = row.get::<_, Option<String>>(13)?;
    let read_at = row.get::<_, Option<String>>(14)?;
    let read_id = row.get::<_, Option<String>>(15)?;
    let deleted_at = row.get::<_, Option<String>>(16)?;
    let waiting = read_id.is_none() && deleted_at.is_none();
    let read = match (read_session_id, read_at, read_id) {
        (Some(session_id), Some(at), Some(read_id)) => Some(MessageRead {
            session_id,
            at,
            read_id,
            fifo,
        }),
        _ => None,
    };
    let queued = match (row.get(17)?, row.get(18)?) {
        (Some(session_id), Some(at)) => Some(MessageQueued { session_id, at }),
        _ => None,
    };
    let body: String = row.get(11)?;
    let preview = message_preview(&body);
    Ok(Message {
        id: row.get(0)?,
        fifo,
        from: requester(row.get(2)?, row.get(3)?, row.get(4)?),
        from_session_id: row.get(5)?,
        to: ActorReference {
            id: row.get(6)?,
            workspace: row.get(7)?,
            name: row.get(8)?,
        },
        to_session_id: row.get(9)?,
        accepted: row.get(10)?,
        label: row.get(19)?,
        queued,
        read,
        body,
        preview,
        anchor: row.get(12)?,
        waiting,
        deleted_at,
    })
}
