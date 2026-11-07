//! `feld apply`: each Actor of the file declared once, and with `--prune` the Actors of the
//! file it no longer declares deleted.

use std::process::ExitCode;

use feld_daemon::api::ActorResponse;

use super::{Change, Report, prepare, stopped};
use crate::args::ApplyArgs;
use crate::client::actor_path;
use crate::format::declaration::print_applied;
use crate::{CliResult, Context};

pub async fn apply(ctx: &Context, args: ApplyArgs) -> CliResult<ExitCode> {
    let prepared = prepare(ctx, &args.file).await?;
    // The declared Actors come first, so that a refusal on the way leaves the deletes undone.
    let mut changes = prepared.declare_each(false).await?;
    let prunable = prepared.prunable();
    let prunable = if args.prune {
        for item in prunable {
            let _: ActorResponse = prepared
                .daemon
                .delete(&actor_path(&prepared.workspace, &item.actor))
                .await
                .map_err(|error| stopped(error, &item.actor, changes.len()))?;
            changes.push(Change::delete(item));
        }
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
    ctx.print(&report, print_applied)?;
    Ok(ExitCode::SUCCESS)
}
