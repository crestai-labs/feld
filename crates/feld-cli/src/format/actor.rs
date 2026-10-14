//! Actors as a person reads them: a declaration, a change to one, and the detail of one.

use std::collections::BTreeSet;

use feld_core::model::{Actor, Grant, LaunchSpec, Schedule};
use feld_core::process::command_line;

use crate::Context;
use crate::args::LaunchArgs;
use crate::format::session::outcome_summary;
use crate::output::{change_line, detail_line, kinded, print_table};

pub fn actor_marks(actor: &Actor) -> String {
    let overdue = actor
        .schedule
        .as_ref()
        .is_some_and(|schedule| schedule.overdue_tick.is_some());
    let marks: Vec<&str> = [
        (actor.unread_mail, "mail"),
        (actor.skip_mark, "skip"),
        (overdue, "overdue"),
        (actor.unlinked_mark, "unlinked"),
    ]
    .into_iter()
    .filter_map(|(shown, mark)| shown.then_some(mark))
    .collect();
    if marks.is_empty() {
        "-".to_owned()
    } else {
        marks.join(",")
    }
}

/// The grants of an Actor, or `-` without any.
pub fn grants_summary(grants: &BTreeSet<Grant>) -> String {
    if grants.is_empty() {
        "-".to_owned()
    } else {
        grants
            .iter()
            .map(|grant| grant.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// The Actors whose Messages an Actor accepts besides the Admin's: `*` for every Actor of
/// the Workspace, `-` for none.
pub fn accept_from_summary(accept_from: Option<&BTreeSet<String>>) -> String {
    match accept_from {
        None => "*".to_owned(),
        Some(names) if names.is_empty() => "-".to_owned(),
        Some(names) => names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(","),
    }
}

pub fn print_actor_created(ctx: &Context, title: &str, actor: &Actor) {
    println!("{title}");
    change_line("actor:", &kinded(&actor.name, actor.kind));
    if let Some(launch) = &actor.launch_spec {
        print_execution_line(launch);
        if let Some(workdir) = &launch.workdir {
            change_line("workdir:", workdir);
        }
        if let Some(names) = env_names(launch) {
            change_line("env:", &names);
        }
        change_line("provider:", &provider_summary(launch));
    }
    print_schedule_change(ctx, actor.schedule.as_ref());
    if !actor.grants.is_empty() {
        change_line("grants:", &grants_summary(&actor.grants));
    }
    if actor.accept_from.is_some() {
        change_line(
            "accept_from:",
            &accept_from_summary(actor.accept_from.as_ref()),
        );
    }
}

fn print_execution_line(launch: &LaunchSpec) {
    if let Some(runtime) = &launch.runtime {
        change_line("runtime:", runtime);
    } else if let Some(command) = &launch.command {
        change_line("command:", &command_line(command));
    }
}

/// The names of the declared environment. The values can be secrets, so only `--json` shows
/// them.
fn env_names(launch: &LaunchSpec) -> Option<String> {
    let env = launch.env.as_ref().filter(|env| !env.is_empty())?;
    Some(env.keys().cloned().collect::<Vec<_>>().join(", "))
}

fn trusts_workdir(launch: &LaunchSpec) -> bool {
    launch.trust_workdir.unwrap_or(false)
}

fn provider_summary(launch: &LaunchSpec) -> String {
    format!(
        "{} · {}",
        launch.provider.as_deref().unwrap_or("-"),
        launch.node.as_deref().unwrap_or("-")
    )
}

fn print_schedule_change(ctx: &Context, schedule: Option<&Schedule>) {
    if let Some(schedule) = schedule {
        change_line(
            "schedule:",
            &format!(
                "{} ({}), {}",
                schedule.cron,
                schedule.timezone,
                if schedule.enabled {
                    "enabled"
                } else {
                    "paused"
                }
            ),
        );
        change_line(
            "next_run:",
            &ctx.local_time_or_dash(schedule.next_run.as_deref()),
        );
    }
}

/// What `feld actor update` changed, and when the change takes effect.
pub fn print_actor_updated(
    ctx: &Context,
    actor: &Actor,
    renamed_from: Option<&str>,
    launch: Option<&LaunchArgs>,
    schedule_updated: bool,
    grants_updated: bool,
    accept_from_updated: bool,
) {
    println!("Actor updated.");
    change_line("actor:", &kinded(&actor.name, actor.kind));
    if let Some(previous) = renamed_from {
        change_line("renamed:", &format!("{previous} -> {}", actor.name));
    }
    if let Some(arguments) = launch
        && let Some(spec) = &actor.launch_spec
    {
        print_execution_line(spec);
        if arguments.workdir.is_some()
            && let Some(workdir) = &spec.workdir
        {
            change_line("workdir:", workdir);
        }
        if arguments.provider.is_some() || arguments.node.is_some() {
            change_line("provider:", &provider_summary(spec));
        }
        if arguments.trust_workdir || arguments.no_trust_workdir {
            change_line("trust:", if trusts_workdir(spec) { "workdir" } else { "-" });
        }
    }
    if schedule_updated {
        print_schedule_change(ctx, actor.schedule.as_ref());
    }
    if grants_updated {
        change_line("grants:", &grants_summary(&actor.grants));
    }
    if accept_from_updated {
        change_line(
            "accept_from:",
            &accept_from_summary(actor.accept_from.as_ref()),
        );
    }
    // A launchSpec takes effect from the next start, a Schedule from the next tick.
    let applies = match (launch.is_some(), schedule_updated) {
        (true, true) => "next start or tick",
        (true, false) => "next start",
        (false, true) => "next tick",
        (false, false) => return,
    };
    change_line(
        "applies:",
        &actor.current_session.as_ref().map_or_else(
            || applies.to_owned(),
            |session| format!("{applies} ({} untouched)", session.id),
        ),
    );
}

/// How many Sessions `feld actor get` lists; `feld session get` reads any one in full.
const RECENT_SESSIONS: usize = 10;

pub fn print_actor_detail(ctx: &Context, actor: &Actor) {
    println!("Actor");
    detail_line("name:", &actor.name);
    detail_line("id:", &actor.id);
    detail_line("kind:", actor.kind.as_str());
    detail_line("workspace:", &actor.workspace);
    detail_line(
        "launch_spec:",
        &actor
            .launch_spec
            .as_ref()
            .map_or_else(|| "attached only".to_owned(), launch_summary),
    );
    if let Some(names) = actor.launch_spec.as_ref().and_then(env_names) {
        detail_line("env:", &names);
    }
    if actor.launch_spec.as_ref().is_some_and(trusts_workdir) {
        detail_line("trust:", "workdir");
    }
    if let Some(schedule) = &actor.schedule {
        detail_line(
            "schedule:",
            &format!(
                "{} ({}) · {}",
                schedule.cron,
                schedule.timezone,
                schedule_state(ctx, schedule)
            ),
        );
        detail_line(
            "next_run:",
            &ctx.local_time_or_dash(schedule.next_run.as_deref()),
        );
        detail_line("skipped_ticks:", &schedule.skipped_ticks.to_string());
        if let Some(overdue) = &schedule.overdue_tick {
            detail_line("overdue_tick:", &ctx.local_time(overdue));
        }
    }
    detail_line("grants:", &grants_summary(&actor.grants));
    detail_line(
        "accept_from:",
        &accept_from_summary(actor.accept_from.as_ref()),
    );
    detail_line(
        "deleted_at:",
        &ctx.local_time_or_dash(actor.deleted_at.as_deref()),
    );
    println!("Sessions ({})", actor.sessions.len());
    let rows: Vec<Vec<String>> = actor
        .sessions
        .iter()
        .take(RECENT_SESSIONS)
        .map(|session| {
            vec![
                if session.end.is_none() { "*" } else { "" }.to_owned(),
                session.generation.to_string(),
                session.id.clone(),
                ctx.local_time(&session.start),
                session.start_kind.as_str().to_owned(),
                outcome_summary(session),
            ]
        })
        .collect();
    if !rows.is_empty() {
        print_table(
            &["", "GEN", "SESSION", "START", "START_KIND", "OUTCOME"],
            &rows,
        );
    }
}

fn schedule_state(ctx: &Context, schedule: &Schedule) -> String {
    let (state, since) = if schedule.enabled {
        ("enabled", schedule.enabled_at.as_deref())
    } else {
        ("paused", schedule.paused_at.as_deref())
    };
    format!(
        "{state} since {}",
        ctx.local_time(since.unwrap_or(&schedule.created_at))
    )
}

fn launch_summary(launch: &LaunchSpec) -> String {
    let execution = launch
        .runtime
        .clone()
        .or_else(|| launch.command.as_ref().map(|command| command_line(command)))
        .unwrap_or_else(|| "-".to_owned());
    format!(
        "{execution} · {} · {}",
        launch.workdir.as_deref().unwrap_or("-"),
        provider_summary(launch)
    )
}
