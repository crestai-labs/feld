//! End-to-end coverage of a Codex Session without a real `codex` binary: a stand-in
//! Runtime records what Feld launched it with and writes the rollout files Codex writes,
//! and the test then plays the parts Codex plays around it (hook, queue helper, OTLP
//! exporter, the `/quit` of a stop) against the real daemon, Collector, store, and
//! projections.
//!
//! Everything runs against fixture roots alone: `FELD_HOME`, `HOME`, and `CODEX_HOME` are
//! temporary directories and tmux is a socket of this test's own, so nothing reaches the
//! real `~/.codex`, `~/.feld`, or a daemon a person is using.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};
use feld_core::telemetry::proto;
use prost::Message;
use reqwest::blocking::Client;
use serde_json::{Value, json};
use tempfile::TempDir;

const FELD: &str = env!("CARGO_BIN_EXE_feld");
const CODEX_SOURCE: &str = "Codex Runtime telemetry";
const ROLLOUT_SOURCE: &str = "Codex rollout";
const CODEX_ATTACHED: &str = "Codex telemetry is attributed only in owned Sessions";
const NOT_PROVIDED: &str = "Not provided by Codex";

/// The threads of the captured rollout files: a Codex and the Sub Agent it spawned.
const PARENT_THREAD: &str = "01a0c15d-8b37-7da3-82ba-6291e17a77ac";
const SUB_AGENT_THREAD: &str = "01a0c15d-9ee7-7823-b1da-519047940755";
/// The thread of the Codex a person starts themselves in the attach test.
const ATTACHED_THREAD: &str = "01a0c15a-2038-70c3-bab6-1791f5a9e6c0";
/// The thread a Codex app-server runs for the TUI a person attaches, and the one a new chat
/// of that TUI moves to.
const SERVED_THREAD: &str = "01a0f564-75c7-71c2-98b8-9ef070359251";
const NEW_CHAT_THREAD: &str = "01a0f567-fc83-7e12-94b7-5e78c07b3f78";

mod support;

struct Fixture {
    temporary: TempDir,
    socket: PathBuf,
    client: Client,
}

impl Fixture {
    fn new() -> Option<Self> {
        if !feld_core::test_require::tmux() {
            return None;
        }
        let temporary = tempfile::tempdir().unwrap();
        let socket = temporary.path().join("codex.sock");
        support::install_plugins_with_codex(temporary.path());
        fs::create_dir_all(temporary.path().join("codex-home")).unwrap();
        Some(Self {
            temporary,
            socket,
            client: Client::new(),
        })
    }

    fn home(&self) -> &Path {
        self.temporary.path()
    }

    /// The configuration root of the Codex of this fixture, which is where its rollout
    /// files, its `hooks.json`, and its skills go.
    fn codex_home(&self) -> PathBuf {
        self.home().join("codex-home")
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(self.home())
            .env("HOME", self.home())
            .env_remove("CODEX_HOME")
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .args(arguments);
        command
    }

    fn json(&self, arguments: &[&str]) -> Value {
        let output = self.command(arguments).output().unwrap();
        assert!(
            output.status.success(),
            "feld {} failed:\nstdout: {}\nstderr: {}\ndaemon log: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            fs::read_to_string(self.home().join("daemon.log")).unwrap_or_default()
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Starts the daemon with the fixture home first on `PATH`, where the stand-in `codex`
    /// is, and with the configuration root of this fixture's Codex. The Adapter hands that
    /// root to the Codex it starts.
    fn start_daemon(&self) -> Value {
        let path = format!(
            "{}:{}",
            self.home().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = self
            .command(&["daemon", "start", "--json"])
            .env("PATH", path)
            .env("CODEX_HOME", self.codex_home())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "daemon start failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Runs tmux on the fixture socket. The server inherits this environment, so the Feld
    /// variables of a Session the tests may run inside stay out of the panes it opens and
    /// the Codex of a pane reads this fixture's configuration root.
    fn tmux(&self, arguments: &[&str]) -> Output {
        let mut command = Command::new("tmux");
        support::without_outside_variables(&mut command)
            .arg("-S")
            .arg(&self.socket)
            .args(arguments)
            .env("HOME", self.home())
            .env("CODEX_HOME", self.codex_home());
        for variable in ["CODEX_THREAD_ID", "CODEX_SESSION_ID", "TMUX", "TMUX_PANE"] {
            command.env_remove(variable);
        }
        command.output().unwrap()
    }

    fn local_token(&self) -> String {
        fs::read_to_string(self.home().join("admin/api-token"))
            .unwrap()
            .trim()
            .to_owned()
    }

    fn get(&self, api: &str, path: &str) -> Value {
        self.client
            .get(format!("{api}{path}"))
            .bearer_auth(self.local_token())
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    }

    /// Polls the whole-Session metrics until `ready` accepts them.
    fn wait_for_metrics(&self, api: &str, session: &str, ready: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let metrics = self.get(api, &format!("/v1/sessions/{session}/metrics"));
            if ready(&metrics) {
                return metrics;
            }
            assert!(
                Instant::now() < deadline,
                "metrics were not projected in time: {metrics}\ndaemon log: {}",
                fs::read_to_string(self.home().join("daemon.log")).unwrap_or_default()
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Sends one OTLP/HTTP export the way Codex's exporter does, with the header its launch
    /// fixed in the environment.
    fn export(&self, collector: &str, signal: &str, header: Option<&str>, body: Vec<u8>) -> u16 {
        let mut request = self
            .client
            .post(format!("{collector}/v1/{signal}"))
            .header("content-type", "application/x-protobuf")
            .body(body);
        if let Some(header) = header {
            let (name, value) = header.split_once('=').expect("an OTLP header is a pair");
            request = request.header(name, value);
        }
        request.send().unwrap().status().as_u16()
    }

    /// Runs one part beside Codex as Codex runs it: the launch environment around it and
    /// the event JSON on its stdin.
    fn hook(&self, environment: &BTreeMap<String, String>, event: &Value) -> Output {
        let mut child = Command::new(self.home().join("plugins/runtime/codex/bin/hook"))
            .env_clear()
            .envs(environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(event.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "the hook failed on {event}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.command(&["daemon", "stop"]).output();
        let _ = self.tmux(&["kill-server"]);
        support::stop_runtime_helpers(self.home());
    }
}

fn wait_until(what: impl Fn() -> String, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready() {
        assert!(Instant::now() < deadline, "{}", what());
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_file(path: &Path, stderr: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let text = fs::read_to_string(path).unwrap_or_default();
        if !text.is_empty() {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{} was not written; stderr: {}",
            path.display(),
            fs::read_to_string(stderr).unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(25));
    }
}

/// The settings of one launch, as the `-c` arguments spell them.
fn overrides(argv: &[String]) -> Vec<String> {
    argv.windows(2)
        .filter(|pair| pair[0] == "-c")
        .map(|pair| pair[1].clone())
        .collect()
}

fn metric<'a>(metrics: &'a Value, name: &str) -> &'a Value {
    metrics["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|metric| metric["metric"] == name)
        .unwrap_or_else(|| panic!("metric {name} is missing from {metrics}"))
}

fn assert_reported<'a>(metrics: &'a Value, name: &str, source: &str) -> &'a Value {
    let projection = metric(metrics, name);
    assert_eq!(projection["status"], "reported", "{name}: {projection}");
    assert_eq!(projection["source"], source, "{name}: {projection}");
    &projection["series"]
}

fn approx(value: &Value, expected: f64) -> bool {
    value
        .as_f64()
        .is_some_and(|value| (value - expected).abs() < 1e-6)
}

fn seconds_of(timestamp: &Value) -> i64 {
    DateTime::parse_from_rfc3339(timestamp.as_str().unwrap())
        .unwrap()
        .timestamp()
}

fn iso(seconds: i64) -> String {
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .unwrap()
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn nanos(seconds: i64) -> u64 {
    seconds as u64 * 1_000_000_000
}

/// One `token_count` event, as Codex appends one to the rollout file at the end of a turn.
/// The gauges the hook reports come from the last one, so a test that pins them writes its
/// own rather than depending on when a captured one was taken.
fn append_token_count(rollout: &Path, used: i64, size: i64, resets_at: i64) {
    let record = json!({
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "last_token_usage": {"total_tokens": used},
                "model_context_window": size,
            },
            "rate_limits": {
                "primary": {"used_percent": 12.5, "window_minutes": 300, "resets_at": resets_at},
                "secondary": Value::Null,
            },
        }
    });
    let mut file = fs::OpenOptions::new().append(true).open(rollout).unwrap();
    writeln!(file, "{record}").unwrap();
}

// --- Codex's OTLP/HTTP exports ---------------------------------------------------------

fn resource() -> proto::Resource {
    proto::Resource::new(vec![proto::KeyValue::new("service.name", "codex")])
}

/// One export interval of Codex metrics: the token kinds of a turn, the cost in micro-USD,
/// the failures of one websocket request, the two records of a shell run beside one other
/// tool call, and the time to the first token. Codex exports delta, which is what the
/// projection reads.
fn codex_metrics_export(start: u64, time: u64) -> Vec<u8> {
    let delta = proto::AggregationTemporality::Delta;
    let tokens = |kind: &str, sum: f64| {
        proto::HistogramDataPoint::new(start, time, sum, &[1_000.0], &[1, 0])
            .with("token_type", kind)
            .with("model", "gpt-5.6")
    };
    let count = |key: &str, value: &str, count: f64| {
        proto::NumberDataPoint::double(start, time, count).with(key, value)
    };
    proto::ExportMetricsServiceRequest::new(
        Some(resource()),
        Some(proto::InstrumentationScope::new("codex", "0.150.0")),
        vec![
            proto::Metric::histogram(
                "codex.turn.token_usage",
                delta,
                vec![
                    tokens("input", 57_877.0),
                    tokens("cached_input", 54_016.0),
                    tokens("output", 212.0),
                    tokens("cache_write_input", 0.0),
                    // Codex counts these inside the four above.
                    tokens("total", 58_089.0),
                    tokens("reasoning_output", 24.0),
                ],
            ),
            proto::Metric::sum(
                "codex.turn.cost_microusd",
                delta,
                vec![count("model", "gpt-5.6", 12_500.0)],
            ),
            proto::Metric::sum(
                "codex.websocket.request",
                delta,
                vec![
                    count("success", "false", 2.0),
                    count("success", "true", 5.0),
                ],
            ),
            proto::Metric::sum(
                "codex.tool.call",
                delta,
                vec![
                    count("tool", "exec_command", 3.0),
                    count("tool", "exec", 3.0),
                    count("tool", "read_file", 2.0),
                ],
            ),
            proto::Metric::histogram(
                "codex.turn.ttft.duration_ms",
                delta,
                vec![
                    proto::HistogramDataPoint::new(
                        start,
                        time,
                        1_700.0,
                        &[100.0, 200.0],
                        &[0, 10, 0],
                    )
                    .with("model", "gpt-5.6"),
                ],
            ),
        ],
    )
    .encode_to_vec()
}

/// One log record as codex-cli 0.150.0 writes it: the instrument is the `event.name`
/// attribute, the OTLP `event_name` is the source location it was written at, and
/// `success` is a boolean.
fn log_record(time: u64, name: &str, success: bool) -> proto::LogRecord {
    proto::LogRecord::new(time)
        .with_event_name("event otel/src/events/session_telemetry.rs:721")
        .with("event.name", name)
        .with("success", success)
}

fn logs_request(records: Vec<proto::LogRecord>) -> Vec<u8> {
    proto::ExportLogsServiceRequest::new(
        Some(resource()),
        Some(proto::InstrumentationScope::new("codex", "")),
        records,
    )
    .encode_to_vec()
}

// --- Tests -------------------------------------------------------------------------------

/// An owned Codex Session, end to end: Feld launches Codex with the launch spec's settings and
/// its own on the command line; the hook delivers the Runtime identity from the rollout
/// file that launch created; the exports Codex signs with the header of that launch become
/// this Session's metrics; a Message reaches Codex's queue; the rollout file projects the
/// Console and the Sub Agent thread it holds; the `Stop` hook turns the rollout's last
/// `token_count` into the context window and the account quota; and `feld session stop` types
/// `/quit` and the line that sends it into the pane, which closes the Session.
#[test]
fn owned_codex_session_projects_its_launch_telemetry_and_rollout() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let launch = fixture.home().join("codex-launch.txt");
    let calls = fixture.home().join("codex-queue.log");
    let started = support::FakeCodex::new()
        .recording(&launch)
        .queueing(&calls)
        .with_rollout("parent", PARENT_THREAD)
        .with_rollout("subagent", SUB_AGENT_THREAD)
        .compile(&fixture.home().join("codex"));
    if !started {
        return;
    }
    let daemon = fixture.start_daemon();
    let api = daemon["daemon"]["api"].as_str().unwrap().to_owned();
    let collector = daemon["daemon"]["otlp"].as_str().unwrap().to_owned();

    let created = fixture.json(&[
        "actor",
        "create",
        "codex-e2e",
        "--kind",
        "agent",
        "--runtime",
        "codex",
        "--runtime-settings",
        r#"{"model":"gpt-5.6","approval_policy":"never"}"#,
        "--provider",
        "tmux",
        "--workdir",
        fixture.home().to_str().unwrap(),
        "--tmux-socket",
        fixture.socket.to_str().unwrap(),
        "--json",
    ]);
    let session = created["session"]["id"].as_str().unwrap().to_owned();
    assert_eq!(created["session"]["ownership"], "owned");
    assert_eq!(created["session"]["execution"]["runtime"], "codex");
    assert_eq!(created["session"]["execution"]["label"], "Codex");
    assert_eq!(created["session"]["observation"]["kind"], "running");
    let session_start = seconds_of(&created["session"]["start"]);
    // The Codex of this Session is the one running in the pane its Provider opened.
    wait_until(
        || {
            format!(
                "codex-e2e is not running: {}",
                fixture.json(&["actor", "list", "--json"])
            )
        },
        || {
            fixture.json(&["actor", "list", "--json"])["actors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|actor| {
                    actor["name"] == "codex-e2e"
                        && actor["currentSession"]["observation"]["kind"] == "running"
                })
        },
    );

    // 1. The launch: the thread runs in the TUI, what the launch spec declared and what the
    //    Adapter owns reach Codex as `-c` overrides, the hooks run on the trust the launch
    //    declares for them, the workdir no one declared trusted is left to Codex's own
    //    configuration, and the first turn starts from the prompt at the end of the argv.
    let (argv, environment) = support::wait_for_codex_launch(&launch);
    assert_eq!(argv[..2], ["codex", "--no-daemon"]);
    assert!(
        !argv.contains(&"--dangerously-bypass-hook-trust".to_owned()),
        "{argv:?}"
    );
    assert_eq!(argv[argv.len() - 2], "--", "{argv:?}");
    let prompt = argv.last().unwrap();
    assert!(
        prompt.contains("Feld") && prompt.contains("Message"),
        "{prompt}"
    );
    let overrides = overrides(&argv);
    assert!(
        !overrides.iter().any(|value| value.starts_with("projects=")),
        "{overrides:?}"
    );
    let plugin = fixture.home().join("plugins/runtime/codex/bin");
    for expected in [
        r#"model="gpt-5.6""#.to_owned(),
        r#"approval_policy="never""#.to_owned(),
        "check_for_update_on_startup=false".to_owned(),
        format!(
            r#"otel.exporter={{otlp-http={{endpoint="{collector}/v1/logs",protocol="binary"}}}}"#
        ),
        format!(
            r#"otel.metrics_exporter={{otlp-http={{endpoint="{collector}/v1/metrics",protocol="binary"}}}}"#
        ),
    ] {
        assert!(
            overrides.contains(&expected),
            "{expected} is not in {overrides:?}"
        );
    }
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PostToolUse",
        "Stop",
        "SubagentStart",
        "SubagentStop",
        "PostCompact",
        "SessionEnd",
    ] {
        assert!(
            overrides.contains(&format!(
                r#"hooks.{event}=[{{matcher="",hooks=[{{type="command",command="{}"}}]}}]"#,
                plugin.join("hook").display()
            )),
            "{event} is not declared in {overrides:?}"
        );
    }
    let trust = overrides
        .iter()
        .find(|value| value.starts_with("hooks.state="))
        .unwrap_or_else(|| panic!("no hook trust in {overrides:?}"));
    assert!(
        trust
            .contains(r#""/<session-flags>/config.toml:session_start:0:0"={trusted_hash="sha256:"#),
        "{trust}"
    );
    // The last setting before the prompt gives the sandbox the spool of this Session, the
    // one part of the spool it writes, which the daemon made before the launch.
    let own_spool = fs::canonicalize(fixture.home())
        .unwrap()
        .join("spool")
        .join(&session);
    assert_eq!(argv[argv.len() - 4], "-c", "{argv:?}");
    let sandbox = &argv[argv.len() - 3];
    assert!(
        sandbox.starts_with("sandbox_workspace_write.writable_roots=[")
            && sandbox.ends_with(&format!("\"{}\"]", own_spool.display())),
        "{sandbox}"
    );
    assert!(own_spool.is_dir(), "{}", own_spool.display());
    let token = environment["FELD_SESSION_TOKEN"].clone();
    let header = environment["OTEL_EXPORTER_OTLP_HEADERS"].clone();
    for (key, expected) in [
        ("FELD_API", api.as_str()),
        ("FELD_ACTOR", "codex-e2e"),
        ("FELD_WORKSPACE", "default"),
        ("FELD_HOME", fixture.home().to_str().unwrap()),
        ("FELD_SESSION_ID", session.as_str()),
        // Codex reads the header once, so the token it signs every export with is the
        // telemetry token of the one this launch put there.
        (
            "OTEL_EXPORTER_OTLP_HEADERS",
            format!(
                "Authorization=Bearer {}",
                feld_core::telemetry::telemetry_token(&token)
            )
            .as_str(),
        ),
        ("OTEL_METRIC_EXPORT_INTERVAL", "10000"),
        // The configuration root is the daemon's, so the rollout files of this Codex are
        // where the Adapter looks for them.
        ("CODEX_HOME", fixture.codex_home().to_str().unwrap()),
    ] {
        assert_eq!(
            environment.get(key).map(String::as_str),
            Some(expected),
            "{key}"
        );
    }

    // 2. The SessionStart hook delivers the Runtime identity of the thread and the rollout
    //    file that launch wrote, and every later event re-delivers it.
    let rollout = support::codex_rollout_path(&fixture.codex_home(), PARENT_THREAD);
    wait_until(
        || format!("{} was not written", rollout.display()),
        || rollout.is_file(),
    );
    let event = |name: &str, model: Value| {
        json!({
            "hook_event_name": name,
            "session_id": PARENT_THREAD,
            "transcript_path": rollout.display().to_string(),
            "cwd": fixture.home().display().to_string(),
            "model": model,
        })
    };
    let started = fixture.hook(&environment, &event("SessionStart", json!("gpt-5.6")));
    // SessionStart also tells the model what a Feld Message is and whose word it carries.
    let started: Value = serde_json::from_slice(&started.stdout).unwrap();
    assert_eq!(
        started["hookSpecificOutput"]["hookEventName"],
        "SessionStart"
    );
    let context = started["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("feld_message"), "{context}");
    assert!(context.contains("`fromKind`"), "{context}");
    let read = fixture.get(&api, &format!("/v1/sessions/{session}"));
    assert_eq!(
        read["session"]["model"],
        json!(["Codex", "gpt-5.6"]),
        "{read}"
    );
    // A Sub Agent's event names the parent's thread and rollout file, so it carries the
    // same identity.
    fixture.hook(&environment, &event("SubagentStop", json!("gpt-5.6-codex")));
    let read = fixture.get(&api, &format!("/v1/sessions/{session}"));
    assert_eq!(
        read["session"]["model"],
        json!(["Codex", "gpt-5.6-codex"]),
        "the model follows the latest identity: {read}"
    );

    // 3. The Collector attributes an export by the launch header alone, and the projection
    //    reads it by the metrics rules of adapter/codex.md.
    let base = (session_start / 60 + 1) * 60;
    let export = codex_metrics_export(nanos(base), nanos(base + 10));
    assert_eq!(
        fixture.export(&collector, "metrics", None, export.clone()),
        401
    );
    assert_eq!(
        fixture.export(&collector, "metrics", Some(&header), export),
        200
    );
    // Over HTTP a request is a log record, whose instrument Codex names in an attribute.
    let events = logs_request(vec![
        log_record(nanos(base + 10), "codex.api_request", false),
        log_record(nanos(base + 10), "codex.api_request", true),
        // An event the projection has no meaning for.
        log_record(nanos(base + 10), "codex.startup_phase", true),
    ]);
    assert_eq!(
        fixture.export(&collector, "logs", Some(&header), events),
        200
    );
    let metrics = fixture.wait_for_metrics(&api, &session, |metrics| {
        metric(metrics, "token_usage")["status"] == "reported"
            && metric(metrics, "api_errors")["series"][0]["value"] == 3.0
    });
    let tokens = assert_reported(&metrics, "token_usage", CODEX_SOURCE);
    assert_eq!(
        tokens[0]["value"],
        json!({
            "input": 3_861.0,
            "output": 212.0,
            "cache_read": 54_016.0,
            "cache_creation": 0.0,
            "total": 58_089.0,
        })
    );
    assert!(approx(
        &assert_reported(&metrics, "prompt_cache_hit_rate", CODEX_SOURCE)[0]["value"],
        54_016.0 / 57_877.0 * 100.0
    ));
    assert!(approx(
        &assert_reported(&metrics, "estimated_cost", CODEX_SOURCE)[0]["value"],
        0.0125
    ));
    // Two failures of the websocket transport and the one failed HTTP request.
    assert_eq!(
        assert_reported(&metrics, "api_errors", CODEX_SOURCE)[0]["value"],
        3.0
    );
    // The `exec` record of a shell run is the same call as its `exec_command`.
    assert_eq!(
        assert_reported(&metrics, "tool_calls", CODEX_SOURCE)[0]["value"],
        5.0
    );
    // The rank falls nine tenths of the way through the bucket above 100 ms.
    assert_eq!(
        assert_reported(&metrics, "api_request_latency_p95", CODEX_SOURCE)[0]["value"],
        195.0
    );
    assert_eq!(metric(&metrics, "active_time")["status"], "n/a");
    assert_eq!(metric(&metrics, "active_time")["source"], NOT_PROVIDED);

    // 4. The hook started the queue helper of this Session, which puts a Message into the
    //    thread with `codex queue` and records the read once a turn took it, which is when
    //    the rollout file shows it as a `user` message. How it holds the mailbox and what it
    //    does with a thread that takes nothing is in `message_delivery.rs`.
    let sent = fixture.json(&["message", "send", "codex-e2e", "queue this", "--json"]);
    let message = sent["message"]["id"].as_str().unwrap().to_owned();
    wait_until(
        || {
            format!(
                "the Message was not queued into the thread: {:?}",
                support::codex_queue_calls(&calls)
            )
        },
        || {
            support::codex_queue_calls(&calls)
                .iter()
                .any(|call| call.last().is_some_and(|line| line.contains(&message)))
        },
    );
    let call = support::codex_queue_calls(&calls).pop().unwrap();
    assert_eq!(call[..4], ["queue", "--thread", PARENT_THREAD, "--message"]);
    let line: Value = serde_json::from_str(&call[4]).unwrap();
    assert_eq!(line["type"], "feld_message");
    assert_eq!(line["message"]["body"], "queue this");
    let listed = || {
        fixture.json(&["message", "list", "--to", "codex-e2e", "--json"])["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|listed| listed["id"] == message)
            .cloned()
            .unwrap()
    };
    // No turn has taken it yet, as while a long tool call holds the turn: the Message is
    // still waiting, and the Actor shows it.
    thread::sleep(Duration::from_secs(1));
    assert_eq!(listed()["waiting"], true);
    let actor = fixture.json(&["actor", "get", "codex-e2e", "--json"]);
    assert_eq!(actor["actor"]["waitingCount"], 1, "{actor}");
    // The next turn takes it.
    let taken = json!({
        "timestamp": "2026-09-21T00:29:10.000Z",
        "type": "response_item",
        "payload": {
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": call[4]}],
        },
    });
    let mut file = fs::OpenOptions::new().append(true).open(&rollout).unwrap();
    writeln!(file, "{taken}").unwrap();
    wait_until(
        || "the read of the taken Message was not recorded".to_owned(),
        || listed()["waiting"] == false,
    );

    // 5. The Console is the rollout file of the identity, projected onto the common shape,
    //    and the Sub Agent thread it names is a Session of its own.
    let page = fixture.get(&api, &format!("/v1/sessions/{session}/transcript"));
    assert_eq!(page["state"], "readable", "{page}");
    let records = page["records"].as_array().unwrap();
    let conversation = records
        .iter()
        .filter(|record| record["kind"] != "other")
        .map(|record| record["kind"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        conversation,
        [
            "system", // task_started
            "input",  // the prompt
            "thinking",
            "tool-call", // spawn_agent
            "tool-result",
            "tool-call", // wait_agent
            "tool-result",
            "system", // the Sub Agent's answer to its parent
            "thinking",
            "tool-call", // interrupt_agent
            "tool-result",
            "response",
            "system",       // task_complete
            "notification", // the Message the next turn took
        ],
        "{page}"
    );
    // The Session's own Console holds its own records alone; the Sub Agent's belong to the
    // Sub Agent Session below.
    assert!(records.iter().all(|record| record["subAgent"].is_null()));
    let read = fixture.get(&api, &format!("/v1/sessions/{session}"));
    let sub_agents = read["subAgents"].as_array().unwrap();
    assert_eq!(sub_agents.len(), 1, "{read}");
    let sub_agent = &sub_agents[0];
    assert_eq!(sub_agent["subAgent"], SUB_AGENT_THREAD);
    assert_eq!(sub_agent["parentSession"], session);
    assert_eq!(sub_agent["startKind"], "sub-agent");
    assert_eq!(sub_agent["start"], "2026-09-21T00:28:53.488Z");
    assert_eq!(sub_agent["end"], "2026-09-21T00:28:56.658Z");
    assert_eq!(sub_agent["outcome"], "ended");
    let inside = fixture.get(
        &api,
        &format!(
            "/v1/sessions/{}/transcript",
            sub_agent["id"].as_str().unwrap()
        ),
    );
    assert_eq!(inside["state"], "readable");
    let inside_records = inside["records"].as_array().unwrap();
    assert_eq!(inside_records.len(), 14, "{inside}");
    assert!(
        inside_records
            .iter()
            .all(|record| record["subAgent"] == SUB_AGENT_THREAD)
    );

    // 6. The `Stop` hook reports the state of the turn that just ended: the context window
    //    of this Session and the account quota of the whole machine.
    let resets_at = Utc::now().timestamp() + 3_600;
    append_token_count(&rollout, 62_000, 212_000, resets_at);
    fixture.hook(&environment, &event("Stop", json!("gpt-5.6")));
    let metrics = fixture.wait_for_metrics(&api, &session, |metrics| {
        metric(metrics, "context_window_utilization")["status"] == "reported"
    });
    assert_eq!(
        assert_reported(&metrics, "context_window_utilization", ROLLOUT_SOURCE)[0]["value"],
        json!({"size": 212_000.0, "used_percent": 25.0})
    );
    let quota = fixture.get(&api, "/v1/runtimes/codex/metrics?metric=quota");
    let codex_quota = &quota["quota"];
    assert_eq!(codex_quota["status"], "reported", "{quota}");
    assert_eq!(codex_quota["source"], ROLLOUT_SOURCE);
    assert_eq!(codex_quota["series"][0]["window"], "five_hour");
    assert_eq!(codex_quota["series"][0]["latest"]["usedPercent"], 12.5);
    assert_eq!(codex_quota["series"][0]["resetsAt"], iso(resets_at));
    assert_eq!(codex_quota["series"][0]["reportedBy"]["session"], session);
    assert_eq!(codex_quota["series"][0]["reportedBy"]["actor"], "codex-e2e");

    // 7. `feld session stop` types the Adapter's `stopInput` into the pane: `/quit`, and the
    //    empty line that sends the command Codex's TUI then shows. Codex reads `/quit` as
    //    the request to end, and the Session closes with what it ended by.
    let stopped = fixture.json(&["session", "stop", "codex-e2e", "--json"])["session"].clone();
    assert_eq!(
        stopped["lifecycle"]["stop"]["attempt"],
        "tmux sent /quit to the pane"
    );
    assert_eq!(stopped["observation"]["kind"], "gone", "{stopped}");
    assert_eq!(stopped["outcome"], "stopped");
    assert_eq!(stopped["exitCode"], 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    let closed = loop {
        let described = fixture.json(&["session", "get", &session, "--json"]);
        if described["session"]["end"].is_string() {
            break described;
        }
        assert!(
            Instant::now() < deadline,
            "the Session did not close: {described}"
        );
        thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(closed["session"]["endKind"], "stopped");
    // What was reported stands after the Session closed.
    let metrics = fixture.get(&api, &format!("/v1/sessions/{session}/metrics"));
    assert_eq!(
        assert_reported(&metrics, "token_usage", CODEX_SOURCE)[0]["value"]["total"],
        58_089.0
    );
}

/// A Codex a person started themselves, attached from inside its own pane: `feld init`
/// merges the Feld hooks into the `hooks.json` that Codex already had and installs the
/// `feld` skill beside them, writing nowhere but that Codex's configuration root; the
/// attach finds them prepared and delivers the identity of the thread in its environment; and because Codex fixed
/// its OTLP header before Feld ever saw it, only the gauges the hook reads out of the
/// rollout file reach this Session's metrics.
#[test]
fn attached_codex_prepares_its_own_root_and_reports_only_the_rollout_gauges() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let real_codex = std::env::var_os("HOME")
        .map(|value| PathBuf::from(value).join(".codex"))
        .expect("the test environment has a HOME");
    let before = preparation_marks(&real_codex);

    let daemon = fixture.start_daemon();
    let api = daemon["daemon"]["api"].as_str().unwrap().to_owned();
    let codex_home = fixture.codex_home();
    let workdir = fixture.home().join("attached");
    fs::create_dir_all(&workdir).unwrap();
    // A hook a person declared for themselves, which the preparation keeps.
    let mine = json!({"matcher": "", "hooks": [{"type": "command", "command": "/usr/bin/mine"}]});
    fs::write(
        codex_home.join("hooks.json"),
        serde_json::to_vec_pretty(&json!({"hooks": {"Notification": [mine]}})).unwrap(),
    )
    .unwrap();

    // `feld init` prepares the configuration root of the Codex a person starts. The one
    // that installed the fixture's Plugins prepared HOME's, which this run must not touch.
    let _ = fs::remove_dir_all(fixture.home().join(".codex"));
    let init = fixture
        .command(&["init", "--runtime", "codex", "--provider", "tmux", "--yes"])
        .env("CODEX_HOME", &codex_home)
        .output()
        .unwrap();
    let init_stdout = String::from_utf8_lossy(&init.stdout);
    assert!(init.status.success(), "{init_stdout}");
    assert!(
        init_stdout.contains("Codex:       prepared \u{b7} writable_roots added")
            && init_stdout
                .contains("note:  Codex: the next Codex you start asks to trust the Feld hooks"),
        "{init_stdout}"
    );
    support::warm_up_plugins(fixture.home());
    support::warm_up_runtime_parts(&codex_home);

    // What Codex itself does: it writes the rollout file of its thread, the model runs
    // `feld actor attach --self` from the `feld` skill, and the next hook that fires delivers
    // the identity with the model and reports the gauges of the turn that ended.
    // Codex resolves the root it writes into, so its hooks name the rollout file by the
    // resolved path, and so does the identity `feld actor attach --self` delivers.
    let rollout =
        support::codex_rollout_path(&fs::canonicalize(&codex_home).unwrap(), ATTACHED_THREAD);
    let event = |name: &str| {
        json!({
            "hook_event_name": name,
            "session_id": ATTACHED_THREAD,
            "transcript_path": rollout,
            "cwd": workdir,
            "model": "gpt-5.6",
        })
        .to_string()
        .replace('\'', "'\\''")
    };
    let child = format!(
        "export HOME='{home}' FELD_HOME='{home}' CODEX_HOME='{codex_home}' CODEX_THREAD_ID={ATTACHED_THREAD}; \
         '{feld}' --home '{home}' actor attach --self --runtime codex --json > '{home}/attach.json' 2> '{home}/attach.err'; \
         printf '%s' '{stop}' | '{hook}' > '{home}/hook.log' 2>&1; \
         printf '%s' '{end}' | '{hook}' >> '{home}/hook.log' 2>&1; \
         echo done > '{home}/done'",
        home = fixture.home().display(),
        codex_home = codex_home.display(),
        feld = FELD,
        stop = event("Stop"),
        end = event("SessionEnd"),
        hook = codex_home.join("skills/feld/bin/hook").display(),
    );
    let started = support::FakeCodex::new()
        .with_rollout("exec", ATTACHED_THREAD)
        .beside(&child)
        .compile(&workdir.join("codex"));
    if !started {
        return;
    }
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "attached",
                "-c",
                workdir.to_str().unwrap(),
                "--",
                "exec ./codex",
            ])
            .status
            .success()
    );
    wait_for_file(
        &fixture.home().join("done"),
        &fixture.home().join("attach.err"),
    );
    let attached: Value = serde_json::from_str(&wait_for_file(
        &fixture.home().join("attach.json"),
        &fixture.home().join("attach.err"),
    ))
    .unwrap();
    let session = attached["session"]["id"].as_str().unwrap().to_owned();
    let actor = attached["session"]["actor"].as_str().unwrap().to_owned();
    assert_eq!(attached["session"]["ownership"], "attached");
    assert_eq!(attached["session"]["execution"]["label"], "Codex");

    // The preparation wrote into the configuration root of the Codex being attached: the
    // Feld hooks joined the one a person had declared, and the skill sits beside them. The
    // attach found them there and had nothing to ask for.
    let notes = fs::read_to_string(fixture.home().join("attach.err")).unwrap();
    assert!(!notes.contains("feld init"), "{notes}");
    let skill = codex_home.join("skills/feld");
    assert!(skill.join("SKILL.md").is_file());
    let hooks: Value =
        serde_json::from_slice(&fs::read(codex_home.join("hooks.json")).unwrap()).unwrap();
    assert_eq!(hooks["hooks"]["Notification"], json!([mine]));
    let spool = fs::canonicalize(fixture.home())
        .unwrap()
        .join("spool/shared");
    assert!(
        fs::read_to_string(codex_home.join("config.toml"))
            .unwrap()
            .contains(&format!("writable_roots = [\"{}\"]", spool.display()))
    );
    assert_eq!(
        hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        skill.join("bin/hook").display().to_string()
    );
    assert_eq!(
        hooks["hooks"]["SessionStart"][0]["hooks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Nothing was written outside the roots the attach was given: neither the `.codex` of
    // the fixture HOME nor the one of the person running the tests.
    assert!(
        !fixture.home().join(".codex").exists(),
        "the preparation wrote into HOME/.codex although CODEX_HOME named its root"
    );
    assert_eq!(
        preparation_marks(&real_codex),
        before,
        "the preparation touched {}",
        real_codex.display()
    );

    // The attach delivered the identity of the thread in the environment, and the hook that
    // fired after it added the model.
    let read = fixture.get(&api, &format!("/v1/sessions/{session}"));
    assert_eq!(
        read["session"]["model"],
        json!(["Codex", "gpt-5.6"]),
        "{read}"
    );
    let page = fixture.get(&api, &format!("/v1/sessions/{session}/transcript"));
    assert_eq!(page["state"], "readable", "{page}");

    // Codex built its OTLP header before Feld saw it, so every metric taken from Runtime
    // telemetry is unavailable and the gauges of the rollout file are what is left.
    let metrics = fixture.wait_for_metrics(&api, &session, |metrics| {
        metric(metrics, "context_window_utilization")["status"] == "reported"
    });
    let context = assert_reported(&metrics, "context_window_utilization", ROLLOUT_SOURCE);
    assert_eq!(context[0]["value"]["size"], 258_400.0);
    assert!(approx(
        &context[0]["value"]["used_percent"],
        0.681_818_181_818_181_8
    ));
    for projection in metrics["metrics"].as_array().unwrap() {
        match projection["metric"].as_str().unwrap() {
            "context_window_utilization" => {}
            "active_time" => assert_eq!(projection["status"], "n/a", "{projection}"),
            other => {
                assert_eq!(projection["status"], "unavailable", "{other}: {projection}");
                assert_eq!(projection["source"], CODEX_ATTACHED, "{other}");
            }
        }
    }

    // Feld started nothing here, so it ends nothing: the Session is one it borrowed.
    let refused = fixture
        .command(&["session", "kill", &actor, "--json"])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("is not owned by Feld"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
}

/// What `prepare` writes in a Codex configuration root, as those files are now. A
/// fixture run must never reach the root of the person running the tests, so this says
/// theirs is exactly as it was, without reading the rollout files a Codex of theirs may be
/// writing while the test runs.
fn preparation_marks(root: &Path) -> Vec<(&'static str, Option<(u64, std::time::SystemTime)>)> {
    [
        "hooks.json",
        "config.toml",
        "skills/feld/SKILL.md",
        "skills/feld/bin/hook",
        "skills/feld/bin/queue-helper",
    ]
    .into_iter()
    .map(|name| {
        let mark = fs::symlink_metadata(root.join(name))
            .ok()
            .map(|data| (data.len(), data.modified().unwrap_or(std::time::UNIX_EPOCH)));
        (name, mark)
    })
    .collect()
}

/// The `runtimeSettings` keys the Codex Adapter builds itself are refused where the
/// Actor is created, so that no Session is ever started with settings that would reach
/// neither its hooks nor its Collector. A key another Adapter owns is Codex's to take.
#[test]
fn creating_an_actor_that_claims_a_codex_owned_setting_is_refused() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fixture.start_daemon();

    for owned in ["hooks", "otel"] {
        let refused = fixture
            .command(&[
                "actor",
                "create",
                "claimed",
                "--kind",
                "agent",
                "--runtime",
                "codex",
                "--runtime-settings",
                &json!({ owned: {} }).to_string(),
                "--workdir",
                "/tmp",
                "--no-start",
            ])
            .output()
            .unwrap();
        // The launch spec is refused, so the Actor is not created.
        assert_eq!(refused.status.code(), Some(1), "{owned} was admitted");
        let reason = String::from_utf8_lossy(&refused.stderr);
        assert!(
            reason.contains(&format!(
                "'{owned}' is owned by the 'codex' Runtime Adapter"
            )),
            "{reason}"
        );
    }
    assert!(
        fixture.json(&["actor", "list", "--json"])["actors"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // What another Adapter owns is an ordinary setting of Codex's own `config.toml`.
    fixture.json(&[
        "actor",
        "create",
        "admitted",
        "--kind",
        "agent",
        "--runtime",
        "codex",
        "--runtime-settings",
        &json!({"statusLine": {"type": "command"}}).to_string(),
        "--workdir",
        "/tmp",
        "--no-start",
        "--json",
    ]);
}

/// A Codex sandbox open to the network reaches the tmux socket of its pane, and `feld actor
/// create` tells so before it asks for the Session: a start that then fails still follows
/// the note.
#[test]
fn a_codex_sandbox_open_to_the_network_on_tmux_is_noted_before_the_start() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fixture.start_daemon();
    let output = fixture
        .command(&[
            "actor",
            "create",
            "open",
            "--kind",
            "agent",
            "--runtime",
            "codex",
            "--provider",
            "tmux",
            "--runtime-settings",
            &json!({
                "sandbox_mode": "workspace-write",
                "sandbox_workspace_write.network_access": true,
            })
            .to_string(),
            "--workdir",
            fixture.home().to_str().unwrap(),
            "--tmux-socket",
            fixture.socket.to_str().unwrap(),
            "--tmux-unit",
            "window",
            "--tmux-session",
            "missing",
            "--json",
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    let note = stderr
        .find("note: the Codex sandbox is open to the network on tmux")
        .unwrap_or_else(|| panic!("no note: {stderr}"));
    let refusal = stderr
        .find("Actor declaration committed")
        .unwrap_or_else(|| panic!("the start did not fail: {stderr}"));
    assert!(note < refusal, "{stderr}");
}

/// A tmux server a person started before the daemon lends every pane its own environment,
/// which has no `CODEX_HOME`. The Codex Feld starts there still writes its rollout under
/// the daemon's configuration root, where the Adapter looks for it.
#[test]
fn owned_codex_in_an_existing_server_writes_its_rollout_under_the_daemons_root() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let launch = fixture.home().join("codex-launch.txt");
    let started = support::FakeCodex::new()
        .recording(&launch)
        .with_rollout("parent", PARENT_THREAD)
        .compile(&fixture.home().join("codex"));
    if !started {
        return;
    }
    let mut server = Command::new("tmux");
    support::without_outside_variables(&mut server)
        .arg("-S")
        .arg(&fixture.socket)
        .args(["new-session", "-d", "-s", "ui", "/bin/sh"])
        .env("HOME", fixture.home());
    for variable in ["CODEX_HOME", "TMUX", "TMUX_PANE"] {
        server.env_remove(variable);
    }
    assert!(server.status().unwrap().success());
    fixture.start_daemon();
    fixture.json(&[
        "actor",
        "create",
        "codex-config",
        "--kind",
        "agent",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
        "--workdir",
        fixture.home().to_str().unwrap(),
        "--tmux-socket",
        fixture.socket.to_str().unwrap(),
        "--json",
    ]);

    let (_, environment) = support::wait_for_codex_launch(&launch);
    assert_eq!(
        environment.get("CODEX_HOME").map(String::as_str),
        fixture.codex_home().to_str()
    );
    let rollout = support::codex_rollout_path(&fixture.codex_home(), PARENT_THREAD);
    wait_until(
        || format!("no rollout at {}", rollout.display()),
        || rollout.exists(),
    );
    assert!(!support::codex_rollout_path(&fixture.home().join(".codex"), PARENT_THREAD).exists());
}

/// A Codex whose threads run in an app-server apart from the TUI in the pane: the tools of a
/// thread are children of the app-server and carry the pane of whoever started it, a pane
/// that is gone. `--self` cannot tell the pane from there, so it lists the panes with Codex in
/// front of them and the person names one. The tools of that thread then find the token by
/// the thread, while another thread of the same app-server and a process outside it do not.
#[test]
fn attached_codex_whose_threads_run_in_an_app_server_is_found_by_its_thread() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let daemon = fixture.start_daemon();
    let api = daemon["daemon"]["api"].as_str().unwrap().to_owned();
    let workdir = fixture.home().join("served");
    fs::create_dir_all(&workdir).unwrap();
    let steps = fixture.home().join("steps");
    fs::create_dir_all(&steps).unwrap();

    // The TUI a person started in a pane, which holds the pane and runs no thread itself.
    if !support::FakeCodex::new().compile(&workdir.join("codex")) {
        return;
    }
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "tui",
                "-c",
                workdir.to_str().unwrap(),
                "--",
                "exec ./codex",
            ])
            .status
            .success()
    );
    let pane = String::from_utf8(
        fixture
            .tmux(&["display-message", "-p", "-t", "tui", "#{pane_id}"])
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    let socket = fixture.socket.display().to_string();

    // What the tools of a thread run, one step at a time: each writes its output, its errors,
    // and its exit code under `steps`.
    let feld = |arguments: &str| format!("'{FELD}' --home \"$FELD_HOME\" {arguments}");
    let other_thread =
        format!("env CODEX_THREAD_ID={NEW_CHAT_THREAD} CODEX_SESSION_ID={NEW_CHAT_THREAD}");
    let take_pane = format!(
        "actor attach --self --runtime codex --tmux-pane {pane} --tmux-socket '{socket}' --json"
    );
    // Codex names the rollout file by the root it resolved, as the attach does.
    let rollout = support::codex_rollout_path(
        &fs::canonicalize(fixture.codex_home()).unwrap(),
        SERVED_THREAD,
    );
    fs::write(
        fixture.home().join("stop.json"),
        json!({
            "hook_event_name": "Stop",
            "session_id": SERVED_THREAD,
            "transcript_path": rollout,
            "cwd": workdir,
            "model": "gpt-5.6",
        })
        .to_string(),
    )
    .unwrap();
    let tools = [
        // The pane in the environment is gone, so the attach lists the panes instead.
        feld("actor attach --self --runtime codex"),
        // The person names the pane the listing offered.
        feld(&take_pane),
        feld("self get --json"),
        // Another thread of the same app-server has no token.
        format!("{other_thread} {}", feld("self token")),
        // A hook starts with no thread in its environment and reads it from its event.
        format!(
            "env -u CODEX_THREAD_ID -u CODEX_SESSION_ID '{}' < '{}'",
            fixture
                .home()
                .join("plugins/runtime/codex/bin/hook")
                .display(),
            fixture.home().join("stop.json").display()
        ),
        feld("self get --json"),
        // A new chat moves the TUI to another thread, which the person attaches again.
        format!("{other_thread} {}", feld(&take_pane)),
        feld("self token"),
        format!("{other_thread} {}", feld("self token")),
        // The listing under `--json` carries the same next step.
        feld("actor attach --self --runtime codex --json"),
    ];
    let mut script = String::from(
        "step() { n=$1; shift; sh -c \"$1\" > \"$STEPS/$n.out\" 2> \"$STEPS/$n.err\"; echo $? > \"$STEPS/$n.code\"; }\n",
    );
    for (index, tool) in tools.iter().enumerate() {
        script.push_str(&format!(
            "step {} '{}'\n",
            index + 1,
            tool.replace('\'', "'\\''")
        ));
    }
    script.push_str("echo done > \"$STEPS/done\"\n");
    fs::write(fixture.home().join("tools.sh"), script).unwrap();

    // The app-server, started from a pane that is gone by now: its tools inherit that pane.
    let server = fixture.home().join("app-server");
    fs::create_dir_all(&server).unwrap();
    if !support::FakeCodex::new()
        .with_rollout("exec", SERVED_THREAD)
        .with_rollout("exec", NEW_CHAT_THREAD)
        .beside(&format!(
            "sh '{}'",
            fixture.home().join("tools.sh").display()
        ))
        .compile(&server.join("codex"))
    {
        return;
    }
    let mut app_server = Command::new(server.join("codex"));
    support::without_outside_variables(&mut app_server)
        .current_dir(&workdir)
        .env("HOME", fixture.home())
        .env("FELD_HOME", fixture.home())
        .env("CODEX_HOME", fixture.codex_home())
        .env("TMUX", format!("{socket},1,0"))
        .env("TMUX_PANE", "%99")
        .env("CODEX_THREAD_ID", SERVED_THREAD)
        .env("CODEX_SESSION_ID", SERVED_THREAD)
        .env("STEPS", &steps)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut app_server = app_server.spawn().unwrap();
    wait_for_file(&steps.join("done"), &steps.join("1.err"));
    let step = |number: usize| {
        let read =
            |suffix: &str| fs::read_to_string(steps.join(format!("{number}.{suffix}"))).unwrap();
        (
            read("code").trim().parse::<i32>().unwrap(),
            read("out"),
            read("err"),
        )
    };

    let (code, _, listing) = step(1);
    assert_eq!(code, 1, "{listing}");
    assert!(
        listing.contains(&format!("--tmux-pane {pane} --tmux-socket"))
            && listing.contains("--no-daemon"),
        "{listing}"
    );
    let next = "ask the person which pane shows this Codex, and attach again with its line below";
    let mut lines = listing.lines();
    assert!(
        lines.next().is_some_and(|line| line.starts_with("Error: "))
            && lines.next() == Some(format!("next: {next}").as_str()),
        "{listing}"
    );
    let (code, attached, error) = step(2);
    assert_eq!(code, 0, "{error}");
    let attached: Value = serde_json::from_str(&attached).unwrap();
    let session = attached["session"]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        attached["session"]["placement"]["placement"]["paneId"],
        pane
    );
    let (code, read, error) = step(3);
    assert_eq!(code, 0, "{error}");
    assert_eq!(
        serde_json::from_str::<Value>(&read).unwrap()["session"]["id"],
        session
    );
    let (code, token, _) = step(4);
    assert_eq!((code, token.as_str()), (1, ""));
    // The hook delivered the identity of its event's thread with the model.
    let (code, _, error) = step(5);
    assert_eq!(code, 0, "{error}");
    let (code, read, error) = step(6);
    assert_eq!(code, 0, "{error}");
    let read: Value = serde_json::from_str(&read).unwrap();
    assert_eq!(
        read["session"]["model"],
        json!(["Codex", "gpt-5.6"]),
        "{read}"
    );

    // The attach from the new chat took the same Runtime again: the Session goes on with a new
    // token, which only the new thread finds.
    let (code, attached, error) = step(7);
    assert_eq!(code, 0, "{error}");
    let attached: Value = serde_json::from_str(&attached).unwrap();
    assert_eq!(attached["session"]["id"], session);
    assert_eq!(attached["reattached"], true);
    // The Session now runs the new thread: the attach delivered its identity, which the
    // hook of the next turn completes with the model.
    assert!(!error.contains("not delivered"), "{error}");
    let read = fixture.get(&api, &format!("/v1/sessions/{session}"));
    assert_eq!(read["session"]["model"], json!(["Codex", null]), "{read}");
    assert_eq!(step(8).0, 1);
    let (code, token, error) = step(9);
    assert_eq!(code, 0, "{error}");
    assert_eq!(token.trim(), attached["token"].as_str().unwrap());
    let (code, _, error) = step(10);
    assert_eq!(code, 1, "{error}");
    let error: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["error"]["next"], next, "{error}");

    // A process outside the app-server names the thread in vain. It is left to launchd
    // here, so that no Codex the tests themselves run under is among its parents.
    let outside = fixture.home().join("outside");
    let detached = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!(
            "( {} > '{out}.out' 2> '{out}.err'; echo $? > '{out}.code' ) & exit 0",
            feld("self token"),
            out = outside.display()
        ))
        .env("FELD_HOME", fixture.home())
        .env("HOME", fixture.home())
        .env("CODEX_HOME", fixture.codex_home())
        .env("CODEX_THREAD_ID", NEW_CHAT_THREAD)
        .env("CODEX_SESSION_ID", NEW_CHAT_THREAD)
        .env_remove("FELD_SESSION_TOKEN")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .status()
        .unwrap();
    assert!(detached.success());
    let code = wait_for_file(
        &outside.with_extension("code"),
        &outside.with_extension("err"),
    );
    assert_eq!(code.trim(), "1");

    let _ = app_server.kill();
    let _ = app_server.wait();
}
