//! The `codex` Runtime Adapter: the Plugin executable core runs for every operation, and
//! the two parts that run beside Codex.
//!
//! `feld-plugin-codex <operation>` reads the request from stdin and writes the response
//! to stdout, both as one JSON object, as `plugin.md` describes. Exit 0 means the Plugin
//! answered; any other exit means it could not, and stderr holds the reason.
//! `feld-plugin-codex helper <name>` is the other entry: Codex runs it through the shims
//! this Adapter declares as hooks, and it always exits 0 so that nothing this Adapter does
//! stops the Runtime.

mod adapter;
mod helper;
mod rollout;
mod skill_dir;
mod toml_value;
mod transcript;
mod writable_roots;

use std::process::ExitCode;

use serde_json::Value;

use feld_core::plugin::sdk::io;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let operation = arguments.next().unwrap_or_default();
    if operation == "helper" {
        return helper::run(&arguments.next().unwrap_or_default());
    }
    let answer = match operation.as_str() {
        "--version" => io::version("codex"),
        "launch" => io::answer(adapter::launch),
        "match" => io::answer(adapter::match_process),
        "identity" => io::answer(adapter::identity),
        "attached" => io::answer(helper::attached),
        "self" => io::answer(|_: Value| adapter::current()),
        "prepare" => io::answer(adapter::prepare),
        "restore" => io::answer(adapter::restore),
        "transcript" => io::answer(transcript::transcript),
        "" => Err(anyhow::anyhow!(
            "feld-plugin-codex takes an operation or `helper <name>`"
        )),
        other => Err(anyhow::anyhow!("'{other}' is no codex operation")),
    };
    io::exit(answer)
}
