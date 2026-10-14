use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Params, params};

use feld_core::id::fresh_id;
use feld_core::model::{
    Actor, ActorKind, Grant, LaunchSpec, RecordKind, Schedule, ScheduleDeclaration, SchedulePatch,
};
use feld_core::time::now;
use std::collections::BTreeSet;

use super::Store;
use super::error::ActorWriteError;
use super::schedule::{get_schedule, write_schedule};
use super::session::list_sessions_for_actor;
use super::sql::Text;
use super::workspace::live_workspace_id;

impl Store {
    pub fn list_actors(
        &self,
        workspace: &str,
        kind: Option<ActorKind>,
        include_deleted: bool,
    ) -> Result<Vec<Actor>> {
        self.read(|connection| {
            read_actor_rows(
                connection,
                "WHERE workspace_id = (SELECT id FROM workspaces WHERE key = ?1 AND deleted_at IS NULL)
                   AND (?2 IS NULL OR kind = ?2) AND (?3 OR deleted_at IS NULL)
                 ORDER BY name, created_at",
                params![workspace, kind.map(Text), include_deleted],
            )?
            .into_iter()
            .map(|row| actor_from_row(connection, row))
            .collect()
        })
    }

    /// The live Actor that holds `name` in the Workspace.
    pub fn get_actor(&self, workspace: &str, name: &str) -> Result<Option<Actor>> {
        self.read(|connection| get_actor(connection, workspace, name))
    }

    /// The Actor with this `id`, deleted or not.
    pub fn get_actor_by_id(&self, id: &str) -> Result<Option<Actor>> {
        self.read(|connection| get_actor_by_id(connection, id))
    }

    /// Creates the Actor, or replaces the declaration of the live Actor of the same name,
    /// which keeps its `id`. The Schedule follows the declaration and keeps `enabled`. The
    /// grants become `grants` when given, and a replaced declaration keeps them otherwise.
    pub fn create_actor(
        &self,
        workspace: &str,
        name: &str,
        kind: ActorKind,
        launch_spec: &LaunchSpec,
        schedule: Option<&ScheduleDeclaration>,
        grants: Option<&BTreeSet<Grant>>,
    ) -> std::result::Result<Actor, ActorWriteError> {
        self.create_actor_as(
            workspace,
            name,
            kind,
            launch_spec,
            schedule,
            grants,
            None,
            Writer::Admin,
        )
    }

    /// [`Self::create_actor`] by `writer`, with the senders the Actor accepts, which a
    /// replaced declaration keeps when None. A Session replaces no Actor with grants or `trustWorkdir`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_actor_as(
        &self,
        workspace: &str,
        name: &str,
        kind: ActorKind,
        launch_spec: &LaunchSpec,
        schedule: Option<&ScheduleDeclaration>,
        grants: Option<&BTreeSet<Grant>>,
        accept_from: Option<&BTreeSet<String>>,
        writer: Writer,
    ) -> std::result::Result<Actor, ActorWriteError> {
        launch_spec
            .validate()
            .map_err(ActorWriteError::InvalidLaunch)?;
        if schedule.is_some() && kind != ActorKind::Job {
            return Err(ActorWriteError::NotJob(name.to_owned()));
        }
        let at = now();
        if let Some(declaration) = schedule {
            Schedule::declare(declaration, None, &at)
                .validate()
                .map_err(ActorWriteError::InvalidSchedule)?;
        }
        self.write("Actor creation", |transaction, notices| {
            let Some(workspace_id) = live_workspace_id(transaction, workspace)? else {
                return Err(ActorWriteError::WorkspaceNotFound(workspace.to_owned()));
            };
            let launch_spec = serde_json::to_string(launch_spec).context("encode launchSpec")?;
            let grants = grants.map(encode_grants).transpose()?;
            let accept_from = accept_from.map(encode_accept_from).transpose()?;
            let id = match live_actor(transaction, workspace, name)? {
                Some((_, existing)) if existing != kind => {
                    return Err(ActorWriteError::KindMismatch {
                        actor: name.to_owned(),
                        existing,
                        requested: kind,
                    });
                }
                Some((id, _)) => {
                    writer.check(transaction, Some(&id), kind, name)?;
                    transaction
                        .execute(
                            "UPDATE actors SET launch_spec = ?2, grants = COALESCE(?3, grants),
                                               accept_from = COALESCE(?4, accept_from),
                                               updated_at = ?5
                             WHERE id = ?1",
                            params![id, launch_spec, grants, accept_from, at],
                        )
                        .context("replace Actor declaration")?;
                    id
                }
                None => {
                    writer.check(transaction, None, kind, name)?;
                    let id = fresh_id("actor");
                    transaction
                        .execute(
                            "INSERT INTO actors (id, workspace_id, workspace_key, name, kind,
                                                 launch_spec, grants, accept_from, created_at,
                                                 updated_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, COALESCE(?7, '[]'), ?8, ?9, ?9)",
                            params![
                                id,
                                workspace_id,
                                workspace,
                                name,
                                Text(kind),
                                launch_spec,
                                grants,
                                accept_from,
                                at
                            ],
                        )
                        .context("create Actor")?;
                    id
                }
            };
            let existing = get_schedule(transaction, &id)?;
            let schedule =
                schedule.map(|declaration| Schedule::declare(declaration, existing, &at));
            write_schedule(transaction, &id, schedule.as_ref())?;
            let actor = get_actor_by_id(transaction, &id)?
                .ok_or_else(|| anyhow::anyhow!("created Actor was not found"))?;
            notices.push("actor", name, Some(workspace));
            Ok(actor)
        })
    }

    /// Changes the launchSpec, the Schedule field by field and the grants in one
    /// transaction. `launch` is written only while the Actor still holds the launchSpec it
    /// was prepared from.
    pub fn update_actor(
        &self,
        workspace: &str,
        name: &str,
        launch: Option<LaunchUpdate<'_>>,
        schedule: Option<&SchedulePatch>,
        grants: Option<&BTreeSet<Grant>>,
        default_timezone: &str,
    ) -> std::result::Result<Actor, ActorWriteError> {
        self.update_actor_as(
            workspace,
            name,
            ActorPatch {
                rename: None,
                launch,
                schedule,
                grants,
                accept_from: None,
            },
            default_timezone,
            Writer::Admin,
        )
    }

    /// [`Self::update_actor`] by `writer`, which also takes the rename of the patch. A Session
    /// changes no Actor with grants or `trustWorkdir`.
    pub fn update_actor_as(
        &self,
        workspace: &str,
        name: &str,
        patch: ActorPatch<'_>,
        default_timezone: &str,
        writer: Writer,
    ) -> std::result::Result<Actor, ActorWriteError> {
        let ActorPatch {
            rename,
            launch,
            schedule,
            grants,
            accept_from,
        } = patch;
        self.write("Actor update", |transaction, notices| {
            let current = get_actor(transaction, workspace, name)?.ok_or_else(|| {
                ActorWriteError::ActorNotFound {
                    workspace: workspace.to_owned(),
                    actor: name.to_owned(),
                }
            })?;
            writer.check(transaction, Some(&current.id), current.kind, name)?;
            let at = now();
            let rename = rename.filter(|rename| *rename != name);
            if let Some(rename) = rename {
                refuse_running(transaction, &current.id, name, "rename")?;
                if live_actor(transaction, workspace, rename)?.is_some() {
                    return Err(ActorWriteError::NameTaken {
                        workspace: workspace.to_owned(),
                        name: rename.to_owned(),
                    });
                }
                transaction
                    .execute(
                        "UPDATE actors SET name = ?2 WHERE id = ?1",
                        params![current.id, rename],
                    )
                    .context("rename Actor")?;
            }
            if let Some(launch) = launch {
                if current.launch_spec.as_ref() != launch.expected {
                    return Err(ActorWriteError::LaunchChanged(name.to_owned()));
                }
                transaction
                    .execute(
                        "UPDATE actors SET launch_spec = ?2 WHERE id = ?1",
                        params![
                            current.id,
                            serde_json::to_string(launch.resolved).context("encode launchSpec")?
                        ],
                    )
                    .context("update Actor launchSpec")?;
            }
            if let Some(patch) = schedule {
                if current.kind != ActorKind::Job {
                    return Err(ActorWriteError::NotJob(name.to_owned()));
                }
                let schedule = match current.schedule {
                    Some(existing) => existing.apply_patch(patch, &at),
                    None => Schedule::from_patch(patch, &at, default_timezone)
                        .ok_or_else(|| ActorWriteError::NoSchedule(name.to_owned()))?,
                };
                schedule
                    .validate()
                    .map_err(ActorWriteError::InvalidSchedule)?;
                write_schedule(transaction, &current.id, Some(&schedule))?;
            }
            if let Some(grants) = grants {
                transaction
                    .execute(
                        "UPDATE actors SET grants = ?2 WHERE id = ?1",
                        params![current.id, encode_grants(grants)?],
                    )
                    .context("update Actor grants")?;
            }
            if let Some(accept_from) = accept_from {
                transaction
                    .execute(
                        "UPDATE actors SET accept_from = ?2 WHERE id = ?1",
                        params![current.id, accept_from.map(encode_accept_from).transpose()?],
                    )
                    .context("update Actor acceptFrom")?;
            }
            transaction
                .execute(
                    "UPDATE actors SET updated_at = ?2 WHERE id = ?1",
                    params![current.id, at],
                )
                .context("update Actor")?;
            let actor = get_actor_by_id(transaction, &current.id)?
                .ok_or_else(|| anyhow::anyhow!("updated Actor was not found"))?;
            notices.push("actor", name, Some(workspace));
            if let Some(rename) = rename {
                notices.push("actor", rename, Some(workspace));
            }
            Ok(actor)
        })
    }

    /// Records `deletedAt` on an Actor whose current Session is not running.
    pub fn delete_actor(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<Actor, ActorWriteError> {
        self.delete_actor_as(workspace, name, Writer::Admin)
    }

    /// [`Self::delete_actor`] by `writer`. A Session deletes no Actor with grants or `trustWorkdir`.
    pub fn delete_actor_as(
        &self,
        workspace: &str,
        name: &str,
        writer: Writer,
    ) -> std::result::Result<Actor, ActorWriteError> {
        self.write("Actor delete", |transaction, notices| {
            let (id, kind) = live_actor(transaction, workspace, name)?.ok_or_else(|| {
                ActorWriteError::ActorNotFound {
                    workspace: workspace.to_owned(),
                    actor: name.to_owned(),
                }
            })?;
            writer.check(transaction, Some(&id), kind, name)?;
            refuse_running(transaction, &id, name, "delete")?;
            transaction
                .execute(
                    "UPDATE actors SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1",
                    params![id, now()],
                )
                .context("delete Actor")?;
            let actor = get_actor_by_id(transaction, &id)?
                .ok_or_else(|| anyhow::anyhow!("deleted Actor was not found"))?;
            notices.push("actor", name, Some(workspace));
            Ok(actor)
        })
    }
}

/// Who writes an Actor. A Session writes only an Actor without grants or `trustWorkdir` of a
/// kind its grants manage, which the write checks in its own transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Writer {
    Admin,
    /// A Session, with whether its grants hold `manage-agent` and `manage-job`.
    Session {
        agents: bool,
        jobs: bool,
    },
}

impl Writer {
    /// Whether the writer manages Actors of `kind`.
    fn manages(self, kind: ActorKind) -> bool {
        match self {
            Writer::Admin => true,
            Writer::Session { agents, jobs } => match kind {
                ActorKind::Agent => agents,
                ActorKind::Job => jobs,
            },
        }
    }

    /// Refuses an Actor of a kind the writer does not manage, then one of a Session's writes
    /// to an Actor with grants or `trustWorkdir`. `id` is the Actor as it is now, when there is one.
    fn check(
        self,
        transaction: &Connection,
        id: Option<&str>,
        kind: ActorKind,
        name: &str,
    ) -> std::result::Result<(), ActorWriteError> {
        if !self.manages(kind) {
            return Err(ActorWriteError::Unmanaged {
                actor: name.to_owned(),
                kind,
            });
        }
        let (Writer::Session { .. }, Some(id)) = (self, id) else {
            return Ok(());
        };
        let (grants, launch_spec): (String, Option<String>) = transaction
            .query_row(
                "SELECT grants, launch_spec FROM actors WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context("read Actor grants")?;
        let grants: BTreeSet<Grant> = serde_json::from_str(&grants).context("decode grants")?;
        if !grants.is_empty() {
            return Err(ActorWriteError::Granted(name.to_owned()));
        }
        let launch_spec: Option<LaunchSpec> = launch_spec
            .map(|launch_spec| serde_json::from_str(&launch_spec))
            .transpose()
            .context("decode launchSpec")?;
        if launch_spec.is_some_and(|launch_spec| launch_spec.trusts_workdir()) {
            return Err(ActorWriteError::Trusted(name.to_owned()));
        }
        Ok(())
    }
}

/// What an update changes: the name, the launchSpec, the Schedule field by field, the grants,
/// and the senders the Actor accepts, where `Some(None)` accepts every Actor of the Workspace
/// again.
#[derive(Clone, Copy, Debug, Default)]
pub struct ActorPatch<'a> {
    /// The name the Actor takes, which only an Actor without a running current Session takes.
    pub rename: Option<&'a str>,
    pub launch: Option<LaunchUpdate<'a>>,
    pub schedule: Option<&'a SchedulePatch>,
    pub grants: Option<&'a BTreeSet<Grant>>,
    pub accept_from: Option<Option<&'a BTreeSet<String>>>,
}

/// A launchSpec an update writes: the one resolved from the patch, and the one it was
/// merged over, which the Actor must still hold.
#[derive(Clone, Copy, Debug)]
pub struct LaunchUpdate<'a> {
    pub expected: Option<&'a LaunchSpec>,
    pub resolved: &'a LaunchSpec,
}

const ACTOR_COLUMNS: &str = "id, workspace_key, name, kind, launch_spec, created_at, updated_at,
     deleted_at, schedule_cron, schedule_timezone, schedule_enabled, schedule_created_at,
     schedule_paused_at, schedule_enabled_at, schedule_revision, grants, accept_from";

fn encode_grants(grants: &BTreeSet<Grant>) -> Result<String> {
    serde_json::to_string(grants).context("encode grants")
}

fn encode_accept_from(accept_from: &BTreeSet<String>) -> Result<String> {
    serde_json::to_string(accept_from).context("encode acceptFrom")
}

/// Whether the Actor `id` accepts a Message from the Actor named `from`.
pub(super) fn accepts(connection: &Connection, id: &str, from: &str) -> Result<bool> {
    let accept_from: Option<String> = connection
        .query_row(
            "SELECT accept_from FROM actors WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .context("read Actor acceptFrom")?;
    let Some(accept_from) = accept_from else {
        return Ok(true);
    };
    let accept_from: BTreeSet<String> =
        serde_json::from_str(&accept_from).context("decode acceptFrom")?;
    Ok(accept_from.contains(from))
}

pub(super) struct ActorRow {
    pub(super) id: String,
    pub(super) workspace: String,
    pub(super) name: String,
    pub(super) kind: ActorKind,
    pub(super) launch_spec: Option<String>,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) deleted_at: Option<String>,
    pub(super) schedule: Option<Schedule>,
    pub(super) grants: String,
    pub(super) accept_from: Option<String>,
}

fn actor_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActorRow> {
    let schedule = match (
        row.get::<_, Option<String>>(8)?,
        row.get::<_, Option<String>>(9)?,
        row.get::<_, Option<bool>>(10)?,
        row.get::<_, Option<String>>(11)?,
    ) {
        (Some(cron), Some(timezone), Some(enabled), Some(created_at)) => Some(Schedule {
            cron,
            timezone,
            enabled,
            created_at,
            paused_at: row.get(12)?,
            enabled_at: row.get(13)?,
            next_run: None,
            skipped_ticks: 0,
            overdue_tick: None,
            revision: row.get(14)?,
        }),
        _ => None,
    };
    Ok(ActorRow {
        id: row.get(0)?,
        workspace: row.get(1)?,
        name: row.get(2)?,
        kind: row.get::<_, Text<_>>(3)?.0,
        launch_spec: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        deleted_at: row.get(7)?,
        schedule,
        grants: row.get(15)?,
        accept_from: row.get(16)?,
    })
}

pub(super) fn read_actor_rows<P: Params>(
    connection: &Connection,
    suffix: &str,
    params: P,
) -> Result<Vec<ActorRow>> {
    connection
        .prepare(&format!("SELECT {ACTOR_COLUMNS} FROM actors {suffix}"))
        .context("prepare Actor read")?
        .query_map(params, actor_row)
        .context("query Actors")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("read Actors")
}

fn actor_from_row(connection: &Connection, row: ActorRow) -> Result<Actor> {
    let waiting_count = connection
        .query_row(
            "SELECT COUNT(*) FROM messages
             WHERE to_actor_id = ?1 AND read_id IS NULL AND deleted_at IS NULL",
            [&row.id],
            |row| row.get(0),
        )
        .context("count waiting Messages")?;
    let (skipped_ticks, last_skipped_at): (u64, Option<String>) = connection
        .query_row(
            "SELECT COUNT(*), MAX(at) FROM records WHERE actor_id = ?1 AND kind = ?2",
            params![row.id, Text(RecordKind::ScheduleSkipped)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .context("count skipped Schedule ticks")?;
    let mut actor = Actor {
        sessions: list_sessions_for_actor(connection, &row.id)?,
        id: row.id,
        name: row.name,
        workspace: row.workspace,
        kind: row.kind,
        launch_spec: row
            .launch_spec
            .map(|value| serde_json::from_str(&value).context("decode launchSpec"))
            .transpose()?
            .flatten(),
        schedule: row.schedule.map(|schedule| Schedule {
            skipped_ticks,
            ..schedule
        }),
        grants: serde_json::from_str(&row.grants).context("decode grants")?,
        accept_from: row
            .accept_from
            .map(|value| serde_json::from_str(&value).context("decode acceptFrom"))
            .transpose()?,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        current_session: None,
        latest_session: None,
        attention: false,
        waiting_count,
        unread_mail: false,
        skip_mark: false,
        unlinked_mark: false,
    };
    actor.derive(last_skipped_at.as_deref());
    Ok(actor)
}

/// The live Actor that holds `name` in the Workspace.
fn get_actor(connection: &Connection, workspace: &str, name: &str) -> Result<Option<Actor>> {
    read_actor_rows(
        connection,
        "WHERE workspace_key = ?1 AND name = ?2 AND deleted_at IS NULL",
        params![workspace, name],
    )?
    .into_iter()
    .next()
    .map(|row| actor_from_row(connection, row))
    .transpose()
}

fn get_actor_by_id(connection: &Connection, id: &str) -> Result<Option<Actor>> {
    read_actor_rows(connection, "WHERE id = ?1", [id])?
        .into_iter()
        .next()
        .map(|row| actor_from_row(connection, row))
        .transpose()
}

/// Refuses `change` to the Actor `id` while its current Session runs.
fn refuse_running(
    transaction: &Connection,
    id: &str,
    name: &str,
    change: &'static str,
) -> std::result::Result<(), ActorWriteError> {
    let session = transaction
        .query_row(
            "SELECT id FROM sessions WHERE actor_id = ?1 AND end IS NULL",
            [id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .with_context(|| format!("read Actor current Session for {change}"))?;
    match session {
        Some(session) => Err(ActorWriteError::Running {
            actor: name.to_owned(),
            session,
            change,
        }),
        None => Ok(()),
    }
}

/// The `id` and kind of the live Actor that holds `name` in the Workspace.
pub(super) fn live_actor(
    connection: &Connection,
    workspace: &str,
    name: &str,
) -> Result<Option<(String, ActorKind)>> {
    connection
        .query_row(
            "SELECT id, kind FROM actors
             WHERE workspace_key = ?1 AND name = ?2 AND deleted_at IS NULL",
            params![workspace, name],
            |row| Ok((row.get(0)?, row.get::<_, Text<_>>(1)?.0)),
        )
        .optional()
        .context("read Actor")
}
