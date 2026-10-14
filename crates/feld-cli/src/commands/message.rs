//! `feld message`.

use std::process::ExitCode;

use anyhow::Context as _;
use feld_daemon::api::{MessageResponse, MessageSummaryList, SendMessageRequest};

use crate::args::{MessageCommand, READ_STATE_NOTE};
use crate::client::{Daemon, actor_path, percent_encode};
use crate::format::message::{message_state, print_message, print_message_summary};
use crate::output::print_table;
use crate::{CliResult, Context};

pub async fn run(ctx: &Context, command: MessageCommand) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    match command {
        MessageCommand::Send { actor, label, body } => {
            let body = match body {
                Some(body) => body,
                None => {
                    let body = std::io::read_to_string(std::io::stdin())
                        .context("read the Message body from standard input")?;
                    if body.trim().is_empty() {
                        return Err(rejected!("Message body is empty"));
                    }
                    body
                }
            };
            let response: MessageResponse = daemon
                .post_json(
                    &format!("{}/messages", actor_path(&ctx.workspace, &actor)),
                    &SendMessageRequest { label, body },
                )
                .await?;
            ctx.print(&response, |response| {
                println!("Message accepted and committed.");
                print_message_summary(ctx, &response.message);
            })?;
        }
        MessageCommand::Delete(target) => {
            let response: MessageResponse = daemon
                .delete(&format!("/v1/messages/{}", percent_encode(&target.message)))
                .await?;
            ctx.print(&response, |response| {
                let message = &response.message;
                println!("Message deleted.");
                println!("  message:      {}", message.id);
                println!("  to:           {}", message.to.name);
                if let Some(deleted_at) = &message.deleted_at {
                    println!("  deleted_at:   {}", ctx.local_time(deleted_at));
                }
            })?;
        }
        MessageCommand::Get(target) => {
            let response: MessageResponse = daemon
                .get(&format!("/v1/messages/{}", percent_encode(&target.message)))
                .await?;
            ctx.print(&response, |response| print_message(ctx, &response.message))?;
        }
        MessageCommand::List(args) => {
            let mut query = vec![("workspace", ctx.workspace.clone())];
            for (key, value) in [
                ("from", args.from),
                ("to", args.to),
                ("label", args.label),
                ("start", args.start),
                ("end", args.end),
            ] {
                if let Some(value) = value {
                    query.push((key, value));
                }
            }
            if args.waiting {
                query.push(("waiting", "true".to_owned()));
            }
            let response: MessageSummaryList = daemon.get_with("/v1/messages", &query).await?;
            ctx.print(&response, |response| {
                let waiting = response
                    .messages
                    .iter()
                    .filter(|message| message.waiting)
                    .count();
                println!(
                    "Messages ({}, {} waiting)",
                    response.messages.len(),
                    waiting
                );
                println!("{READ_STATE_NOTE}");
                let rows: Vec<Vec<String>> = response
                    .messages
                    .iter()
                    .map(|message| {
                        vec![
                            message.id.clone(),
                            message.from.name().to_owned(),
                            message.to.name.clone(),
                            message.label.clone().unwrap_or_else(|| "-".to_owned()),
                            ctx.local_time(&message.accepted),
                            message_state(message.waiting, message.queued.as_ref()).to_owned(),
                            message.preview.replace('\n', " "),
                        ]
                    })
                    .collect();
                print_table(
                    &[
                        "MESSAGE", "FROM", "TO", "LABEL", "ACCEPTED", "STATE", "PREVIEW",
                    ],
                    &rows,
                );
            })?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
