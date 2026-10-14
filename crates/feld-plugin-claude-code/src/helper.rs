//! The four parts that run beside Claude Code.
//!
//! Claude Code starts them from the plugin and settings this Adapter writes, and each one
//! addresses its own Session with the Session token of `auth.md`. None of them ever
//! fails the Runtime: they exit 0 whatever the daemon answers, and they wait rather than
//! call a Session API while no token is there (before an attach).

use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use feld_core::home::FeldHome;
use feld_core::self_api::{
    Delivery, MESSAGE_SENDER_CONTEXT, deliver_identity, hook_identity, message_line, poll_mailbox,
};
use feld_core::session_token::session_token;
use feld_core::telemetry::telemetry_token;

use feld_core::plugin::sdk::env::feld_home;

use crate::{status, transcript};

/// How long the Stop hook waits for the turn's response to reach the transcript file.
const STOP_RESPONSE_WAIT: Duration = Duration::from_secs(2);

/// Claude Code cuts each Monitor stdout line after this many UTF-16 code units.
const MONITOR_LINE_LIMIT: usize = 500;

/// What the model reads at SessionStart about the lines the monitor delivers, which
/// Claude Code otherwise hands over as background notifications.
fn message_context() -> String {
    format!(
        "This session runs under Feld. A `feld_message` event from the Feld mailbox monitor is \
a Message addressed to you. {MESSAGE_SENDER_CONTEXT} When the Message carries an `excerpt` \
instead of a `body`, run its `show` command to read the whole Message first."
    )
}

pub fn run(name: &str, arguments: &[String]) -> ExitCode {
    let home = match feld_home() {
        Ok(home) => home,
        Err(error) => {
            eprintln!("{error:#}");
            return ExitCode::FAILURE;
        }
    };
    match name {
        "hook" => {
            // A hook that cannot reach the daemon leaves the identity undelivered; the
            // next hook that fires carries the same one.
            if let Err(error) = hook(&home) {
                eprintln!("{error:#}");
            }
            ExitCode::SUCCESS
        }
        "monitor" => monitor(&home),
        "otel-headers" => {
            otel_headers(&home);
            ExitCode::SUCCESS
        }
        "status-line" => {
            status_line(&home, chained(arguments));
            ExitCode::SUCCESS
        }
        "" => {
            eprintln!("`helper` takes one of hook, monitor, otel-headers, status-line");
            ExitCode::FAILURE
        }
        other => {
            eprintln!("'{other}' is no claude-code helper");
            ExitCode::FAILURE
        }
    }
}

/// Delivers the Runtime identity Claude Code passes the hook on stdin, and at SessionStart
/// gives the model the meaning of a Feld Message. A hook input without `model` (every
/// event but SessionStart) takes it from the transcript, which Stop waits for briefly.
fn hook(home: &FeldHome) -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let value: Value = serde_json::from_str(&input)?;
    if let Some(output) = hook_output(&value) {
        println!("{output}");
    }
    let mut identity = hook_identity(&value)?;
    if identity.model.is_null() {
        let path = Path::new(&identity.transcript_path);
        let model = if value.get("hook_event_name").and_then(Value::as_str) == Some("Stop") {
            transcript::awaited_model(path, STOP_RESPONSE_WAIT)
        } else {
            transcript::latest_model(path)
        };
        identity.model = model.map_or(Value::Null, Value::from);
    }
    let token = session_token(home)?
        .context("no attached Session token is available")?
        .token;
    deliver_identity(home, &token, &identity)
}

/// The hook's stdout for the event in `input`: SessionStart adds [`message_context`] to the
/// model's context, whether or not a Session token is there yet.
fn hook_output(input: &Value) -> Option<Value> {
    (input.get("hook_event_name").and_then(Value::as_str) == Some("SessionStart")).then(|| {
        json!({
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": message_context(),
            }
        })
    })
}

/// Long-polls the mailbox and writes each Message as one line Claude Code turns into a
/// notification, then records the read.
fn monitor(home: &FeldHome) -> ExitCode {
    poll_mailbox(
        home,
        |_| {},
        |message| {
            println!("{}", message_line(message, Some(MONITOR_LINE_LIMIT)));
            match std::io::stdout().flush() {
                Ok(()) => Delivery::Delivered,
                Err(_) => Delivery::End,
            }
        },
    );
    ExitCode::SUCCESS
}

/// Prints the OTLP headers Claude Code signs its telemetry with, which carry the Session's
/// telemetry token, and an empty object while the Session has no token.
fn otel_headers(home: &FeldHome) {
    let header = session_token(home)
        .ok()
        .flatten()
        .map(|resolved| json!({"Authorization": format!("Bearer {}", telemetry_token(&resolved.token))}))
        .unwrap_or_else(|| json!({}));
    println!("{header}");
}

/// The person's status line command `--chain` names, which the status line runs first.
fn chained(arguments: &[String]) -> Option<&str> {
    match arguments {
        [flag, command, ..] if flag == "--chain" => Some(command),
        _ => None,
    }
}

/// Reports the status line's gauges to the Collector and prints the one line Claude Code
/// shows, whether or not the report reached it. A chained command's first line comes
/// before Feld's part of it.
fn status_line(home: &FeldHome, chain: Option<&str>) {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let status = serde_json::from_str::<Value>(&input).unwrap_or(Value::Null);
    let resolved = session_token(home).ok().flatten();
    if let Some(resolved) = &resolved {
        let _ = feld_core::plugin::sdk::gauges::send_gauges(
            home,
            &resolved.token,
            status::SCOPE,
            &status::gauges(&status, status::account().as_ref()),
        );
    }
    let actor = resolved
        .and_then(|resolved| resolved.actor)
        .unwrap_or_else(|| "Feld".to_owned());
    let feld = status::line(&actor, &status);
    match chain.and_then(|command| chained_line(command, &input)) {
        Some(theirs) => println!("{theirs} · {feld}"),
        None => println!("{feld}"),
    }
}

/// The first line a chained status line command prints for `input`, or None when it fails
/// or prints nothing.
fn chained_line(command: &str, input: &str) -> Option<String> {
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // A command that never reads its input still has its line taken.
    let _ = child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(input.as_bytes());
    let output = child.wait_with_output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().next().unwrap_or_default().trim_end();
    (output.status.success() && !first.is_empty()).then(|| first.to_owned())
}
