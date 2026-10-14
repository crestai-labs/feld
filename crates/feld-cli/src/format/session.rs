//! Sessions as a person reads them: a start, an attach, a stop or kill, and the detail of
//! one Session.

use std::process::ExitCode;

use feld_core::model::{Execution, LifecycleAction, Ownership, Requester, Session};
use feld_core::plugin::Plugins;
use feld_core::process::command_line;
use feld_daemon::console::{ConsoleState, SubAgentSession, TranscriptPage};
use serde_json::Value;

use crate::Context;
use crate::output::{change_line, detail_line, kinded};

/// A stop or kill as `feld session get` prints it: who asked, when, and what came of it.
pub fn lifecycle_action_summary(
    ctx: &Context,
    action: &LifecycleAction,
    requester: &str,
) -> String {
    let observed = action.observed.as_ref().map_or_else(
        || "pending".to_owned(),
        |observation| ctx.observed(observation.kind.as_str(), &observation.at),
    );
    format!(
        "{} by {requester} · attempt {} · observed {observed}",
        ctx.local_time(&action.accepted),
        attempt_summary(action),
    )
}

/// What the Execution Provider tried: `pending` until it answers, `unknown` when it could not.
fn attempt_summary(action: &LifecycleAction) -> &str {
    match (&action.attempt, &action.observed) {
        (Some(attempt), _) => attempt,
        (None, Some(_)) => "unknown",
        (None, None) => "pending",
    }
}

/// The transcript entries of one transcript read, one line each; the whereabouts of a
/// transcript that cannot be read is said once, before the first read.
pub fn print_transcript_page(ctx: &Context, page: &TranscriptPage, first: bool) {
    if first && page.state != ConsoleState::Readable {
        let (state, reason) = match page.state {
            ConsoleState::Pending => (
                "pending",
                "the Runtime has not said where its transcript file is",
            ),
            ConsoleState::Missing => ("missing", "the transcript file does not exist"),
            ConsoleState::Lost => ("lost", "the transcript file can no longer be found"),
            ConsoleState::Readable => ("", ""),
        };
        println!("{state}: {}", page.detail.as_deref().unwrap_or(reason));
    }
    for record in &page.records {
        let mut lines = record.text.lines();
        println!(
            "{}  {}  {}",
            ctx.local_time_or_dash(record.at.as_deref()),
            record.kind.as_str(),
            lines.next().unwrap_or_default()
        );
        for line in lines {
            println!("    {line}");
        }
    }
}

pub fn print_start(ctx: &Context, title: &str, session: &Session, replaced: Option<&str>) {
    println!("{title}");
    change_line("actor:", &kinded(&session.actor, session.actor_kind));
    print_start_lines(ctx, session, replaced);
}

pub fn print_start_lines(ctx: &Context, session: &Session, replaced: Option<&str>) {
    change_line(
        "session:",
        &replaced.map_or_else(
            || session.id.clone(),
            |previous| format!("{} (replaces {previous})", session.id),
        ),
    );
    change_line("generation:", &session.generation.to_string());
    change_line("start_kind:", session.start_kind.as_str());
    change_line("placement:", &placement_summary(ctx, session, false));
    change_line("result:", &start_result(ctx, session));
    if let Some(reason) = start_failure(session) {
        change_line("reason:", &reason);
    }
}

pub fn print_attached(ctx: &Context, title: &str, session: &Session) {
    println!("{title}");
    change_line("actor:", &kinded(&session.actor, session.actor_kind));
    change_line("session:", &session.id);
    change_line(
        "start_kind:",
        &kinded(session.start_kind.as_str(), session.ownership.as_str()),
    );
    change_line("execution:", &execution_summary(&session.execution));
    change_line("placement:", &placement_summary(ctx, session, false));
    change_line("observation:", &observation_summary(ctx, session));
}

pub fn print_lifecycle(ctx: &Context, title: &str, session: &Session, kill: bool) {
    println!("{title}");
    change_line("session:", &session.id);
    let requested = if kill {
        session.lifecycle.kill.as_ref()
    } else {
        session.lifecycle.stop.as_ref()
    };
    if let Some(action) = requested.or(session.lifecycle.kill.as_ref()) {
        change_line("accepted:", &ctx.local_time(&action.accepted));
        change_line("attempt:", attempt_summary(action));
        change_line(
            "observed:",
            &action.observed.as_ref().map_or_else(
                || "pending".to_owned(),
                |observation| ctx.observed(observation.kind.as_str(), &observation.at),
            ),
        );
    }
    change_line("result:", &outcome_summary(session));
}

/// The result of a start: the outcome and the fact that settled it.
fn start_result(ctx: &Context, session: &Session) -> String {
    match &session.lifecycle.start_settled {
        Some(settled) => format!(
            "{} (start settled {})",
            session.outcome.as_str(),
            ctx.local_time(settled)
        ),
        None => outcome_summary(session),
    }
}

/// The title a start prints: what committed, and whether the Session began.
pub fn start_title<'a>(session: &Session, started: &'a str, failed: &'a str) -> &'a str {
    if start_failure(session).is_some() {
        failed
    } else {
        started
    }
}

/// A Session that never began leaves the request unfulfilled, so the CLI reports a
/// rejection even though the records it names committed.
pub fn start_exit_code(session: &Session) -> ExitCode {
    if start_failure(session).is_some() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Why a start never began. `lifecycle.startFailed` is the record that it did not, and the
/// Provider's observation detail is the only account of why.
fn start_failure(session: &Session) -> Option<String> {
    session.lifecycle.start_failed.as_ref()?;
    Some(
        session
            .observation
            .as_ref()
            .and_then(|observation| observation.detail.clone())
            .unwrap_or_else(|| "the Provider reported no reason".to_owned()),
    )
}

/// The outcome, telling a Session that never began from other `unknown` ones.
pub fn outcome_label(session: &Session) -> String {
    if session.lifecycle.start_failed.is_some() {
        format!("{} (start failed)", session.outcome.as_str())
    } else {
        session.outcome.as_str().to_owned()
    }
}

pub fn outcome_summary(session: &Session) -> String {
    match session.exit_code {
        Some(exit_code) => format!("{} (exit_code {exit_code})", session.outcome.as_str()),
        None => outcome_label(session),
    }
}

pub fn observation_summary(ctx: &Context, session: &Session) -> String {
    session.observation.as_ref().map_or_else(
        || "unavailable".to_owned(),
        |observation| {
            let mut summary = ctx.observed(observation.kind.as_str(), &observation.at);
            if let Some(detail) = &observation.detail {
                summary.push_str(&format!(" · {detail}"));
            }
            summary
        },
    )
}

pub fn execution_summary(execution: &Execution) -> String {
    let what = if execution.execution_type == "agent-runtime" {
        execution
            .runtime
            .clone()
            .unwrap_or_else(|| execution.label.clone())
    } else {
        execution
            .command
            .as_ref()
            .map_or_else(|| execution.label.clone(), |command| command_line(command))
    };
    format!("{} · {what}", execution.execution_type)
}

fn requester_label(requester: &Requester, session_id: Option<&str>) -> String {
    let name = requester.name();
    session_id.map_or_else(|| name.to_owned(), |session| kinded(name, session))
}

/// The model name the Runtime Adapter reported, if it reported one.
pub fn model_name(model: &Value) -> Option<String> {
    let reported = model.get(1).unwrap_or(model);
    if let Some(name) = reported.as_str() {
        return Some(name.to_owned());
    }
    for key in ["id", "display_name"] {
        if let Some(name) = reported.get(key).and_then(Value::as_str) {
            return Some(name.to_owned());
        }
    }
    (!reported.is_null()).then(|| reported.to_string())
}

/// `workspace` adds the Workspace line, which `feld self get` shows because its caller
/// names no Workspace.
pub fn print_session_detail(
    ctx: &Context,
    session: &Session,
    workspace: bool,
    requests: &[(&str, String)],
) {
    println!("Session {}", session.id);
    detail_line(
        "actor:",
        &kinded(&session.actor, session.actor_kind.as_str()),
    );
    if workspace {
        detail_line("workspace:", &session.workspace);
    }
    detail_line("generation:", &session.generation.to_string());
    detail_line("start:", &ctx.local_time(&session.start));
    detail_line("end:", &ctx.local_time_or_dash(session.end.as_deref()));
    let mut start_kind = kinded(session.start_kind.as_str(), session.ownership.as_str());
    if let Some(origin) = &session.spawned_by {
        start_kind.push_str(&format!(
            " · spawned_by {}",
            requester_label(&origin.spawner, origin.spawner_session_id.as_deref())
        ));
    }
    if let Some(planned) = &session.planned {
        start_kind.push_str(&format!(" · planned {}", ctx.local_time(planned)));
    }
    detail_line("start_kind:", &start_kind);
    detail_line("end_kind:", session.end_kind.as_str());
    detail_line("outcome:", &outcome_summary(session));
    for (label, request) in requests {
        detail_line(label, request);
    }
    detail_line("execution:", &execution_summary(&session.execution));
    if let Some(model) = session.model.as_ref().and_then(model_name) {
        detail_line("model:", &model);
    } else if session.is_unlinked() {
        detail_line("model:", "- · unlinked");
    }
    detail_line("placement:", &placement_summary(ctx, session, true));
    detail_line(
        "start_settled:",
        &match start_failure(session) {
            Some(_) => "- · start failed".to_owned(),
            None => ctx.local_time_or_dash(session.lifecycle.start_settled.as_deref()),
        },
    );
    detail_line("observation:", &observation_summary(ctx, session));
    if let Some(command) = derive_attach_command(ctx, session) {
        detail_line("attach_command:", &command);
    }
    if session.is_unlinked() {
        detail_line("note:", UNLINKED_NOTE);
    }
}

/// What a person does about a Runtime that stays unlinked.
const UNLINKED_NOTE: &str = "a Runtime that stays unlinked is waiting for an answer in its pane, such as whether to trust the workdir; answer it there through attach_command";

/// The Placement as a person reads it: the part core owns, and the `summary` its Execution
/// Provider gives the rest.
fn placement_summary(ctx: &Context, session: &Session, with_workdir: bool) -> String {
    let placement = &session.placement;
    let mut summary = format!("{} · {}", placement.provider, placement.node);
    let own = ctx
        .plugins
        .provider(&placement.provider)
        .map(|provider| provider.summary(&placement.placement))
        .unwrap_or_default();
    if !own.is_empty() {
        summary.push_str(&format!(" · {own}"));
    }
    if with_workdir {
        summary.push_str(&format!(" · {}", placement.workdir));
    }
    summary
}

/// The command that puts a terminal on an `owned` Session, as its Provider derives it.
fn derive_attach_command(ctx: &Context, session: &Session) -> Option<String> {
    if session.end.is_some() || session.ownership != Ownership::Owned {
        return None;
    }
    ctx.plugins
        .provider(&session.placement.provider)?
        .attach_command(&session.placement.placement)
}

pub fn print_sub_agent_session_detail(ctx: &Context, session: &SubAgentSession) {
    println!("Sub Agent Session {}", session.id);
    detail_line("actor:", &session.actor);
    detail_line("parent:", &session.parent_session);
    detail_line("sub_agent:", &session.sub_agent);
    detail_line(
        "start:",
        &session
            .start
            .as_deref()
            .map_or_else(|| "unavailable".to_owned(), |start| ctx.local_time(start)),
    );
    detail_line("end:", &ctx.local_time_or_dash(session.end.as_deref()));
    detail_line("start_kind:", &session.start_kind);
    detail_line("outcome:", session.outcome.as_str());
    detail_line("execution:", &execution_summary(&session.execution));
    if let Some(model) = session.model.as_ref().and_then(model_name) {
        detail_line("model:", &model);
    }
    if session.console_state == ConsoleState::Lost {
        let detail = session.console_detail.as_deref().unwrap_or_default();
        detail_line("console:", &format!("lost · {detail}"));
    }
}
