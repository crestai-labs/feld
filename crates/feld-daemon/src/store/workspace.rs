use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use feld_core::id::fresh_id;
use feld_core::time::now;

use super::Store;
use super::error::WorkspaceDeleteError;

/// The Workspace the store keeps live: commands without a Workspace act on it.
pub(super) const DEFAULT_WORKSPACE: &str = "default";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub key: String,
    pub actor_count: u64,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug)]
pub enum CreateWorkspace {
    Created(Workspace),
    Exists(Workspace),
}

impl Store {
    pub fn list_workspaces(&self, include_deleted: bool) -> Result<Vec<Workspace>> {
        self.read(|connection| {
            let mut statement = connection
                .prepare(&format!(
                    "{WORKSPACE_SELECT} WHERE ?1 OR w.deleted_at IS NULL
                     GROUP BY w.id ORDER BY w.key, w.created_at"
                ))
                .context("prepare Workspace listing")?;
            let rows = statement
                .query_map([include_deleted], workspace_row)
                .context("query Workspaces")?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .context("read Workspaces")
        })
    }

    /// The live Workspace that holds `key`, or else the one deleted last.
    pub fn get_workspace(&self, key: &str) -> Result<Option<Workspace>> {
        self.read(|connection| get_workspace(connection, key))
    }

    pub fn create_workspace(&self, key: &str) -> Result<CreateWorkspace> {
        self.write("Workspace creation", |transaction, notices| {
            let inserted = transaction
                .execute(
                    "INSERT OR IGNORE INTO workspaces (id, key, created_at) VALUES (?1, ?2, ?3)",
                    params![fresh_id("workspace"), key, now()],
                )
                .context("create Workspace")?;
            let workspace =
                get_workspace(transaction, key)?.context("created Workspace was not found")?;
            Ok(if inserted == 1 {
                notices.push("workspace", &workspace.key, None);
                CreateWorkspace::Created(workspace)
            } else {
                CreateWorkspace::Exists(workspace)
            })
        })
    }

    /// Records `deletedAt` on the live Workspace that holds `key` and has no live Actor;
    /// the default Workspace stays live.
    pub fn delete_workspace(
        &self,
        key: &str,
    ) -> std::result::Result<Workspace, WorkspaceDeleteError> {
        if key == DEFAULT_WORKSPACE {
            return Err(WorkspaceDeleteError::Default(key.to_owned()));
        }
        self.write("Workspace delete", |transaction, notices| {
            let workspace = get_workspace(transaction, key)?
                .filter(|workspace| workspace.deleted_at.is_none())
                .ok_or_else(|| WorkspaceDeleteError::NotFound(key.to_owned()))?;
            if workspace.actor_count > 0 {
                return Err(WorkspaceDeleteError::LiveActors {
                    workspace: key.to_owned(),
                    actors: workspace.actor_count,
                });
            }
            transaction
                .execute(
                    "UPDATE workspaces SET deleted_at = ?2 WHERE id = ?1",
                    params![workspace.id, now()],
                )
                .context("delete Workspace")?;
            let workspace =
                get_workspace(transaction, key)?.context("deleted Workspace was not found")?;
            notices.push("workspace", key, None);
            Ok(workspace)
        })
    }
}

const WORKSPACE_SELECT: &str = "SELECT w.id, w.key, COUNT(a.id), w.created_at, w.deleted_at
     FROM workspaces w
     LEFT JOIN actors a ON a.workspace_id = w.id AND a.deleted_at IS NULL";

fn workspace_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get(0)?,
        key: row.get(1)?,
        actor_count: row.get(2)?,
        created_at: row.get(3)?,
        deleted_at: row.get(4)?,
    })
}

fn get_workspace(connection: &Connection, key: &str) -> Result<Option<Workspace>> {
    connection
        .query_row(
            &format!(
                "{WORKSPACE_SELECT} WHERE w.key = ?1 GROUP BY w.id
                 ORDER BY w.deleted_at IS NOT NULL, w.deleted_at DESC LIMIT 1"
            ),
            [key],
            workspace_row,
        )
        .optional()
        .context("read Workspace")
}

pub(super) fn live_workspace_id(connection: &Connection, key: &str) -> Result<Option<String>> {
    connection
        .query_row(
            "SELECT id FROM workspaces WHERE key = ?1 AND deleted_at IS NULL",
            [key],
            |row| row.get(0),
        )
        .optional()
        .context("read Workspace")
}
