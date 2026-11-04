//! `feld diff` and `feld apply`: the Actors of a declaration file against the Actors of its
//! Workspace.

mod execute;
mod file;
mod plan;

use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;

use feld_core::model::{ActorKind, Grant, LaunchSpec};
use feld_core::plugin::{Registry, choose_provider};
use feld_daemon::api::ActorList;
use serde::Serialize;

pub use execute::{AppliedReport, apply};
use plan::plan;
pub use plan::{Action, Change, FieldChange};

use crate::args::DiffArgs;
use crate::client::{Daemon, actors_path};
use crate::format::declaration::print_changes;
use crate::launch::check_workdir;
use crate::{CliResult, Context};

/// What a declaration file declares, with `${VAR}` expanded and paths made absolute.
pub struct Declaration {
    /// The file's `name`, which the Actors it declares record as `declaredIn`.
    pub name: String,
    pub workspace: Option<String>,
    pub actors: Vec<DeclaredActor>,
}

pub struct DeclaredActor {
    pub name: String,
    pub kind: ActorKind,
    pub launch: LaunchSpec,
    pub schedule: Option<DeclaredSchedule>,
    pub grants: BTreeSet<Grant>,
    pub accept_from: Option<BTreeSet<String>>,
}

pub struct DeclaredSchedule {
    pub cron: String,
    /// None leaves the timezone to the daemon.
    pub timezone: Option<String>,
    /// None keeps whether the Schedule is paused.
    pub enabled: Option<bool>,
}

/// The changes of one declaration file against its Workspace, as `--json` prints them.
#[derive(Serialize)]
pub struct Report<'a> {
    pub name: &'a str,
    pub workspace: &'a str,
    pub changes: &'a [Change],
}

/// A declaration file read against the daemon: what it declares, the Workspace it
/// declares into, and the changes that make the Workspace match it.
struct Prepared {
    daemon: Daemon,
    declaration: Declaration,
    workspace: String,
    changes: Vec<Change>,
}

pub async fn diff(ctx: &Context, args: DiffArgs) -> CliResult<ExitCode> {
    let prepared = prepare(ctx, &args.file).await?;
    let report = Report {
        name: &prepared.declaration.name,
        workspace: &prepared.workspace,
        changes: &prepared.changes,
    };
    ctx.print(&report, print_changes)?;
    Ok(ExitCode::SUCCESS)
}

/// Reads the file, completes each launchSpec as `feld actor create` would, and compares it
/// with the live Actors of the Workspace. The daemon is spoken to as the Admin, who alone
/// reads every launchSpec and sets grants.
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
    let changes = plan(&declaration, &current.actors).map_err(|reason| rejected!("{reason}"))?;
    Ok(Prepared {
        daemon,
        declaration,
        workspace,
        changes,
    })
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
