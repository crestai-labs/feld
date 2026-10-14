//! The `transcript` operation: Claude Code's transcript files projected onto the common transcript entry
//! shape `plugin.md` defines, so that core draws the Console, the turn, and Sub Agent
//! Sessions without knowing anything about Claude Code.
//!
//! One read covers every transcript file of the Runtime: the transcript the identity names and
//! the Sub Agent transcript files Claude Code writes beside it. A Sub Agent's records carry
//! `subAgent`, and the parent record that shows a Sub Agent finished carries
//! `subAgentEnded`.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::Value;

use feld_core::plugin::protocol::{TranscriptRequest, TranscriptResponse};
use feld_core::plugin::sdk::transcript::{
    Block, INPUT, NOTIFICATION, OTHER, Projected, RESPONSE, Reading, SYSTEM, THINKING, TOOL_CALL,
    TOOL_RESULT, block, page, read_records, record_timestamp, timed,
};

/// What Claude Code calls the files it writes, as every reason names them.
const NOUN: &str = "transcript";

/// The `user` records Claude Code writes for a turn someone or something took, as opposed
/// to the lines it keeps for the transcript alone.
const HUMAN_PROMPT_SOURCES: [&str; 4] = ["typed", "queued", "suggestion_accepted", "sdk"];

pub fn transcript(request: TranscriptRequest) -> Result<TranscriptResponse> {
    let path = PathBuf::from(&request.identity.transcript_path);
    let parent = match read_records(&path, NOUN).into_records() {
        Ok(records) => records,
        Err(unreadable) => return Ok(unreadable),
    };
    let agents = read_sub_agents(&path);
    let endings = completions(&parent, &agents);

    let mut projected = project_stream("", &parent, &endings);
    for agent in &agents {
        match &agent.lost {
            Some(reason) => projected.push(Projected::lost(
                format!("{:013}-{}-{:06}-{:02}", 0, agent.id, 0, 0),
                None,
                &agent.id,
                reason,
            )),
            None => projected.extend(project_stream(&agent.id, &agent.records, &BTreeMap::new())),
        }
    }
    Ok(page(projected, &request))
}

/// The records of one transcript file, in the order Claude Code wrote them. The cursor orders
/// every record of every file by the instant it carries, so a record appended to any of
/// them sorts after everything read before it. Records Claude Code wrote without an
/// instant sort with the last instant of their own file.
fn project_stream(
    stream: &str,
    records: &[Value],
    endings: &BTreeMap<(usize, usize), String>,
) -> Vec<Projected> {
    let mut projected = Vec::new();
    for entry in timed(records) {
        let (line, ordering) = (entry.line, entry.ordering);
        for (index, item) in project_record(entry.record).into_iter().enumerate() {
            projected.push(Projected {
                cursor: format!("{ordering:013}-{stream}-{line:06}-{index:02}"),
                at: entry.at.clone(),
                kind: item.kind,
                text: item.text,
                error: item.error,
                state: None,
                sub_agent: (!stream.is_empty()).then(|| stream.to_owned()),
                sub_agent_ended: endings.get(&(line, index)).cloned(),
                raw: entry.record.clone(),
            });
        }
    }
    projected
}

/// Which kinds one Claude Code record carries, in the order they are shown. A record with
/// several blocks of interest becomes several records: an assistant turn that answers and
/// then calls a tool is a `response` followed by a `tool-call`.
fn project_record(record: &Value) -> Vec<Block> {
    match record
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "user" => project_user(record),
        "assistant" => project_assistant(record),
        "system" => match record.get("subtype").and_then(Value::as_str) {
            Some("local_command") => vec![block(
                SYSTEM,
                record
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )],
            Some("turn_duration") => vec![block(
                SYSTEM,
                match record.get("durationMs").and_then(Value::as_i64) {
                    Some(duration) => format!("Turn completed in {duration} ms"),
                    None => "Turn completed".to_owned(),
                },
            )],
            _ => vec![block(OTHER, "")],
        },
        _ => vec![block(OTHER, "")],
    }
}

fn project_user(record: &Value) -> Vec<Block> {
    if !carries_turn(record) {
        return vec![block(OTHER, "")];
    }
    let results = message_blocks(record)
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("tool_result"))
        .map(|item| Block {
            kind: TOOL_RESULT,
            text: block_result_text(item),
            error: item.get("is_error").and_then(Value::as_bool) == Some(true),
        })
        .collect::<Vec<_>>();
    if !results.is_empty() {
        return results;
    }
    let Some(text) = record
        .pointer("/message/content")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return vec![block(OTHER, "")];
    };
    match record.pointer("/origin/kind").and_then(Value::as_str) {
        None | Some("human") => vec![block(INPUT, text)],
        Some(_) => vec![block(NOTIFICATION, text)],
    }
}

fn project_assistant(record: &Value) -> Vec<Block> {
    let projected = message_blocks(record)
        .filter_map(|item| match item.get("type").and_then(Value::as_str) {
            Some("text") => Some(block(
                RESPONSE,
                item.get("text").and_then(Value::as_str).unwrap_or_default(),
            )),
            Some("tool_use") => Some(block(TOOL_CALL, tool_call_text(item))),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !projected.is_empty() {
        return projected;
    }
    let thinking = message_blocks(record)
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("thinking"))
        .filter_map(|item| item.get("thinking").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if thinking.is_empty() {
        vec![block(OTHER, "")]
    } else {
        vec![block(THINKING, thinking.join("\n"))]
    }
}

/// A tool call reads as its name and then the input it was called with, so the Console can
/// name the call in one line and show the rest below it.
fn tool_call_text(call: &Value) -> String {
    let name = call
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_owned();
    match call.get("input") {
        None | Some(Value::Null) => name,
        Some(input) => format!(
            "{name}\n{}",
            serde_json::to_string_pretty(input).unwrap_or_default()
        ),
    }
}

fn block_result_text(result: &Value) -> String {
    match result.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(value) => serde_json::to_string_pretty(value).unwrap_or_default(),
        None => String::new(),
    }
}

/// Whether a `user` record carries a turn at all: a prompt someone gave, a notification
/// the Runtime delivered, or a tool result. A line Claude Code keeps for the transcript
/// alone — injected context, a compact summary, `[Request interrupted by user]` — carries
/// none.
fn carries_turn(record: &Value) -> bool {
    let flag = |name: &str| record.get(name).and_then(Value::as_bool) == Some(true);
    if flag("isMeta") || flag("isCompactSummary") || flag("isVisibleInTranscriptOnly") {
        return false;
    }
    if message_blocks(record)
        .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
    {
        return true;
    }
    let origin = record.pointer("/origin/kind").and_then(Value::as_str);
    if origin == Some("task-notification") {
        return true;
    }
    origin == Some("human")
        || record
            .get("promptSource")
            .and_then(Value::as_str)
            .is_some_and(|source| HUMAN_PROMPT_SOURCES.contains(&source))
}

// ------------------------------------------------------------------ Sub Agent records

/// One Sub Agent's transcript file: the id Claude Code names it by, what its meta file says,
/// and the records themselves. `lost` says why the file could not be read, when it could
/// not be.
struct SubAgent {
    id: String,
    meta: SubAgentMeta,
    records: Vec<Value>,
    lost: Option<String>,
}

/// What Claude Code writes in `agent-<id>.meta.json` beside a Sub Agent transcript file: the
/// tool call that launched it, its agent type, and the description the parent's stop
/// notification quotes.
#[derive(Default)]
struct SubAgentMeta {
    description: Option<String>,
    tool_use_id: Option<String>,
}

/// The Sub Agent transcript files Claude Code writes beside a transcript, in id order. A file
/// that cannot be read, or whose records name another Sub Agent, is kept with the reason
/// rather than dropped: the Sub Agent Session is still there, and the parent's own records
/// stay readable either way.
fn read_sub_agents(transcript: &Path) -> Vec<SubAgent> {
    let Some(parent) = transcript.parent() else {
        return Vec::new();
    };
    let directory = parent
        .join(transcript.file_stem().unwrap_or_default())
        .join("subagents");
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut agents = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(id) = path
            .file_stem()
            .and_then(|value| value.to_str())
            .and_then(|value| value.strip_prefix("agent-"))
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let (records, lost) = match read_records(&path, NOUN) {
            Reading::Records(records) => {
                let names_another = records
                    .iter()
                    .filter_map(|record| record.get("agentId").and_then(Value::as_str))
                    .any(|named| named != id);
                if names_another {
                    (
                        Vec::new(),
                        Some(
                            "subagent transcript file identity does not match its filename"
                                .to_owned(),
                        ),
                    )
                } else if !records
                    .iter()
                    .any(|record| record_timestamp(record).is_some())
                {
                    (
                        Vec::new(),
                        Some("subagent transcript file has no timestamped line".to_owned()),
                    )
                } else {
                    (records, None)
                }
            }
            Reading::Missing => (
                Vec::new(),
                Some("subagent transcript file does not exist".to_owned()),
            ),
            Reading::Lost(reason) => (Vec::new(), Some(reason)),
        };
        agents.push(SubAgent {
            id: id.to_owned(),
            meta: read_meta(&path),
            records,
            lost,
        });
    }
    agents.sort_by(|left, right| left.id.cmp(&right.id));
    agents
}

fn read_meta(transcript: &Path) -> SubAgentMeta {
    let Ok(text) = fs::read_to_string(transcript.with_extension("meta.json")) else {
        return SubAgentMeta::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return SubAgentMeta::default();
    };
    let field = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_owned);
    SubAgentMeta {
        description: field("description"),
        tool_use_id: field("toolUseId"),
    }
}

/// Where in the parent's records each Sub Agent is shown to have finished, as the line and
/// the projected record within it.
fn completions(parent: &[Value], agents: &[SubAgent]) -> BTreeMap<(usize, usize), String> {
    let mut endings = BTreeMap::new();
    for agent in agents {
        if let Some(position) = completion_position(parent, &agent.id, &agent.meta) {
            endings.entry(position).or_insert_with(|| agent.id.clone());
        }
    }
    endings
}

/// The parent record that shows the Sub Agent finished: the `tool_result` of a synchronous
/// call, or for a background call the Runtime notification that reports the task stopped,
/// whichever way it stopped.
fn completion_position(
    records: &[Value],
    agent_id: &str,
    meta: &SubAgentMeta,
) -> Option<(usize, usize)> {
    let mut agent_tool_uses = HashSet::new();
    let mut async_launch = None;
    for (line, record) in records.iter().enumerate() {
        agent_tool_uses.extend(message_blocks(record).filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("tool_use")
                && matches!(
                    block.get("name").and_then(Value::as_str),
                    Some("Agent" | "Task")
                ))
            .then(|| block.get("id").and_then(Value::as_str))
            .flatten()
        }));
        let Some(result) = record.get("toolUseResult") else {
            continue;
        };
        let Some((block, tool_use_id)) = message_blocks(record)
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
            .enumerate()
            .find_map(|(block, item)| {
                item.get("tool_use_id")
                    .and_then(Value::as_str)
                    .filter(|id| agent_tool_uses.contains(id))
                    .map(|id| (block, id))
            })
        else {
            continue;
        };
        let launched_this_agent = result.get("agentId").and_then(Value::as_str) == Some(agent_id)
            || meta.tool_use_id.as_deref() == Some(tool_use_id);
        if !launched_this_agent {
            continue;
        }
        let asynchronous = result.get("isAsync").and_then(Value::as_bool) == Some(true)
            || result.get("status").and_then(Value::as_str) == Some("async_launched");
        if asynchronous {
            async_launch = Some((line, tool_use_id));
            break;
        }
        if result.get("status").and_then(Value::as_str) == Some("completed") {
            return Some((line, block));
        }
    }
    let (launch_line, tool_use_id) = async_launch?;
    records[launch_line + 1..]
        .iter()
        .enumerate()
        .find(|(_, record)| {
            verified_completion_notification(record, agent_id, tool_use_id)
                || stop_notification(record, meta)
        })
        .map(|(offset, _)| (launch_line + 1 + offset, 0))
}

/// A record Claude Code itself enqueued or delivered as a task notification, as opposed to
/// text a person or the model wrote.
fn runtime_notification(record: &Value) -> bool {
    (record.get("type").and_then(Value::as_str) == Some("queue-operation")
        && record.get("operation").and_then(Value::as_str) == Some("enqueue"))
        || (record.pointer("/origin/kind").and_then(Value::as_str) == Some("task-notification")
            && record.get("promptSource").and_then(Value::as_str) == Some("system"))
}

/// The `<task-notification>` for this launch. Claude Code sends one whenever the background
/// task stops, and its `status` (completed, failed, killed) says how; each of them ends the
/// Sub Agent Session.
fn verified_completion_notification(record: &Value, agent_id: &str, tool_use_id: &str) -> bool {
    runtime_notification(record)
        && notification_texts(record).any(|text| {
            xml_field(text, "task-id") == Some(agent_id)
                && xml_field(text, "tool-use-id") == Some(tool_use_id)
                && xml_field(text, "status").is_some_and(|status| !status.trim().is_empty())
        })
}

/// The plain-text notice Claude Code enqueues when a person stops a background agent; it
/// names the agent by the description in its meta file.
fn stop_notification(record: &Value, meta: &SubAgentMeta) -> bool {
    let Some(description) = meta.description.as_deref() else {
        return false;
    };
    let notice = format!("Background agent \"{description}\" was stopped by the user");
    runtime_notification(record) && notification_texts(record).any(|text| text.contains(&notice))
}

fn notification_texts(record: &Value) -> impl Iterator<Item = &str> {
    record
        .get("content")
        .and_then(Value::as_str)
        .into_iter()
        .chain(record.pointer("/message/content").and_then(Value::as_str))
        .chain(message_blocks(record).filter_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| block.get("text").and_then(Value::as_str))
                .flatten()
        }))
}

fn xml_field<'a>(text: &'a str, field: &str) -> Option<&'a str> {
    let start_tag = format!("<{field}>");
    let end_tag = format!("</{field}>");
    let start = text.find(&start_tag)? + start_tag.len();
    let end = text[start..].find(&end_tag)? + start;
    Some(&text[start..end])
}

/// How much of a transcript file one step of [`latest_model`] reads, back from where the
/// previous step started.
const TAIL_CHUNK: u64 = 64 * 1024;

/// The model of the last response Claude Code wrote to the transcript at `path`, read back
/// from the file's end. Claude Code hands `model` to the SessionStart hook alone, so this is
/// where the other hooks and an attach find it.
pub fn latest_model(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut end = file.metadata().ok()?.len();
    let mut tail = Vec::new();
    while end > 0 {
        let start = end.saturating_sub(TAIL_CHUNK);
        let mut chunk = vec![0; (end - start) as usize];
        file.seek(SeekFrom::Start(start)).ok()?;
        file.read_exact(&mut chunk).ok()?;
        chunk.append(&mut tail);
        tail = chunk;
        end = start;
        // The lines after the first line break are whole; the first one is whole at the
        // file's start alone.
        let whole = if start == 0 {
            0
        } else {
            match tail.iter().position(|byte| *byte == b'\n') {
                Some(index) => index + 1,
                None => continue,
            }
        };
        if let Some(model) = tail[whole..]
            .split(|byte| *byte == b'\n')
            .rev()
            .find_map(response_model)
        {
            return Some(model);
        }
        tail.truncate(whole);
    }
    None
}

/// How often [`awaited_model`] reads the transcript file again.
const AWAIT_STEP: Duration = Duration::from_millis(100);

/// [`latest_model`], read again until a response is there or `wait` has passed. Claude Code
/// starts the Stop hook before it writes the turn's response to the transcript file, so
/// the first turn's Stop finds no response at once.
pub fn awaited_model(path: &Path, wait: Duration) -> Option<String> {
    let deadline = Instant::now() + wait;
    loop {
        if let Some(model) = latest_model(path) {
            return Some(model);
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        std::thread::sleep(AWAIT_STEP.min(deadline - now));
    }
}

/// The model of one transcript line that is a response of the Runtime itself. A Sub Agent's
/// response and one Claude Code made up (`<synthetic>`) name no model of the Runtime.
fn response_model(line: &[u8]) -> Option<String> {
    let record: Value = serde_json::from_slice(line).ok()?;
    if record.get("type").and_then(Value::as_str) != Some("assistant")
        || record.get("isSidechain").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    record
        .pointer("/message/model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty() && !model.starts_with('<'))
        .map(str::to_owned)
}

fn message_blocks(record: &Value) -> impl Iterator<Item = &Value> {
    record
        .pointer("/message/content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;

    use feld_core::plugin::protocol::{RuntimeIdentity, TranscriptState};
    use feld_core::plugin::sdk::transcript::LOST;
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    fn identity(path: &Path) -> RuntimeIdentity {
        RuntimeIdentity {
            session_id: "0f1e2d3c".to_owned(),
            transcript_path: path.display().to_string(),
            model: Value::Null,
        }
    }

    fn read(path: &Path) -> TranscriptResponse {
        transcript(TranscriptRequest {
            identity: identity(path),
            after: None,
            limit: 0,
        })
        .unwrap()
    }

    fn write_records(path: &Path, records: &[Value]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            records
                .iter()
                .map(|record| format!("{record}\n"))
                .collect::<String>(),
        )
        .unwrap();
    }

    fn kinds(response: &TranscriptResponse) -> Vec<&str> {
        response
            .records
            .iter()
            .map(|record| record["kind"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn the_latest_model_is_the_one_of_the_last_response_of_the_runtime() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("projects/-work/0f1e2d3c.jsonl");
        assert_eq!(latest_model(&path), None);
        let response =
            |model: &str| json!({"type": "assistant", "message": {"model": model, "content": []}});
        let mut sub_agent = response("claude-haiku-4-5");
        sub_agent["isSidechain"] = json!(true);
        // A tool result longer than one read back reaches across the steps.
        let long = json!({"type": "user", "message": {"content": "x".repeat(200 * 1024)}});
        write_records(
            &path,
            &[
                json!({"type": "user", "message": {"content": "hi"}}),
                response("claude-opus-5"),
                long.clone(),
                response("claude-fable-5"),
                long,
                sub_agent,
                response("<synthetic>"),
                json!({"type": "user", "message": {"content": "next"}}),
            ],
        );
        assert_eq!(latest_model(&path).as_deref(), Some("claude-fable-5"));

        // A line still being written is not read yet.
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(
            file,
            "{{\"type\":\"assistant\",\"message\":{{\"model\":\"claude-opus"
        )
        .unwrap();
        assert_eq!(latest_model(&path).as_deref(), Some("claude-fable-5"));
        writeln!(file, "-5-5\"}}}}").unwrap();
        assert_eq!(latest_model(&path).as_deref(), Some("claude-opus-5-5"));

        write_records(
            &path,
            &[json!({"type": "user", "message": {"content": "hi"}})],
        );
        assert_eq!(latest_model(&path), None);
    }

    #[test]
    fn the_awaited_model_is_the_one_of_a_response_written_while_waiting() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("projects/-work/0f1e2d3c.jsonl");
        write_records(
            &path,
            &[json!({"type": "user", "message": {"content": "hi"}})],
        );
        assert_eq!(awaited_model(&path, Duration::from_millis(300)), None);

        let writer = {
            let path = path.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
                writeln!(
                    file,
                    "{}",
                    json!({"type": "assistant", "message": {"model": "claude-haiku-4-5", "content": []}})
                )
                .unwrap();
            })
        };
        assert_eq!(
            awaited_model(&path, Duration::from_secs(5)).as_deref(),
            Some("claude-haiku-4-5")
        );
        writer.join().unwrap();
    }

    #[test]
    fn kinds_follow_what_claude_code_wrote() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("runtime.jsonl");
        write_records(
            &path,
            &[
                json!({"type":"user","timestamp":"2026-09-15T01:00:00Z","origin":{"kind":"human"},
                       "message":{"content":"delegate"}}),
                json!({"type":"user","timestamp":"2026-09-15T01:00:01Z","origin":{"kind":"task-notification"},
                       "promptSource":"system","message":{"content":"<task-notification>done</task-notification>"}}),
                json!({"type":"assistant","timestamp":"2026-09-15T01:00:02Z",
                       "message":{"content":[{"type":"text","text":"on it"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}),
                json!({"type":"user","timestamp":"2026-09-15T01:00:03Z",
                       "message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"README.md","is_error":true}]}}),
                json!({"type":"assistant","timestamp":"2026-09-15T01:00:04Z",
                       "message":{"content":[{"type":"thinking","thinking":"weighing"}]}}),
                json!({"type":"assistant","timestamp":"2026-09-15T01:00:05Z",
                       "message":{"content":[{"type":"text","text":"done"}]}}),
                json!({"type":"system","subtype":"local_command","timestamp":"2026-09-15T01:00:06Z","content":"/model opus"}),
                json!({"type":"user","timestamp":"2026-09-15T01:00:07Z","isMeta":true,"message":{"content":"injected"}}),
                json!({"type":"file-history-snapshot","timestamp":"2026-09-15T01:00:08Z"}),
            ],
        );
        let page = read(&path);
        assert_eq!(page.state, TranscriptState::Readable);
        assert_eq!(
            kinds(&page),
            vec![
                INPUT,
                NOTIFICATION,
                RESPONSE,
                TOOL_CALL,
                TOOL_RESULT,
                THINKING,
                RESPONSE,
                SYSTEM,
                OTHER,
                OTHER
            ]
        );
        assert_eq!(page.records[0]["text"], "delegate");
        assert_eq!(page.records[3]["text"], "Bash\n{\n  \"command\": \"ls\"\n}");
        assert_eq!(page.records[4]["text"], "README.md");
        // Only a tool result that failed carries `error`.
        assert_eq!(page.records[4]["error"], true);
        assert!(page.records[3].get("error").is_none());
        assert_eq!(page.records[0]["at"], "2026-09-15T01:00:00Z");
        assert_eq!(page.records[0]["raw"]["type"], "user");
        // The cursor of every record resumes the read at the record after it.
        let after = page.records[3]["cursor"].as_str().unwrap().to_owned();
        let rest = transcript(TranscriptRequest {
            identity: identity(&path),
            after: Some(after),
            limit: 2,
        })
        .unwrap();
        assert_eq!(kinds(&rest), vec![TOOL_RESULT, THINKING]);
        assert_eq!(rest.next.as_deref(), rest.records[1]["cursor"].as_str());
        // A cursor that names no record is refused rather than read as the end.
        let unknown = transcript(TranscriptRequest {
            identity: identity(&path),
            after: Some("garbage".to_owned()),
            limit: 0,
        })
        .unwrap();
        assert_eq!(unknown.state, TranscriptState::InvalidCursor);
        assert!(unknown.records.is_empty());
    }

    #[test]
    fn a_prompt_claude_code_keeps_for_the_transcript_alone_carries_no_turn() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("runtime.jsonl");
        write_records(
            &path,
            &[
                json!({"type":"user","message":{"content":"[Request interrupted by user]"}}),
                json!({"type":"user","promptSource":"typed","message":{"content":"/review"}}),
                json!({"type":"user","isCompactSummary":true,"promptSource":"typed","message":{"content":"summary"}}),
            ],
        );
        assert_eq!(kinds(&read(&path)), vec![OTHER, INPUT, OTHER]);
    }

    #[test]
    fn the_state_separates_a_missing_file_from_a_location_that_is_lost() {
        let directory = TempDir::new().unwrap();
        let missing = directory.path().join("missing.jsonl");
        assert_eq!(read(&missing).state, TranscriptState::Missing);
        let unborn = directory.path().join("projects/-work/runtime.jsonl");
        assert_eq!(read(&unborn).state, TranscriptState::Missing);
        let file = directory.path().join("file");
        fs::write(&file, b"").unwrap();
        assert_eq!(
            read(&file.join("runtime.jsonl")).state,
            TranscriptState::Lost
        );
        assert_eq!(
            read(Path::new("relative.jsonl")).state,
            TranscriptState::Lost
        );
        let malformed = directory.path().join("malformed.jsonl");
        fs::write(&malformed, b"{not-json}\n").unwrap();
        let page = read(&malformed);
        assert_eq!(page.state, TranscriptState::Lost);
        assert!(page.records.is_empty());
        assert!(page.reason.unwrap().contains("line 1"));
    }

    #[test]
    fn a_line_still_being_written_waits_for_the_next_read() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("runtime.jsonl");
        let mut file = File::create(&path).unwrap();
        write!(
            file,
            "{{\"type\":\"user\",\"promptSource\":\"typed\",\"message\":{{\"content\":\"one\"}}}}\r\n{{\"partial\":"
        )
        .unwrap();
        let first = read(&path);
        assert_eq!(kinds(&first), vec![INPUT]);
        writeln!(file, "true}}").unwrap();
        let appended = transcript(TranscriptRequest {
            identity: identity(&path),
            after: first.next.clone(),
            limit: 0,
        })
        .unwrap();
        assert_eq!(kinds(&appended), vec![OTHER]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_location_is_lost() {
        use std::os::unix::fs::symlink;

        let directory = TempDir::new().unwrap();
        let target = directory.path().join("target.jsonl");
        let link = directory.path().join("link.jsonl");
        fs::write(&target, b"{}\n").unwrap();
        symlink(&target, &link).unwrap();
        assert_eq!(read(&link).state, TranscriptState::Lost);
    }

    /// The six ways a Sub Agent ends, each marked on the parent record that shows it.
    #[test]
    fn every_way_a_sub_agent_ends_marks_the_parent_record_that_shows_it() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("runtime.jsonl");
        let at = |second: u32| format!("2026-09-15T01:00:{second:02}Z");
        let launches = [
            ("agent-sync", "Agent"),
            ("agent-done", "Agent"),
            ("agent-failed", "Task"),
            ("agent-killed", "Agent"),
            ("agent-stopped", "Agent"),
            ("agent-open", "Agent"),
        ]
        .iter()
        .map(|(id, name)| json!({"type":"tool_use","id":id,"name":name}))
        .collect::<Vec<_>>();
        let notification = |agent: &str, tool: &str, status: &str, second: u32| {
            json!({
                "type":"queue-operation","operation":"enqueue","timestamp":at(second),
                "content":format!("<task-notification><task-id>{agent}</task-id><tool-use-id>{tool}</tool-use-id><status>{status}</status></task-notification>")
            })
        };
        let launched = |tool: &str, agent: Option<&str>, second: u32| {
            let mut result = json!({"isAsync":true,"status":"async_launched"});
            if let Some(agent) = agent {
                result["agentId"] = json!(agent);
            }
            json!({
                "type":"user","timestamp":at(second),
                "message":{"content":[{"type":"tool_result","tool_use_id":tool}]},
                "toolUseResult":result
            })
        };
        write_records(
            &path,
            &[
                json!({"type":"user","timestamp":at(0),"promptSource":"typed","message":{"content":"delegate"}}),
                json!({"type":"assistant","timestamp":at(1),"message":{"content":launches}}),
                json!({
                    "type":"user","timestamp":at(5),
                    "message":{"content":[{"type":"tool_result","tool_use_id":"agent-sync"}]},
                    "toolUseResult":{"agentId":"sync-1","status":"completed"}
                }),
                launched("agent-done", Some("done-2"), 1),
                launched("agent-failed", Some("failed-3"), 1),
                launched("agent-killed", Some("killed-4"), 1),
                launched("agent-stopped", None, 1),
                launched("agent-open", Some("open-6"), 1),
                notification("done-2", "agent-done", "completed", 6),
                notification("failed-3", "agent-failed", "failed", 7),
                notification("killed-4", "agent-killed", "killed", 8),
                json!({
                    "type":"user","timestamp":at(9),"origin":{"kind":"task-notification"},"promptSource":"system",
                    "message":{"content":"Background agent \"Stopped job\" was stopped by the user."}
                }),
            ],
        );
        let subagents = directory.path().join("runtime/subagents");
        for agent in [
            "sync-1",
            "done-2",
            "failed-3",
            "killed-4",
            "stopped-5",
            "open-6",
        ] {
            write_records(
                &subagents.join(format!("agent-{agent}.jsonl")),
                &[
                    json!({"type":"user","agentId":agent,"timestamp":at(2),"promptSource":"typed","message":{"content":"task"}}),
                    json!({"type":"assistant","agentId":agent,"timestamp":at(3),"message":{"content":[{"type":"text","text":"done"}]}}),
                ],
            );
        }
        fs::write(
            subagents.join("agent-stopped-5.meta.json"),
            json!({"agentType":"general-purpose","description":"Stopped job","toolUseId":"agent-stopped"})
                .to_string(),
        )
        .unwrap();

        let page = read(&path);
        let ended = page
            .records
            .iter()
            .filter_map(|record| {
                Some((
                    record.get("subAgentEnded")?.as_str()?.to_owned(),
                    record["at"].as_str()?.to_owned(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            ended,
            BTreeMap::from([
                ("sync-1".to_owned(), at(5)),
                ("done-2".to_owned(), at(6)),
                ("failed-3".to_owned(), at(7)),
                ("killed-4".to_owned(), at(8)),
                ("stopped-5".to_owned(), at(9)),
            ])
        );
        // Every Sub Agent's own records carry its id, and the parent's carry none.
        let mut counted = BTreeMap::new();
        for record in &page.records {
            let owner = record
                .get("subAgent")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            *counted.entry(owner).or_insert(0) += 1;
        }
        assert_eq!(counted["open-6"], 2);
        assert_eq!(counted[""], 17);
    }

    /// A Sub Agent transcript file Feld cannot read keeps its Sub Agent: one record says the
    /// records are lost and why, and the Session's own records stay readable.
    #[test]
    fn a_sub_agent_file_that_cannot_be_read_is_one_lost_record() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("runtime.jsonl");
        write_records(
            &path,
            &[
                json!({"type":"user","timestamp":"2026-09-15T01:00:00Z","promptSource":"typed","message":{"content":"go"}}),
            ],
        );
        let subagents = directory.path().join("runtime/subagents");
        // Records that name another Sub Agent, a file that is not JSON, and a file with no
        // instant to start from.
        write_records(
            &subagents.join("agent-child-7.jsonl"),
            &[
                json!({"type":"assistant","agentId":"other","message":{"content":[{"type":"text","text":"x"}]}}),
            ],
        );
        fs::write(subagents.join("agent-broken-8.jsonl"), b"{bad}\n").unwrap();
        write_records(
            &subagents.join("agent-timeless-9.jsonl"),
            &[
                json!({"type":"assistant","agentId":"timeless-9","message":{"content":[{"type":"text","text":"x"}]}}),
            ],
        );
        let page = read(&path);
        let lost = page
            .records
            .iter()
            .filter(|record| record.get("state").and_then(Value::as_str) == Some(LOST))
            .collect::<Vec<_>>();
        assert_eq!(
            lost.iter()
                .map(|record| record["subAgent"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["broken-8", "child-7", "timeless-9"]
        );
        assert!(lost.iter().all(|record| record["kind"] == OTHER
            && record["at"].is_null()
            && !record["text"].as_str().unwrap().is_empty()));
        // The Session's own records are untouched by any of them.
        let own = page
            .records
            .iter()
            .filter(|record| record.get("subAgent").is_none())
            .collect::<Vec<_>>();
        assert_eq!(own.len(), 1);
        assert_eq!(own[0]["kind"], INPUT);
    }

    /// Claude Code 2.1.272 opens a Sub Agent's file with a record that carries no instant;
    /// the Sub Agent's own records still order after the parent's and keep their own `at`.
    #[test]
    fn a_sub_agent_file_that_opens_without_an_instant_keeps_its_own_instants() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("runtime.jsonl");
        write_records(
            &path,
            &[
                json!({"type":"user","timestamp":"2026-09-15T01:00:00Z","promptSource":"typed","message":{"content":"go"}}),
            ],
        );
        let child = directory
            .path()
            .join("runtime/subagents/agent-a2d1f8842487e78ce.jsonl");
        fs::create_dir_all(child.parent().unwrap()).unwrap();
        fs::write(
            &child,
            include_bytes!("../../feld-cli/tests/fixtures/claude-2.1.272-child.jsonl"),
        )
        .unwrap();
        let page = read(&path);
        let records = page
            .records
            .iter()
            .filter(|record| record.get("subAgent").is_some())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 3);
        assert!(records[0]["at"].is_null());
        assert_eq!(records[2]["at"], "2026-09-15T01:00:07Z");
    }

    #[test]
    fn a_real_claude_transcript_reads_when_supplied() {
        let Some(path) = std::env::var_os("FELD_REAL_CLAUDE_TRANSCRIPT") else {
            return;
        };
        let page = read(Path::new(&path));
        assert_eq!(page.state, TranscriptState::Readable);
        assert!(!page.records.is_empty());
    }
}
