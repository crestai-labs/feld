//! The `claude-code` Runtime Adapter: the Plugin executable core runs for every operation,
//! and the four parts that run beside Claude Code.
//!
//! `feld-plugin-claude-code <operation>` reads the request from stdin and writes the
//! response to stdout, both as one JSON object, as `plugin.md` describes. Exit 0 means the
//! Plugin answered; any other exit means it could not, and stderr holds the reason.
//! `feld-plugin-claude-code helper <name>` is the other entry: Claude Code runs it through
//! the shims in the Claude Code plugin this Adapter writes, and it always exits 0 so that
//! nothing this Adapter does stops the Runtime.

mod adapter;
mod helper;
mod plugin_dir;
mod status;
mod transcript;

use std::process::ExitCode;

use serde_json::Value;

use feld_core::plugin::protocol::{AttachedResponse, SelfResponse};
use feld_core::plugin::sdk::io;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let operation = arguments.next().unwrap_or_default();
    if operation == "helper" {
        let name = arguments.next().unwrap_or_default();
        return helper::run(&name, &arguments.collect::<Vec<_>>());
    }
    let answer = match operation.as_str() {
        "--version" => io::version("claude-code"),
        "launch" => io::answer(adapter::launch),
        "match" => io::answer(adapter::match_process),
        "identity" => io::answer(adapter::identity),
        // The Monitor helper resolves the token by itself once the attach wrote it, so an
        // attach has nothing to start beside a Claude Code.
        "attached" => io::answer(|_: Value| Ok(AttachedResponse::default())),
        // A Claude Code runs its tools as its own children, so the pane it is in names it
        // and the Provider's self index finds its token.
        "self" => io::answer(|_: Value| Ok(SelfResponse::default())),
        "prepare" => io::answer(adapter::prepare),
        "restore" => io::answer(adapter::restore),
        "transcript" => io::answer(transcript::transcript),
        "" => Err(anyhow::anyhow!(
            "feld-plugin-claude-code takes an operation or `helper <name>`"
        )),
        other => Err(anyhow::anyhow!("'{other}' is no claude-code operation")),
    };
    io::exit(answer)
}
