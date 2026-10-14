//! `feld session`.

use std::process::ExitCode;

use feld_core::model::Execution;
use feld_daemon::api::{LifecycleResponse, SessionList, SessionResponse, SubAgentSessionResponse};
use feld_daemon::console::{OutputPage, TranscriptPage, split_sub_agent_session_id};
use feld_daemon::metrics::SessionMetricsResponse;
use serde_json::Value;

use super::{FOLLOW_INTERVAL, check_bucket, metrics_query};
use crate::args::{SessionCommand, SessionListArgs, SessionLogArgs, SessionMetricsArgs};
use crate::client::{Daemon, Target, session_path};
use crate::format::metrics::{metric_label, metric_value};
use crate::format::session::{
    execution_summary, lifecycle_action_summary, model_name, observation_summary, outcome_summary,
    print_lifecycle, print_session_detail, print_sub_agent_session_detail, print_transcript_page,
};
use crate::output::{
    change_line, kinded, print_json, print_json_line, print_table, print_table_or,
};
use crate::{CliResult, Context};

pub async fn run(ctx: &Context, command: SessionCommand) -> CliResult<ExitCode> {
    match command {
        SessionCommand::Get(target) => get(ctx, &target.target).await,
        SessionCommand::List(args) => list(ctx, args).await,
        SessionCommand::Stop(target) => lifecycle(ctx, &target.target, false).await,
        SessionCommand::Kill(target) => lifecycle(ctx, &target.target, true).await,
        SessionCommand::Observe(target) => observe(ctx, &target.target).await,
        SessionCommand::Log(args) => log(ctx, args).await,
        SessionCommand::Metrics(args) => metrics(ctx, args).await,
    }
}

async fn get(ctx: &Context, target: &str) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    let session_id = daemon
        .resolve_session(&ctx.workspace, target, Target::Latest)
        .await?;
    if split_sub_agent_session_id(&session_id).is_some() {
        let sub_agent_session = daemon.get_sub_agent_session(&session_id).await?;
        if ctx.json {
            print_json(&SubAgentSessionResponse { sub_agent_session })?;
        } else {
            print_sub_agent_session_detail(ctx, &sub_agent_session);
        }
        return Ok(ExitCode::SUCCESS);
    }
    let response = daemon.get_session(&session_id).await?;
    if ctx.json {
        print_json(&response)?;
        return Ok(ExitCode::SUCCESS);
    }
    let session = &response.session;
    let mut requests = Vec::new();
    for (label, action) in [
        ("stop:", session.lifecycle.stop.as_ref()),
        ("kill:", session.lifecycle.kill.as_ref()),
    ] {
        if let Some(action) = action {
            let requester = match &action.requester_session_id {
                Some(id) => {
                    let requester = daemon.get_session(id).await?;
                    kinded(&requester.session.actor, id)
                }
                None => "Admin".to_owned(),
            };
            requests.push((label, lifecycle_action_summary(ctx, action, &requester)));
        }
    }
    print_session_detail(ctx, session, false, &requests);
    if !response.sub_agents.is_empty() {
        println!("Sub Agent Sessions");
        for sub_agent in &response.sub_agents {
            println!(
                "  {}  {}  {}",
                sub_agent.id,
                sub_agent
                    .start
                    .as_deref()
                    .map_or_else(|| "unavailable".to_owned(), |start| ctx.local_time(start)),
                sub_agent.outcome.as_str()
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

async fn list(ctx: &Context, args: SessionListArgs) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    let mut query = vec![("workspace", ctx.workspace.clone())];
    if let Some(actor) = args.actor {
        query.push(("actor", actor));
    }
    if let Some(kind) = args.kind {
        query.push(("kind", kind.as_str().to_owned()));
    }
    if args.current {
        query.push(("current", "true".to_owned()));
    }
    if let Some(start) = args.start {
        query.push(("start", start));
    }
    if let Some(end) = args.end {
        query.push(("end", end));
    }
    let response: SessionList = daemon.get_with("/v1/sessions", &query).await?;
    ctx.print(&response, |response| {
        println!("Workspace: {}", ctx.workspace);
        let rows: Vec<Vec<String>> = response
            .sessions
            .iter()
            .map(|session| {
                vec![
                    session.id.clone(),
                    session.actor.clone(),
                    session.generation.to_string(),
                    ctx.local_time(&session.start),
                    session.start_kind.as_str().to_owned(),
                    session.end_kind.as_str().to_owned(),
                    outcome_summary(session),
                ]
            })
            .collect();
        print_table_or(
            "No Sessions.",
            &[
                "SESSION",
                "ACTOR",
                "GEN",
                "START",
                "START_KIND",
                "END_KIND",
                "OUTCOME",
            ],
            &rows,
        );
    })?;
    Ok(ExitCode::SUCCESS)
}

async fn observe(ctx: &Context, target: &str) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    let session_id = daemon
        .resolve_session(&ctx.workspace, target, Target::Current)
        .await?;
    let previous = daemon.get_session(&session_id).await?;
    let response: SessionResponse = daemon
        .post(&format!("{}/observe", session_path(&session_id)))
        .await?;
    ctx.print(&response, |response| {
        println!("Observation committed.");
        change_line("session:", &response.session.id);
        change_line("observation:", &observation_summary(ctx, &response.session));
        change_line("previous:", &observation_summary(ctx, &previous.session));
    })?;
    Ok(ExitCode::SUCCESS)
}

async fn log(ctx: &Context, args: SessionLogArgs) -> CliResult<ExitCode> {
    let json = ctx.json;
    let daemon = Daemon::connect(ctx).await?;
    let session_id = daemon
        .resolve_session(&ctx.workspace, &args.target, Target::Latest)
        .await?;
    let session = subject(&daemon, &session_id).await?;
    let transcript = session.execution.execution_type == "agent-runtime";
    // A program Session's cursor is a line's sequence number.
    if !transcript
        && let Some(after) = &args.after
        && after.parse::<u64>().is_err()
    {
        return Err(operational!(
            "--after for program output takes a sequence number, got '{after}'"
        ));
    }
    let heading = format!(
        "{} {} ({})",
        if transcript { "Transcript" } else { "Output" },
        session.id,
        execution_summary(&session.execution)
    );
    let path = format!(
        "{}/{}",
        session_path(&session_id),
        if transcript { "transcript" } else { "output" }
    );
    // `--follow --json` prints each entry or line as a line of its own.
    let lines = args.follow && json;
    let mut after = args.after;
    let mut tail = args.tail;
    let mut first = true;
    // A read that follows the end being seen holds everything the Session put out.
    let mut ended = session.end.is_some();
    loop {
        let mut query = Vec::new();
        if let Some(after) = &after {
            query.push(("after", after.clone()));
        }
        if let Some(tail) = tail.take() {
            query.push(("tail", "true".to_owned()));
            query.push(("limit", tail.to_string()));
        }
        let (next, empty) = if transcript {
            query.push(("view", args.view.clone()));
            let page: TranscriptPage = daemon.get_with(&path, &query).await?;
            if first && !json {
                println!("{heading}");
            }
            if lines {
                for entry in &page.records {
                    print_json_line(entry)?;
                }
            } else {
                ctx.print(&page, |page| print_transcript_page(ctx, page, first))?;
            }
            (
                page.next_after.clone().or(after.clone()),
                page.records.is_empty(),
            )
        } else {
            let page: OutputPage = daemon.get_with(&path, &query).await?;
            if first && !json {
                println!("{heading}");
            }
            if lines {
                for line in &page.lines {
                    print_json_line(line)?;
                }
            } else {
                ctx.print(&page, |page| {
                    for line in &page.lines {
                        println!(
                            "{}  {}  {}",
                            ctx.local_time(&line.timestamp),
                            line.stream,
                            line.text
                        );
                    }
                })?;
            }
            (Some(page.next_after.to_string()), page.lines.is_empty())
        };
        first = false;
        if !args.follow || (empty && ended) {
            if !json && let Some(next) = &next {
                println!("next cursor: {next}");
            }
            return Ok(ExitCode::SUCCESS);
        }
        after = next;
        if empty {
            tokio::time::sleep(FOLLOW_INTERVAL).await;
            ended = subject(&daemon, &session_id).await?.end.is_some();
        }
    }
}

async fn metrics(ctx: &Context, args: SessionMetricsArgs) -> CliResult<ExitCode> {
    check_bucket(ctx, args.bucket)?;
    let daemon = Daemon::connect(ctx).await?;
    let session_id = daemon
        .resolve_session(&ctx.workspace, &args.target, Target::Latest)
        .await?;
    let response: SessionMetricsResponse = daemon
        .get_with(
            &format!("{}/metrics", session_path(&session_id)),
            &metrics_query(args.start.as_ref(), args.end.as_ref(), args.bucket),
        )
        .await?;
    if ctx.json {
        print_json(&response)?;
        return Ok(ExitCode::SUCCESS);
    }
    let session = subject(&daemon, &session_id).await?;
    let mut about = session.execution.label.clone();
    if let Some(model) = session.model.as_ref().and_then(model_name) {
        about.push_str(&format!(" · {model}"));
    }
    println!("Metrics {} ({about})", session.id);
    let rows: Vec<Vec<String>> = response
        .metrics
        .iter()
        .map(|metric| {
            vec![
                metric_label(&metric.metric),
                metric.status.as_str().to_owned(),
                metric.source.clone(),
                metric.series.first().map_or_else(
                    || "-".to_owned(),
                    |bin| metric_value(&metric.metric, &bin.value),
                ),
            ]
        })
        .collect();
    print_table(&["METRIC", "STATUS", "SOURCE", "VALUE"], &rows);
    Ok(ExitCode::SUCCESS)
}

/// What `log` and `metrics` show of their target: a Session or a Sub Agent Session.
struct Subject {
    id: String,
    execution: Execution,
    model: Option<Value>,
    end: Option<String>,
}

async fn subject(daemon: &Daemon, session_id: &str) -> CliResult<Subject> {
    if split_sub_agent_session_id(session_id).is_none() {
        let session = daemon.get_session(session_id).await?.session;
        return Ok(Subject {
            id: session.id,
            execution: session.execution,
            model: session.model,
            end: session.end,
        });
    }
    let sub_agent = daemon.get_sub_agent_session(session_id).await?;
    Ok(Subject {
        id: sub_agent.id,
        execution: sub_agent.execution,
        model: sub_agent.model,
        end: sub_agent.end,
    })
}

async fn lifecycle(ctx: &Context, target: &str, kill: bool) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    let session_id = daemon
        .resolve_session(&ctx.workspace, target, Target::Current)
        .await?;
    let action = if kill { "kill" } else { "stop" };
    let response: LifecycleResponse = daemon
        .post(&format!("{}/{action}", session_path(&session_id)))
        .await?;
    ctx.print(&response, |response| {
        let title = if response.already_requested {
            "Lifecycle action was already requested."
        } else if kill {
            "Kill accepted and committed."
        } else {
            "Stop accepted and committed."
        };
        print_lifecycle(ctx, title, &response.session, kill);
    })?;
    Ok(ExitCode::SUCCESS)
}
