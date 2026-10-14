use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use chrono::{DateTime, Duration as ChronoDuration, FixedOffset};
use feld_daemon::store::{AcceptedLifecycleAction, LifecycleActionKind, Store};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use tempfile::TempDir;

const FELD: &str = env!("CARGO_BIN_EXE_feld");

mod support;

struct Fixture {
    temporary: TempDir,
    socket: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().expect("create lifecycle test directory");
        let socket = temporary.path().join("tmux.sock");
        support::install_plugins(temporary.path());
        Self { temporary, socket }
    }

    fn home(&self) -> &Path {
        self.temporary.path()
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = support::feld();
        // The fixture owns HOME too, so nothing this runs reads or writes the real one.
        command
            .arg("--home")
            .arg(self.home())
            .env("HOME", self.home())
            .env_remove("TMUX")
            .args(arguments);
        command
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().expect("run feld")
    }

    fn json(&self, arguments: &[&str]) -> Value {
        let output = self.feld(arguments);
        assert!(
            output.status.success(),
            "feld {} failed:\nstdout: {}\nstderr: {}\ndaemon log: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            fs::read_to_string(self.home().join("daemon.log")).unwrap_or_default()
        );
        serde_json::from_slice(&output.stdout).expect("decode feld JSON output")
    }

    fn start_daemon(&self) -> Value {
        self.json(&["daemon", "start", "--json"])
    }

    fn create(&self, actor: &str, command: &[&str]) -> Value {
        let socket = self.socket.to_string_lossy();
        let mut arguments = vec![
            "actor",
            "create",
            actor,
            "--kind",
            "job",
            "--tmux-socket",
            socket.as_ref(),
            "--json",
            "--",
        ];
        arguments.extend(command);
        self.json(&arguments)
    }

    fn create_and_start(&self, actor: &str, command: &[&str]) -> Value {
        self.create_and_start_with(actor, &[], command)
    }

    /// Creates and starts a Job whose Session starts other Actors, which `control` allows.
    fn create_and_start_controlling(&self, actor: &str, command: &[&str]) -> Value {
        self.create_and_start_with(actor, &["--grant", "control"], command)
    }

    fn create_and_start_with(&self, actor: &str, options: &[&str], command: &[&str]) -> Value {
        let socket = self.socket.to_string_lossy();
        let mut arguments = vec![
            "actor",
            "create",
            actor,
            "--kind",
            "job",
            "--start",
            "--tmux-socket",
            socket.as_ref(),
        ];
        arguments.extend(options);
        arguments.extend(["--json", "--"]);
        arguments.extend(command);
        self.json(&arguments)
    }

    fn kill_tmux_server(&self) {
        let _ = Command::new("tmux")
            .arg("-S")
            .arg(&self.socket)
            .arg("kill-server")
            .output();
    }

    fn actor(&self, actor: &str) -> Value {
        self.json(&["actor", "get", actor, "--json"])
    }

    /// The Actor after every current Session of the Workspace is observed on its Provider.
    fn refreshed_actor(&self, actor: &str) -> Value {
        self.json(&["actor", "list", "--refresh", "--json"]);
        self.actor(actor)
    }

    fn wait_for_outcome(&self, actor: &str, outcome: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let value = self.refreshed_actor(actor);
            if value["actor"]["latestSession"]["outcome"] == outcome {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "Actor {actor} did not reach outcome {outcome}: {value}"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
        self.kill_tmux_server();
    }
}

#[test]
fn a_new_daemon_redacts_the_previous_local_token_from_output_and_describe() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    let previous_token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let command = format!("printf '%s\\n' {previous_token}");
    let launched = fixture.create_and_start("token-history", &["sh", "-c", &command]);
    let session = launched["session"]["id"].as_str().unwrap().to_owned();
    fixture.wait_for_outcome("token-history", "succeeded");

    // A restart keeps the local token, and a stop followed by a start issues a new one.
    fixture.json(&["daemon", "stop", "--json"]);
    let restarted = fixture.json(&["daemon", "start", "--json"]);
    let api = restarted["daemon"]["api"].as_str().unwrap();
    let current_token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    assert_ne!(current_token, previous_token);
    let client = Client::new();
    let output: Value = client
        .get(format!("{api}/v1/sessions/{session}/output"))
        .bearer_auth(current_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(output["lines"][0]["text"], "[REDACTED]");
    let described: Value = client
        .get(format!("{api}/v1/sessions/{session}"))
        .bearer_auth(current_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        described["session"]["execution"]["command"][2],
        "printf '%s\\n' [REDACTED]"
    );
}

#[test]
fn natural_program_transition_publishes_exact_session_observation_and_console_events() {
    let fixture = Fixture::new();
    let daemon = fixture.start_daemon();
    fixture.create("evented", &["/bin/sh", "-c", "echo event-output; sleep 1"]);
    let api = daemon["daemon"]["api"].as_str().unwrap().to_owned();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let event_api = api.clone();
    let reader = thread::spawn(move || {
        let response = Client::builder()
            .timeout(Duration::from_secs(12))
            .build()
            .unwrap()
            .get(format!("{event_api}/v1/events?workspace=default"))
            .bearer_auth(token.trim())
            .send()
            .unwrap()
            .error_for_status()
            .unwrap();
        let mut response = BufReader::new(response);
        loop {
            let mut line = String::new();
            if response.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if let Some(data) = line.strip_prefix("data:") {
                let event: Value = serde_json::from_str(data.trim()).unwrap();
                if events_tx.send(event.clone()).is_err() || event["id"] == "shutdown" {
                    break;
                }
            }
        }
    });
    assert_eq!(
        events_rx.recv_timeout(Duration::from_secs(2)).unwrap()["resource"],
        "daemon"
    );

    let started = fixture.json(&["actor", "start", "evented", "--json"]);
    let session = started["session"]["id"].as_str().unwrap().to_owned();
    fixture.wait_for_outcome("evented", "succeeded");
    let mut saw_session = false;
    let mut saw_record = false;
    let mut saw_log = false;
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline && !(saw_session && saw_record && saw_log) {
        let event = events_rx
            .recv_timeout(Duration::from_millis(500))
            .unwrap_or_else(|_| json!({}));
        if event["id"] != session {
            continue;
        }
        saw_session |= event["resource"] == "session";
        saw_record |= event["resource"] == "record";
        saw_log |= event["resource"] == "log";
    }
    assert!(saw_session && saw_record && saw_log);
    let output: Value = Client::new()
        .get(format!("{api}/v1/sessions/{session}/output"))
        .bearer_auth(
            fs::read_to_string(fixture.home().join("admin/api-token"))
                .unwrap()
                .trim(),
        )
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert!(output["lines"].as_array().unwrap().iter().any(|line| {
        line["text"]
            .as_str()
            .is_some_and(|text| text.contains("event-output"))
    }));
    fixture.json(&["daemon", "stop", "--json"]);
    reader.join().unwrap();
}

#[test]
fn output_written_before_the_first_observation_publishes_a_console_event() {
    let fixture = Fixture::new();
    let daemon = fixture.start_daemon();
    let api = daemon["daemon"]["api"].as_str().unwrap().to_owned();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let (events_tx, events_rx) = std::sync::mpsc::channel();
    let reader = thread::spawn(move || {
        let response = Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap()
            .get(format!("{api}/v1/events?workspace=default"))
            .bearer_auth(token.trim())
            .send()
            .unwrap()
            .error_for_status()
            .unwrap();
        let mut response = BufReader::new(response);
        loop {
            let mut line = String::new();
            if response.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if let Some(data) = line.strip_prefix("data:") {
                let event: Value = serde_json::from_str(data.trim()).unwrap();
                if events_tx.send(event.clone()).is_err() || event["id"] == "shutdown" {
                    break;
                }
            }
        }
    });
    assert_eq!(
        events_rx.recv_timeout(Duration::from_secs(2)).unwrap()["resource"],
        "daemon"
    );

    // Each Job writes all of its output at once, before the first observation
    // poll reaches it, and stays silent afterwards, so its one log event can only
    // come from that output.
    let mut sessions = Vec::new();
    for index in 0..5 {
        let started = fixture.create_and_start(
            &format!("burst-{index}"),
            &["sh", "-c", "echo burst; sleep 60"],
        );
        sessions.push(started["session"]["id"].as_str().unwrap().to_owned());
    }
    let mut logs: HashMap<String, usize> = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline
        && sessions
            .iter()
            .any(|session| logs.get(session).copied().unwrap_or(0) < 1)
    {
        let Ok(event) = events_rx.recv_timeout(Duration::from_millis(500)) else {
            continue;
        };
        if event["resource"] == "log"
            && let Some(id) = event["id"].as_str()
        {
            *logs.entry(id.to_owned()).or_default() += 1;
        }
    }
    for session in &sessions {
        assert!(
            logs.get(session).copied().unwrap_or(0) >= 1,
            "Session {session} published no log event for its first output: {logs:?}"
        );
    }
    fixture.json(&["daemon", "stop", "--json"]);
    reader.join().unwrap();
}

#[test]
fn every_console_response_redacts_tokens_for_session_callers() {
    let fixture = Fixture::new();
    let started = fixture.start_daemon();
    let old_local = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let internal = "collector-secret-that-must-survive-restart-and-be-redacted";
    let command = format!("sleep 30 # {old_local}");
    let launched = fixture.create_and_start("response-secrets", &["sh", "-c", &command]);
    assert!(!launched.to_string().contains(old_local.trim()));
    let session = launched["session"]["id"].as_str().unwrap().to_owned();
    let connection = rusqlite::Connection::open(fixture.home().join("admin/feld.db")).unwrap();
    let session_token: String = connection
        .query_row(
            "SELECT token FROM sessions WHERE id = ?1",
            [&session],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO token_secrets (kind, secret, created_at)
             VALUES ('collector-internal', ?1, datetime('now'))",
            [internal],
        )
        .unwrap();
    let initial_api = started["daemon"]["api"].as_str().unwrap();
    let client = Client::new();
    client
        .post(format!(
            "{initial_api}/v1/workspaces/default/actors/response-secrets/messages"
        ))
        .bearer_auth(old_local.trim())
        .json(&json!({
            "body": format!("old={old_local} session={session_token} internal={internal}")
        }))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();

    let restarted = fixture.json(&["daemon", "restart", "--json"]);
    let api = restarted["daemon"]["api"].as_str().unwrap();
    let new_local = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    client
        .post(format!(
            "{api}/v1/workspaces/default/actors/response-secrets/messages"
        ))
        .bearer_auth(new_local.trim())
        .json(&json!({"body": format!("current={new_local}")}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    client
        .post(format!(
            "{api}/v1/workspaces/default/actors/response-secrets/messages"
        ))
        .bearer_auth(new_local.trim())
        .json(&json!({"body": format!("commit {digest} please check")}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();

    for path in [
        "/v1/workspaces/default/actors",
        "/v1/workspaces/default/actors/response-secrets",
        "/v1/sessions?workspace=default",
        "/v1/messages",
        "/v1/self",
        "/v1/self/messages",
        &format!("/v1/sessions/{session}"),
        &format!("/v1/records?session={session}"),
        "/v1/records?workspace=default",
    ] {
        let response = client
            .get(format!("{api}{path}"))
            .bearer_auth(&session_token)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .unwrap();
        for secret in [old_local.trim(), new_local.trim(), &session_token, internal] {
            assert!(
                !response.contains(secret),
                "{path} exposed a token: {response}"
            );
        }
        if path.ends_with("/messages") {
            // The list carries the preview alone; the mailbox carries the body.
            let unredacted = if path == "/v1/messages" {
                format!("commit {}", &digest[..33])
            } else {
                format!("commit {digest} please check")
            };
            assert!(
                response.contains(&unredacted),
                "{path} redacted a value that is not a token: {response}"
            );
            let messages: Value = serde_json::from_str(&response).unwrap();
            let previews = messages["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|message| message["preview"].as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(previews.len(), 3, "{path}: {response}");
            assert_eq!(
                messages["messages"][0].get("body").is_some(),
                path == "/v1/self/messages",
                "{path}: {response}"
            );
            for secret in [old_local.trim(), new_local.trim(), &session_token] {
                for part in secret.as_bytes().windows(8) {
                    let part = std::str::from_utf8(part).unwrap();
                    assert!(
                        previews.iter().all(|preview| !preview.contains(part)),
                        "{path} exposed part of a token in a preview: {previews:?}"
                    );
                }
            }
        }
    }

    let lifecycle = client
        .post(format!("{api}/v1/sessions/{session}/stop"))
        .bearer_auth(new_local.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .unwrap();
    for secret in [old_local.trim(), new_local.trim(), &session_token, internal] {
        assert!(!lifecycle.contains(secret));
    }
}

#[test]
fn telemetry_admission_is_internal_atomic_and_idempotent() {
    let fixture = Fixture::new();
    let started = fixture.start_daemon();
    let launched = fixture.create_and_start("telemetry", &["sleep", "30"]);
    let session = launched["session"]["id"].as_str().unwrap();
    let connection = rusqlite::Connection::open(fixture.home().join("admin/feld.db")).unwrap();
    let session_token: String = connection
        .query_row(
            "SELECT token FROM sessions WHERE id = ?1",
            [session],
            |row| row.get(0),
        )
        .unwrap();
    // Telemetry carries the Session's telemetry token, never the Session token.
    let token = feld_core::telemetry::telemetry_token(&session_token);
    let internal: String = connection
        .query_row(
            "SELECT secret FROM token_secrets WHERE kind = 'collector-internal'
             ORDER BY created_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let api = started["daemon"]["api"].as_str().unwrap();
    let local = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let client = Client::new();
    fixture.json(&["workspace", "create", "other", "--json"]);
    let (quota_tx, quota_rx) = std::sync::mpsc::channel();
    let event_api = api.to_owned();
    let event_token = local.clone();
    let quota_reader = thread::spawn(move || {
        let response = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .get(format!("{event_api}/v1/events?workspace=other"))
            .bearer_auth(event_token.trim())
            .send()
            .unwrap()
            .error_for_status()
            .unwrap();
        quota_tx.send(()).unwrap();
        let mut response = BufReader::new(response);
        let mut events = Vec::new();
        while events.len() < 2 {
            let mut line = String::new();
            response.read_line(&mut line).unwrap();
            if let Some(data) = line.strip_prefix("data:") {
                events.push(serde_json::from_str::<Value>(data.trim()).unwrap());
            }
        }
        events
    });
    quota_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    // The Collector checks a telemetry token with the internal token before it reads a body.
    let check_token = |bearer: &str, token: &str| {
        client
            .post(format!("{api}/v1/self/telemetry/token"))
            .bearer_auth(bearer)
            .json(&json!({"token": token}))
            .send()
            .unwrap()
            .status()
    };
    assert_eq!(
        check_token(&internal, &token),
        reqwest::StatusCode::NO_CONTENT
    );
    assert_eq!(
        check_token(local.trim(), &token),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        check_token(&internal, &session_token),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY
    );
    let envelope = json!({
        "requestId":"request-one",
        "token":token,
        "signal":"metrics",
        "contentType":"application/x-protobuf",
        "payload":base64::engine::general_purpose::STANDARD.encode([1_u8, 2, 3]),
    });
    let unauthorized = client
        .post(format!("{api}/v1/self/telemetry"))
        .bearer_auth(local.trim())
        .json(&envelope)
        .send()
        .unwrap();
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
    let first: Value = client
        .post(format!("{api}/v1/self/telemetry"))
        .bearer_auth(&internal)
        .json(&envelope)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(first["duplicate"], false);
    let quota_events = quota_reader.join().unwrap();
    assert_eq!(quota_events[1]["resource"], "quota");
    assert_eq!(quota_events[1]["id"], "request-one");
    assert!(
        !quota_events
            .iter()
            .any(|event| event["resource"] == "telemetry")
    );
    let duplicate: Value = client
        .post(format!("{api}/v1/self/telemetry"))
        .bearer_auth(&internal)
        .json(&envelope)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(duplicate["duplicate"], true);
    let mut conflict = envelope.clone();
    conflict["payload"] = Value::String(base64::engine::general_purpose::STANDARD.encode([4_u8]));
    assert_eq!(
        client
            .post(format!("{api}/v1/self/telemetry"))
            .bearer_auth(&internal)
            .json(&conflict)
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::CONFLICT
    );
    // The Session token itself attributes no telemetry, and the telemetry token opens no
    // part of the API: whoever reads either header learns nothing the other accepts.
    let mut raw = envelope.clone();
    raw["requestId"] = Value::String("request-raw".to_owned());
    raw["token"] = Value::String(session_token.clone());
    assert_eq!(
        client
            .post(format!("{api}/v1/self/telemetry"))
            .bearer_auth(&internal)
            .json(&raw)
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        client
            .get(format!("{api}/v1/self"))
            .bearer_auth(&token)
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    fixture.json(&["session", "stop", "telemetry", "--json"]);
    assert_eq!(
        check_token(&internal, &token),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut closed = envelope;
    closed["requestId"] = Value::String("request-closed".to_owned());
    assert_eq!(
        client
            .post(format!("{api}/v1/self/telemetry"))
            .bearer_auth(&internal)
            .json(&closed)
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[test]
fn collector_persists_accepted_telemetries_through_its_raw_size_limit() {
    const MAX_RAW: usize = 4 * 1024 * 1024;

    let fixture = Fixture::new();
    let started = fixture.start_daemon();
    let launched = fixture.create_and_start("telemetry-size", &["sleep", "30"]);
    let session = launched["session"]["id"].as_str().unwrap();
    let connection = rusqlite::Connection::open(fixture.home().join("admin/feld.db")).unwrap();
    let token: String = connection
        .query_row(
            "SELECT token FROM sessions WHERE id = ?1",
            [session],
            |row| row.get(0),
        )
        .unwrap();
    // Telemetry carries the Session's telemetry token, never the Session token.
    let token = feld_core::telemetry::telemetry_token(&token);
    let collector = started["daemon"]["otlp"].as_str().unwrap();
    let client = Client::new();

    for (expected_count, size) in [(1_i64, 2 * 1024 * 1024), (2, MAX_RAW)] {
        let response = client
            .post(format!("{collector}/v1/metrics"))
            .bearer_auth(&token)
            .header("content-type", "application/x-protobuf")
            .body(support::metrics_export_of_length(size))
            .send()
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let count: i64 = connection
                .query_row("SELECT COUNT(*) FROM telemetries", [], |row| row.get(0))
                .unwrap();
            if count == expected_count {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Collector acknowledged {size} bytes without persisting it"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    let rejected = client
        .post(format!("{collector}/v1/metrics"))
        .bearer_auth(&token)
        .header("content-type", "application/x-protobuf")
        .body(support::metrics_export_of_length(MAX_RAW + 1))
        .send()
        .unwrap();
    assert_eq!(rejected.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    thread::sleep(Duration::from_millis(100));
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM telemetries", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn record_list_filters_skips_by_their_own_timestamp() {
    let fixture = Fixture::new();
    let started = fixture.start_daemon();
    let launched = fixture.create_and_start("skip-range", &["sleep", "30"]);
    let session = launched["session"]["id"].as_str().unwrap();
    let connection = rusqlite::Connection::open(fixture.home().join("admin/feld.db")).unwrap();
    connection
        .execute(
            "INSERT INTO records (at, kind, actor_id, session_id, data)
             SELECT '2026-09-15T01:00:02.000Z', 'schedule.skipped', a.id, s.id,
                    json_object('planned', '2026-09-15T01:00:00Z', 'currentSessionId', s.id)
             FROM sessions s JOIN actors a ON a.id = s.actor_id WHERE s.id = ?1",
            [session],
        )
        .unwrap();
    let api = started["daemon"]["api"].as_str().unwrap();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let client = Client::new();
    let outside: Value = client
        .get(format!(
            "{api}/v1/records?kind=schedule.skipped&start=2026-09-15T01:00:03Z&end=2026-09-15T01:00:04Z"
        ))
        .bearer_auth(token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert!(outside["records"].as_array().unwrap().is_empty());
    let inside: Value = client
        .get(format!(
            "{api}/v1/records?kind=schedule.skipped&start=2026-09-15T01:00:02Z&end=2026-09-15T01:00:03Z"
        ))
        .bearer_auth(token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    let skipped = inside["records"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["sessionId"], session);
}

#[test]
fn record_list_follow_json_prints_one_record_per_line() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    let after = fixture.json(&["record", "list", "--json"])["nextAfter"].to_string();
    let printed = fixture.home().join("follow.jsonl");
    let mut follower = fixture
        .command(&["record", "list", "--follow", "--json", "--after", &after])
        .stdout(fs::File::create(&printed).unwrap())
        .spawn()
        .expect("start record list --follow");
    // Several polls pass with nothing new.
    thread::sleep(Duration::from_millis(2500));
    let waited = fs::read_to_string(&printed).unwrap();
    // One start commits several records, which one poll brings together.
    fixture.create_and_start("recorded", &["true"]);
    let expected = fixture.json(&["record", "list", "--after", &after, "--json"])["records"]
        .as_array()
        .unwrap()
        .len();
    assert!(expected > 1);
    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::read_to_string(&printed).unwrap().lines().count() < expected
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(50));
    }
    let _ = follower.kill();
    let _ = follower.wait();
    assert_eq!(waited, "");
    let printed = fs::read_to_string(&printed).unwrap();
    let records: Vec<Value> = printed
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(records.len() >= expected, "{printed}");
    let mut sequence = after.parse::<u64>().unwrap();
    for record in &records {
        assert!(record["sequence"].as_u64().unwrap() > sequence, "{printed}");
        sequence = record["sequence"].as_u64().unwrap();
        assert!(record["kind"].is_string(), "{printed}");
    }
}

#[test]
fn human_output_prints_the_facts_the_cli_spec_shows() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    let socket = fixture.socket.to_string_lossy().into_owned();

    let first = fixture.create_and_start("visible", &["sleep", "60"]);
    let first_session = first["session"]["id"].as_str().unwrap().to_owned();

    let stopped = fixture.feld(&["session", "stop", "visible"]);
    assert!(stopped.status.success());
    let stopped = String::from_utf8(stopped.stdout).unwrap();
    assert!(stopped.contains("  observed:     gone ("), "{stopped}");
    assert!(
        stopped.contains("  result:       stopped (exit_code "),
        "{stopped}"
    );

    let started = fixture.feld(&["actor", "start", "visible"]);
    assert!(started.status.success());
    let started = String::from_utf8(started.stdout).unwrap();
    assert!(
        started.contains(&format!("(replaces {first_session})")),
        "{started}"
    );

    let current = fixture.actor("visible")["actor"]["currentSession"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let described = fixture.feld(&["session", "get", &current]);
    let described = String::from_utf8(described.stdout).unwrap();
    assert!(
        described.contains(&format!(
            "  attach_command:   tmux -S {socket} attach-session -t '$"
        )),
        "{described}"
    );
    // The Placement line is the stable ID the Binding holds, the name tmux answers for it
    // now, and how the resource came to be, as the CLI spec shows it.
    let placement = described
        .lines()
        .find_map(|line| line.trim().strip_prefix("placement:"))
        .unwrap_or_default()
        .trim();
    let (stable_id, rest) = placement
        .strip_prefix("tmux · local · session $")
        .and_then(|rest| rest.split_once(' '))
        .unwrap_or_else(|| panic!("{described}"));
    assert!(stable_id.parse::<u32>().is_ok(), "{described}");
    assert!(
        rest.starts_with("(feld-default-visible-2, created) · "),
        "{described}"
    );
    let ended = fixture.feld(&["session", "get", &first_session]);
    let ended = String::from_utf8(ended.stdout).unwrap();
    assert!(!ended.contains("attach_command:"), "{ended}");

    fixture.json(&["message", "send", "visible", "left unread", "--json"]);
    let records = fixture.feld(&["record", "list", "--session", &current]);
    let records = String::from_utf8(records.stdout).unwrap();
    assert!(
        records
            .lines()
            .any(|line| line.contains("  message.accepted  ") && line.ends_with(" (waiting)")),
        "{records}"
    );

    fixture.json(&[
        "actor",
        "create",
        "paused-visible",
        "--kind",
        "job",
        "--start",
        "--cron",
        "*/20 * * * *",
        "--tmux-socket",
        socket.as_str(),
        "--json",
        "--",
        "sleep",
        "60",
    ]);
    let scheduled = fixture.actor("paused-visible")["actor"]["currentSession"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let paused = fixture.feld(&["actor", "pause", "paused-visible"]);
    assert!(paused.status.success());
    let paused = String::from_utf8(paused.stdout).unwrap();
    assert!(
        paused.contains(&format!(
            "  current:      {scheduled} (running, not touched)"
        )),
        "{paused}"
    );
}

#[test]
fn a_start_that_never_began_is_rejected_and_says_why() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    let socket = fixture.socket.to_string_lossy().into_owned();

    // A workdir that cannot hold an execution is refused before anything is created.
    let refused = fixture.feld(&[
        "actor",
        "create",
        "missing-dir",
        "--kind",
        "job",
        "--workdir",
        "/definitely/not/a/real/path",
        "--start",
        "--tmux-socket",
        socket.as_str(),
        "--",
        "pwd",
    ]);
    assert_eq!(refused.status.code(), Some(1));
    let message = String::from_utf8(refused.stderr).unwrap();
    assert!(
        message.contains("workdir /definitely/not/a/real/path does not exist"),
        "{message}"
    );
    let actors = fixture.json(&["actor", "list", "--json"]).to_string();
    assert!(!actors.contains("missing-dir"), "{actors}");

    // A workdir that disappears after the creation fails at the start instead, and the
    // start reports the committed declaration together with what the Provider said.
    let workdir = fixture.home().join("removed");
    fs::create_dir(&workdir).expect("create the workdir the Actor names");
    fixture.json(&[
        "actor",
        "create",
        "removed-dir",
        "--kind",
        "job",
        "--workdir",
        workdir.to_str().unwrap(),
        "--tmux-socket",
        socket.as_str(),
        "--json",
        "--",
        "pwd",
    ]);
    fs::remove_dir(&workdir).expect("remove the workdir before the start");

    let started = fixture.feld(&["actor", "start", "removed-dir"]);
    assert_eq!(started.status.code(), Some(1));
    let started = String::from_utf8(started.stdout).unwrap();
    assert!(
        started.contains("Start accepted and committed; Session failed to start."),
        "{started}"
    );
    assert!(
        started.contains("  result:       unknown (start failed)"),
        "{started}"
    );
    assert!(
        started.contains("  reason:       invalid tmux provider request: prepared workdir"),
        "{started}"
    );

    let session = fixture.actor("removed-dir")["actor"]["latestSession"].clone();
    let session_id = session["id"].as_str().unwrap();
    let described =
        String::from_utf8(fixture.feld(&["session", "get", session_id]).stdout).unwrap();
    assert!(
        described.contains("  start_settled:    - · start failed"),
        "{described}"
    );
    assert!(
        described.contains("prepared workdir must still be an existing absolute directory"),
        "{described}"
    );

    let records = String::from_utf8(
        fixture
            .feld(&["record", "list", "--actor", "removed-dir"])
            .stdout,
    )
    .unwrap();
    assert!(
        records
            .lines()
            .any(|line| line.contains("  observation") && line.contains("gone · invalid tmux")),
        "{records}"
    );

    assert!(session["lifecycle"]["startFailed"].is_string(), "{session}");
    assert!(session["lifecycle"]["startSettled"].is_null(), "{session}");
}

/// tmux exits 0 with nothing created when it cannot create the server's socket, so the
/// start fails with a reason instead of leaving a running Session behind.
#[test]
fn a_start_on_a_socket_tmux_cannot_create_fails() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    let socket = fixture.home().join("missing").join("sock");
    let socket = socket.to_string_lossy().into_owned();

    let created = fixture.feld(&[
        "actor",
        "create",
        "unsocketed",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        socket.as_str(),
        "--",
        "true",
    ]);
    assert_eq!(created.status.code(), Some(1));
    let created = String::from_utf8(created.stdout).unwrap();
    assert!(
        created.contains("Actor declaration committed; Session failed to start."),
        "{created}"
    );
    assert!(
        created.contains("  result:       unknown (start failed)"),
        "{created}"
    );
    // tmux 3.4 and later say why on stderr; tmux 3.2 says nothing, and Feld names the
    // socket tmux left missing.
    assert!(
        created.contains(&format!(
            "error creating {socket} (No such file or directory)"
        )) || created.contains(&format!(
            "tmux created nothing and left no socket at {socket}"
        )),
        "{created}"
    );

    let actor = fixture.actor("unsocketed")["actor"].clone();
    assert!(actor["currentSession"].is_null(), "{actor}");
    let session = &actor["latestSession"];
    assert!(session["lifecycle"]["startFailed"].is_string(), "{session}");
    assert!(session["lifecycle"]["startSettled"].is_null(), "{session}");
}

/// A launch that created its tmux session but could not put the command in it removes the
/// session again: the command never began, so the start fails the same way as one that
/// created nothing.
#[test]
fn a_start_whose_command_never_reached_the_created_pane_fails() {
    let fixture = Fixture::new();
    let real_tmux = Command::new("sh")
        .args(["-c", "command -v tmux"])
        .output()
        .expect("find tmux");
    let real_tmux = String::from_utf8(real_tmux.stdout).unwrap();
    let bin = fixture.home().join("bin");
    fs::create_dir(&bin).expect("create the tmux wrapper directory");
    let commands = fixture.home().join("tmux-commands");
    let wrapper = bin.join("tmux");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\nfor argument in \"$@\"; do\n  if [ \"$argument\" = respawn-pane ]; then\n    echo 'respawn pane failed: fork failed: Device not configured' >&2\n    exit 1\n  fi\ndone\nexec '{}' \"$@\"\n",
            commands.display(),
            real_tmux.trim()
        ),
    )
    .expect("write the tmux wrapper");
    let mut permissions = fs::metadata(&wrapper).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    fs::set_permissions(&wrapper, permissions).unwrap();
    // The daemon and the tmux Plugin it runs inherit this PATH.
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let started = fixture
        .command(&["daemon", "start", "--json"])
        .env("PATH", &path)
        .output()
        .expect("start the daemon");
    assert!(started.status.success(), "{started:?}");

    let socket = fixture.socket.to_string_lossy().into_owned();
    let created = fixture.feld(&[
        "actor",
        "create",
        "unrespawned",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        socket.as_str(),
        "--",
        "true",
    ]);
    assert_eq!(created.status.code(), Some(1));
    let created = String::from_utf8(created.stdout).unwrap();
    assert!(
        created.contains("  result:       unknown (start failed)"),
        "{created}"
    );
    assert!(
        created.contains("  reason:       tmux command failed (tmux): respawn pane failed"),
        "{created}"
    );
    assert!(
        created.contains("cleanup: the created tmux resource was removed"),
        "{created}"
    );
    // The Provider closed the resource of the failed respawn and tried once more in one it
    // created anew. tmux 3.2a to 3.7c crashes on any command but a kill that reaches the
    // pane of a failed respawn.
    let commands = fs::read_to_string(&commands).unwrap();
    let arguments = |command: &str| command.split(' ').map(str::to_owned).collect::<Vec<_>>();
    let lines = commands.lines().map(arguments).collect::<Vec<_>>();
    let respawned = lines
        .iter()
        .enumerate()
        .filter(|(_, command)| command.iter().any(|argument| argument == "respawn-pane"))
        .map(|(index, command)| {
            let target = command
                .iter()
                .position(|argument| argument == "-t")
                .unwrap();
            (index, command[target + 1].clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(respawned.len(), 2, "{commands}");
    for (index, pane) in &respawned {
        let kill = lines[index + 1..]
            .iter()
            .position(|command| command.iter().any(|argument| argument.starts_with("kill-")))
            .map(|offset| index + 1 + offset)
            .unwrap_or_else(|| panic!("{commands}"));
        assert!(
            lines[index + 1..kill]
                .iter()
                .all(|command| !command.contains(pane)),
            "{commands}"
        );
        assert!(
            respawned
                .iter()
                .all(|(other, _)| other <= index || *other > kill),
            "{commands}"
        );
    }

    let session = fixture.actor("unrespawned")["actor"]["latestSession"].clone();
    assert!(session["lifecycle"]["startFailed"].is_string(), "{session}");
    assert!(session["lifecycle"]["startSettled"].is_null(), "{session}");
    let session_id = session["id"].as_str().unwrap();
    let described =
        String::from_utf8(fixture.feld(&["session", "get", session_id]).stdout).unwrap();
    assert!(
        described.contains("  start_settled:    - · start failed"),
        "{described}"
    );

    let sessions = support::without_outside_variables(&mut Command::new(real_tmux.trim()))
        .arg("-S")
        .arg(&fixture.socket)
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
        .expect("list tmux sessions");
    let sessions = String::from_utf8(sessions.stdout).unwrap();
    assert!(!sessions.contains("unrespawned"), "{sessions}");
}

#[test]
fn program_jobs_report_exit_stop_kill_and_replacement_facts() {
    let fixture = Fixture::new();
    fixture.start_daemon();

    support::without_outside_variables(&mut Command::new("tmux"))
        .arg("-S")
        .arg(&fixture.socket)
        .args(["new-session", "-d", "-s", "feld-default-collision-1"])
        .status()
        .expect("create colliding tmux session");
    // The name is taken, so the Provider makes no resource and the start never begins.
    let socket = fixture.socket.to_string_lossy().into_owned();
    let refused = fixture.feld(&[
        "actor",
        "create",
        "collision",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        socket.as_str(),
        "--json",
        "--",
        "true",
    ]);
    assert_eq!(refused.status.code(), Some(1));
    let collision: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(collision["session"]["observation"]["kind"], "gone");
    assert_eq!(collision["session"]["endKind"], "exited");
    assert_eq!(collision["session"]["outcome"], "unknown");
    assert!(collision["session"]["lifecycle"]["startFailed"].is_string());

    let succeeded = fixture.create_and_start("succeeds", &["sh", "-c", "exit 0"]);
    assert_eq!(succeeded["actor"]["name"], "succeeds");
    assert!(
        succeeded["actor"]["id"]
            .as_str()
            .unwrap()
            .starts_with("actor_")
    );
    assert_eq!(succeeded["session"]["generation"], 1);
    assert_eq!(succeeded["session"]["startKind"], "start");
    // The start answers with what the Provider saw right after the launch, which on a loaded
    // machine can be the program still running, so the exit is read once it is observed.
    let succeeded = fixture.wait_for_outcome("succeeds", "succeeded");
    assert_eq!(succeeded["actor"]["latestSession"]["exitCode"], 0);

    fixture.create_and_start("fails", &["sh", "-c", "exit 23"]);
    let failed = fixture.wait_for_outcome("fails", "failed")["actor"]["latestSession"].clone();
    assert_eq!(failed["exitCode"], 23);

    let running = fixture.create_and_start("stoppable", &["sleep", "30"]);
    assert_eq!(running["session"]["outcome"], "running");
    let stopped_session = running["session"]["id"].as_str().unwrap().to_owned();
    let stopped = fixture.json(&["session", "stop", "stoppable", "--json"]);
    assert_eq!(stopped["session"]["outcome"], "stopped");
    assert_eq!(stopped["session"]["endKind"], "stopped");
    assert_eq!(
        stopped["session"]["lifecycle"]["stop"]["observed"]["kind"],
        "gone"
    );
    // A closed Session refuses stop, kill and observe alike, whether or not a stop closed it,
    // and its records stay as they were.
    let records_of = |session: &str| {
        fixture.json(&["record", "list", "--session", session, "--json"])["records"].clone()
    };
    let failed_session = failed["id"].as_str().unwrap();
    let before = [records_of(&stopped_session), records_of(failed_session)];
    for (session, command) in [
        (stopped_session.as_str(), "stop"),
        (stopped_session.as_str(), "kill"),
        (stopped_session.as_str(), "observe"),
        (failed_session, "stop"),
        (failed_session, "kill"),
        (failed_session, "observe"),
    ] {
        let refused = fixture.feld(&["session", command, session]);
        assert_eq!(refused.status.code(), Some(1), "{command} {session}");
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("is already closed"),
            "{command} {session}"
        );
    }
    assert_eq!(
        [records_of(&stopped_session), records_of(failed_session)],
        before
    );
    assert_eq!(
        fixture.json(&["session", "get", &stopped_session, "--json"])["session"]["observation"]["kind"],
        "gone"
    );

    let term_tail = fixture.create_and_start(
        "term-tail",
        &[
            "sh",
            "-c",
            "trap 'printf term-tail >&2; exit 0' TERM; while :; do sleep 0.05; done",
        ],
    );
    fixture.json(&["session", "stop", "term-tail", "--json"]);
    let term_path = fixture
        .home()
        .join("sessions")
        .join(term_tail["session"]["id"].as_str().unwrap())
        .join("console/output.jsonl");
    let deadline = Instant::now() + Duration::from_secs(3);
    let term_output = loop {
        let output = fs::read_to_string(&term_path).unwrap();
        if output.contains("term-tail") || Instant::now() >= deadline {
            break output;
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert!(term_output.contains("term-tail"));

    fixture.json(&["actor", "start", "stoppable", "--json"]);
    let killed = fixture.json(&["session", "kill", "stoppable", "--json"]);
    assert_eq!(killed["session"]["generation"], 2);
    assert_eq!(killed["session"]["startKind"], "replacement");
    assert_eq!(killed["session"]["outcome"], "stopped");
    assert_eq!(killed["session"]["exitCode"], 137);

    let described = fixture.actor("stoppable");
    assert_eq!(described["actor"]["sessions"].as_array().unwrap().len(), 2);
    let killed_session = killed["session"]["id"].as_str().unwrap();
    let records = fixture.json(&["record", "list", "--session", killed_session, "--json"]);
    assert!(
        records["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["kind"] == "kill.requested")
    );
    let after = records["records"][0]["sequence"]
        .as_u64()
        .unwrap()
        .to_string();
    let continued = fixture.json(&[
        "record",
        "list",
        "--session",
        killed_session,
        "--after",
        &after,
        "--json",
    ]);
    assert_eq!(continued["nextAfter"], records["nextAfter"]);
    assert_eq!(
        continued["records"].as_array().unwrap().len(),
        records["records"].as_array().unwrap().len() - 1
    );
    assert!(
        continued["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|record| record["sequence"].as_u64().unwrap() > after.parse().unwrap())
    );
}

#[test]
fn restart_stops_the_running_session_and_starts_the_next_generation() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    fixture.create_and_start("looping", &["sleep", "30"]);

    let restarted = fixture.json(&["actor", "restart", "looping", "--json"]);
    assert_eq!(restarted["stopped"]["generation"], 1);
    assert!(restarted["stopped"]["lifecycle"]["stop"].is_object());
    let session = &restarted["session"];
    assert_eq!(session["generation"], 2);
    assert_eq!(session["startKind"], "replacement");
    assert_eq!(session["ownership"], "owned");
    assert_eq!(session["observation"]["kind"], "running");
    assert_eq!(session["spawnedBy"]["spawner"]["type"], "admin");
    let stopped = fixture.json(&[
        "session",
        "get",
        restarted["stopped"]["id"].as_str().unwrap(),
        "--json",
    ]);
    assert_eq!(stopped["session"]["outcome"], "stopped");

    fixture.json(&["session", "kill", "looping", "--json"]);
    let rejected = fixture.feld(&["actor", "restart", "looping", "--json"]);
    assert_eq!(rejected.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&rejected.stderr).unwrap();
    assert_eq!(error["error"]["kind"], "rejected");
    assert_eq!(
        fixture.actor("looping")["actor"]["sessions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn restart_starts_the_next_generation_once_the_stopped_session_closes() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    let trapped = fixture.home().join("trapped");
    let program = format!("trap '' TERM; touch '{}'; sleep 300", trapped.display());
    fixture.create_and_start("stubborn", &["sh", "-c", &program]);
    // A stop that arrives before the trap ends the program, and the Session closes before the
    // kill below.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !trapped.exists() {
        assert!(Instant::now() < deadline, "the program never trapped TERM");
        thread::sleep(Duration::from_millis(25));
    }

    let restart = fixture
        .command(&["actor", "restart", "stubborn", "--json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let actor = fixture.actor("stubborn");
        if actor["actor"]["currentSession"]["lifecycle"]["stop"].is_object() {
            break;
        }
        assert!(Instant::now() < deadline, "stop was not accepted: {actor}");
        thread::sleep(Duration::from_millis(25));
    }
    // The stop was accepted but the program ignores it, so only the kill closes the Session.
    fixture.json(&["session", "kill", "stubborn", "--json"]);
    let killed = Instant::now();
    let output = restart.wait_with_output().unwrap();
    assert!(
        killed.elapsed() < Duration::from_secs(5),
        "restart waited {:?} after the kill",
        killed.elapsed()
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let restarted: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(restarted["stopped"]["generation"], 1);
    assert_eq!(restarted["session"]["generation"], 2);
    assert_eq!(restarted["session"]["startKind"], "replacement");
    assert_eq!(
        restarted["session"]["spawnedBy"]["spawner"]["type"],
        "admin"
    );
}

#[test]
fn concurrent_start_has_one_admission_and_session_tokens_identify_spawns() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    fixture.create("single", &["sleep", "30"]);

    let mut children = Vec::new();
    for _ in 0..8 {
        let mut command = fixture.command(&["actor", "start", "single", "--json"]);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        children.push(command.spawn().expect("spawn concurrent start"));
    }
    let outputs: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().expect("wait for concurrent start"))
        .collect();
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1
    );
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.status.code() == Some(1))
            .count(),
        7
    );
    for rejected in outputs
        .iter()
        .filter(|output| output.status.code() == Some(1))
    {
        let error: Value = serde_json::from_slice(&rejected.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "rejected");
    }
    let actor = fixture.actor("single");
    assert_eq!(actor["actor"]["sessions"].as_array().unwrap().len(), 1);
    fixture.json(&["session", "kill", "single", "--json"]);

    fixture.create("child", &["true"]);
    fixture.create_and_start_controlling("parent", &[FELD, "actor", "start", "child", "--json"]);
    let child = fixture.wait_for_outcome("child", "succeeded");
    let child_session = child["actor"]["latestSession"]["id"].as_str().unwrap();

    let status = fixture.json(&["daemon", "status", "--json"]);
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let spawns: Value = Client::new()
        .get(format!(
            "{}/v1/records?workspace=default&kind=spawned",
            status["api"].as_str().unwrap()
        ))
        .bearer_auth(token)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    let child_spawn = spawns["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|spawn| spawn["sessionId"] == child_session)
        .expect("child Spawn is recorded");
    assert_eq!(child_spawn["kind"], "spawned");
    assert_eq!(child_spawn["actor"]["name"], "child");
    assert_eq!(child_spawn["data"]["spawner"]["type"], "actor");
    assert_eq!(child_spawn["data"]["spawner"]["name"], "parent");
    assert_eq!(
        child_spawn["data"]["spawner"]["id"],
        fixture.actor("parent")["actor"]["id"]
    );
    assert!(child_spawn["data"]["spawnerSessionId"].is_string());

    // `--kind` narrows the list to one kind; `--session` to one Session's records.
    let all = fixture.json(&["record", "list", "--json"]);
    assert!(
        all["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["kind"] != "spawned")
    );
    let spawned = fixture.json(&["record", "list", "--kind", "spawned", "--json"]);
    let spawned = spawned["records"].as_array().unwrap();
    assert!(spawned.iter().all(|record| record["kind"] == "spawned"));
    assert!(
        spawned
            .iter()
            .any(|record| record["sessionId"] == child_session)
    );
    let child_records = fixture.json(&["record", "list", "--session", child_session, "--json"]);
    let child_records = child_records["records"].as_array().unwrap();
    assert!(
        child_records
            .iter()
            .all(|record| record["sessionId"] == child_session)
    );
    for kind in ["spawned", "session.started"] {
        assert!(
            child_records.iter().any(|record| record["kind"] == kind),
            "{child_records:?}"
        );
    }
    // The spawner's Session names the Spawn too.
    let parent_session = child_spawn["data"]["spawnerSessionId"].as_str().unwrap();
    let parent_records = fixture.json(&["record", "list", "--session", parent_session, "--json"]);
    assert!(
        parent_records["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["kind"] == "spawned" && record["sessionId"] == child_session),
        "{parent_records:?}"
    );

    let child_at = DateTime::parse_from_rfc3339(child_spawn["at"].as_str().unwrap()).unwrap();
    let offset = FixedOffset::east_opt(9 * 60 * 60).unwrap();
    let range_start = child_at.with_timezone(&offset).to_rfc3339();
    let range_end = (child_at + ChronoDuration::milliseconds(1))
        .with_timezone(&offset)
        .to_rfc3339();
    let offset_spawns: Value = Client::new()
        .get(format!("{}/v1/records", status["api"].as_str().unwrap()))
        .bearer_auth(fs::read_to_string(fixture.home().join("admin/api-token")).unwrap())
        .query(&[
            ("workspace", "default"),
            ("kind", "spawned"),
            ("start", range_start.as_str()),
            ("end", range_end.as_str()),
        ])
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(offset_spawns["records"].as_array().unwrap().len(), 1);
    assert_eq!(offset_spawns["records"][0]["sessionId"], child_session);
}

#[test]
fn program_output_survives_fast_exit_and_daemon_restart() {
    let fixture = Fixture::new();
    let mut status = fixture.start_daemon();
    let script = r#"import os,sys
print(os.environ['FELD_SESSION_TOKEN'], flush=True)
sys.stdout.write('\n'); sys.stdout.flush()
sys.stderr.write('crlf\r\n'); sys.stderr.flush()
for i in range(550):
 print(f'out-{i}', flush=True)
 print(f'err-{i}', file=sys.stderr, flush=True)
sys.stdout.write('x' * 70000)
sys.stdout.flush()"#;
    let started = fixture.create_and_start("recorded", &["python3", "-c", script]);
    let session = started["session"]["id"].as_str().unwrap();
    fixture.wait_for_outcome("recorded", "succeeded");

    let client = Client::new();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let page = loop {
        let page: Value = client
            .get(format!(
                "{}/v1/sessions/{session}/output",
                status["daemon"]["api"].as_str().unwrap()
            ))
            .bearer_auth(token.trim())
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap();
        if page["lines"].as_array().unwrap().len() == 1_104 {
            break page;
        }
        assert!(Instant::now() < deadline, "capture did not finish: {page}");
        thread::sleep(Duration::from_millis(20));
    };
    let lines = page["lines"].as_array().unwrap();
    assert_eq!(page["nextAfter"], 1_104);
    assert!(lines.iter().any(|line| line["text"] == "[REDACTED]"));
    assert!(lines.iter().any(|line| line["text"] == ""));
    assert!(lines.iter().any(|line| line["text"] == "crlf"));
    assert!(
        lines
            .iter()
            .any(|line| line["stream"] == "stderr" && line["text"] == "err-549")
    );
    assert!(lines.iter().any(|line| line["stream"] == "stdout" && line["text"].as_str().unwrap().len() == 70000));
    assert!(
        lines
            .iter()
            .enumerate()
            .all(|(index, line)| line["sequence"] == (index + 1) as u64)
    );
    let recent: Value = client
        .get(format!(
            "{}/v1/sessions/{session}/output?tail=true&limit=250",
            status["daemon"]["api"].as_str().unwrap()
        ))
        .bearer_auth(token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(recent["lines"][0]["sequence"], 855);
    assert_eq!(recent["lines"][249]["sequence"], 1_104);
    assert_eq!(recent["previousBefore"], 855);
    assert_eq!(recent["hasOlder"], true);
    let older: Value = client
        .get(format!(
            "{}/v1/sessions/{session}/output?before=855&limit=250",
            status["daemon"]["api"].as_str().unwrap()
        ))
        .bearer_auth(token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(older["lines"][0]["sequence"], 605);
    assert_eq!(older["lines"][249]["sequence"], 854);
    assert_eq!(older["previousBefore"], 605);
    assert_eq!(older["hasOlder"], true);

    fixture.json(&["daemon", "stop", "--json"]);
    status = fixture.start_daemon();
    let tail: Value = client
        .get(format!(
            "{}/v1/sessions/{session}/output?after=1103",
            status["daemon"]["api"].as_str().unwrap()
        ))
        .bearer_auth(
            fs::read_to_string(fixture.home().join("admin/api-token"))
                .unwrap()
                .trim(),
        )
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(tail["after"], 1_103);
    assert_eq!(tail["nextAfter"], 1_104);
    assert_eq!(tail["lines"].as_array().unwrap().len(), 1);
}

#[test]
fn daemon_restart_recovers_sessions_and_tmux_restart_allows_replacement() {
    let fixture = Fixture::new();
    let started = fixture.start_daemon();
    let api = started["daemon"]["api"].clone();
    let running = fixture.create_and_start("recoverable", &["sleep", "30"]);
    let first_session = running["session"]["id"].clone();
    fixture.create("token-child", &["true"]);
    fixture.create_and_start_controlling(
        "token-parent",
        &[
            "sh",
            "-c",
            "sleep 5; exec \"$1\" actor start token-child --json",
            "feld-session",
            FELD,
        ],
    );

    let restarted = fixture.json(&["daemon", "restart", "--json"]);
    assert_eq!(restarted["daemon"]["api"], api);
    assert_eq!(restarted["daemon"]["restored"]["runningSessions"], 2);
    let recovered = fixture.refreshed_actor("recoverable");
    assert_eq!(recovered["actor"]["currentSession"]["id"], first_session);
    assert_eq!(
        recovered["actor"]["currentSession"]["observation"]["kind"],
        "running"
    );
    fixture.wait_for_outcome("token-child", "succeeded");

    fixture.kill_tmux_server();
    let lost = fixture.refreshed_actor("recoverable");
    assert_eq!(
        lost["actor"]["currentSession"]["observation"]["kind"],
        "lost"
    );
    assert_eq!(lost["actor"]["attention"], true);

    let replacement = fixture.json(&["actor", "start", "recoverable", "--json"]);
    assert_eq!(replacement["session"]["generation"], 2);
    assert_eq!(replacement["session"]["startKind"], "replacement");
    let history = fixture.actor("recoverable");
    let previous = history["actor"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["id"] == first_session)
        .unwrap();
    assert_eq!(previous["endKind"], "unknown");
    assert_eq!(previous["outcome"], "unknown");
    fixture.json(&["session", "kill", "recoverable", "--json"]);
}

#[test]
fn borrowed_window_and_pane_units_record_stable_placements() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    support::without_outside_variables(&mut Command::new("tmux"))
        .arg("-S")
        .arg(&fixture.socket)
        .args(["new-session", "-d", "-s", "borrowed"])
        .status()
        .expect("create borrowed tmux session");

    let socket = fixture.socket.to_string_lossy();
    let window = fixture.json(&[
        "actor",
        "create",
        "window-unit",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        socket.as_ref(),
        "--tmux-unit",
        "window",
        "--tmux-session",
        "borrowed",
        "--json",
        "--",
        "sleep",
        "30",
    ]);
    assert_eq!(
        window["session"]["placement"]["placement"]["unit"],
        "window"
    );
    assert_eq!(
        window["session"]["placement"]["placement"]["sessionOrigin"],
        "existing"
    );
    assert!(window["session"]["placement"]["placement"]["windowId"].is_string());
    fixture.json(&["session", "kill", "window-unit", "--json"]);

    let pane = fixture.json(&[
        "actor",
        "create",
        "pane-unit",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        socket.as_ref(),
        "--tmux-unit",
        "pane",
        "--tmux-window",
        "borrowed:^",
        "--json",
        "--",
        "sleep",
        "30",
    ]);
    assert_eq!(pane["session"]["placement"]["placement"]["unit"], "pane");
    assert_eq!(
        pane["session"]["placement"]["placement"]["windowOrigin"],
        "existing"
    );
    assert!(pane["session"]["placement"]["placement"]["paneId"].is_string());
    fixture.json(&["session", "kill", "pane-unit", "--json"]);
}

/// The Placement keeps the stable ID; the name beside it is what tmux answered when the
/// Session was last read, so renaming the tmux session changes the display and nothing else.
#[test]
fn the_placement_display_follows_the_current_tmux_name() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    support::without_outside_variables(&mut Command::new("tmux"))
        .arg("-S")
        .arg(&fixture.socket)
        .args(["new-session", "-d", "-s", "before"])
        .status()
        .expect("create borrowed tmux session");

    let socket = fixture.socket.to_string_lossy().into_owned();
    let started = fixture.json(&[
        "actor",
        "create",
        "renamed",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        socket.as_str(),
        "--tmux-unit",
        "window",
        "--tmux-session",
        "before",
        "--json",
        "--",
        "sleep",
        "60",
    ]);
    let window = started["session"]["placement"]["placement"]["windowId"]
        .as_str()
        .unwrap()
        .to_owned();
    // The index tmux gave the window is its answer, not Feld's, so the test asks for it.
    let index = String::from_utf8(
        Command::new("tmux")
            .arg("-S")
            .arg(&fixture.socket)
            .args(["display-message", "-p", "-t", &window, "#{window_index}"])
            .output()
            .expect("read the window index")
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    assert_eq!(
        started["session"]["placement"]["placement"]["summary"],
        format!("window {window} (before:{index}, created)")
    );

    assert!(
        Command::new("tmux")
            .arg("-S")
            .arg(&fixture.socket)
            .args(["rename-session", "-t", "before", "after"])
            .status()
            .expect("rename the borrowed tmux session")
            .success()
    );
    let refreshed = fixture.refreshed_actor("renamed");
    let placement = &refreshed["actor"]["currentSession"]["placement"]["placement"];
    // The record is the stable ID; only the name beside it moved.
    assert_eq!(placement["windowId"], window);
    assert_eq!(
        placement["summary"],
        format!("window {window} (after:{index}, created)")
    );
    fixture.json(&["session", "kill", "renamed", "--json"]);
}

#[test]
fn daemon_recovery_executes_committed_pending_stop_and_kill_actions() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    let stopping = fixture.create_and_start("pending-stop", &["sleep", "30"]);
    let killing = fixture.create_and_start("pending-kill", &["sleep", "30"]);
    let stopping_id = stopping["session"]["id"].as_str().unwrap().to_owned();
    let killing_id = killing["session"]["id"].as_str().unwrap().to_owned();
    fixture.json(&["daemon", "stop", "--json"]);

    let store = Store::open(&fixture.home().join("admin/feld.db")).unwrap();
    assert!(matches!(
        store
            .accept_lifecycle_action(&stopping_id, LifecycleActionKind::Stop, None)
            .unwrap(),
        AcceptedLifecycleAction::NeedsProvider(_)
    ));
    assert!(matches!(
        store
            .accept_lifecycle_action(&killing_id, LifecycleActionKind::Kill, None)
            .unwrap(),
        AcceptedLifecycleAction::NeedsProvider(_)
    ));
    let pending_stop = store.get_session(&stopping_id).unwrap().unwrap();
    let pending_kill = store.get_session(&killing_id).unwrap().unwrap();
    assert!(pending_stop.lifecycle.stop.unwrap().attempt.is_none());
    assert!(pending_kill.lifecycle.kill.unwrap().observed.is_none());
    drop(store);

    fixture.start_daemon();
    let stopped = fixture.json(&["session", "get", &stopping_id, "--json"]);
    let killed = fixture.json(&["session", "get", &killing_id, "--json"]);
    let seen = format!(
        "stop: {stopped}\nkill: {killed}\nstop records: {}\nkill records: {}\ndaemon log: {}",
        String::from_utf8_lossy(
            &fixture
                .feld(&["record", "list", "--session", &stopping_id])
                .stdout
        ),
        String::from_utf8_lossy(
            &fixture
                .feld(&["record", "list", "--session", &killing_id])
                .stdout
        ),
        fs::read_to_string(fixture.home().join("daemon.log")).unwrap_or_default()
    );
    assert_eq!(stopped["session"]["outcome"], "stopped", "{seen}");
    assert_eq!(stopped["session"]["exitCode"], 143, "{seen}");
    assert!(
        stopped["session"]["lifecycle"]["stop"]["attempt"]
            .as_str()
            .unwrap()
            .contains("SIGTERM"),
        "{seen}"
    );
    assert_eq!(
        stopped["session"]["lifecycle"]["stop"]["observed"]["kind"], "gone",
        "{seen}"
    );
    assert_eq!(killed["session"]["outcome"], "stopped", "{seen}");
    assert_eq!(killed["session"]["exitCode"], 137, "{seen}");
    assert!(
        killed["session"]["lifecycle"]["kill"]["attempt"]
            .as_str()
            .unwrap()
            .contains("SIGKILL"),
        "{seen}"
    );
    assert_eq!(
        killed["session"]["lifecycle"]["kill"]["observed"]["kind"], "gone",
        "{seen}"
    );

    for (session, command) in [(&stopping_id, "stop"), (&killing_id, "kill")] {
        let refused = fixture.feld(&["session", command, session]);
        assert_eq!(refused.status.code(), Some(1), "{command}");
        assert!(String::from_utf8_lossy(&refused.stderr).contains("is already closed"));
    }
}

#[test]
fn session_list_narrows_and_session_log_reads_program_output() {
    let fixture = Fixture::new();
    fixture.start_daemon();

    let first = fixture.create_and_start(
        "printer",
        &[
            "sh",
            "-c",
            "echo one; sleep 0.2; echo two >&2; sleep 0.2; echo three",
        ],
    );
    let first_id = first["session"]["id"].as_str().unwrap().to_owned();
    fixture.wait_for_outcome("printer", "succeeded");
    // A read before the start leaves the first Session out of a range that ends there.
    thread::sleep(Duration::from_millis(20));
    let between = chrono::Utc::now().to_rfc3339();
    thread::sleep(Duration::from_millis(20));
    fixture.json(&["actor", "start", "printer", "--json"]);
    fixture.wait_for_outcome("printer", "succeeded");
    let running = fixture.create_and_start("sleeper", &["sleep", "30"]);
    let running_id = running["session"]["id"].as_str().unwrap().to_owned();

    let ids = |arguments: &[&str]| -> Vec<(String, String, u64)> {
        let mut arguments = arguments.to_vec();
        arguments.extend(["--json"]);
        fixture.json(&arguments)["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|session| {
                (
                    session["id"].as_str().unwrap().to_owned(),
                    session["actor"].as_str().unwrap().to_owned(),
                    session["generation"].as_u64().unwrap(),
                )
            })
            .collect()
    };
    let all = ids(&["session", "list"]);
    assert_eq!(
        all.iter()
            .map(|(_, actor, generation)| (actor.as_str(), *generation))
            .collect::<Vec<_>>(),
        [("sleeper", 1), ("printer", 2), ("printer", 1)],
        "newest first"
    );
    let printer = ids(&["session", "list", "--actor", "printer"]);
    assert_eq!(printer.len(), 2);
    assert_eq!(printer[1].0, first_id);
    assert_eq!(
        ids(&["session", "list", "--current"]),
        [(running_id.clone(), "sleeper".to_owned(), 1)]
    );
    assert_eq!(ids(&["session", "list", "--kind", "job"]).len(), 3);
    assert!(ids(&["session", "list", "--kind", "agent"]).is_empty());
    let before = ids(&["session", "list", "--end", &between]);
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].0, first_id);
    let after = ids(&["session", "list", "--start", &between]);
    assert!(after.iter().all(|(id, _, _)| id != &first_id), "{after:?}");
    assert!(ids(&["--workspace", "other", "session", "list"]).is_empty());

    let listed = fixture.feld(&["session", "list", "--actor", "printer"]);
    assert!(listed.status.success());
    let listed = String::from_utf8(listed.stdout).unwrap();
    assert!(
        listed.starts_with("Workspace: default\nSESSION "),
        "{listed}"
    );
    assert!(
        listed.lines().any(|line| line.starts_with(&first_id)
            && line.contains(" printer ")
            && line.ends_with(" succeeded (exit_code 0)")),
        "{listed}"
    );

    // The output of an ended Session is read by its ID; the capture may still be writing.
    let deadline = Instant::now() + Duration::from_secs(10);
    let page = loop {
        let page = fixture.json(&["session", "log", &first_id, "--json"]);
        if page["lines"].as_array().unwrap().len() == 3 {
            break page;
        }
        assert!(Instant::now() < deadline, "output did not arrive: {page}");
        thread::sleep(Duration::from_millis(20));
    };
    let lines: Vec<_> = page["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| {
            (
                line["stream"].as_str().unwrap(),
                line["text"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        lines,
        [("stdout", "one"), ("stderr", "two"), ("stdout", "three")]
    );

    let tail = fixture.json(&["session", "log", &first_id, "--tail", "1", "--json"]);
    assert_eq!(tail["lines"][0]["text"], "three");
    assert_eq!(tail["lines"].as_array().unwrap().len(), 1);
    let rest = fixture.json(&["session", "log", &first_id, "--after", "1", "--json"]);
    assert_eq!(rest["lines"].as_array().unwrap().len(), 2);
    let none = fixture.feld(&["session", "log", &first_id, "--tail", "0"]);
    assert!(none.status.success());
    let none = String::from_utf8(none.stdout).unwrap();
    assert_eq!(none.lines().skip(1).collect::<Vec<_>>(), ["next cursor: 3"]);
    let malformed = fixture.feld(&["session", "log", &first_id, "--after", "abc"]);
    assert_eq!(malformed.status.code(), Some(2));
    assert!(malformed.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&malformed.stderr)
            .contains("--after for program output takes a sequence number")
    );

    let printed = fixture.feld(&["session", "log", &first_id, "--tail", "2"]);
    assert!(printed.status.success());
    let printed = String::from_utf8(printed.stdout).unwrap();
    let printed: Vec<_> = printed.lines().collect();
    assert_eq!(
        printed[0],
        format!(
            "Output {first_id} (program · sh -c 'echo one; sleep 0.2; echo two >&2; sleep 0.2; echo three')"
        )
    );
    assert!(printed[1].ends_with("  stderr  two"), "{printed:?}");
    assert!(printed[2].ends_with("  stdout  three"), "{printed:?}");
    assert_eq!(printed[3], "next cursor: 3");

    // `--follow` prints what arrives until the Session ends.
    fixture.create_and_start("follower", &["sh", "-c", "echo early; sleep 1; echo late"]);
    let followed = fixture.feld(&["session", "log", "follower", "--follow"]);
    assert!(followed.status.success());
    let followed = String::from_utf8(followed.stdout).unwrap();
    assert!(followed.contains("  stdout  early\n"), "{followed}");
    assert!(followed.contains("  stdout  late\n"), "{followed}");
    assert!(followed.ends_with("next cursor: 2\n"), "{followed}");
    // `--follow --json` prints each line as a JSON line of its own.
    fixture.create_and_start(
        "json-follower",
        &["sh", "-c", "echo early; sleep 1; echo late"],
    );
    let followed = fixture.feld(&["session", "log", "json-follower", "--follow", "--json"]);
    assert!(followed.status.success());
    let followed = String::from_utf8(followed.stdout).unwrap();
    let lines: Vec<Value> = followed
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        lines
            .iter()
            .map(|line| (
                line["sequence"].as_u64().unwrap(),
                line["text"].as_str().unwrap()
            ))
            .collect::<Vec<_>>(),
        [(1, "early"), (2, "late")],
        "{followed}"
    );

    // An Actor names its current Session after it ends, and has none before its first.
    let ended = fixture.json(&["session", "get", "printer", "--json"]);
    assert_eq!(ended["session"]["id"], printer[0].0.as_str());
    fixture.create("idle", &["true"]);
    let rejected = fixture.feld(&["session", "log", "idle"]);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("Actor 'idle' has no Session"));
    // stop, kill and observe act on a running Session only.
    for command in ["stop", "kill", "observe"] {
        let refused = fixture.feld(&["session", command, "printer"]);
        assert_eq!(refused.status.code(), Some(1), "{command}");
        assert!(
            String::from_utf8_lossy(&refused.stderr)
                .contains("Actor 'printer' has no current Session")
        );
    }
}

#[test]
fn a_program_keeps_none_of_the_claude_code_markers_its_daemon_inherited() {
    let fixture = Fixture::new();
    // A daemon started from inside a Claude Code inherits what it puts in its child
    // processes, and so does the tmux server the daemon starts. The configuration root is the
    // daemon's own.
    let config_root = fixture.home().join("claude");
    let mut daemon = fixture.command(&["daemon", "start", "--json"]);
    for marker in feld_core::process::CLAUDE_CODE_PARENT_MARKERS {
        daemon.env(marker, "1");
    }
    daemon
        .env("CLAUDE_CODE_ENTRYPOINT", "cli")
        .env("CLAUDE_CONFIG_DIR", &config_root);
    assert!(daemon.output().expect("start daemon").status.success());

    let started =
        fixture.create_and_start("env-printer", &["sh", "-c", "env | grep CLAUDE; echo end"]);
    let session = started["session"]["id"].as_str().unwrap().to_owned();
    fixture.wait_for_outcome("env-printer", "succeeded");

    let deadline = Instant::now() + Duration::from_secs(10);
    let lines = loop {
        let page = fixture.json(&["session", "log", &session, "--json"]);
        let lines: Vec<String> = page["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line["text"].as_str().unwrap().to_owned())
            .collect();
        if lines.last().is_some_and(|line| line == "end") {
            break lines;
        }
        assert!(Instant::now() < deadline, "output did not arrive: {page}");
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        lines,
        [
            format!("CLAUDE_CONFIG_DIR={}", config_root.display()),
            "end".to_owned()
        ]
    );
}

/// The Session token reaches the program through its environment, never through a command
/// line, which `ps` shows every user of the machine.
#[test]
fn the_session_token_stays_off_every_command_line() {
    let fixture = Fixture::new();
    let argv = support::ArgvLog::wrapping(&[&support::on_path("tmux")]);
    let mut daemon = fixture.command(&["daemon", "start", "--json"]);
    argv.apply(&mut daemon);
    assert!(daemon.output().expect("start the daemon").status.success());
    let written = fixture.home().join("token");
    let script = format!(
        "printf %s \"$FELD_SESSION_TOKEN\" > {}; sleep 30",
        written.display()
    );
    fixture.create_and_start("quiet", &["sh", "-c", &script]);
    let token = support::wait_for_written(&written);
    assert!(
        argv.holding("respawn-pane").len() == 1,
        "tmux was run through the stand-in"
    );
    let seen = argv.holding(&token);
    assert!(seen.is_empty(), "the token was on a command line: {seen:?}");
    support::assert_no_env_file_holds(&token);
}

/// The launchSpec `env` reaches the program, beneath the variables Feld sets itself.
#[test]
fn a_program_starts_in_the_declared_environment() {
    let fixture = Fixture::new();
    let mut daemon = fixture.command(&["daemon", "start", "--json"]);
    assert!(daemon.output().expect("start the daemon").status.success());
    let written = fixture.home().join("env");
    let socket = fixture.socket.to_string_lossy();
    let script = format!(
        "printf '%s %s' \"$GH_CONFIG_DIR\" \"$FELD_ACTOR\" > {}",
        written.display()
    );
    fixture.json(&[
        "actor",
        "create",
        "env-reader",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        socket.as_ref(),
        "--env",
        "GH_CONFIG_DIR=/gh agent",
        "--env",
        "FELD_ACTOR=other",
        "--json",
        "--",
        "sh",
        "-c",
        &script,
    ]);
    fixture.wait_for_outcome("env-reader", "succeeded");
    assert_eq!(support::wait_for_written(&written), "/gh agent env-reader");
}
