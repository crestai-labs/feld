//! The `transcript` operation: Codex's rollout files projected onto the common transcript entry
//! shape `plugin.md` defines, so that core draws the Console, the turn, and Sub Agent
//! Sessions without knowing anything about Codex.
//!
//! One read covers every rollout file of the Runtime: the one the identity names and the
//! rollout of every Sub Agent the parent spawned, which Codex writes in the same
//! directory. A Sub Agent's records carry `subAgent`, and the parent record that shows a
//! Sub Agent finished carries `subAgentEnded`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use feld_core::plugin::protocol::{TranscriptRequest, TranscriptResponse};
use feld_core::plugin::sdk::transcript::{
    Block, INPUT, NOTIFICATION, OTHER, Projected, RESPONSE, Reading, SYSTEM, THINKING, TOOL_CALL,
    TOOL_RESULT, block, page, read_records, timed,
};

/// What Codex calls the files it writes, as every reason names them.
const NOUN: &str = "rollout";

pub fn transcript(request: TranscriptRequest) -> Result<TranscriptResponse> {
    let path = PathBuf::from(&request.identity.transcript_path);
    // An identity that carries no location at all names a rollout Codex has not written yet.
    let reading = match path.as_os_str().is_empty() {
        true => Reading::Missing,
        false => read_records(&path, NOUN),
    };
    let parent = match reading.into_records() {
        Ok(records) => records,
        Err(unreadable) => return Ok(unreadable),
    };
    let agents = read_sub_agents(&path, &parent, &request.identity.session_id);

    let mut projected = project_stream("", &parent, true);
    for agent in &agents {
        match &agent.lost {
            Some(reason) => projected.push(Projected::lost(
                cursor(agent.ordering, &agent.id, agent.line),
                agent.at.clone(),
                &agent.id,
                reason,
            )),
            None => projected.extend(project_stream(&agent.id, &agent.records, false)),
        }
    }
    Ok(page(projected, &request))
}

/// The place one record holds in the whole column: the instant it carries, the rollout
/// file it came from, and its line. Every record of every file is ordered by this, so a
/// line appended to any of them sorts after everything read before it.
fn cursor(ordering: i64, stream: &str, line: usize) -> String {
    format!("{ordering:013}-{stream}-{line:06}")
}

/// The records of one rollout file, in the order Codex wrote them. A record Codex wrote
/// without an instant sorts with the last instant of its own file.
///
/// `ends_sub_agents` says whether this is the parent's own file: a Sub Agent finishes
/// where the parent recorded it, and a Sub Agent of a Sub Agent is not a Session of this
/// Runtime.
fn project_stream(stream: &str, records: &[Value], ends_sub_agents: bool) -> Vec<Projected> {
    timed(records)
        .map(|entry| {
            let item = project_record(entry.record);
            Projected {
                cursor: cursor(entry.ordering, stream, entry.line),
                at: entry.at,
                kind: item.kind,
                text: item.text,
                error: item.error,
                state: None,
                sub_agent: (!stream.is_empty()).then(|| stream.to_owned()),
                sub_agent_ended: ends_sub_agents
                    .then(|| ended_sub_agent(entry.record))
                    .flatten(),
                raw: entry.record.clone(),
            }
        })
        .collect()
}

/// Which kind one Codex record carries. Codex writes one record per line, so each line is
/// one transcript entry of the common shape.
fn project_record(record: &Value) -> Block {
    let payload = record.get("payload").unwrap_or(&Value::Null).clone();
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match record
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "response_item" => match payload_type {
            "message" => project_message(&payload),
            "reasoning" => block(THINKING, reasoning_text(&payload)),
            // The errand a Sub Agent and its parent send each other, which the Console
            // shows beside the conversation it belongs to.
            "agent_message" => block(SYSTEM, content_text(&payload)),
            name if name.ends_with("_call_output") => tool_result(&payload),
            name if name.ends_with("_call") => block(TOOL_CALL, tool_call_text(&payload)),
            _ => block(OTHER, ""),
        },
        "event_msg" => match payload_type {
            "task_started" => block(SYSTEM, "Turn started"),
            "task_complete" => block(SYSTEM, "Turn completed"),
            _ => block(OTHER, ""),
        },
        _ => block(OTHER, ""),
    }
}

/// The `message` records: what a person or the queue helper wrote, what the model
/// answered, and the instructions Codex itself puts in the conversation.
fn project_message(payload: &Value) -> Block {
    let text = content_text(payload);
    match payload.get("role").and_then(Value::as_str) {
        Some("assistant") => block(RESPONSE, text),
        Some("user") if carries_turn(payload) => match is_feld_message(&text) {
            true => block(NOTIFICATION, text),
            false => block(INPUT, text),
        },
        _ => block(OTHER, ""),
    }
}

/// Whether a `user` message carries a turn at all. Codex writes the context it injects
/// itself — the environment, the plugins it recommends — as a `user` message too, and says
/// in `content_item_kinds` what each part of the message is; only `user.*` is what someone
/// gave the Runtime.
fn carries_turn(payload: &Value) -> bool {
    let Some(kinds) = payload
        .pointer("/internal_chat_message_metadata_passthrough/content_item_kinds")
        .and_then(Value::as_array)
    else {
        return true;
    };
    kinds
        .iter()
        .filter_map(Value::as_str)
        .any(|kind| kind.starts_with("user."))
}

/// Whether the body is the JSON line the queue helper puts in the Codex queue, which is a
/// Message of the Feld mailbox rather than something a person typed.
fn is_feld_message(text: &str) -> bool {
    serde_json::from_str::<Value>(text.trim())
        .is_ok_and(|line| line.get("type").and_then(Value::as_str) == Some("feld_message"))
}

/// The text of a `message` or an `agent_message`: the parts Codex kept as text, in order.
fn content_text(payload: &Value) -> String {
    let Some(content) = payload.get("content") else {
        return String::new();
    };
    match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// What the Console shows of a reasoning record: the summary Codex kept, when the model
/// gave one. The rest of the reasoning is encrypted and belongs to nobody but the model.
fn reasoning_text(payload: &Value) -> String {
    payload
        .get("summary")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| match item {
            Value::String(text) => Some(text.clone()),
            _ => item.get("text").and_then(Value::as_str).map(str::to_owned),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A tool call reads as its name and then what it was called with, so the Console can name
/// the call in one line and show the rest below it.
fn tool_call_text(payload: &Value) -> String {
    let name = payload
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            // A call Codex names by its type alone, such as `web_search_call`.
            payload
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .trim_end_matches("_call")
                .to_owned()
        });
    let arguments = ["arguments", "input", "action", "query"]
        .into_iter()
        .find_map(|key| payload.get(key))
        .filter(|value| !value.is_null());
    match arguments {
        None => name,
        Some(value) => format!("{name}\n{}", readable(value)),
    }
}

/// The output of a tool call, and whether that output reports a failure. Codex hands the
/// exec tool's script the whole command, so a script that ran a command reports what the
/// script itself did: `Script failed` is the tool reporting a failure, and the exit status
/// of a command inside a script that completed is part of the output the Console shows.
fn tool_result(payload: &Value) -> Block {
    let output = payload.get("output").unwrap_or(&Value::Null);
    let text = match output {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item.get("text").and_then(Value::as_str) {
                Some(text) => text.to_owned(),
                None => readable(item),
            })
            .collect::<Vec<_>>()
            .join(""),
        Value::Null => String::new(),
        value => match value.get("content").and_then(Value::as_str) {
            Some(content) => content.to_owned(),
            None => readable(value),
        },
    };
    Block {
        kind: TOOL_RESULT,
        error: reports_failure(payload, output, &text),
        text,
    }
}

/// Whether a tool result says the call failed: what Codex put beside the output, what a
/// structured output says of itself, and the first line of the exec tool's envelope.
fn reports_failure(payload: &Value, output: &Value, text: &str) -> bool {
    if payload.get("success").and_then(Value::as_bool) == Some(false) {
        return true;
    }
    let structured = match output {
        Value::String(text) => serde_json::from_str::<Value>(text.trim()).ok(),
        Value::Array(_) | Value::Null => None,
        value => Some(value.clone()),
    };
    if let Some(value) = structured
        && (value.get("success").and_then(Value::as_bool) == Some(false)
            || value
                .get("error")
                .is_some_and(|error| !error.is_null() && error != "")
            || matches!(
                value.get("status").and_then(Value::as_str),
                Some("failed" | "error")
            ))
    {
        return true;
    }
    text.lines().next() == Some("Script failed")
}

/// A value the Console shows to a person: a string as it stands, anything else as JSON.
fn readable(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        value => serde_json::to_string_pretty(value).unwrap_or_default(),
    }
}

// ------------------------------------------------------------------ Sub Agent records

/// One Sub Agent's rollout file: the thread id Codex names it by, where the parent
/// recorded that it started, and the records themselves. `lost` says why the file could
/// not be read, when it could not be.
struct SubAgent {
    id: String,
    /// Where in the parent's column a Sub Agent that cannot be read stands.
    ordering: i64,
    at: Option<String>,
    line: usize,
    records: Vec<Value>,
    lost: Option<String>,
}

/// The `SubAgentActivity` item of one record, when it carries one.
fn sub_agent_activity(record: &Value) -> Option<&Value> {
    let payload = record.get("payload")?;
    (record.get("type").and_then(Value::as_str) == Some("event_msg")
        && payload.get("type").and_then(Value::as_str) == Some("item_completed"))
    .then(|| payload.get("item"))
    .flatten()
    .filter(|item| item.get("type").and_then(Value::as_str) == Some("SubAgentActivity"))
}

/// The Sub Agent a parent record shows finished, whichever way it finished.
fn ended_sub_agent(record: &Value) -> Option<String> {
    let item = sub_agent_activity(record)?;
    matches!(
        item.get("kind").and_then(Value::as_str),
        Some("completed" | "interrupted" | "failed" | "closed")
    )
    .then(|| item.get("agent_thread_id").and_then(Value::as_str))
    .flatten()
    .map(str::to_owned)
}

/// The rollout files of the Sub Agents the parent spawned, in the order the parent started
/// them. Codex writes them in the same directory as the parent's own, named after the Sub
/// Agent's thread id. A file that cannot be read, or that names another parent, is kept
/// with the reason rather than dropped: the Sub Agent Session is still there, and the
/// parent's own records stay readable either way.
fn read_sub_agents(transcript: &Path, parent: &[Value], session_id: &str) -> Vec<SubAgent> {
    let Some(directory) = transcript.parent() else {
        return Vec::new();
    };
    let threads = parent_threads(parent, session_id);
    let mut agents: Vec<SubAgent> = Vec::new();
    for entry in timed(parent) {
        let Some(item) = sub_agent_activity(entry.record)
            .filter(|item| item.get("kind").and_then(Value::as_str) == Some("started"))
        else {
            continue;
        };
        let Some(id) = item
            .get("agent_thread_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id != &session_id)
        else {
            continue;
        };
        if agents.iter().any(|agent| agent.id == id) {
            continue;
        }
        let (records, lost) = read_sub_agent(directory, id, &threads);
        agents.push(SubAgent {
            id: id.to_owned(),
            ordering: entry.ordering,
            at: entry.at,
            line: entry.line,
            records,
            lost,
        });
    }
    agents
}

/// The thread ids a Sub Agent of this Runtime may name as its parent: the one the identity
/// carries, and the one the parent's own rollout was opened with.
fn parent_threads(parent: &[Value], session_id: &str) -> Vec<String> {
    let mut threads = vec![session_id.to_owned()];
    if let Some(meta) = parent
        .iter()
        .find(|record| record.get("type").and_then(Value::as_str) == Some("session_meta"))
    {
        threads.extend(
            ["/payload/id", "/payload/session_id"]
                .into_iter()
                .filter_map(|pointer| meta.pointer(pointer))
                .filter_map(Value::as_str)
                .map(str::to_owned),
        );
    }
    threads
}

/// One Sub Agent's rollout, read from the directory the parent's own sits in.
fn read_sub_agent(
    directory: &Path,
    id: &str,
    parent_threads: &[String],
) -> (Vec<Value>, Option<String>) {
    let Some(path) = rollout_of(directory, id) else {
        return (
            Vec::new(),
            Some(format!("sub agent {id} has no rollout file")),
        );
    };
    match read_records(&path, NOUN) {
        Reading::Missing => (
            Vec::new(),
            Some(format!("sub agent {id} has no rollout file")),
        ),
        Reading::Lost(reason) => (Vec::new(), Some(reason)),
        Reading::Records(records) => {
            // Codex names the thread that spawned it in the Sub Agent's own `session_meta`.
            let named = records
                .iter()
                .find(|record| record.get("type").and_then(Value::as_str) == Some("session_meta"))
                .and_then(|record| record.pointer("/payload/parent_thread_id"))
                .and_then(Value::as_str);
            match named {
                Some(parent) if !parent_threads.iter().any(|thread| thread == parent) => (
                    Vec::new(),
                    Some(format!("sub agent {id} names another thread as its parent")),
                ),
                _ => (records, None),
            }
        }
    }
}

/// The rollout file of one thread: the file of the directory whose name ends with the
/// thread id, which is how Codex names `rollout-<started at>-<thread id>.jsonl`.
fn rollout_of(directory: &Path, thread: &str) -> Option<PathBuf> {
    let suffix = format!("-{thread}.jsonl");
    fs::read_dir(directory)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(&suffix))
        })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use feld_core::plugin::protocol::{RuntimeIdentity, TranscriptState};
    use feld_core::plugin::sdk::transcript::MISSING_REASON;
    use tempfile::TempDir;

    use super::*;

    /// The rollout files of a real `codex-cli` 0.150.0, with the long instruction and
    /// reasoning blobs shortened. The parent and the sub agent are two threads of one run.
    const PARENT_THREAD: &str = "01a0c15d-8b37-7da3-82ba-6291e17a77ac";
    const SUB_AGENT_THREAD: &str = "01a0c15d-9ee7-7823-b1da-519047940755";

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../feld-cli/tests/fixtures"
        ))
        .join(format!("codex-0.150.0-{name}.jsonl"))
    }

    /// One thread's rollout, named the way Codex names it in the directory of its day.
    fn rollout(directory: &Path, name: &str, thread: &str) -> PathBuf {
        let path = directory.join(format!("rollout-2026-09-21T09-28-48-{thread}.jsonl"));
        fs::create_dir_all(directory).unwrap();
        fs::copy(fixture(name), &path).unwrap();
        path
    }

    /// A day directory holding the rollout of one thread alone.
    fn day(name: &str, thread: &str) -> (TempDir, PathBuf) {
        let directory = TempDir::new().unwrap();
        let path = rollout(&directory.path().join("2026/09/21"), name, thread);
        (directory, path)
    }

    fn identity(path: &Path, thread: &str) -> RuntimeIdentity {
        RuntimeIdentity {
            session_id: thread.to_owned(),
            transcript_path: path.display().to_string(),
            model: Value::Null,
        }
    }

    fn read_from(path: &Path, thread: &str, after: Option<&str>) -> TranscriptResponse {
        transcript(TranscriptRequest {
            identity: identity(path, thread),
            after: after.map(str::to_owned),
            limit: 0,
        })
        .unwrap()
    }

    fn read(path: &Path, thread: &str) -> TranscriptResponse {
        read_from(path, thread, None)
    }

    /// The records of the Conversation view: everything the All events view alone holds is
    /// dropped, and each record reads as its kind and the start of its text.
    fn conversation(response: &TranscriptResponse) -> Vec<(String, String)> {
        response
            .records
            .iter()
            .filter(|record| record["kind"] != "other")
            .map(|record| {
                let text = record["text"].as_str().unwrap_or_default();
                (
                    record["kind"].as_str().unwrap().to_owned(),
                    text.lines().next().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn a_cursor_that_names_no_record_is_refused() {
        let (_directory, path) = day("exec", PARENT_THREAD);
        let unknown = read_from(&path, PARENT_THREAD, Some("garbage"));
        assert_eq!(unknown.state, TranscriptState::InvalidCursor);
        assert!(unknown.records.is_empty());
    }

    #[test]
    fn kinds_follow_what_codex_wrote() {
        let (_directory, path) = day("exec", PARENT_THREAD);
        let response = read(&path, PARENT_THREAD);
        assert_eq!(response.state, TranscriptState::Readable);
        // One line of the rollout is one record, whatever its kind.
        assert_eq!(response.records.len(), 20);
        let kinds = conversation(&response)
            .into_iter()
            .map(|(kind, _)| kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [
                "system",      // task_started
                "input",       // the prompt someone gave
                "response",    // the model's commentary
                "tool-call",   // custom_tool_call
                "tool-result", // custom_tool_call_output
                "response",    // the model's answer
                "system",      // task_complete
            ]
        );
        // The context Codex injects itself is a `user` message too, and stays out of the
        // Conversation view.
        let injected = response
            .records
            .iter()
            .find(|record| record["raw"]["payload"]["role"] == "user")
            .unwrap();
        assert_eq!(injected["kind"], "other");
        assert!(
            injected["raw"]["payload"]["content"][1]["text"]
                .as_str()
                .unwrap()
                .starts_with("<environment_context>")
        );
        // `raw` is the line Codex wrote, and the cursor orders the column by the instant.
        let first = &response.records[0];
        assert_eq!(first["raw"]["type"], "session_meta");
        assert_eq!(first["at"], "2026-09-21T00:25:04.480Z");
        assert!(
            first["cursor"].as_str().unwrap() < response.records[1]["cursor"].as_str().unwrap()
        );
        assert_eq!(
            response.next.as_deref(),
            response.records[19]["cursor"].as_str()
        );
    }

    #[test]
    fn a_tool_call_reads_as_its_name_and_its_result_reports_failure() {
        let (_directory, path) = day("tool-failure", PARENT_THREAD);
        let response = read(&path, PARENT_THREAD);
        let call = response
            .records
            .iter()
            .find(|record| record["kind"] == "tool-call")
            .unwrap();
        assert_eq!(call["text"].as_str().unwrap().lines().next(), Some("exec"));
        assert!(call["text"].as_str().unwrap().contains("apply_patch"));
        let result = response
            .records
            .iter()
            .find(|record| record["kind"] == "tool-result")
            .unwrap();
        assert_eq!(result["error"], true);
        assert!(
            result["text"]
                .as_str()
                .unwrap()
                .contains("apply_patch verification failed")
        );
        assert_eq!(
            result["raw"]["payload"]["call_id"],
            call["raw"]["payload"]["call_id"]
        );

        // A tool whose script completed reports no failure, whatever the command inside it
        // exited with; the Console shows the output.
        let (_other, exec) = day("exec", PARENT_THREAD);
        let completed = read(&exec, PARENT_THREAD);
        let result = completed
            .records
            .iter()
            .find(|record| record["kind"] == "tool-result")
            .unwrap();
        assert!(result.get("error").is_none(), "{result}");
    }

    #[test]
    fn a_message_of_the_queue_is_a_notification() {
        let (_directory, path) = day("queued-message", PARENT_THREAD);
        let response = read(&path, PARENT_THREAD);
        let inputs = conversation(&response)
            .into_iter()
            .filter(|(kind, _)| kind == "input" || kind == "notification")
            .collect::<Vec<_>>();
        assert_eq!(
            inputs,
            [
                ("input".to_owned(), "Reply with the single word pong.".to_owned()),
                (
                    "notification".to_owned(),
                    r#"{"type":"feld_message","message":{"id":"message_test1","fifo":1,"from":"Admin","fromSessionId":null,"accepted":"2026-09-21T09:34:00+09:00","body":"Reply with exactly the words: queued ok"}}"#.to_owned()
                ),
            ]
        );
        // Codex marks the same Message with the client id the queue gave it.
        let notification = response
            .records
            .iter()
            .position(|record| record["kind"] == "notification")
            .unwrap();
        assert!(
            response.records[notification + 1]["raw"]["payload"]["item"]["client_id"].is_string()
        );
    }

    #[test]
    fn sub_agent_records_join_the_parent_column_and_end_where_the_parent_says() {
        let directory = TempDir::new().unwrap();
        let day = directory.path().join("2026/09/21");
        let path = rollout(&day, "parent", PARENT_THREAD);
        rollout(&day, "subagent", SUB_AGENT_THREAD);
        let response = read(&path, PARENT_THREAD);

        let mine = |name: &str| {
            response
                .records
                .iter()
                .filter(|record| record["subAgent"] == name)
                .count()
        };
        assert_eq!(mine(SUB_AGENT_THREAD), 14);
        // The Sub Agent's own answer is in the parent's column, under its thread id.
        let answer = response
            .records
            .iter()
            .find(|record| record["subAgent"] == SUB_AGENT_THREAD && record["kind"] == "response")
            .unwrap();
        assert_eq!(answer["text"], "pong");
        assert!(answer.get("state").is_none());

        // The records of both files are one column, ordered by the instant each carries.
        let cursors = response
            .records
            .iter()
            .map(|record| record["cursor"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let mut sorted = cursors.clone();
        sorted.sort();
        assert_eq!(cursors, sorted);

        // The parent's `SubAgentActivity` records say where the Sub Agent finished: core
        // takes the first of them, which is the completion the `wait_agent` saw.
        let ended = response
            .records
            .iter()
            .filter(|record| record["subAgentEnded"] == SUB_AGENT_THREAD)
            .collect::<Vec<_>>();
        assert_eq!(ended.len(), 2, "{ended:?}");
        assert_eq!(ended[0]["at"], "2026-09-21T00:28:56.658Z");
        assert_eq!(ended[0]["raw"]["payload"]["item"]["kind"], "completed");
        assert_eq!(ended[1]["raw"]["payload"]["item"]["kind"], "interrupted");
        // The record that started it carries no ending of its own.
        assert!(
            !response
                .records
                .iter()
                .any(
                    |record| record["raw"]["payload"]["item"]["kind"] == "started"
                        && record.get("subAgentEnded").is_some()
                )
        );
    }

    #[test]
    fn a_sub_agent_whose_rollout_is_not_there_is_one_lost_record() {
        let (_directory, path) = day("parent", PARENT_THREAD);
        let response = read(&path, PARENT_THREAD);
        let lost = response
            .records
            .iter()
            .filter(|record| record["subAgent"] == SUB_AGENT_THREAD)
            .collect::<Vec<_>>();
        assert_eq!(lost.len(), 1, "{lost:?}");
        assert_eq!(lost[0]["state"], "lost");
        assert_eq!(lost[0]["kind"], "other");
        assert!(
            lost[0]["text"]
                .as_str()
                .unwrap()
                .contains("has no rollout file")
        );
        // The Sub Agent Session it stands for starts where the parent spawned it.
        assert_eq!(lost[0]["at"], "2026-09-21T00:28:53.485Z");
        // The parent's own records stay readable.
        assert_eq!(response.state, TranscriptState::Readable);

        // A rollout of that thread id that names another parent is not this Sub Agent's.
        let directory = TempDir::new().unwrap();
        let day = directory.path().join("2026/09/21");
        let path = rollout(&day, "parent", PARENT_THREAD);
        rollout(&day, "subagent", SUB_AGENT_THREAD);
        let stranger = day.join(format!(
            "rollout-2026-09-21T09-28-48-{SUB_AGENT_THREAD}.jsonl"
        ));
        let mut lines = fs::read_to_string(&stranger).unwrap();
        lines = lines.replace(PARENT_THREAD, "01a0c15d-0000-0000-0000-000000000000");
        fs::write(&stranger, lines).unwrap();
        let response = read(&path, PARENT_THREAD);
        let lost = response
            .records
            .iter()
            .filter(|record| record["subAgent"] == SUB_AGENT_THREAD)
            .collect::<Vec<_>>();
        assert_eq!(lost.len(), 1, "{lost:?}");
        assert!(
            lost[0]["text"]
                .as_str()
                .unwrap()
                .contains("names another thread as its parent")
        );
    }

    #[test]
    fn a_read_goes_on_from_the_cursor_it_left() {
        let directory = TempDir::new().unwrap();
        let day = directory.path().join("2026/09/21");
        let path = rollout(&day, "parent", PARENT_THREAD);
        rollout(&day, "subagent", SUB_AGENT_THREAD);
        let whole = read(&path, PARENT_THREAD);

        let limited = transcript(TranscriptRequest {
            identity: identity(&path, PARENT_THREAD),
            after: None,
            limit: 5,
        })
        .unwrap();
        assert_eq!(limited.records.len(), 5);
        assert_eq!(limited.records, whole.records[..5]);
        let next = limited.next.clone().unwrap();

        let rest = read_from(&path, PARENT_THREAD, Some(&next));
        assert_eq!(rest.records, whole.records[5..]);
        // Nothing has been appended, so a read from the end answers nothing and keeps the
        // cursor where it was.
        let end = whole.next.clone().unwrap();
        let nothing = read_from(&path, PARENT_THREAD, Some(&end));
        assert!(nothing.records.is_empty());
        assert_eq!(nothing.next, Some(end));
    }

    #[test]
    fn a_line_codex_has_not_finished_writing_waits_for_the_next_read() {
        let (_directory, path) = day("exec", PARENT_THREAD);
        let whole = read(&path, PARENT_THREAD);
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(br#"{"timestamp":"2026-09-21T00:25:13.000Z","type":"event_"#)
            .unwrap();
        let torn = read(&path, PARENT_THREAD);
        assert_eq!(torn.records, whole.records);
        assert_eq!(torn.next, whole.next);

        // The rest of the line arrives, and the record comes with the next read.
        file.write_all(b"msg\",\"payload\":{\"type\":\"task_started\"}}\n")
            .unwrap();
        let whole_again = read_from(&path, PARENT_THREAD, whole.next.as_deref());
        assert_eq!(whole_again.records.len(), 1);
        assert_eq!(whole_again.records[0]["kind"], "system");
    }

    #[test]
    fn a_rollout_file_that_is_not_there_is_missing_and_one_that_cannot_be_read_is_lost() {
        let directory = TempDir::new().unwrap();
        let day = directory.path().join("2026/09/21");
        let path = day.join(format!("rollout-2026-09-21T09-28-48-{PARENT_THREAD}.jsonl"));

        // The day directory Codex creates with the file is not there yet either.
        let unborn = read(&path, PARENT_THREAD);
        assert_eq!(unborn.state, TranscriptState::Missing);
        assert_eq!(unborn.reason.as_deref(), Some(MISSING_REASON));

        fs::create_dir_all(&day).unwrap();
        let waiting = read(&path, PARENT_THREAD);
        assert_eq!(waiting.state, TranscriptState::Missing);

        // An identity that carries no location at all.
        let none = read(Path::new(""), PARENT_THREAD);
        assert_eq!(none.state, TranscriptState::Missing);

        fs::write(&path, b"{\"type\":\"session_meta\"}\nnot json\n").unwrap();
        let torn = read(&path, PARENT_THREAD);
        assert_eq!(torn.state, TranscriptState::Lost);
        assert_eq!(
            torn.reason.as_deref(),
            Some("rollout line 2 is not valid JSON")
        );

        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let directory_instead = read(&path, PARENT_THREAD);
        assert_eq!(directory_instead.state, TranscriptState::Lost);
        assert!(directory_instead.reason.unwrap().contains("regular file"));
    }
}
