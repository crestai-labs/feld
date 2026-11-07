//! `feld actor label` and `feld self label`, which set and clear the labels of an Actor.

use std::process::ExitCode;

use feld_daemon::api::{ActorResponse, SessionResponse, SetLabelRequest};

use crate::args::{ActorLabelCommand, LabelSetArgs, SelfLabelCommand};
use crate::client::{Daemon, actor_path, percent_encode};
use crate::format::actor::label_summary;
use crate::output::{change_line, kinded};
use crate::{CliResult, Context};

pub async fn run_actor(ctx: &Context, command: ActorLabelCommand) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    match command {
        ActorLabelCommand::Set { actor, label } => {
            set(ctx, &daemon, &ctx.workspace, &actor, label).await
        }
        ActorLabelCommand::Clear { actor, key } => {
            clear(ctx, &daemon, &ctx.workspace, &actor, &key).await
        }
    }
}

/// The label commands of the caller's Session, on the Actor its Session token names.
pub async fn run_self(ctx: &Context, command: SelfLabelCommand) -> CliResult<ExitCode> {
    let daemon = Daemon::connect_self(ctx).await?;
    let own: SessionResponse = daemon.get("/v1/self").await?;
    let (workspace, actor) = (&own.session.workspace, &own.session.actor);
    match command {
        SelfLabelCommand::Set(label) => set(ctx, &daemon, workspace, actor, label).await,
        SelfLabelCommand::Clear { key } => clear(ctx, &daemon, workspace, actor, &key).await,
    }
}

fn label_path(workspace: &str, actor: &str, key: &str) -> String {
    format!(
        "{}/labels/{}",
        actor_path(workspace, actor),
        percent_encode(key)
    )
}

async fn set(
    ctx: &Context,
    daemon: &Daemon,
    workspace: &str,
    actor: &str,
    label: LabelSetArgs,
) -> CliResult<ExitCode> {
    let request = SetLabelRequest {
        until: label.until.map(|until| until.resolve(chrono::Utc::now())),
        reason: label.reason,
    };
    let response: ActorResponse = daemon
        .put_json(&label_path(workspace, actor, &label.key), &request)
        .await?;
    ctx.print(&response, |response| {
        println!("Label set.");
        print_change(ctx, &response.actor, &label.key);
    })?;
    Ok(ExitCode::SUCCESS)
}

async fn clear(
    ctx: &Context,
    daemon: &Daemon,
    workspace: &str,
    actor: &str,
    key: &str,
) -> CliResult<ExitCode> {
    let response: ActorResponse = daemon.delete(&label_path(workspace, actor, key)).await?;
    ctx.print(&response, |response| {
        println!("Label cleared.");
        print_change(ctx, &response.actor, key);
    })?;
    Ok(ExitCode::SUCCESS)
}

fn print_change(ctx: &Context, actor: &feld_core::model::Actor, key: &str) {
    change_line("actor:", &kinded(&actor.name, actor.kind));
    change_line("label:", key);
    if let Some(label) = actor.labels.iter().find(|label| label.key == key) {
        change_line("until:", &ctx.local_time_or_dash(label.until.as_deref()));
        if let Some(reason) = &label.reason {
            change_line("reason:", reason);
        }
    }
    change_line("labels:", &label_summary(&actor.labels));
}
