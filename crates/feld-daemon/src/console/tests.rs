use std::fs;
use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use feld_core::id::fresh_token;
use feld_core::model::{
    ActorKind, EndKind, Execution, Outcome, Ownership, Placement, RuntimeIdentity, Session,
    SessionLifecycle, StartKind, Turn, message_preview,
};
use feld_core::plugin::protocol::{
    IdentityRequest, IdentityResponse, MatchRequest, MatchResponse, PrepareRequest,
    PrepareResponse, RestoreRequest, RestoreResponse, RuntimeLaunchRequest, RuntimeLaunchResponse,
    TranscriptRequest, TranscriptResponse, TranscriptState,
};
use feld_core::plugin::{Manifest, RuntimeAdapter};

fn read_transcript(
    source: &TranscriptSource,
    session: &str,
    stream: &Stream,
    window: Window,
) -> TranscriptPage {
    read_transcript_view(source, session, stream, window, false).expect("the cursor names an entry")
}

/// A Runtime Adapter that answers `transcript` with entries already in the common
/// shape, which is all core knows of any Runtime.
struct StubAdapter {
    manifest: Manifest,
    answer: TranscriptResponse,
}

impl StubAdapter {
    fn readable(records: Vec<Value>) -> Arc<dyn RuntimeAdapter> {
        Self::answering(TranscriptResponse {
            state: TranscriptState::Readable,
            reason: None,
            next: records
                .last()
                .and_then(|record| record["cursor"].as_str())
                .map(str::to_owned),
            records,
        })
    }

    fn answering(answer: TranscriptResponse) -> Arc<dyn RuntimeAdapter> {
        Arc::new(Self {
            manifest: Manifest {
                name: "stub".to_owned(),
                kind: feld_core::plugin::PluginKind::Runtime,
                label: "Stub".to_owned(),
                stop_input: None,
                owned_settings: Vec::new(),
                extra: Default::default(),
            },
            answer,
        })
    }
}

impl RuntimeAdapter for StubAdapter {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn launch(&self, _: &RuntimeLaunchRequest) -> Result<RuntimeLaunchResponse> {
        unimplemented!("the Console never launches")
    }

    fn match_process(&self, _: &MatchRequest) -> Result<MatchResponse> {
        unimplemented!("the Console never matches")
    }

    fn identity(&self, _: &IdentityRequest) -> Result<IdentityResponse> {
        unimplemented!("the Console never asks for an identity")
    }

    fn prepare(&self, _: &PrepareRequest) -> Result<PrepareResponse> {
        unimplemented!("the Console never prepares a Runtime")
    }

    fn restore(&self, _: &RestoreRequest) -> Result<RestoreResponse> {
        unimplemented!("the Console never restores a Runtime")
    }

    fn transcript(&self, request: &TranscriptRequest) -> Result<TranscriptResponse> {
        let mut answer = self.answer.clone();
        if let Some(after) = request.after.as_deref() {
            if !answer
                .records
                .iter()
                .any(|record| record["cursor"].as_str() == Some(after))
            {
                answer.state = TranscriptState::InvalidCursor;
                answer.records.clear();
                return Ok(answer);
            }
            answer
                .records
                .retain(|record| record["cursor"].as_str().unwrap_or_default() > after);
        }
        if request.limit > 0 && (request.limit as usize) < answer.records.len() {
            answer.records.truncate(request.limit as usize);
        }
        answer.next = answer
            .records
            .last()
            .and_then(|record| record["cursor"].as_str())
            .map(str::to_owned)
            .or_else(|| request.after.clone());
        Ok(answer)
    }
}

fn record(cursor: &str, kind: &str, at: &str) -> Value {
    json!({"cursor": cursor, "at": at, "kind": kind, "text": kind, "raw": {}})
}

fn identity() -> RuntimeIdentity {
    RuntimeIdentity {
        session_id: "runtime".to_owned(),
        transcript_path: "/tmp/runtime.jsonl".to_owned(),
        model: Value::Null,
    }
}

fn cursors(page: &TranscriptPage) -> Vec<String> {
    page.records
        .iter()
        .map(|record| record.cursor.clone())
        .collect()
}

fn session() -> Session {
    Session {
        id: "session_one".to_owned(),
        workspace: "default".to_owned(),
        actor_id: "actor_one".to_owned(),
        actor: "agent".to_owned(),
        actor_kind: ActorKind::Agent,
        generation: 1,
        start: "2026-09-15T01:00:00Z".to_owned(),
        end: None,
        start_kind: StartKind::Start,
        ownership: Ownership::Owned,
        end_kind: EndKind::Running,
        exit_code: None,
        planned: None,
        execution: Execution {
            execution_type: "agent-runtime".to_owned(),
            runtime: Some("stub".to_owned()),
            label: "Stub".to_owned(),
            command: None,
            ai: true,
            telemetry_collector_available: None,
            telemetry_runtime_prepared: None,
        },
        placement: Placement {
            provider: "tmux".to_owned(),
            node: "local".to_owned(),
            workdir: "/tmp".to_owned(),
            placement: json!({}),
        },
        lifecycle: SessionLifecycle::default(),
        observation: None,
        model: Some(json!(["Stub", "model"])),
        spawned_by: None,
        turn: None,
        last_entry_at: None,
        outcome: Outcome::Running,
    }
}

#[test]
fn a_cursor_that_names_no_entry_is_refused() {
    let records = (1..=3)
        .map(|value| {
            record(
                &format!("{value:03}"),
                "response",
                &format!("2026-09-15T01:00:0{value}Z"),
            )
        })
        .collect::<Vec<_>>();
    let source = TranscriptSource::new(Some(StubAdapter::readable(records)), Some(identity()));
    for window in [
        Window::After {
            after: Some("garbage".to_owned()),
            limit: 10,
        },
        Window::Before {
            before: "garbage".to_owned(),
            limit: 10,
        },
    ] {
        let refused = read_transcript_view(&source, "session", &Stream::Parent, window, false)
            .expect_err("an unknown cursor is refused");
        assert_eq!(refused.0, "garbage");
    }
}

#[test]
fn pages_walk_the_projected_records_without_gaps() {
    let records = (1..=7)
        .map(|value| {
            record(
                &format!("{value:03}"),
                "response",
                &format!("2026-09-15T01:00:0{value}Z"),
            )
        })
        .collect::<Vec<_>>();
    let source = TranscriptSource::new(Some(StubAdapter::readable(records)), Some(identity()));
    let tail = read_transcript(
        &source,
        "session",
        &Stream::Parent,
        Window::Tail { limit: 3 },
    );
    assert_eq!(cursors(&tail), vec!["005", "006", "007"]);
    assert_eq!(tail.previous_before.as_deref(), Some("005"));
    assert!(tail.has_older);
    let older = read_transcript(
        &source,
        "session",
        &Stream::Parent,
        Window::Before {
            before: "005".to_owned(),
            limit: 3,
        },
    );
    assert_eq!(cursors(&older), vec!["002", "003", "004"]);
    assert!(older.has_older);
    assert_eq!(older.previous_before.as_deref(), Some("002"));
    let forward = read_transcript(
        &source,
        "session",
        &Stream::Parent,
        Window::After {
            after: Some("004".to_owned()),
            limit: 3,
        },
    );
    assert_eq!(cursors(&forward), vec!["005", "006", "007"]);
    assert_eq!(forward.next_after.as_deref(), Some("007"));
    assert!(!forward.has_older);
    let none = read_transcript(
        &source,
        "session",
        &Stream::Parent,
        Window::Tail { limit: 0 },
    );
    assert!(none.records.is_empty());
    assert_eq!(none.next_after.as_deref(), Some("007"));
    let none = read_transcript(
        &source,
        "session",
        &Stream::Parent,
        Window::After {
            after: Some("003".to_owned()),
            limit: 0,
        },
    );
    assert!(none.records.is_empty());
    assert_eq!(none.next_after.as_deref(), Some("003"));
}

#[test]
fn each_stream_reads_only_its_own_records() {
    let mut child = record("003", "response", "2026-09-15T01:00:03Z");
    child["subAgent"] = json!("child-7");
    let mut ended = record("004", "tool-result", "2026-09-15T01:00:04Z");
    ended["subAgentEnded"] = json!("child-7");
    let records = vec![
        record("001", "input", "2026-09-15T01:00:01Z"),
        record("002", "tool-call", "2026-09-15T01:00:02Z"),
        child,
        ended,
    ];
    let source = TranscriptSource::new(Some(StubAdapter::readable(records)), Some(identity()));
    let parent = read_transcript(
        &source,
        "session",
        &Stream::Parent,
        Window::Tail { limit: 100 },
    );
    assert_eq!(cursors(&parent), vec!["001", "002", "004"]);
    let sub = read_transcript(
        &source,
        "session",
        &Stream::SubAgent("child-7".to_owned()),
        Window::Tail { limit: 100 },
    );
    assert_eq!(cursors(&sub), vec!["003"]);
}

#[test]
fn sub_agent_sessions_come_from_sub_agent_and_sub_agent_ended() {
    let mut open = record("005", "response", "2026-09-15T01:00:05Z");
    open["subAgent"] = json!("open-6");
    let mut first = record("003", "input", "2026-09-15T01:00:03Z");
    first["subAgent"] = json!("child-7");
    let mut second = record("006", "response", "2026-09-15T01:00:06Z");
    second["subAgent"] = json!("child-7");
    let mut ended = record("007", "tool-result", "2026-09-15T01:00:07Z");
    ended["subAgentEnded"] = json!("child-7");
    let source = TranscriptSource::new(
        Some(StubAdapter::readable(vec![first, open, second, ended])),
        Some(identity()),
    );
    let mut parent = session();
    let projected = project_sub_agents(&parent, &source).unwrap();
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].sub_agent, "child-7");
    assert_eq!(projected[0].id, "session_one_subagent_child-7");
    assert_eq!(projected[0].parent_session, "session_one");
    assert_eq!(projected[0].start.as_deref(), Some("2026-09-15T01:00:03Z"));
    assert_eq!(projected[0].end.as_deref(), Some("2026-09-15T01:00:07Z"));
    assert_eq!(projected[0].outcome, Outcome::Ended);
    assert_eq!(projected[0].start_kind, "sub-agent");
    assert!(projected[0].placement.is_none());
    // A Sub Agent the parent never reported finished runs until the parent ends.
    assert_eq!(projected[1].sub_agent, "open-6");
    assert_eq!(projected[1].end, None);
    assert_eq!(projected[1].outcome, Outcome::Running);
    parent.end = Some("2026-09-15T02:00:00Z".to_owned());
    let closed = project_sub_agents(&parent, &source).unwrap();
    assert_eq!(closed[1].end.as_deref(), Some("2026-09-15T02:00:00Z"));
}

/// A Sub Agent whose own entries the Adapter cannot read is still a Sub Agent Session:
/// its Console says it is lost and why, while the Session's own entries stay readable.
#[test]
fn a_lost_sub_agent_keeps_its_session_and_says_why() {
    let mut lost = record("002", "other", "");
    lost["at"] = Value::Null;
    lost["subAgent"] = json!("child-7");
    lost["state"] = json!("lost");
    lost["text"] = json!("subagent transcript file does not exist");
    let source = TranscriptSource::new(
        Some(StubAdapter::readable(vec![
            lost,
            record("003", "response", "2026-09-15T01:00:03Z"),
        ])),
        Some(identity()),
    );
    let parent = session();
    let projected = project_sub_agents(&parent, &source).unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].sub_agent, "child-7");
    assert_eq!(projected[0].console_state, ConsoleState::Lost);
    assert_eq!(
        projected[0].console_detail.as_deref(),
        Some("subagent transcript file does not exist")
    );
    assert_eq!(projected[0].start, None);
    assert_eq!(projected[0].outcome, Outcome::Unknown);
    let console = read_transcript(
        &source,
        "session",
        &Stream::SubAgent("child-7".to_owned()),
        Window::Tail { limit: 100 },
    );
    assert_eq!(console.state, ConsoleState::Lost);
    assert_eq!(
        console.detail.as_deref(),
        Some("subagent transcript file does not exist")
    );
    assert!(console.records.is_empty());
    // The Session's own Console is unaffected.
    let own = read_transcript(
        &source,
        "session",
        &Stream::Parent,
        Window::Tail { limit: 100 },
    );
    assert_eq!(own.state, ConsoleState::Readable);
    assert_eq!(cursors(&own), vec!["003"]);
}

#[test]
fn a_tool_result_carries_whether_it_failed() {
    let mut failed = record("002", "tool-result", "2026-09-15T01:00:02Z");
    failed["error"] = json!(true);
    let source = TranscriptSource::new(
        Some(StubAdapter::readable(vec![
            record("001", "tool-call", "2026-09-15T01:00:01Z"),
            failed,
        ])),
        Some(identity()),
    );
    let page = read_transcript(
        &source,
        "session",
        &Stream::Parent,
        Window::Tail { limit: 9 },
    );
    assert!(!page.records[0].error);
    assert!(page.records[1].error);
    assert_eq!(page.records[1].state, EntryState::Readable);
}

#[test]
fn the_states_of_the_records_are_told_apart() {
    let pending = TranscriptSource::new(Some(StubAdapter::readable(Vec::new())), None);
    assert_eq!(
        read_transcript(
            &pending,
            "session",
            &Stream::Parent,
            Window::Tail { limit: 1 }
        )
        .state,
        ConsoleState::Pending
    );
    for (answered, expected) in [
        (TranscriptState::Missing, ConsoleState::Missing),
        (TranscriptState::Lost, ConsoleState::Lost),
    ] {
        let source = TranscriptSource::new(
            Some(StubAdapter::answering(TranscriptResponse {
                state: answered,
                reason: Some("said why".to_owned()),
                records: Vec::new(),
                next: None,
            })),
            Some(identity()),
        );
        let page = read_transcript(
            &source,
            "session",
            &Stream::Parent,
            Window::Tail { limit: 1 },
        );
        assert_eq!(page.state, expected);
        assert_eq!(page.detail.as_deref(), Some("said why"));
        assert!(page.records.is_empty());
    }
    let uninstalled = TranscriptSource::without_adapter(Some("gone"), Some(identity()));
    let page = read_transcript(
        &uninstalled,
        "session",
        &Stream::Parent,
        Window::Tail { limit: 1 },
    );
    assert_eq!(page.state, ConsoleState::Lost);
    assert!(page.detail.unwrap().contains("'gone' is not installed"));
}

#[test]
fn the_turn_is_read_from_the_last_entry_that_carries_one() {
    let turn_of = |records: Vec<Value>| {
        read_turn(&TranscriptSource::new(
            Some(StubAdapter::readable(records)),
            Some(identity()),
        ))
    };
    let at = |at: &str| Some(at.to_owned());
    assert_eq!(
        turn_of(vec![
            record("001", "input", "2026-09-15T01:00:01Z"),
            record("002", "response", "2026-09-15T01:00:02Z"),
            record("003", "system", "2026-09-15T01:00:03Z"),
            record("004", "something-new", "2026-09-15T01:00:04Z"),
        ]),
        (Turn::WaitingInput, at("2026-09-15T01:00:02Z"))
    );
    for kind in [
        "input",
        "notification",
        "thinking",
        "tool-call",
        "tool-result",
    ] {
        assert_eq!(
            turn_of(vec![
                record("001", "response", "2026-09-15T01:00:01Z"),
                record("002", kind, "2026-09-15T01:00:02Z"),
            ]),
            (Turn::Responding, at("2026-09-15T01:00:02Z")),
            "{kind}"
        );
    }
    // A Sub Agent's entries are its own, not the Session's.
    let mut child = record("002", "tool-call", "2026-09-15T01:00:02Z");
    child["subAgent"] = json!("child-7");
    assert_eq!(
        turn_of(vec![
            record("001", "response", "2026-09-15T01:00:01Z"),
            child
        ]),
        (Turn::WaitingInput, at("2026-09-15T01:00:01Z"))
    );
    assert_eq!(
        turn_of(vec![record("001", "system", "2026-09-15T01:00:01Z")]),
        (Turn::Unavailable, None)
    );
    let missing = TranscriptSource::new(
        Some(StubAdapter::answering(TranscriptResponse {
            state: TranscriptState::Missing,
            reason: None,
            records: Vec::new(),
            next: None,
        })),
        Some(identity()),
    );
    assert_eq!(read_turn(&missing), (Turn::Unavailable, None));
    let pending = TranscriptSource::new(Some(StubAdapter::readable(Vec::new())), None);
    assert_eq!(read_turn(&pending), (Turn::Unavailable, None));
}

#[test]
fn a_kind_this_feld_does_not_know_reads_as_other() {
    let records = vec![record("001", "hallucinated", "2026-09-15T01:00:01Z")];
    let source = TranscriptSource::new(Some(StubAdapter::readable(records)), Some(identity()));
    let page = read_transcript(
        &source,
        "session",
        &Stream::Parent,
        Window::Tail { limit: 9 },
    );
    assert_eq!(page.records[0].kind, EntryKind::Other);
    assert!(!page.records[0].kind.carries_turn());
    assert!(EntryKind::Response.carries_turn());
    assert!(!EntryKind::System.carries_turn());
}

#[test]
fn a_sub_agent_session_id_splits_back_into_its_parts() {
    let id = sub_agent_session_id("session_0aa4d9", "child-7");
    assert_eq!(
        split_sub_agent_session_id(&id),
        Some(("session_0aa4d9", "child-7"))
    );
    assert_eq!(split_sub_agent_session_id("session_0aa4d9"), None);
}

#[test]
fn redaction_visits_unknown_nested_records() {
    let mut value = json!({"unknown": ["before-secret-after"]});
    redact_json(&mut value, &Secrets::from_iter(["secret"]));
    assert_eq!(value["unknown"][0], "before-[REDACTED]-after");
}

#[test]
fn redaction_keeps_values_that_are_not_tokens() {
    let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let mut text = format!("commit {digest} please check");
    redact_text(&mut text, &Secrets::from_iter(["secret"]));
    assert_eq!(text, format!("commit {digest} please check"));
}

#[test]
fn redaction_rebuilds_a_message_preview_from_the_redacted_body() {
    let token = "0123456789abcdef".repeat(4);
    let body = format!("tok={token} end");
    let mut value = json!({"body": body, "preview": message_preview(&body)});
    redact_json(&mut value, &Secrets::from_iter([&token]));
    assert_eq!(value["body"], "tok=[REDACTED] end");
    assert_eq!(value["preview"], "tok=[REDACTED] end");
}

#[test]
fn redaction_finds_tokens_inside_runs_of_hexadecimal_digits() {
    let first = fresh_token();
    let second = fresh_token();
    let secrets = Secrets::from_iter([&first, &second]);
    let mut text = format!("abc{first}{second}def {first}");
    redact_text(&mut text, &secrets);
    assert_eq!(text, "abc[REDACTED][REDACTED]def [REDACTED]");
}

#[test]
fn redaction_finds_tokens_in_upper_case() {
    let token = fresh_token();
    let secrets = Secrets::from_iter([&token]);
    let mixed: String = token
        .chars()
        .enumerate()
        .map(|(index, digit)| {
            if index % 2 == 0 {
                digit.to_ascii_uppercase()
            } else {
                digit
            }
        })
        .collect();
    let mut text = format!("{} {mixed}", token.to_ascii_uppercase());
    redact_text(&mut text, &secrets);
    assert_eq!(text, "[REDACTED] [REDACTED]");
}

#[test]
fn redaction_takes_out_tokens_in_object_keys() {
    let token = fresh_token();
    let mut value = json!({"env": {"before": 1, (format!("TOKEN_{token}")): token, "after": 2}});
    redact_json(&mut value, &Secrets::from_iter([&token]));
    assert_eq!(
        value,
        json!({"env": {"before": 1, "TOKEN_[REDACTED]": "[REDACTED]", "after": 2}})
    );
    let keys: Vec<&String> = value["env"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["before", "TOKEN_[REDACTED]", "after"]);
}

#[test]
fn redaction_keeps_a_token_it_never_issued() {
    let secrets = Secrets::from_iter([fresh_token()]);
    let other = fresh_token();
    let mut text = format!("{other}0");
    redact_text(&mut text, &secrets);
    assert_eq!(text, format!("{other}0"));
}

#[test]
fn redaction_takes_out_a_secret_of_another_form() {
    let secrets = Secrets::from_iter(["Local-Token", ""]);
    let mut text = "a Local-Token here".to_owned();
    redact_text(&mut text, &secrets);
    assert_eq!(text, "a [REDACTED] here");
}

#[test]
fn redaction_time_does_not_follow_the_number_of_tokens() {
    let few: Vec<String> = (0..10).map(|_| fresh_token()).collect();
    let many: Vec<String> = few
        .iter()
        .cloned()
        .chain((0..100_000).map(|_| fresh_token()))
        .collect();
    let text = format!(
        "{} {} {}",
        "0123456789abcdef".repeat(64),
        few[0],
        "plain text ".repeat(100)
    );
    let fastest = |secrets: &Secrets| {
        (0..20)
            .map(|_| {
                let mut copy = text.clone();
                let started = std::time::Instant::now();
                redact_text(&mut copy, secrets);
                started.elapsed()
            })
            .min()
            .unwrap()
    };
    let few = Secrets::from_iter(&few);
    let many = Secrets::from_iter(&many);
    let (few, many) = (fastest(&few), fastest(&many));
    assert!(
        many < few * 10,
        "10 tokens: {few:?}, 100000 tokens: {many:?}"
    );
}

#[test]
fn program_output_pages_advance_without_gaps() {
    let directory = TempDir::new().unwrap();
    let output = directory.path().join("output.jsonl");
    fs::write(
        &output,
        (1..=7)
            .map(|sequence| {
                format!(
                    "{{\"sequence\":{sequence},\"stream\":\"stdout\",\"timestamp\":\"2026-09-16T00:00:00Z\",\"text\":\"line {sequence}\"}}\n"
                )
            })
            .collect::<String>(),
    )
    .unwrap();
    let tail = read_output_tail("session", &output, 3).unwrap();
    assert_eq!(tail.next_after, 7);
    assert_eq!(tail.previous_before, Some(5));
    assert!(tail.has_older);
    let older = read_output_before("session", &output, 5, 3).unwrap();
    assert_eq!(older.previous_before, Some(2));
    assert!(older.has_older);
    assert_eq!(
        older
            .lines
            .iter()
            .map(|line| line.sequence)
            .collect::<Vec<_>>(),
        vec![2, 3, 4]
    );
    assert_eq!(
        tail.lines
            .into_iter()
            .map(|line| line.sequence)
            .collect::<Vec<_>>(),
        vec![5, 6, 7]
    );
    let none = read_output_tail("session", &output, 0).unwrap();
    assert!(none.lines.is_empty());
    assert_eq!(none.next_after, 7);
    let none = read_output_limited("session", &output, 2, 0).unwrap();
    assert!(none.lines.is_empty());
    assert_eq!(none.next_after, 2);
}
