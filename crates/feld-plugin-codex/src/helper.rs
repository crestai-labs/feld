//! The two parts that run beside Codex.
//!
//! Codex starts the hook from the hook declarations this Adapter writes, and the hook starts the
//! queue helper. Each one addresses its own Session with the Session token of `auth.md`. Neither
//! ever fails the Runtime: they exit 0 whatever the daemon answers, and they wait rather than call
//! a Session API while no token is there (before an attach).

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use feld_core::home::{FELD_SESSION_ID_ENV, FELD_SESSION_TOKEN_ENV, FeldHome, recorded_api};
use feld_core::model::Message;
use feld_core::plugin::protocol::{AttachedRequest, AttachedResponse};
use feld_core::self_api::{
    Delivery, MESSAGE_SENDER_CONTEXT, addressed_session, client, deliver_identity, hook_identity,
    is_rejected, mailbox_lock_is_held, message_line, poll_mailbox, stop_marker_path,
};

use crate::adapter::{CONFIG_ROOT, SESSION_ENV, is_thread_id, locate_rollout, rollout_config_root};
use feld_core::plugin::sdk::env::feld_home;

use crate::rollout;

/// Where Codex names the thread of the processes it starts, which is the thread the queue
/// helper puts this Session's Messages into.
const THREAD_ENV: &str = "CODEX_THREAD_ID";

/// Where the hook hands the queue helper the rollout file of that thread, in which the
/// helper sees Codex take a queued Message into a turn.
const ROLLOUT_ENV: &str = "FELD_CODEX_ROLLOUT";

/// Where `attached` asks the queue helper to wait for the helper an attach again replaced,
/// rather than leave the mailbox to it.
const TAKE_OVER_ENV: &str = "FELD_CODEX_TAKE_OVER";

/// The executable the queue helper reaches Codex through. It comes from `PATH`, which the
/// queue helper inherits from the Codex it belongs to, together with the configuration
/// root that decides which Codex the thread is in.
const CODEX: &str = "codex";

/// The event whose hook adds [`message_context`] to the model's context.
const SESSION_START: &str = "SessionStart";

/// The event that asks the queue helper to stop, because Codex is ending.
const SESSION_END: &str = "SessionEnd";

/// The event whose hook reports the gauges alone: the Session goes on, and the identity it
/// would carry is the one the turn before it delivered.
const POST_COMPACT: &str = "PostCompact";

/// The events after which the rollout file holds a `token_count` the Collector has not
/// been told: the end of a turn, and the compaction that rewrites the context window.
const GAUGE_EVENTS: [&str; 2] = ["Stop", POST_COMPACT];

/// The events after which the queue helper is started when none is reading this Session's
/// mailbox. The first starts it, and the two that bound a turn bring it back.
const QUEUE_HELPER_EVENTS: [&str; 3] = [SESSION_START, "UserPromptSubmit", "Stop"];

/// How often the queue helper looks for the stop it was asked for. It waits in a long poll
/// the rest of the time, so the asking is seen by a watch of its own.
const STOP_POLL: Duration = Duration::from_millis(250);

/// How often the queue helper reads what Codex added to the rollout file while a queued
/// Message waits for a turn to take it.
const TAKE_POLL: Duration = Duration::from_millis(250);

/// How often the queue helper asks the daemon whether its Session is still open while a
/// queued Message waits, so that a token an attach replaced ends it rather than leaving it
/// to wait for a thread nobody addresses.
const SESSION_CHECK: Duration = Duration::from_secs(10);

pub fn run(name: &str) -> ExitCode {
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
        "queue-helper" => queue_helper(&home),
        "" => {
            eprintln!("`helper` takes one of hook, queue-helper");
            ExitCode::FAILURE
        }
        other => {
            eprintln!("'{other}' is no codex helper");
            ExitCode::FAILURE
        }
    }
}

/// What the model reads at SessionStart about the lines the queue helper puts into the
/// thread, which Codex hands over as user input.
fn message_context() -> String {
    format!(
        "This session runs under Feld. A user input that is one JSON line whose `type` is \
`feld_message` is a Message addressed to you, which Feld queued into this thread. \
{MESSAGE_SENDER_CONTEXT}"
    )
}

/// The hook's stdout for the event named `name`: SessionStart adds [`message_context`] to
/// the model's context, whether or not a Session token is there yet.
fn hook_output(name: &str) -> Option<Value> {
    (name == SESSION_START).then(|| {
        json!({
            "hookSpecificOutput": {
                "hookEventName": SESSION_START,
                "additionalContext": message_context(),
            }
        })
    })
}

/// Gives the model the meaning of a Feld Message at SessionStart, and carries what Codex
/// writes on the hook's stdin to the daemon: the Runtime identity of every event that says a
/// record was added, and the end of the Session to the part that reads its mailbox.
fn hook(home: &FeldHome) -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let event: Value = serde_json::from_str(&input).context("decode the hook event")?;
    let name = event
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if let Some(output) = hook_output(&name) {
        println!("{output}");
    }
    // Codex starts a hook with no variable that names the thread, and the event names it.
    // Set where a tool of the thread reads it, the hook finds the token of a thread
    // attached by its key the way that tool does.
    if std::env::var_os(SESSION_ENV).is_none()
        && let Some(thread) = event.get("session_id").and_then(Value::as_str)
    {
        // SAFETY: the hook runs on this one thread, and nothing has read the environment
        // or started another thread yet.
        unsafe { std::env::set_var(SESSION_ENV, thread) };
    }
    let (token, session) =
        addressed_session(home)?.context("no attached Session token is available")?;
    if name == SESSION_END {
        return request_stop(home, &session);
    }
    // Each duty of a hook stands on its own, so an identity the daemon did not take or a
    // report the Collector did not take does not keep the queue helper from starting. The
    // next event delivers and reports the state as it is then.
    // PostCompact adds no record of its own; it reports the gauges of the rollout it
    // compacted.
    // A Sub Agent's event names the thread and the rollout file of the Session the Sub
    // Agent runs in, which is the parent's, so every event answers the same identity.
    if name != POST_COMPACT
        && let Err(error) = deliver_identity(home, &token, &hook_identity(&event)?)
    {
        eprintln!("{error:#}");
    }
    if GAUGE_EVENTS.contains(&name.as_str())
        && let Err(error) = rollout::report(home, &token, &event)
    {
        eprintln!("{error:#}");
    }
    if QUEUE_HELPER_EVENTS.contains(&name.as_str()) && !mailbox_lock_is_held(home, &session) {
        let thread = event
            .get("session_id")
            .and_then(Value::as_str)
            .context("hook input has no session_id")?;
        let rollout = event.get("transcript_path").and_then(Value::as_str);
        start_queue_helper(
            home,
            &session,
            &token,
            thread,
            rollout.map(Path::new),
            false,
        )?;
    }
    Ok(())
}

/// `attached`: starts the queue helper of a Session an attach just took, with the token of
/// that attach. A Codex waiting for its next input fires no hook, and a hook that fired
/// before the attach had no token to start one with.
pub fn attached(request: AttachedRequest) -> Result<AttachedResponse> {
    let Some(identity) = request.identity else {
        return Ok(AttachedResponse {
            notes: vec![QUEUE_HELPER_LATER.to_owned()],
        });
    };
    // An attach again to the same Codex replaced the token of the helper that runs, which
    // ends once the daemon refuses it. The one started here takes over from it.
    start_queue_helper(
        &feld_home()?,
        &request.session,
        &request.token,
        &identity.session_id,
        Some(Path::new(&identity.transcript_path)),
        true,
    )?;
    Ok(AttachedResponse::default())
}

/// What an attach that delivered no identity leaves to the hooks.
const QUEUE_HELPER_LATER: &str =
    "Messages reach this Codex once a hook fires on its next input, which starts the queue helper.";

/// Asks the queue helper of this Session to stop, by the marker it watches for. Codex ends
/// the helper it started itself, and the marker reaches one that outlives it.
fn request_stop(home: &FeldHome, session: &str) -> Result<()> {
    let path = stop_marker_path(home, session);
    home.create_owner_dir(path.parent().expect("the marker sits in a directory"))?;
    home.write_owner_file(&path, b"")
}

/// Starts the queue helper of this Session, as the Session's own long-lived process: it outlives
/// the hook that started it, and Codex, which ends a hook and its process group at the hook's
/// timeout, does not end it. What it needs to address the Session is handed to it, because a
/// process of a process group of its own is no longer in the Runtime's line for the self index to
/// be read through. The helper of an attached Session ends when that token is refused, and the next
/// hook starts one with the token of the attach that replaced it.
fn start_queue_helper(
    home: &FeldHome,
    session: &str,
    token: &str,
    thread: &str,
    rollout: Option<&Path>,
    take_over: bool,
) -> Result<()> {
    // The Session runs on, so an end an earlier Codex asked for is not this helper's.
    let _ = fs::remove_file(stop_marker_path(home, session));
    let mut command = Command::new(std::env::current_exe().context("find this executable")?);
    if let Some(rollout) = rollout {
        command.env(ROLLOUT_ENV, rollout);
        // `codex queue` reaches the thread through the configuration root that Codex reads,
        // which an attach from outside may not have in its environment.
        if let Some(root) = rollout_config_root(rollout) {
            command.env(CONFIG_ROOT.variable, root);
        }
    }
    if take_over {
        command.env(TAKE_OVER_ENV, "1");
    }
    command
        .arg("helper")
        .arg("queue-helper")
        .env(THREAD_ENV, thread)
        .env(FELD_SESSION_TOKEN_ENV, token)
        .env(FELD_SESSION_ID_ENV, session)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: the closure runs in the child between fork and exec, and calls only setsid,
    // which is async-signal-safe.
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.spawn().context("start the queue helper")?;
    Ok(())
}

/// Long-polls the mailbox and puts each Message into this Codex thread's queue with
/// `codex queue`, then records the read once Codex took it into a turn.
fn queue_helper(home: &FeldHome) -> ExitCode {
    let Some(thread) = thread_id() else {
        eprintln!("no Codex thread to queue Messages into");
        return ExitCode::SUCCESS;
    };
    if std::env::var_os(TAKE_OVER_ENV).is_some() && !wait_for_the_mailbox(home) {
        return ExitCode::SUCCESS;
    }
    poll_mailbox(
        home,
        |session| watch_for_stop(home, session),
        |message| deliver(home, &thread, message),
    );
    ExitCode::SUCCESS
}

/// Waits while another helper holds this Session's mailbox, which a helper whose token an
/// attach replaced does until the daemon refuses that token. Answers false when this helper's
/// own token is refused or the Feld home is gone, and nothing is left to take over.
fn wait_for_the_mailbox(home: &FeldHome) -> bool {
    let Ok(session) = std::env::var(FELD_SESSION_ID_ENV) else {
        return true;
    };
    let mut checked = Instant::now();
    while mailbox_lock_is_held(home, &session) {
        if !home.root().exists() {
            return false;
        }
        if checked.elapsed() >= SESSION_CHECK {
            checked = Instant::now();
            if session_is_closed(home) {
                return false;
            }
        }
        sleep(TAKE_POLL);
    }
    true
}

/// Puts one Message into the thread's queue and waits until Codex takes it into a turn,
/// which is when it appears in the rollout file as a `user` message. Until then the
/// Message has no read, so one that a busy turn has not taken stays `waiting`.
fn deliver(home: &FeldHome, thread: &str, message: &Message) -> Delivery {
    // A thread without a rollout file takes nothing yet: the Message keeps its place in
    // the mailbox and the next poll offers it again.
    let Some(rollout) = rollout_path(thread) else {
        return Delivery::Retry;
    };
    // Only what Codex writes after the queueing can be the turn that took it.
    let start = fs::metadata(&rollout).map_or(0, |metadata| metadata.len());
    // A queued Message cannot be taken back, so Feld learns of the queueing first and
    // refuses the delete of the Message from then on. One deleted in the meantime is
    // gone from the mailbox, and the next poll offers the one after it.
    match record_queued(home, &message.id) {
        Recorded::Yes => {}
        Recorded::No => return Delivery::Retry,
        Recorded::Refused => return Delivery::End,
    }
    // Codex takes a queued Message whole, so the line carries the body itself. A Message
    // Codex refused is in no queue, and is Feld's to delete again.
    if !queue_message(thread, &message_line(message, None)) {
        unrecord_queued(home, &message.id);
        return Delivery::Retry;
    }
    let mut reader = RolloutReader::new(rollout, start);
    let mut checked = Instant::now();
    loop {
        if reader.took(&message.id) {
            return Delivery::Delivered;
        }
        if !home.root().exists() {
            return Delivery::End;
        }
        if checked.elapsed() >= SESSION_CHECK {
            checked = Instant::now();
            if session_is_closed(home) {
                return Delivery::End;
            }
        }
        sleep(TAKE_POLL);
    }
}

/// What the daemon answered to the queueing of a Message.
enum Recorded {
    Yes,
    /// Not recorded: the Message is gone, or the daemon did not answer.
    No,
    /// The daemon refused this helper's token.
    Refused,
}

/// Records that the thread's queue holds the Message, with `POST /v1/self/messages/queued`.
fn record_queued(home: &FeldHome, message: &str) -> Recorded {
    let Some((token, api, client)) = session_api(home) else {
        return Recorded::No;
    };
    match client
        .post(format!("{api}/v1/self/messages/queued"))
        .bearer_auth(token)
        .json(&[message])
        .send()
    {
        Ok(response) if response.status().is_success() => Recorded::Yes,
        Ok(response) if is_rejected(response.status()) => Recorded::Refused,
        _ => Recorded::No,
    }
}

/// Takes back the queueing of a Message Codex refused. One the daemon does not hear of keeps
/// its `queued` until this helper queues it on a later poll or the Session closes.
fn unrecord_queued(home: &FeldHome, message: &str) {
    if let Some((token, api, client)) = session_api(home) {
        let _ = client
            .delete(format!("{api}/v1/self/messages/{message}/queued"))
            .bearer_auth(token)
            .send();
    }
}

/// What a call of this helper's own reaches the daemon with: its Session token, the API
/// the Feld home records, and a client.
fn session_api(home: &FeldHome) -> Option<(String, String, reqwest::blocking::Client)> {
    Some((
        std::env::var(FELD_SESSION_TOKEN_ENV).ok()?,
        recorded_api(home).ok()?,
        client(home).ok()?,
    ))
}

/// The rollout file of the thread: the one the hook named, or the one named after the
/// thread under Codex's configuration root.
fn rollout_path(thread: &str) -> Option<PathBuf> {
    std::env::var_os(ROLLOUT_ENV)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| locate_rollout(&CONFIG_ROOT.path()?, thread))
}

/// Whether the daemon refuses this helper's token, which it does once the Session is
/// closed or an attach replaced the token. An answer of any other kind keeps it waiting.
fn session_is_closed(home: &FeldHome) -> bool {
    let Some((token, api, client)) = session_api(home) else {
        return false;
    };
    client
        .get(format!("{api}/v1/self"))
        .bearer_auth(token)
        .send()
        .is_ok_and(|response| is_rejected(response.status()))
}

/// Reads the records Codex appends to a rollout file, from a byte offset on, a whole line
/// at a time.
struct RolloutReader {
    path: PathBuf,
    offset: u64,
}

impl RolloutReader {
    fn new(path: PathBuf, offset: u64) -> Self {
        Self { path, offset }
    }

    /// Whether a record appended since the last call is the turn input that carries the
    /// Message. A line Codex is still writing is read again on the next call.
    fn took(&mut self, message: &str) -> bool {
        let Ok(mut file) = File::open(&self.path) else {
            return false;
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return false;
        }
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(read) if read > 0 && line.ends_with('\n') => {
                    self.offset += read as u64;
                    if takes_message(&line, message) {
                        return true;
                    }
                }
                _ => return false,
            }
        }
    }
}

/// Whether one rollout record is the `user` message a turn took the queued line of
/// `message` in as.
fn takes_message(record: &str, message: &str) -> bool {
    // Most records are something else, and need no decoding to tell.
    if !record.contains(message) {
        return false;
    }
    let Ok(record) = serde_json::from_str::<Value>(record) else {
        return false;
    };
    let Some(payload) = record
        .get("payload")
        .filter(|_| record.get("type").and_then(Value::as_str) == Some("response_item"))
        .filter(|payload| payload.get("type").and_then(Value::as_str) == Some("message"))
        .filter(|payload| payload.get("role").and_then(Value::as_str) == Some("user"))
    else {
        return false;
    };
    payload
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .filter_map(|text| serde_json::from_str::<Value>(text.trim()).ok())
        .any(|line| {
            line.get("type").and_then(Value::as_str) == Some("feld_message")
                && line.pointer("/message/id").and_then(Value::as_str) == Some(message)
        })
}

/// Ends this helper once the SessionEnd hook has asked it to. The helper waits in a long
/// poll the rest of the time, so the asking is seen here.
fn watch_for_stop(home: &FeldHome, session: &str) {
    let marker = stop_marker_path(home, session);
    thread::spawn(move || {
        loop {
            if marker.exists() {
                let _ = fs::remove_file(&marker);
                std::process::exit(0);
            }
            sleep(STOP_POLL);
        }
    });
}

/// The thread this helper queues into: the one of the event the hook started it from.
fn thread_id() -> Option<String> {
    std::env::var(THREAD_ENV)
        .ok()
        .filter(|thread| is_thread_id(thread))
}

/// What puts one line into a Codex thread's queue.
fn queue_command(thread: &str, line: &str) -> Command {
    let mut command = Command::new(CODEX);
    command
        .arg("queue")
        .arg("--thread")
        .arg(thread)
        .arg("--message")
        .arg(line)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn queue_message(thread: &str, line: &str) -> bool {
    queue_command(thread, line)
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREAD: &str = "01a0c16a-1e37-76b3-a04a-31f907002406";
    /// The line one Message reaches Codex as, and the command that puts it into the thread.
    #[test]
    fn a_message_is_queued_into_the_thread_as_one_line() {
        let line = r#"{"type":"feld_message","message":{"id":"message_f2a9d0"}}"#;
        let command = queue_command(THREAD, line);
        assert_eq!(command.get_program(), CODEX);
        let arguments: Vec<&str> = command
            .get_args()
            .map(|argument| argument.to_str().unwrap())
            .collect();
        assert_eq!(arguments, ["queue", "--thread", THREAD, "--message", line]);
    }

    /// A queued Message is taken once a turn writes its line as a `user` message. The
    /// same line elsewhere (the model quoting it, a tool's output) takes nothing, and
    /// neither does a line Codex has not finished writing.
    #[test]
    fn a_message_is_taken_when_a_turn_writes_it_as_a_user_message() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../feld-cli/tests/fixtures/codex-0.150.0-queued-message.jsonl");
        let captured = fs::read_to_string(fixture).unwrap();
        assert!(
            captured
                .lines()
                .any(|record| takes_message(record, "message_test1"))
        );
        assert!(
            !captured
                .lines()
                .any(|record| takes_message(record, "message_other"))
        );

        let line = r#"{"type":"feld_message","message":{"id":"message_f2a9d0","body":"x"}}"#;
        let record = |role: &str| {
            serde_json::json!({
                "type": "response_item",
                "payload": {"type": "message", "role": role, "content": [{"type": "input_text", "text": line}]},
            })
            .to_string()
        };
        assert!(takes_message(&record("user"), "message_f2a9d0"));
        assert!(!takes_message(&record("assistant"), "message_f2a9d0"));

        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("rollout.jsonl");
        let before = record("user") + "\n";
        fs::write(&path, &before).unwrap();
        // What was there before the queueing is not the turn that took it.
        let mut reader = RolloutReader::new(path.clone(), before.len() as u64);
        assert!(!reader.took("message_f2a9d0"));
        let whole = record("user");
        let (head, tail) = whole.split_at(whole.len() / 2);
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        use std::io::Write;
        file.write_all(head.as_bytes()).unwrap();
        assert!(!reader.took("message_f2a9d0"));
        file.write_all(format!("{tail}\n").as_bytes()).unwrap();
        assert!(reader.took("message_f2a9d0"));
    }

    /// The end the SessionEnd hook asks for is a marker beside the Session's mailbox lock,
    /// which the helper watching for it takes away again.
    #[test]
    fn the_end_is_asked_for_beside_the_mailbox_lock() {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().to_owned())).unwrap();
        let session = "session_0aa4d9";
        let path = stop_marker_path(&home, session);
        assert!(!path.exists());
        request_stop(&home, session).unwrap();
        assert!(path.is_file());
        assert_eq!(path.parent(), home.monitor_lock_path(session).parent());
    }
}
