//! `feld diff` and `feld apply`: the Actors of a declaration file against the Actors of its
//! Workspace, each declared through `PUT /v1/workspaces/{workspace}/actors/{actor}`, which
//! compares with `dryRun` and writes without it.

mod execute;
mod file;

use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;

use feld_core::model::{Actor, ActorKind, DeclareAction, FieldChange, Grant, LaunchSpec};
use feld_core::plugin::{Registry, choose_provider};
use feld_daemon::api::{ActorList, DeclareActorRequest, DeclareResponse, DeclareScheduleRequest};
use serde::Serialize;

pub use execute::apply;

use crate::args::DiffArgs;
use crate::client::{Daemon, actor_path, actors_path};
use crate::format::declaration::print_changes;
use crate::launch::check_workdir;
use crate::{CliError, CliResult, Context};

/// What a declaration file declares, with `${VAR}` expanded and paths made absolute.
pub struct Declaration {
    /// The file's `name`, which the Actors it declares record as `declaredIn`.
    pub name: String,
    pub workspace: Option<String>,
    pub actors: Vec<DeclaredActor>,
}

/// One Actor of the file. A field that is None is left out of the file, which keeps it as
/// it is on a live Actor.
pub struct DeclaredActor {
    pub name: String,
    pub kind: ActorKind,
    pub launch: LaunchSpec,
    pub schedule: Option<DeclaredSchedule>,
    pub grants: Option<BTreeSet<Grant>>,
    /// `Some(None)` is a written `null`, which accepts every Actor of the Workspace.
    pub accept_from: Option<Option<BTreeSet<String>>>,
}

pub struct DeclaredSchedule {
    pub cron: String,
    pub timezone: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// A new Actor.
    Create,
    /// A live Actor whose declaration differs.
    Replace,
    /// An Actor an earlier apply of the file declared, which the file no longer has.
    Delete,
}

impl Action {
    pub fn mark(self) -> char {
        match self {
            Self::Create => '+',
            Self::Replace => '~',
            Self::Delete => '-',
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub action: Action,
    pub actor: String,
    pub kind: ActorKind,
    /// The fields that differ, by their dotted path. A delete names none.
    pub fields: Vec<FieldChange>,
}

/// A live Actor an earlier apply of the file declared and the file no longer has, which
/// `--prune` deletes.
#[derive(Clone, Debug, Serialize)]
pub struct Prunable {
    pub actor: String,
    pub kind: ActorKind,
}

/// The changes of one declaration file against its Workspace, as `--json` prints them.
#[derive(Serialize)]
pub struct Report<'a> {
    pub name: &'a str,
    pub workspace: &'a str,
    pub changes: Vec<Change>,
    /// The Actors `--prune` would delete, when it was not given.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prunable: Vec<Prunable>,
}

/// A declaration file read against the daemon: what it declares, the Workspace it
/// declares into, and the live Actors of that Workspace.
struct Prepared {
    daemon: Daemon,
    declaration: Declaration,
    workspace: String,
    current: Vec<Actor>,
}

pub async fn diff(ctx: &Context, args: DiffArgs) -> CliResult<ExitCode> {
    let prepared = prepare(ctx, &args.file).await?;
    let mut changes = prepared.declare_each(true).await?;
    let prunable = prepared.prunable();
    let prunable = if args.prune {
        changes.extend(prunable.into_iter().map(Change::delete));
        Vec::new()
    } else {
        prunable
    };
    let report = Report {
        name: &prepared.declaration.name,
        workspace: &prepared.workspace,
        changes,
        prunable,
    };
    ctx.print(&report, print_changes)?;
    Ok(ExitCode::SUCCESS)
}

/// Reads the file, completes each launchSpec as `feld actor create` would, and reads the
/// live Actors of the Workspace. The daemon is spoken to as the Admin, who alone reads every
/// launchSpec and sets grants.
async fn prepare(ctx: &Context, path: &Path) -> CliResult<Prepared> {
    let mut declaration = file::read(path, &|name| std::env::var(name).ok())?;
    let workspace = declaration
        .workspace
        .clone()
        .unwrap_or_else(|| ctx.workspace.clone());
    let daemon = Daemon::connect_local(ctx).await?;
    let registry = Registry::load(&ctx.home, Some(&daemon.api));
    for actor in &mut declaration.actors {
        complete_launch(&mut actor.launch, actor.kind, &daemon.providers, &registry)
            .map_err(|reason| rejected!("Actor '{}': {reason}", actor.name))?;
    }
    let current: ActorList = daemon.get(&actors_path(&workspace)).await?;
    Ok(Prepared {
        daemon,
        declaration,
        workspace,
        current: current.actors,
    })
}

impl Prepared {
    /// Declares each Actor of the file in its order, and answers the changes: what the
    /// daemon made, or with `dry_run` what it would make. A refusal stops at that Actor,
    /// and the error says how many changes before it were committed.
    async fn declare_each(&self, dry_run: bool) -> CliResult<Vec<Change>> {
        let mut changes = Vec::new();
        for declared in &self.declaration.actors {
            let response: DeclareResponse = self
                .daemon
                .put_json(
                    &actor_path(&self.workspace, &declared.name),
                    &self.request(declared),
                    dry_run.then_some(&[("dryRun", "true")]),
                )
                .await
                .map_err(|error| {
                    stopped(
                        error,
                        &declared.name,
                        if dry_run { 0 } else { changes.len() },
                    )
                })?;
            let action = match response.action {
                DeclareAction::Create => Action::Create,
                DeclareAction::Replace => Action::Replace,
                DeclareAction::Unchanged => continue,
            };
            changes.push(Change {
                action,
                actor: declared.name.clone(),
                kind: declared.kind,
                fields: response.fields,
            });
        }
        Ok(changes)
    }

    fn request(&self, declared: &DeclaredActor) -> DeclareActorRequest {
        DeclareActorRequest {
            kind: declared.kind,
            launch_spec: declared.launch.clone(),
            schedule: declared
                .schedule
                .as_ref()
                .map(|schedule| DeclareScheduleRequest {
                    cron: schedule.cron.clone(),
                    timezone: schedule.timezone.clone(),
                    enabled: schedule.enabled,
                }),
            grants: declared.grants.clone(),
            accept_from: declared.accept_from.clone(),
            declared_in: Some(self.declaration.name.clone()),
            caller: None,
        }
    }

    fn prunable(&self) -> Vec<Prunable> {
        prunable_of(&self.declaration, &self.current)
    }
}

/// The live Actors of `current` whose `declaredIn` is the file's `name` and that the file no
/// longer declares, by name.
fn prunable_of(declaration: &Declaration, current: &[Actor]) -> Vec<Prunable> {
    let mut prunable: Vec<Prunable> = current
        .iter()
        .filter(|actor| actor.deleted_at.is_none())
        .filter(|actor| actor.declared_in.as_deref() == Some(declaration.name.as_str()))
        .filter(|actor| {
            !declaration
                .actors
                .iter()
                .any(|declared| declared.name == actor.name)
        })
        .map(|actor| Prunable {
            actor: actor.name.clone(),
            kind: actor.kind,
        })
        .collect();
    prunable.sort_by(|left, right| left.actor.cmp(&right.actor));
    prunable
}

impl Change {
    fn delete(prunable: Prunable) -> Self {
        Self {
            action: Action::Delete,
            actor: prunable.actor,
            kind: prunable.kind,
            fields: Vec::new(),
        }
    }
}

/// The error of the Actor the command stopped at, which says how many changes before it
/// were committed.
fn stopped(error: CliError, actor: &str, committed: usize) -> CliError {
    let error = match error {
        CliError::Rejected(reason) => CliError::Rejected(format!("Actor '{actor}': {reason}")),
        error => error,
    };
    if committed == 0 {
        error
    } else {
        error.after_commit(&format!("{committed} earlier change(s)"))
    }
}

/// Fills what the file leaves out as `feld actor create` does: the only installed Execution
/// Provider, the `local` Node, and the workdir made canonical. The launchSpec is then checked
/// as the daemon would check it.
fn complete_launch(
    launch: &mut LaunchSpec,
    kind: ActorKind,
    installed: &[String],
    registry: &Registry,
) -> Result<(), String> {
    let node = launch
        .node
        .get_or_insert_with(|| "local".to_owned())
        .clone();
    if launch.provider.is_none() {
        launch.provider = Some(
            choose_provider(None, &[], installed, None).map_err(|error| match error {
                feld_core::plugin::ProviderChoiceError::Ambiguous => format!(
                    "several Execution Providers are installed ({}); declare `provider`",
                    installed.join(", ")
                ),
                error => error.to_string(),
            })?,
        );
    }
    if let Some(workdir) = &launch.workdir {
        let workdir =
            check_workdir(Path::new(workdir), &node).map_err(|error| error.to_string())?;
        launch.workdir = Some(workdir.display().to_string());
    }
    launch.validate()?;
    launch.validate_kind(kind)?;
    launch
        .validate_installed(registry)
        .map_err(|refusal| refusal.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn actor(name: &str, declared_in: Option<&str>, deleted: bool) -> Actor {
        serde_json::from_value(json!({
            "id": format!("actor_{name}"),
            "name": name,
            "workspace": "default",
            "kind": "job",
            "launchSpec": null,
            "schedule": null,
            "grants": [],
            "acceptFrom": null,
            "declaredIn": declared_in,
            "createdAt": "2026-11-04T00:00:00Z",
            "updatedAt": "2026-11-04T00:00:00Z",
            "deletedAt": deleted.then_some("2026-11-05T00:00:00Z"),
            "currentSession": null,
            "latestSession": null,
            "sessions": [],
            "attention": false,
            "waitingCount": 0,
            "unreadMail": false,
            "skipMark": false,
            "unlinkedMark": false,
        }))
        .expect("actor")
    }

    #[test]
    fn prunable_are_the_live_actors_of_the_file_it_no_longer_declares() {
        let declaration = Declaration {
            name: "harness".to_owned(),
            workspace: None,
            actors: vec![DeclaredActor {
                name: "dispatcher".to_owned(),
                kind: ActorKind::Job,
                launch: LaunchSpec::default(),
                schedule: None,
                grants: None,
                accept_from: None,
            }],
        };
        let current = vec![
            actor("dispatcher", Some("harness"), false),
            actor("nightly", Some("harness"), false),
            actor("gone", Some("harness"), true),
            actor("other", Some("elsewhere"), false),
            actor("manual", None, false),
            actor("archive", Some("harness"), false),
        ];
        let prunable = prunable_of(&declaration, &current);
        let names: Vec<&str> = prunable.iter().map(|item| item.actor.as_str()).collect();
        assert_eq!(names, ["archive", "nightly"]);
    }
}
