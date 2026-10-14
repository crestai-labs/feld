//! `feld self`, the commands a Session runs about itself with its own token.

use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::Context as _;
use feld_daemon::api::{MessageList, MessageReadList, SessionResponse};

use crate::args::{SelfCommand, SelfMessageCommand};
use crate::client::{Daemon, self_token};
use crate::format::session::print_session_detail;
use crate::{CliResult, Context};

pub async fn run(ctx: &Context, command: SelfCommand) -> CliResult<ExitCode> {
    match command {
        SelfCommand::Token => println!("{}", self_token(ctx)?),
        SelfCommand::Message(args) => match args.command {
            SelfMessageCommand::Recv { wait } => message_recv(ctx, wait).await?,
        },
        SelfCommand::Get => {
            let daemon = Daemon::connect_self(ctx).await?;
            let response: SessionResponse = daemon.get("/v1/self").await?;
            ctx.print(&response, |response| {
                print_session_detail(ctx, &response.session, true, &[]);
            })?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

async fn message_recv(ctx: &Context, wait: bool) -> CliResult<()> {
    let json = ctx.json;
    let daemon = Daemon::connect_self(ctx).await?;
    let response: MessageList = daemon
        .get_with("/v1/self/messages", &[("wait", wait)])
        .await?;
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    if json {
        let messages = serde_json::to_vec(&response.messages)
            .context("encode mailbox messages for JSON output")?;
        stdout
            .write_all(b"{\"messages\":")
            .and_then(|()| stdout.write_all(&messages))
            .and_then(|()| stdout.write_all(b",\"reads\":"))
            .context("write mailbox messages")?;
        stdout.flush().context("flush mailbox messages")?;
    } else if response.messages.is_empty() {
        stdout
            .write_all(b"No pending messages.\n")
            .context("write empty mailbox result")?;
        stdout.flush().context("flush mailbox output")?;
    } else {
        let mut output = Vec::new();
        writeln!(&mut output, "Messages ({})", response.messages.len())
            .context("format mailbox output")?;
        for message in &response.messages {
            writeln!(
                &mut output,
                "--- {} from {} at {}",
                message.id,
                message.from.name(),
                ctx.local_time(&message.accepted)
            )
            .context("format mailbox Message header")?;
            writeln!(&mut output, "{}", message.body).context("format mailbox Message body")?;
        }
        stdout
            .write_all(&output)
            .context("write mailbox messages")?;
        stdout.flush().context("flush mailbox messages")?;
    }
    let committed = if response.messages.is_empty() {
        MessageReadList { reads: Vec::new() }
    } else {
        let ids = response
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>();
        daemon.post_json("/v1/self/messages/read", &ids).await?
    };
    let mut output = Vec::new();
    if json {
        output = serde_json::to_vec(&committed.reads)
            .context("encode committed mailbox reads for JSON output")?;
        output.extend_from_slice(b"}\n");
    } else if !committed.reads.is_empty() {
        output.extend_from_slice(b"Mailbox read committed.\n");
        for read in &committed.reads {
            let message = response
                .messages
                .iter()
                .find(|message| message.fifo == read.fifo)
                .expect("read belongs to fetched Message");
            writeln!(
                &mut output,
                "  read:         {} ({}, fifo {})",
                read.read_id, message.id, read.fifo
            )
            .context("format mailbox read")?;
            writeln!(&mut output, "  session:      {}", read.session_id)
                .context("format mailbox read Session")?;
            writeln!(&mut output, "  at:           {}", ctx.local_time(&read.at))
                .context("format mailbox read time")?;
        }
    }
    match stdout.write_all(&output).and_then(|()| stdout.flush()) {
        Ok(()) => Ok(()),
        // The reads stay committed, so the next `recv` hands these Messages out no more.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe && !committed.reads.is_empty() => {
            let ids = response
                .messages
                .iter()
                .map(|message| message.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Err(anyhow::anyhow!(
                "mailbox read committed for {ids}, but the reader of stdout closed before the \
                 result was written; read the Message again with `feld message get <message-id>`"
            )
            .into())
        }
        Err(error) => Err(anyhow::Error::new(error)
            .context("write committed mailbox reads")
            .into()),
    }
}
