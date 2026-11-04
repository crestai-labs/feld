//! The labels of an Actor: their set and clear, and their removal once `until` passes.

use anyhow::Context;
use rusqlite::{Connection, Transaction, params};
use serde_json::json;

use feld_core::id::valid_key;
use feld_core::model::{Actor, ActorLabel, LabelSetter, RecordKind};
use feld_core::time;

use super::error::ActorWriteError;
use super::record::{append_actor_record, append_record};
use super::sql::Text;
use super::{Notices, Store};

/// Who sets or clears a label: the Admin, or a Session of the labelled Actor.
#[derive(Clone, Copy, Debug)]
pub enum LabelWriter<'a> {
    Admin,
    Session(&'a str),
}

impl LabelWriter<'_> {
    fn setter(self) -> LabelSetter {
        match self {
            LabelWriter::Admin => LabelSetter::Admin,
            LabelWriter::Session(_) => LabelSetter::Actor,
        }
    }
}

impl Store {
    /// Sets the label `key` on the live Actor, replacing the one of the same key. `until` is an
    /// RFC 3339 time after now.
    pub fn set_label(
        &self,
        workspace: &str,
        actor: &str,
        key: &str,
        until: Option<&str>,
        reason: Option<&str>,
        writer: LabelWriter<'_>,
    ) -> Result<Actor, ActorWriteError> {
        if !valid_key(key) {
            return Err(ActorWriteError::InvalidLabel(
                "label keys must match ^[a-z0-9][a-z0-9-]*$".to_owned(),
            ));
        }
        let at = time::now();
        let until = until
            .map(|until| {
                time::parse(until).map(time::format).map_err(|_| {
                    ActorWriteError::InvalidLabel(format!(
                        "`until` '{until}' is not an RFC 3339 time"
                    ))
                })
            })
            .transpose()?;
        if until.as_deref().is_some_and(|until| until <= at.as_str()) {
            return Err(ActorWriteError::InvalidLabel(
                "`until` must be after now".to_owned(),
            ));
        }
        self.write("label set", |transaction, notices| {
            let id = labelled_actor(transaction, workspace, actor, writer)?;
            expire_labels(transaction, notices, &at)?;
            transaction
                .execute(
                    "INSERT INTO actor_labels (actor_id, key, until, reason, set_by, set_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (actor_id, key) DO UPDATE SET
                       until = excluded.until, reason = excluded.reason,
                       set_by = excluded.set_by, set_at = excluded.set_at",
                    params![id, key, until, reason, writer.setter().as_str(), at],
                )
                .context("set label")?;
            let data = json!({
                "key": key,
                "until": until,
                "reason": reason,
                "by": writer.setter(),
            });
            record(transaction, &id, writer, &at, RecordKind::LabelSet, &data)?;
            notices.push("actor", actor, Some(workspace));
            labelled(transaction, &id)
        })
    }

    /// Clears the label `key` of the live Actor; an Actor without it stays as it is.
    pub fn clear_label(
        &self,
        workspace: &str,
        actor: &str,
        key: &str,
        writer: LabelWriter<'_>,
    ) -> Result<Actor, ActorWriteError> {
        let at = time::now();
        self.write("label clear", |transaction, notices| {
            let id = labelled_actor(transaction, workspace, actor, writer)?;
            expire_labels(transaction, notices, &at)?;
            let cleared = transaction
                .execute(
                    "DELETE FROM actor_labels WHERE actor_id = ?1 AND key = ?2",
                    params![id, key],
                )
                .context("clear label")?;
            if cleared > 0 {
                let data = json!({ "key": key, "by": writer.setter() });
                record(
                    transaction,
                    &id,
                    writer,
                    &at,
                    RecordKind::LabelCleared,
                    &data,
                )?;
                notices.push("actor", actor, Some(workspace));
            }
            labelled(transaction, &id)
        })
    }

    /// Removes the labels whose `until` has passed, recording each as `label.expired`. Only a
    /// store holding such a label is written.
    pub fn expire_labels(&self) -> anyhow::Result<()> {
        let at = time::now();
        let lapsed = self.read(|connection| {
            connection
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM actor_labels WHERE until <= ?1)",
                    [&at],
                    |row| row.get::<_, bool>(0),
                )
                .context("find lapsed labels")
        })?;
        if !lapsed {
            return Ok(());
        }
        self.write("label expiry", |transaction, notices| {
            expire_labels(transaction, notices, &at)
        })
    }
}

/// The `id` of the live Actor a label write names. A Session writes only its own Actor's.
fn labelled_actor(
    transaction: &Transaction<'_>,
    workspace: &str,
    actor: &str,
    writer: LabelWriter<'_>,
) -> Result<String, ActorWriteError> {
    let not_found = || ActorWriteError::ActorNotFound {
        workspace: workspace.to_owned(),
        actor: actor.to_owned(),
    };
    let (id, _) = super::actor::live_actor(transaction, workspace, actor)?.ok_or_else(not_found)?;
    if let LabelWriter::Session(session) = writer {
        let own: Option<String> = transaction
            .query_row(
                "SELECT actor_id FROM sessions WHERE id = ?1",
                [session],
                |row| row.get(0),
            )
            .ok();
        if own.as_deref() != Some(id.as_str()) {
            return Err(ActorWriteError::OthersLabel(actor.to_owned()));
        }
    }
    Ok(id)
}

fn labelled(transaction: &Transaction<'_>, id: &str) -> Result<Actor, ActorWriteError> {
    Ok(super::actor::get_actor_by_id(transaction, id)?
        .ok_or_else(|| anyhow::anyhow!("labelled Actor was not found"))?)
}

fn record(
    transaction: &Transaction<'_>,
    actor_id: &str,
    writer: LabelWriter<'_>,
    at: &str,
    kind: RecordKind,
    data: &serde_json::Value,
) -> anyhow::Result<()> {
    match writer {
        LabelWriter::Admin => append_actor_record(transaction, actor_id, at, kind, data),
        LabelWriter::Session(session) => append_record(transaction, session, at, kind, data),
    }
}

/// Removes the labels whose `until` is at or before `at`, recording each as `label.expired`.
pub(super) fn expire_labels(
    transaction: &Transaction<'_>,
    notices: &mut Notices,
    at: &str,
) -> anyhow::Result<()> {
    let expired = transaction
        .prepare(
            "DELETE FROM actor_labels WHERE until <= ?1
             RETURNING actor_id, key, until, set_by,
               (SELECT name FROM actors WHERE id = actor_id),
               (SELECT workspace_key FROM actors WHERE id = actor_id)",
        )
        .context("prepare label expiry")?
        .query_map([at], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Text<LabelSetter>>(3)?.0,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .context("expire labels")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("read expired labels")?;
    for (actor_id, key, until, by, name, workspace) in expired {
        let data = json!({ "key": key, "until": until, "by": by });
        append_actor_record(transaction, &actor_id, at, RecordKind::LabelExpired, &data)?;
        notices.push("actor", &name, Some(&workspace));
    }
    Ok(())
}

/// The labels of the Actor `id` whose `until` has yet to pass, by key.
pub(super) fn list_labels(connection: &Connection, id: &str) -> anyhow::Result<Vec<ActorLabel>> {
    connection
        .prepare(
            "SELECT key, until, reason, set_by, set_at FROM actor_labels
             WHERE actor_id = ?1 AND (until IS NULL OR until > ?2) ORDER BY key",
        )
        .context("prepare label read")?
        .query_map(params![id, time::now()], |row| {
            Ok(ActorLabel {
                key: row.get(0)?,
                until: row.get(1)?,
                reason: row.get(2)?,
                by: row.get::<_, Text<LabelSetter>>(3)?.0,
                set_at: row.get(4)?,
            })
        })
        .context("query labels")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("read labels")
}
