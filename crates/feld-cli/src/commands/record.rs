//! `feld record`.

use std::process::ExitCode;

use feld_daemon::api::RecordList;
use reqwest::Method;

use super::FOLLOW_INTERVAL;
use crate::args::RecordListArgs;
use crate::client::Daemon;
use crate::format::record::{RECORD_HEADERS, record_description};
use crate::output::{print_json_line, print_table, table_row, widen_columns};
use crate::{CliResult, Context};

pub async fn list(ctx: &Context, args: RecordListArgs) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    // A Session ID names its Session in every Workspace.
    let mut query = Vec::new();
    if args.session.is_none() {
        query.push(("workspace", ctx.workspace.clone()));
    }
    for (key, value) in [
        ("actor", args.actor),
        ("session", args.session),
        ("kind", args.kind.map(|kind| kind.as_str().to_owned())),
        ("start", args.start),
        ("end", args.end),
    ] {
        if let Some(value) = value {
            query.push((key, value));
        }
    }
    let mut after = args.after.unwrap_or(0);
    // `--follow` prints the column heading once, before the first record, and keeps the
    // columns it has grown to so that later records line up under it.
    let mut widths: Vec<usize> = Vec::new();
    loop {
        let response: RecordList = daemon
            .send(
                daemon
                    .call(Method::GET, "/v1/records")
                    .query(&query)
                    .query(&[("after", after)]),
            )
            .await?;
        // `--follow --json` prints each record as a line of its own, and a person reads
        // only the polls that bring new records.
        if args.follow && ctx.json {
            for record in &response.records {
                print_json_line(record)?;
            }
        } else if !args.follow || !response.records.is_empty() {
            ctx.print(&response, |response| {
                let rows: Vec<Vec<String>> = response
                    .records
                    .iter()
                    .map(|record| {
                        vec![
                            record.sequence.to_string(),
                            ctx.local_time(&record.at),
                            record.actor.name.clone(),
                            record.kind.to_string(),
                            record_description(ctx, record),
                        ]
                    })
                    .collect();
                if !args.follow {
                    println!("Records ({})", rows.len());
                    print_table(&RECORD_HEADERS, &rows);
                } else {
                    let heading = widths.is_empty();
                    widen_columns(&mut widths, RECORD_HEADERS.into_iter());
                    for row in &rows {
                        widen_columns(&mut widths, row.iter().map(String::as_str));
                    }
                    if heading {
                        println!("{}", table_row(&widths, RECORD_HEADERS.into_iter()));
                    }
                    for row in &rows {
                        println!("{}", table_row(&widths, row.iter().map(String::as_str)));
                    }
                }
            })?;
        }
        if !args.follow {
            if !ctx.json {
                println!("next cursor: {}", response.next_after);
            }
            return Ok(ExitCode::SUCCESS);
        }
        after = response.next_after;
        tokio::time::sleep(FOLLOW_INTERVAL).await;
    }
}
