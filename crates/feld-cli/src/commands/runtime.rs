//! `feld runtime`.

use std::process::ExitCode;

use feld_core::time;
use feld_daemon::api::RuntimeList;
use feld_daemon::metrics::RuntimeMetricsResponse;

use super::{check_bucket, metrics_query};
use crate::args::{RuntimeCommand, RuntimeMetricsArgs, Since};
use crate::client::{Daemon, percent_encode};
use crate::format::metrics::print_runtime_metrics;
use crate::output::{print_json, print_table_or};
use crate::{CliResult, Context};

pub async fn run(ctx: &Context, command: RuntimeCommand) -> CliResult<ExitCode> {
    if let RuntimeCommand::Metrics(args) = &command {
        check_bucket(ctx, args.bucket)?;
    }
    let daemon = Daemon::connect(ctx).await?;
    let listing: RuntimeList = daemon.get("/v1/runtimes").await?;
    let args = match command {
        RuntimeCommand::List => {
            ctx.print(&listing, |listing| {
                let rows: Vec<Vec<String>> = listing
                    .runtimes
                    .iter()
                    .map(|runtime| {
                        vec![
                            runtime.plugin.name.clone(),
                            runtime.plugin.label.clone(),
                            runtime.running_sessions.to_string(),
                        ]
                    })
                    .collect();
                print_table_or(
                    "No Runtimes are installed.",
                    &["NAME", "LABEL", "RUNNING_SESSIONS"],
                    &rows,
                );
            })?;
            return Ok(ExitCode::SUCCESS);
        }
        RuntimeCommand::Metrics(args) => args,
    };
    let names = match &args.runtime {
        Some(runtime) => vec![runtime.clone()],
        None => listing
            .runtimes
            .iter()
            .map(|runtime| runtime.plugin.name.clone())
            .collect(),
    };
    let (start, end) = metrics_range(&args)?;
    let query = metrics_query(Some(&start), Some(&end), args.bucket);
    let mut responses = Vec::with_capacity(names.len());
    for name in &names {
        let response: RuntimeMetricsResponse = daemon
            .get_with(
                &format!("/v1/runtimes/{}/metrics", percent_encode(name)),
                &query,
            )
            .await?;
        responses.push(response);
    }
    if ctx.json {
        match (&args.runtime, responses.as_slice()) {
            (Some(_), [response]) => print_json(response)?,
            _ => print_json(&serde_json::json!({ "runtimes": responses }))?,
        }
        return Ok(ExitCode::SUCCESS);
    }
    let range = format!("{} – {}", ctx.local_time(&start), ctx.local_time(&end));
    match args.since {
        Some(since) => println!("Range: last {since} ({range})"),
        None if args.start.is_none() && args.end.is_none() => {
            println!("Range: last {} ({range})", Since::FIVE_HOURS);
        }
        None => println!("Range: {range}"),
    }
    for response in &responses {
        println!();
        print_runtime_metrics(ctx, response);
    }
    Ok(ExitCode::SUCCESS)
}

/// The range to read: `--start` and `--end` as given, else the span of `--since`, 5 hours by
/// default, up to now.
fn metrics_range(args: &RuntimeMetricsArgs) -> CliResult<(String, String)> {
    let end = args.end.clone().unwrap_or_else(time::now);
    if let Some(start) = &args.start {
        return Ok((start.clone(), end));
    }
    let instant = time::parse(&end)
        .map_err(|_| operational!("--end must be an RFC 3339 time, got '{end}'"))?;
    let since = args.since.unwrap_or(Since::FIVE_HOURS);
    Ok((time::format(instant - since.duration()), end))
}
