//! `feld apply`: the changes of the plan, made once through the Actor API.

use std::process::ExitCode;

use feld_daemon::api::{
    ActorResponse, CreateActorRequest, CreateScheduleRequest, UpdateActorRequest,
    UpdateScheduleRequest,
};
use serde::Serialize;

use super::plan::{Action, Change};
use super::{DeclaredActor, Prepared, prepare};
use crate::args::ApplyArgs;
use crate::client::{Daemon, actor_path, actors_path};
use crate::format::declaration::print_applied;
use crate::{CliError, CliResult, Context};

/// What became of one change.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied<'a> {
    #[serde(flatten)]
    pub change: &'a Change,
    /// Why the change was left undone, which only a delete the daemon refuses has: an Actor
    /// whose current Session runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept: Option<String>,
}

/// The changes of one apply, as `--json` prints them.
#[derive(Serialize)]
pub struct AppliedReport<'a> {
    pub name: &'a str,
    pub workspace: &'a str,
    pub changes: Vec<Applied<'a>>,
}

pub async fn apply(ctx: &Context, args: ApplyArgs) -> CliResult<ExitCode> {
    let Prepared {
        daemon,
        declaration,
        workspace,
        changes,
    } = prepare(ctx, &args.file).await?;
    let mut applied = Vec::with_capacity(changes.len());
    // The declared Actors come first, so that a refusal on the way leaves the deletes undone.
    for change in &changes {
        let kept = match change.action {
            Action::Create | Action::Replace => {
                let declared = declaration
                    .actors
                    .iter()
                    .find(|actor| actor.name == change.actor)
                    .expect("a declared Actor is in the file");
                declare(&daemon, &workspace, &declaration.name, declared)
                    .await
                    .map_err(|error| stopped(error, change, applied.len()))?;
                None
            }
            Action::Delete => match daemon
                .delete::<ActorResponse>(&actor_path(&workspace, &change.actor))
                .await
            {
                Ok(_) => None,
                Err(CliError::Rejected(reason)) => Some(reason),
                Err(error) => return Err(stopped(error, change, applied.len())),
            },
        };
        applied.push(Applied { change, kept });
    }
    let kept = applied.iter().any(|applied| applied.kept.is_some());
    let report = AppliedReport {
        name: &declaration.name,
        workspace: &workspace,
        changes: applied,
    };
    ctx.print(&report, print_applied)?;
    Ok(if kept {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// The error of the change the apply stopped at, which says how many changes before it were
/// committed.
fn stopped(error: CliError, change: &Change, committed: usize) -> CliError {
    let error = match error {
        CliError::Rejected(reason) => {
            CliError::Rejected(format!("Actor '{}': {reason}", change.actor))
        }
        error => error,
    };
    if committed == 0 {
        error
    } else {
        error.after_commit(&format!("{committed} earlier change(s)"))
    }
}

/// Declares one Actor as `feld actor create` does, recording the file as `declaredIn`. A
/// declared `enabled` and an `acceptFrom` the file leaves out follow with an update, since a
/// replaced declaration keeps both.
async fn declare(
    daemon: &Daemon,
    workspace: &str,
    file: &str,
    declared: &DeclaredActor,
) -> CliResult<()> {
    let created: ActorResponse = daemon
        .post_json(
            &actors_path(workspace),
            &CreateActorRequest {
                name: declared.name.clone(),
                kind: declared.kind,
                launch_spec: declared.launch.clone(),
                schedule: declared
                    .schedule
                    .as_ref()
                    .map(|schedule| CreateScheduleRequest {
                        cron: schedule.cron.clone(),
                        timezone: schedule.timezone.clone(),
                    }),
                grants: Some(declared.grants.clone()),
                accept_from: declared.accept_from.clone(),
                declared_in: Some(file.to_owned()),
                caller: None,
            },
        )
        .await?;
    let actor = created.actor;
    let enabled = declared
        .schedule
        .as_ref()
        .and_then(|schedule| schedule.enabled)
        .filter(|enabled| {
            actor
                .schedule
                .as_ref()
                .is_some_and(|schedule| schedule.enabled != *enabled)
        });
    let accept_every_actor = declared.accept_from.is_none() && actor.accept_from.is_some();
    if enabled.is_some() || accept_every_actor {
        let _: ActorResponse = daemon
            .patch(
                &actor_path(workspace, &declared.name),
                &UpdateActorRequest {
                    schedule: enabled.map(|enabled| UpdateScheduleRequest {
                        cron: None,
                        timezone: None,
                        enabled: Some(enabled),
                    }),
                    accept_from: accept_every_actor.then_some(None),
                    ..UpdateActorRequest::default()
                },
            )
            .await
            .map_err(|error| error.after_commit("Actor declaration"))?;
    }
    Ok(())
}
