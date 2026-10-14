//! End-to-end coverage of a Claude Code Session without a real `claude` binary: a fake
//! Runtime records what Feld launched it with, and the test then plays the parts Claude
//! Code plays around it (hook, otelHeadersHelper, OTLP exporter, status line) against the
//! real daemon, Collector, store, and projections.

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
/// A logs export captured from Claude Code 2.1.273: two `api_request` events (1563 ms and
/// 802 ms), one `tool_result`, and events the projection ignores.
const REAL_CLAUDE_LOGS: &[u8] = include_bytes!("fixtures/claude-2.1.273-logs.pb");
const CLAUDE_SOURCE: &str = "Claude Code Runtime telemetry";
const STATUS_SOURCE: &str = "Claude Code status line";
const NO_USAGE: &str = "No correlated Runtime usage record";
const RUNTIME_UNPREPARED: &str = "Runtime did not start with this daemon's telemetry environment; start it again from a shell that ran `eval \"$(feld env)\"`";

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
        let socket = temporary.path().join("runtime.sock");
        support::install_plugins(temporary.path());
        Some(Self {
            temporary,
            socket,
            client: Client::new(),
        })
    }

    fn home(&self) -> &Path {
        self.temporary.path()
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(self.home())
            .env("HOME", self.home())
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

    /// Starts the daemon with the fixture home first on `PATH`, where the fake `claude` is.
    /// The daemon is started from inside a Claude Code, as a person starting one from their
    /// own Claude Code does, so the tmux server it starts and every pane under it inherit
    /// that Claude Code's markers.
    fn start_daemon(&self) -> Value {
        self.start_daemon_with(&BTreeMap::new())
    }

    /// Starts the daemon as `start_daemon` does, with `environment` added to its own.
    fn start_daemon_with(&self, environment: &BTreeMap<String, String>) -> Value {
        let path = format!(
            "{}:{}",
            self.home().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = self
            .command(&["daemon", "start", "--json"])
            .env("PATH", path)
            .env("CLAUDECODE", "1")
            .env("CLAUDE_CODE_CHILD_SESSION", "1")
            .env(
                "CLAUDE_CODE_SESSION_ID",
                "9a8b7c6d-5e4f-3a2b-1c0d-9e8f7a6b5c4d",
            )
            .env("CLAUDE_CODE_SESSION_ATTENDED", "1")
            .env("CLAUDE_PID", "4242")
            .envs(environment)
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
    /// variables of a Session the tests may run inside are kept away from the fake Runtime.
    fn tmux(&self, arguments: &[&str]) -> Output {
        let mut command = Command::new("tmux");
        support::without_outside_variables(&mut command)
            .arg("-S")
            .arg(&self.socket)
            .args(arguments);
        for variable in ["CLAUDE_CONFIG_DIR", "TMUX", "TMUX_PANE"] {
            command.env_remove(variable);
        }
        command.output().unwrap()
    }

    fn new_tmux_session(&self, name: &str, workdir: &Path) {
        self.new_tmux_session_with(name, workdir, &BTreeMap::new());
    }

    /// Starts the fake `claude` in a new pane whose environment adds `environment`, as a
    /// shell that ran `eval "$(feld env)"` would start it.
    fn new_tmux_session_with(
        &self,
        name: &str,
        workdir: &Path,
        environment: &BTreeMap<String, String>,
    ) {
        let mut arguments = vec![
            "new-session".to_owned(),
            "-d".to_owned(),
            "-s".to_owned(),
            name.to_owned(),
            "-c".to_owned(),
            workdir.display().to_string(),
        ];
        for (key, value) in environment {
            arguments.push("-e".to_owned());
            arguments.push(format!("{key}={value}"));
        }
        arguments.extend(["--".to_owned(), "exec ./claude".to_owned()]);
        let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
        assert!(self.tmux(&arguments).status.success());
    }

    /// What `feld env` exports for the fixture home.
    fn shell_environment(&self) -> BTreeMap<String, String> {
        serde_json::from_value(self.json(&["env", "--json"])).unwrap()
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

    fn session_metrics(&self, api: &str, session: &str, query: &str) -> Value {
        self.get(api, &format!("/v1/sessions/{session}/metrics{query}"))
    }

    /// Polls the whole-Session metrics until `ready` accepts them.
    fn wait_for_metrics(&self, api: &str, session: &str, ready: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let metrics = self.session_metrics(api, session, "");
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

    fn telemetry_count(&self) -> i64 {
        rusqlite::Connection::open(self.home().join("admin/feld.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM telemetries", [], |row| row.get(0))
            .unwrap()
    }

    /// Sends one OTLP/HTTP export to the Collector the way Claude Code's exporter does,
    /// with the headers the otelHeadersHelper printed.
    fn export(&self, collector: &str, signal: &str, headers: &Value, body: Vec<u8>) -> u16 {
        let mut request = self
            .client
            .post(format!("{collector}/v1/{signal}"))
            .header("content-type", "application/x-protobuf")
            .body(body);
        for (name, value) in headers.as_object().unwrap() {
            request = request.header(name.as_str(), value.as_str().unwrap());
        }
        request.send().unwrap().status().as_u16()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.command(&["daemon", "stop"]).output();
        let _ = self.tmux(&["kill-server"]);
        support::stop_runtime_helpers(self.home());
    }
}

/// Compiles a fake `claude`: it records its argv and environment to `record`, runs
/// `child` through the shell, and stays alive like an idle Claude Code.
fn compile_runtime(path: &Path, record: Option<&Path>, child: Option<&str>) -> bool {
    let escape = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
    let record = record
        .map(|record| {
            format!(
                "FILE *f = fopen(\"{}\", \"w\"); if (f) {{ for (int i = 0; i < argc; i++) fprintf(f, \"%s\\n\", argv[i]); fprintf(f, \"\\n\"); for (char **e = environ; *e; e++) fprintf(f, \"%s\\n\", *e); fclose(f); }}",
                escape(&record.display().to_string())
            )
        })
        .unwrap_or_default();
    let child = child
        .map(|command| format!("if (system(\"{}\") != 0) return 2;", escape(command)))
        .unwrap_or_default();
    support::compile_program(
        path,
        &format!(
            "#include <stdio.h>\n#include <stdlib.h>\n#include <unistd.h>\nextern char **environ;\nint main(int argc, char **argv) {{ {record} {child} sleep(300); return 0; }}\n"
        ),
    )
}

/// Waits for the fake `claude` to record its launch: argv, then its environment.
fn wait_for_launch(record: &Path) -> (Vec<String>, BTreeMap<String, String>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = fs::read_to_string(record).unwrap_or_default();
        if let Some((argv, environment)) = text.split_once("\n\n")
            && text.ends_with('\n')
        {
            let environment = environment
                .lines()
                .filter_map(|line| line.split_once('='))
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect();
            return (argv.lines().map(str::to_owned).collect(), environment);
        }
        assert!(Instant::now() < deadline, "claude was not launched");
        thread::sleep(Duration::from_millis(25));
    }
}

/// Runs one Runtime-side part (hook, helper) as Claude Code would: with the launch
/// environment and the JSON Claude Code passes on stdin.
fn run_part(command: &Path, environment: &BTreeMap<String, String>, stdin: &str) -> Output {
    let mut child = Command::new(command)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("run {}: {error}", command.display()));
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{} failed: {}",
        command.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn wait_for_file(path: &Path, stderr: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
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

// --- Claude Code's OTLP/HTTP exports ---------------------------------------------------

fn resource() -> proto::Resource {
    proto::Resource::new(vec![proto::KeyValue::new("service.name", "claude-code")])
}

fn scope(name: &str) -> proto::InstrumentationScope {
    proto::InstrumentationScope::new(name, "2.1.273")
}

/// One event as Claude Code writes it: the fully qualified name in the body and the short
/// one in the `event.name` attribute.
fn log_record(time: u64, event: &str, attributes: &[(&str, proto::AnyValue)]) -> proto::LogRecord {
    attributes.iter().cloned().fold(
        proto::LogRecord::new(time)
            .with_body(format!("claude_code.{event}"))
            .with("event.name", event),
        |record, (key, value)| record.with(key, value),
    )
}

fn logs_request(records: Vec<proto::LogRecord>) -> Vec<u8> {
    proto::ExportLogsServiceRequest::new(
        Some(resource()),
        Some(scope("com.anthropic.claude_code.events")),
        records,
    )
    .encode_to_vec()
}

/// One export interval of Claude Code metrics: token usage by type, cost, and active time,
/// plus a metric the projection does not use.
fn claude_metrics_export(
    start: u64,
    time: u64,
    tokens: &[(&str, i64)],
    cost: f64,
    active: &[(&str, i64)],
) -> Vec<u8> {
    let delta = proto::AggregationTemporality::Delta;
    let token_points = tokens
        .iter()
        .map(|(kind, value)| {
            proto::NumberDataPoint::int(start, time, *value)
                .with("type", *kind)
                .with("model", "claude-opus-5")
        })
        .collect();
    let active_points = active
        .iter()
        .map(|(kind, value)| proto::NumberDataPoint::int(start, time, *value).with("type", *kind))
        .collect();
    proto::ExportMetricsServiceRequest::new(
        Some(resource()),
        Some(scope("com.anthropic.claude_code")),
        vec![
            proto::Metric::sum("claude_code.token.usage", delta, token_points),
            proto::Metric::sum(
                "claude_code.cost.usage",
                delta,
                vec![
                    proto::NumberDataPoint::double(start, time, cost)
                        .with("model", "claude-opus-5"),
                ],
            ),
            proto::Metric::sum("claude_code.active_time.total", delta, active_points),
            proto::Metric::sum(
                "claude_code.lines_of_code.count",
                delta,
                vec![proto::NumberDataPoint::int(start, time, 7).with("type", "added")],
            ),
        ],
    )
    .encode_to_vec()
}

// --- Tests -------------------------------------------------------------------------------

/// An owned Claude Code Session, end to end: Feld launches the Runtime with its settings
/// file and telemetry environment; the hook delivers the Runtime identity; the
/// otelHeadersHelper signs the exports; metrics and events flow through the Collector into
/// the store and come back as Session metrics, their bins, and quota; the transcript
/// projects Sub Agent Sessions, including ones that end by failure, kill, or a person's
/// stop; and closing the Session keeps its metrics while refusing new telemetry.
#[test]
fn owned_claude_code_session_projects_telemetry_and_sub_agents() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let launch = fixture.home().join("claude-launch.txt");
    if !compile_runtime(&fixture.home().join("claude"), Some(&launch), None) {
        return;
    }
    let daemon = fixture.start_daemon();
    let api = daemon["daemon"]["api"].as_str().unwrap().to_owned();
    let collector = daemon["daemon"]["otlp"].as_str().unwrap().to_owned();
    fixture.json(&[
        "actor",
        "create",
        "claude-e2e",
        "--kind",
        "agent",
        "--no-start",
        "--runtime",
        "claude-code",
        "--workdir",
        fixture.home().to_str().unwrap(),
        "--tmux-socket",
        fixture.socket.to_str().unwrap(),
        "--json",
    ]);
    let spawned = fixture.json(&["actor", "start", "claude-e2e", "--json"]);
    let session = spawned["session"]["id"].as_str().unwrap().to_owned();
    assert_eq!(spawned["session"]["ownership"], "owned");
    let session_start = seconds_of(&spawned["session"]["start"]);

    // 1. The launch: argv points at the Session's settings file and plugin, and the
    //    environment carries the Feld and OTel variables the spec lists.
    let (argv, environment) = wait_for_launch(&launch);
    assert_eq!(argv[0], "claude");
    assert_eq!(argv[1], "--settings");
    assert_eq!(argv[3], "--plugin-dir");
    let plugin = PathBuf::from(&argv[4]);
    let settings: Value = serde_json::from_slice(&fs::read(&argv[2]).unwrap()).unwrap();
    assert_eq!(
        settings["otelHeadersHelper"],
        format!("'{}'", plugin.join("bin/otel-headers").display())
    );
    assert_eq!(
        settings["statusLine"]["command"],
        format!("'{}'", plugin.join("bin/status-line").display())
    );
    assert_eq!(settings["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"], collector);
    // The sandbox and the Read tool are kept from the Feld home but the Session's own
    // directory of the spool, which the daemon made before the launch.
    let root = fs::canonicalize(fixture.home()).unwrap();
    let own_spool = root.join("spool").join(&session);
    assert!(own_spool.is_dir(), "{}", own_spool.display());
    let denied = ["admin", "spool", "tokens"].map(|name| root.join(name).display().to_string());
    assert_eq!(settings["sandbox"]["filesystem"]["denyRead"], json!(denied));
    for key in ["allowRead", "allowWrite"] {
        assert_eq!(
            settings["sandbox"]["filesystem"][key],
            json!([own_spool.display().to_string()]),
            "{key}"
        );
    }
    assert_eq!(
        settings["permissions"]["deny"],
        json!(denied.map(|path| format!("Read(/{path}/**)")))
    );
    for (key, expected) in [
        ("FELD_API", api.as_str()),
        ("FELD_ACTOR", "claude-e2e"),
        ("FELD_WORKSPACE", "default"),
        ("FELD_HOME", fixture.home().to_str().unwrap()),
        ("FELD_SESSION_ID", session.as_str()),
        ("CLAUDE_CODE_ENABLE_TELEMETRY", "1"),
        ("OTEL_METRICS_EXPORTER", "otlp"),
        ("OTEL_LOGS_EXPORTER", "otlp"),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
        ("OTEL_EXPORTER_OTLP_ENDPOINT", collector.as_str()),
        ("OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE", "delta"),
        ("OTEL_METRIC_EXPORT_INTERVAL", "10000"),
        // The markers the tmux server inherited from the Claude Code the daemon was
        // started in are cancelled, so this Claude Code keeps its own transcript.
        ("CLAUDECODE", ""),
        ("CLAUDE_CODE_CHILD_SESSION", ""),
        ("CLAUDE_CODE_SESSION_ID", ""),
        ("CLAUDE_CODE_SESSION_ATTENDED", ""),
        ("CLAUDE_PID", ""),
        ("CLAUDE_CODE_FORCE_SESSION_PERSISTENCE", "1"),
    ] {
        assert_eq!(
            environment.get(key).map(String::as_str),
            Some(expected),
            "{key}"
        );
    }
    let token = environment["FELD_SESSION_TOKEN"].clone();
    assert!(!token.is_empty());
    assert_eq!(settings["env"]["FELD_SESSION_TOKEN"], token);

    // 2. The SessionStart hook delivers the Runtime identity from Claude Code's hook input.
    let runtime_session = "0f1e2d3c-4b5a-6978-8a9b-0c1d2e3f4a5b";
    let records = fixture.home().join("records");
    fs::create_dir(&records).unwrap();
    let transcript = records.join(format!("{runtime_session}.jsonl"));
    let started = run_part(
        &plugin.join("bin/hook"),
        &environment,
        &json!({
            "session_id": runtime_session,
            "transcript_path": transcript,
            "model": "claude-opus-5",
            "hook_event_name": "SessionStart",
            "cwd": fixture.home(),
        })
        .to_string(),
    );
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
    assert!(context.contains("`show`"), "{context}");
    assert!(context.contains("`fromKind`"), "{context}");
    let read = fixture.get(&api, &format!("/v1/sessions/{session}"));
    assert_eq!(
        read["session"]["model"],
        json!(["Claude Code", "claude-opus-5"])
    );
    let page = fixture.get(&api, &format!("/v1/sessions/{session}/transcript"));
    assert_eq!(page["state"], "missing", "{page}");
    // The Claude Code plugin the Adapter wrote declares every event that carries the
    // identity, and each of them reaches the daemon through the same helper.
    let hooks: Value =
        serde_json::from_slice(&fs::read(plugin.join("hooks/hooks.json")).unwrap()).unwrap();
    let declared = hooks["hooks"].as_object().unwrap();
    assert_eq!(
        declared.keys().cloned().collect::<Vec<_>>(),
        vec!["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop"]
    );
    for (event, declaration) in declared {
        assert_eq!(
            declaration[0]["hooks"][0]["command"], "${CLAUDE_PLUGIN_ROOT}/bin/hook",
            "{event}"
        );
    }
    run_part(
        &plugin.join("bin/hook"),
        &environment,
        &json!({
            "session_id": runtime_session,
            "transcript_path": transcript,
            "model": "claude-opus-4-5",
            "hook_event_name": "PostToolUse",
            "cwd": fixture.home(),
        })
        .to_string(),
    );
    let read = fixture.get(&api, &format!("/v1/sessions/{session}"));
    assert_eq!(
        read["session"]["model"],
        json!(["Claude Code", "claude-opus-4-5"]),
        "PostToolUse re-delivers the identity: {read}"
    );
    // Claude Code starts the Stop hook without `model` and before it writes the turn's
    // response to the transcript; the hook waits for the response and delivers its model.
    fs::write(
        &transcript,
        format!(
            "{}\n",
            json!({"type": "user", "message": {"role": "user", "content": "one"}})
        ),
    )
    .unwrap();
    let stop = thread::spawn({
        let hook = plugin.join("bin/hook");
        let environment = environment.clone();
        let input = json!({
            "session_id": runtime_session,
            "transcript_path": transcript,
            "hook_event_name": "Stop",
            "cwd": fixture.home(),
        })
        .to_string();
        move || run_part(&hook, &environment, &input)
    });
    thread::sleep(Duration::from_millis(500));
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    writeln!(
        file,
        "{}",
        json!({"type": "assistant", "message": {"model": "claude-opus-5", "role": "assistant", "content": [{"type": "text", "text": "pong"}]}})
    )
    .unwrap();
    stop.join().unwrap();
    let read = fixture.get(&api, &format!("/v1/sessions/{session}"));
    assert_eq!(
        read["session"]["model"],
        json!(["Claude Code", "claude-opus-5"]),
        "the Stop hook delivers the model of the response written after it started: {read}"
    );

    // 3. The otelHeadersHelper signs exports with the Session's telemetry token; an export without
    //    it (the header of a Claude Code before attach) is refused by the Collector.
    let headers: Value = serde_json::from_slice(
        &run_part(&plugin.join("bin/otel-headers"), &environment, "").stdout,
    )
    .unwrap();
    assert_eq!(
        headers["Authorization"],
        format!("Bearer {}", feld_core::telemetry::telemetry_token(&token))
    );
    let base = (session_start / 60 + 1) * 60;
    let first = claude_metrics_export(
        nanos(base),
        nanos(base + 5),
        &[
            ("input", 1_200),
            ("output", 300),
            ("cacheRead", 8_000),
            ("cacheCreation", 500),
        ],
        0.0125,
        &[("user", 12), ("cli", 30)],
    );
    assert_eq!(
        fixture.export(&collector, "metrics", &json!({}), first.clone()),
        401
    );
    assert_eq!(fixture.export(&collector, "metrics", &headers, first), 200);
    let second = claude_metrics_export(
        nanos(base + 60),
        nanos(base + 65),
        &[("input", 800), ("output", 200), ("cacheRead", 4_000)],
        0.0075,
        &[("user", 8), ("other", 5)],
    );
    assert_eq!(fixture.export(&collector, "metrics", &headers, second), 200);
    // Events as Claude Code emits them: request durations as integers, the tool result's
    // as a string, and token counts on the request that the projection must not add to
    // the token usage metric.
    let events = logs_request(vec![
        log_record(
            nanos(base + 5),
            "api_request",
            &[
                ("duration_ms", 1_500_i64.into()),
                ("input_tokens", 1_200_i64.into()),
                ("output_tokens", 300_i64.into()),
                ("model", "claude-opus-5".into()),
            ],
        ),
        log_record(
            nanos(base + 5),
            "api_error",
            &[("status_code", 529_i64.into())],
        ),
        log_record(
            nanos(base + 65),
            "api_request",
            &[("duration_ms", 300_i64.into())],
        ),
        log_record(
            nanos(base + 65),
            "tool_result",
            &[("tool_name", "Bash".into()), ("duration_ms", "17".into())],
        ),
        log_record(
            nanos(base + 66),
            "tool_result",
            &[("tool_name", "Read".into())],
        ),
        log_record(
            nanos(base + 66),
            "user_prompt",
            &[("prompt_length", 42_i64.into())],
        ),
    ]);
    assert_eq!(fixture.export(&collector, "logs", &headers, events), 200);
    assert_eq!(
        fixture.export(&collector, "logs", &headers, REAL_CLAUDE_LOGS.to_vec()),
        200
    );

    // 4. The whole-Session projection sums every Telemetry, the real export included.
    let metrics = fixture.wait_for_metrics(&api, &session, |metrics| {
        metric(metrics, "tool_calls")["series"][0]["value"] == 3.0
            && metric(metrics, "token_usage")["series"][0]["value"]["total"] == 15_000.0
    });
    let tokens = assert_reported(&metrics, "token_usage", CLAUDE_SOURCE);
    assert_eq!(tokens.as_array().unwrap().len(), 1);
    assert_eq!(
        tokens[0]["value"],
        json!({
            "input": 2_000.0,
            "output": 500.0,
            "cache_read": 12_000.0,
            "cache_creation": 500.0,
            "total": 15_000.0
        })
    );
    assert_eq!(tokens[0]["start"], spawned["session"]["start"]);
    assert!(tokens[0]["end"].is_null());
    let hit_rate = assert_reported(&metrics, "prompt_cache_hit_rate", CLAUDE_SOURCE);
    assert!(
        approx(&hit_rate[0]["value"], 12_000.0 / 14_500.0 * 100.0),
        "{hit_rate}"
    );
    let cost = assert_reported(&metrics, "estimated_cost", CLAUDE_SOURCE);
    assert!(approx(&cost[0]["value"], 0.02), "{cost}");
    assert_eq!(
        assert_reported(&metrics, "api_errors", CLAUDE_SOURCE)[0]["value"],
        1.0
    );
    assert_eq!(
        assert_reported(&metrics, "tool_calls", CLAUDE_SOURCE)[0]["value"],
        3.0
    );
    assert_eq!(
        assert_reported(&metrics, "api_request_latency_p95", CLAUDE_SOURCE)[0]["value"],
        1_563.0
    );
    assert_eq!(
        assert_reported(&metrics, "active_time", CLAUDE_SOURCE)[0]["value"],
        50.0
    );
    assert_eq!(
        metric(&metrics, "context_window_utilization")["status"],
        "unavailable"
    );
    assert_eq!(
        metric(&metrics, "context_window_utilization")["source"],
        NO_USAGE
    );

    // 5. Bins: the two export intervals land in their own minutes; a count metric is a
    //    reported 0 where token usage was reported without any event.
    let range = format!("?start={}&end={}&bucket=60", iso(base), iso(base + 120));
    let binned = fixture.session_metrics(&api, &session, &range);
    let tokens = assert_reported(&binned, "token_usage", CLAUDE_SOURCE);
    assert_eq!(tokens.as_array().unwrap().len(), 2, "{tokens}");
    assert_eq!(seconds_of(&tokens[0]["start"]), base);
    assert_eq!(seconds_of(&tokens[0]["end"]), base + 60);
    assert_eq!(tokens[0]["value"]["total"], 10_000.0);
    assert_eq!(seconds_of(&tokens[1]["start"]), base + 60);
    assert_eq!(tokens[1]["value"]["total"], 5_000.0);
    let errors = assert_reported(&binned, "api_errors", CLAUDE_SOURCE);
    assert_eq!(errors[0]["value"], 1.0);
    assert_eq!(errors[1]["value"], 0.0);
    let tools = assert_reported(&binned, "tool_calls", CLAUDE_SOURCE);
    assert_eq!(tools[0]["value"], 0.0);
    assert_eq!(tools[1]["value"], 2.0);
    let latency = assert_reported(&binned, "api_request_latency_p95", CLAUDE_SOURCE);
    assert_eq!(latency[0]["value"], 1_500.0);
    assert_eq!(latency[1]["value"], 300.0);
    let active = assert_reported(&binned, "active_time", CLAUDE_SOURCE);
    assert_eq!(active[0]["value"], 42.0);
    assert_eq!(active[1]["value"], 8.0);
    let cost = assert_reported(&binned, "estimated_cost", CLAUDE_SOURCE);
    assert!(approx(&cost[0]["value"], 0.0125) && approx(&cost[1]["value"], 0.0075));

    // 6. The status line helper turns Claude Code's status JSON into gauges: context window
    //    utilization for the Session and rate limits for the account quota, under the
    //    account its configuration root is signed in with.
    let reset_five = Utc::now().timestamp() + 3_600;
    let reset_seven = Utc::now().timestamp() + 5 * 86_400;
    let config_root = fixture.home().join("claude-config");
    fs::create_dir_all(&config_root).unwrap();
    fs::write(
        config_root.join(".claude.json"),
        json!({"oauthAccount": {"accountUuid": "uuid-e2e", "emailAddress": "e2e@example.com"}})
            .to_string(),
    )
    .unwrap();
    let mut signed_in = environment.clone();
    signed_in.insert(
        "CLAUDE_CONFIG_DIR".to_owned(),
        config_root.display().to_string(),
    );
    let status_line = run_part(
        &plugin.join("bin/status-line"),
        &signed_in,
        &json!({
            "model": {"id": "claude-opus-5"},
            "context_window": {"context_window_size": 200_000, "used_percentage": 42.0},
            "rate_limits": {
                "five_hour": {"used_percentage": 17.0, "resets_at": reset_five},
                "seven_day": {"used_percentage": 3.0, "resets_at": reset_seven}
            }
        })
        .to_string(),
    );
    assert_eq!(
        String::from_utf8_lossy(&status_line.stdout),
        "claude-e2e · context 42% · 5h 17% · 7d 3%\n"
    );
    let metrics = fixture.wait_for_metrics(&api, &session, |metrics| {
        metric(metrics, "context_window_utilization")["status"] == "reported"
    });
    let context = assert_reported(&metrics, "context_window_utilization", STATUS_SOURCE);
    assert_eq!(
        context[0]["value"],
        json!({"size": 200_000.0, "used_percent": 42.0})
    );
    let quota = fixture.get(&api, "/v1/runtimes/claude-code/metrics?metric=quota");
    let claude_quota = &quota["quota"];
    assert_eq!(claude_quota["status"], "reported", "{quota}");
    for (window, used, reset) in [
        ("five_hour", 17.0, reset_five),
        ("seven_day", 3.0, reset_seven),
    ] {
        let series = claude_quota["series"]
            .as_array()
            .unwrap()
            .iter()
            .find(|series| series["window"] == window)
            .unwrap_or_else(|| panic!("{window} quota is missing: {quota}"));
        assert_eq!(series["latest"]["usedPercent"], used);
        assert_eq!(series["resetsAt"], iso(reset));
        assert_eq!(series["reportedBy"]["session"], session);
        assert_eq!(series["reportedBy"]["actor"], "claude-e2e");
        assert_eq!(
            series["account"],
            json!({"id": "uuid-e2e", "name": "e2e@example.com"})
        );
    }

    // 7. Sub Agent Sessions come from the transcript the hook located. Each way a Sub Agent
    //    ends gives its Session an `end`: the synchronous tool result, and for background
    //    agents the task notification whatever its status, or the notice that a person
    //    stopped it. One stays open.
    let t = |offset: i64| iso(base + offset);
    let launches = [
        ("agent-sync", "Agent"),
        ("agent-done", "Agent"),
        ("agent-failed", "Task"),
        ("agent-killed", "Agent"),
        ("agent-stopped", "Agent"),
        ("agent-open", "Agent"),
    ]
    .iter()
    .map(|(id, name)| json!({"type": "tool_use", "id": id, "name": name}))
    .collect::<Vec<_>>();
    let notification = |agent: &str, tool: &str, status: &str, at: i64| {
        json!({
            "type": "queue-operation",
            "operation": "enqueue",
            "timestamp": t(at),
            "content": format!("<task-notification><task-id>{agent}</task-id><tool-use-id>{tool}</tool-use-id><status>{status}</status></task-notification>")
        })
    };
    let launched = |tool: &str, agent: Option<&str>, at: i64| {
        let mut result = json!({"isAsync": true, "status": "async_launched"});
        if let Some(agent) = agent {
            result["agentId"] = json!(agent);
        }
        json!({
            "type": "user",
            "timestamp": t(at),
            "message": {"content": [{"type": "tool_result", "tool_use_id": tool}]},
            "toolUseResult": result
        })
    };
    let parent_records = [
        json!({"type": "user", "timestamp": t(0), "message": {"role": "user", "content": "delegate"}}),
        json!({"type": "assistant", "timestamp": t(1), "message": {"role": "assistant", "content": launches}}),
        json!({
            "type": "user", "timestamp": t(5),
            "message": {"content": [{"type": "tool_result", "tool_use_id": "agent-sync"}]},
            "toolUseResult": {"agentId": "sync-1", "status": "completed"}
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
            "type": "user", "timestamp": t(9),
            "origin": {"kind": "task-notification"}, "promptSource": "system",
            "message": {"role": "user", "content": "Background agent \"Stopped job\" was stopped by the user."}
        }),
    ];
    fs::write(
        &transcript,
        parent_records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let subagents = records.join(runtime_session).join("subagents");
    fs::create_dir_all(&subagents).unwrap();
    for agent in [
        "sync-1",
        "done-2",
        "failed-3",
        "killed-4",
        "stopped-5",
        "open-6",
    ] {
        fs::write(
            subagents.join(format!("agent-{agent}.jsonl")),
            format!(
                "{}\n{}\n",
                json!({"type": "user", "agentId": agent, "timestamp": t(2), "message": {"role": "user", "content": "task"}}),
                json!({"type": "assistant", "agentId": agent, "timestamp": t(3), "message": {"role": "assistant", "content": [{"type": "text", "text": "done"}]}})
            ),
        )
        .unwrap();
    }
    fs::write(
        subagents.join("agent-stopped-5.meta.json"),
        json!({"agentType": "general-purpose", "description": "Stopped job", "toolUseId": "agent-stopped", "stoppedByUser": true}).to_string(),
    )
    .unwrap();
    let read = fixture.get(&api, &format!("/v1/sessions/{session}"));
    let sub_agents = read["subAgents"].as_array().unwrap();
    assert_eq!(sub_agents.len(), 6, "{read}");
    let sub_agent = |suffix: &str| {
        sub_agents
            .iter()
            .find(|agent| {
                agent["id"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("_{suffix}"))
            })
            .unwrap_or_else(|| panic!("Sub Agent {suffix} is missing: {read}"))
    };
    for (suffix, end) in [
        ("sync-1", Some(5)),
        ("done-2", Some(6)),
        ("failed-3", Some(7)),
        ("killed-4", Some(8)),
        ("stopped-5", Some(9)),
        ("open-6", None),
    ] {
        let agent = sub_agent(suffix);
        assert_eq!(agent["parentSession"], session, "{suffix}");
        assert_eq!(agent["startKind"], "sub-agent", "{suffix}");
        assert!(agent["placement"].is_null(), "{suffix}");
        assert_eq!(agent["start"], t(2), "{suffix}: {agent}");
        assert_eq!(
            agent["end"],
            end.map(t).map_or(Value::Null, Value::String),
            "{suffix}: {agent}"
        );
        assert_eq!(
            agent["model"],
            json!(["Claude Code", "claude-opus-5"]),
            "{suffix}"
        );
        assert_eq!(agent["consoleState"], "readable", "{suffix}");
    }
    assert_eq!(sub_agent("stopped-5")["subAgent"], "stopped-5");
    assert_eq!(sub_agent("open-6")["outcome"], "running");
    let sub_agent_transcript = fixture.get(
        &api,
        &format!(
            "/v1/sessions/{}/transcript",
            sub_agent("failed-3")["id"].as_str().unwrap()
        ),
    );
    assert_eq!(sub_agent_transcript["state"], "readable");
    assert_eq!(sub_agent_transcript["records"].as_array().unwrap().len(), 2);

    // 8. The parent's metrics over a range come in 60 s bins; the Sub Agents' telemetry is
    //    the parent's.
    let binned =
        fixture.session_metrics(&api, &session, &format!("?start={}&bucket=60", iso(base)));
    let tokens = assert_reported(&binned, "token_usage", CLAUDE_SOURCE);
    assert_eq!(tokens.as_array().unwrap().len(), 2);
    assert_eq!(tokens[0]["value"]["total"], 10_000.0);
    assert_eq!(tokens[1]["value"]["total"], 5_000.0);

    // 9. Closing the Session keeps what was reported and refuses later telemetry: the
    //    Collector accepts the export, the daemon rejects the closed token, and no
    //    Telemetry is stored. The open Sub Agent ends with its parent.
    assert_eq!(fixture.telemetry_count(), 5);
    fixture.json(&["session", "kill", "claude-e2e", "--json"]);
    let deadline = Instant::now() + Duration::from_secs(10);
    let closed = loop {
        let described = fixture.json(&["session", "get", &session, "--json"]);
        if described["session"]["end"].is_string() {
            break described;
        }
        assert!(
            Instant::now() < deadline,
            "Session did not close: {described}"
        );
        thread::sleep(Duration::from_millis(25));
    };
    let session_end = closed["session"]["end"].clone();
    let late = claude_metrics_export(
        nanos(base + 120),
        nanos(base + 125),
        &[("input", 999)],
        0.5,
        &[],
    );
    assert_eq!(fixture.export(&collector, "metrics", &headers, late), 200);
    thread::sleep(Duration::from_millis(500));
    assert_eq!(fixture.telemetry_count(), 5);
    let metrics = fixture.session_metrics(&api, &session, "");
    let tokens = assert_reported(&metrics, "token_usage", CLAUDE_SOURCE);
    assert_eq!(tokens[0]["value"]["total"], 15_000.0);
    assert_eq!(tokens[0]["end"], session_end);
    let read = fixture.get(&api, &format!("/v1/sessions/{session}"));
    let open = read["subAgents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["subAgent"] == "open-6")
        .unwrap();
    assert_eq!(open["end"], session_end);
}

/// Two attached Claude Codes in the configuration root `feld init` prepared. The first
/// starts without `feld env`, so its metrics say it has to start again from such a shell
/// while its status line still reports. The second starts from a shell that ran
/// `feld env`: its helpers resolve the token
/// from the self index, the hook adds the `model` of the transcript's latest response to the
/// identity the CLI delivered, and its exports become metrics.
#[test]
fn attached_claude_code_started_with_feld_env_exports_telemetry() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let daemon = fixture.start_daemon();
    let api = daemon["daemon"]["api"].as_str().unwrap().to_owned();
    let collector = daemon["daemon"]["otlp"].as_str().unwrap().to_owned();
    let config = fixture.home().join("claude-config");
    let project = config.join("projects/-Users-me-work");
    fs::create_dir_all(&project).unwrap();
    // `feld init` writes where the Claude Code being attached reads: its configuration
    // root, which `CLAUDE_CONFIG_DIR` names here.
    let plugin = config.join("skills/feld");
    let init = fixture
        .command(&["init", "--runtime", "claude-code", "--provider", "tmux"])
        .env("CLAUDE_CONFIG_DIR", &config)
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stdout)
    );
    support::warm_up_plugins(fixture.home());
    support::warm_up_runtime_parts(&config);
    let exports = |name: &str, runtime_session: &str| {
        format!(
            "export HOME='{home}' FELD_HOME='{home}' CLAUDE_CONFIG_DIR='{config}' CLAUDE_CODE_SESSION_ID={runtime_session}; cd '{home}/{name}' && {feld} --home '{home}' actor attach --self --runtime claude-code --json > '{home}/{name}.attach.json' 2> '{home}/{name}.err'",
            home = fixture.home().display(),
            config = config.display(),
            feld = FELD,
        )
    };
    let hook_input = |runtime_session: &str| {
        json!({
            "session_id": runtime_session,
            "transcript_path": project.join(format!("{runtime_session}.jsonl")),
            "hook_event_name": "UserPromptSubmit"
        })
        .to_string()
        .replace('\'', "'\\''")
    };
    let status_input = json!({
        "context_window": {"context_window_size": 200_000, "used_percentage": 31.0},
        "rate_limits": {"five_hour": {"used_percentage": 9.0, "resets_at": Utc::now().timestamp() + 1_800}}
    })
    .to_string();

    // The first Claude Code: its status line helper, which Claude Code re-reads while it
    // runs, reports although its telemetry cannot. The attach takes the model of the
    // response already in its transcript.
    let first_runtime = "11111111-1111-4111-8111-111111111111";
    fs::create_dir(fixture.home().join("first")).unwrap();
    fs::write(
        project.join(format!("{first_runtime}.jsonl")),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"one\"}}\n{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-4-5\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n",
    )
    .unwrap();
    let first_child = format!(
        "{}; printf '%s' '{status_input}' | '{}' > '{}'; echo done > '{}'",
        exports("first", first_runtime),
        plugin.join("bin/status-line").display(),
        fixture.home().join("first.status").display(),
        fixture.home().join("first.done").display(),
    );
    if !compile_runtime(
        &fixture.home().join("first/claude"),
        None,
        Some(&first_child),
    ) {
        return;
    }
    fixture.new_tmux_session("first", &fixture.home().join("first"));
    wait_for_file(
        &fixture.home().join("first.done"),
        &fixture.home().join("first.err"),
    );
    let first: Value = serde_json::from_str(&wait_for_file(
        &fixture.home().join("first.attach.json"),
        &fixture.home().join("first.err"),
    ))
    .unwrap();
    let first_session = first["session"]["id"].as_str().unwrap().to_owned();
    assert_eq!(first["session"]["actor"], "first-1");
    assert_eq!(
        fixture.get(&api, &format!("/v1/sessions/{first_session}"))["session"]["model"],
        json!(["Claude Code", "claude-opus-4-5"])
    );
    assert!(config.join("settings.json").is_file());
    let first_metrics = fixture.wait_for_metrics(&api, &first_session, |metrics| {
        metric(metrics, "context_window_utilization")["status"] == "reported"
    });
    assert_eq!(
        metric(&first_metrics, "token_usage")["status"],
        "unavailable"
    );
    assert_eq!(
        metric(&first_metrics, "token_usage")["source"],
        RUNTIME_UNPREPARED
    );
    assert_eq!(
        assert_reported(&first_metrics, "context_window_utilization", STATUS_SOURCE)[0]["value"]["used_percent"],
        31.0
    );
    assert_eq!(
        fs::read_to_string(fixture.home().join("first.status")).unwrap(),
        "first-1 · context 31% · 5h 9%\n"
    );

    // The second Claude Code starts with the prepared settings. Its child processes find
    // the token through the self index: the helper signs exports with it. Its transcript
    // has no model at the attach; the hook delivers the one of the response written after.
    let second_runtime = "22222222-2222-4222-8222-222222222222";
    fs::create_dir(fixture.home().join("second")).unwrap();
    fs::write(
        project.join(format!("{second_runtime}.jsonl")),
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"two\"}}\n{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n",
    )
    .unwrap();
    let second_child = format!(
        "{}; '{}' > '{}'; printf '%s\\n' '{}' >> '{}'; printf '%s' '{}' | '{}'; echo done > '{}'",
        exports("second", second_runtime),
        plugin.join("bin/otel-headers").display(),
        fixture.home().join("second.headers").display(),
        json!({"type": "assistant", "message": {"model": "claude-opus-5", "role": "assistant", "content": [{"type": "text", "text": "again"}]}}),
        project.join(format!("{second_runtime}.jsonl")).display(),
        hook_input(second_runtime),
        plugin.join("bin/hook").display(),
        fixture.home().join("second.done").display(),
    );
    if !compile_runtime(
        &fixture.home().join("second/claude"),
        None,
        Some(&second_child),
    ) {
        return;
    }
    let shell = fixture.shell_environment();
    assert_eq!(shell["OTEL_EXPORTER_OTLP_ENDPOINT"], collector);
    assert_eq!(shell["FELD_API"], api);
    fixture.new_tmux_session_with("second", &fixture.home().join("second"), &shell);
    wait_for_file(
        &fixture.home().join("second.done"),
        &fixture.home().join("second.err"),
    );
    let second: Value = serde_json::from_str(&wait_for_file(
        &fixture.home().join("second.attach.json"),
        &fixture.home().join("second.err"),
    ))
    .unwrap();
    let second_session = second["session"]["id"].as_str().unwrap().to_owned();
    assert_eq!(second["session"]["actor"], "second-1");
    assert_ne!(second_session, first_session);
    let headers: Value =
        serde_json::from_str(&fs::read_to_string(fixture.home().join("second.headers")).unwrap())
            .unwrap();
    assert_eq!(
        headers["Authorization"],
        format!(
            "Bearer {}",
            feld_core::telemetry::telemetry_token(second["token"].as_str().unwrap())
        )
    );
    let read = fixture.get(&api, &format!("/v1/sessions/{second_session}"));
    assert_eq!(
        read["session"]["model"],
        json!(["Claude Code", "claude-opus-5"]),
        "{read}"
    );
    let page = fixture.get(&api, &format!("/v1/sessions/{second_session}/transcript"));
    assert_eq!(page["state"], "readable", "{page}");
    assert_eq!(page["records"].as_array().unwrap().len(), 3);

    // The settings name the helpers alone; the Collector comes from the shell.
    let settings: Value =
        serde_json::from_slice(&fs::read(config.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings.get("env"), None, "{settings}");
    let now = Utc::now().timestamp();
    let export = claude_metrics_export(
        nanos(now - 10),
        nanos(now),
        &[("input", 100), ("output", 50), ("cacheRead", 850)],
        0.003,
        &[("user", 4)],
    );
    assert_eq!(fixture.export(&collector, "metrics", &headers, export), 200);
    let second_metrics = fixture.wait_for_metrics(&api, &second_session, |metrics| {
        metric(metrics, "token_usage")["status"] == "reported"
    });
    let tokens = assert_reported(&second_metrics, "token_usage", CLAUDE_SOURCE);
    assert_eq!(tokens[0]["value"]["total"], 1_000.0);
    assert!(approx(
        &assert_reported(&second_metrics, "prompt_cache_hit_rate", CLAUDE_SOURCE)[0]["value"],
        850.0 / 950.0 * 100.0
    ));
    assert_eq!(
        metric(&second_metrics, "context_window_utilization")["status"],
        "unavailable"
    );
    // The first Session's Runtime is still the unprepared one.
    let first_metrics = fixture.session_metrics(&api, &first_session, "");
    assert_eq!(
        metric(&first_metrics, "token_usage")["source"],
        RUNTIME_UNPREPARED
    );
    let quota = fixture.get(&api, "/v1/runtimes/claude-code/metrics?metric=quota");
    let claude_quota = &quota["quota"];
    assert_eq!(claude_quota["status"], "reported");
    assert_eq!(
        claude_quota["series"][0]["reportedBy"]["session"],
        first_session
    );

    // Attached again from outside its pane, the first Claude Code is still one that started
    // without `feld env`, and the attach says so and what to do.
    let pane = fixture.tmux(&["list-panes", "-t", "first", "-F", "#{pane_id}"]);
    let pane = String::from_utf8_lossy(&pane.stdout).trim().to_owned();
    let output = fixture
        .command(&[
            "actor",
            "attach",
            "first-1",
            "--runtime",
            "claude-code",
            "--tmux-pane",
            &pane,
            "--tmux-socket",
            fixture.socket.to_str().unwrap(),
        ])
        .env("CLAUDE_CONFIG_DIR", &config)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    let telemetry = stdout
        .lines()
        .find(|line| line.trim_start().starts_with("telemetry:"))
        .unwrap_or_else(|| panic!("no telemetry line: {stdout}"));
    assert!(
        telemetry.contains("unavailable") && telemetry.contains("eval \"$(feld env)\""),
        "{telemetry}"
    );
    assert!(!stdout.contains("feld init"), "{stdout}");

    // From a configuration root `feld init` never prepared, the attach is still accepted,
    // writes nothing there, and says to run `feld init`.
    let unprepared = fixture.home().join("unprepared");
    let output = fixture
        .command(&[
            "actor",
            "attach",
            "first-1",
            "--runtime",
            "claude-code",
            "--tmux-pane",
            &pane,
            "--tmux-socket",
            fixture.socket.to_str().unwrap(),
        ])
        .env("CLAUDE_CONFIG_DIR", &unprepared)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    let line = |label: &str| {
        stdout
            .lines()
            .find(|line| line.trim_start().starts_with(label))
            .unwrap_or_else(|| panic!("no {label} line: {stdout}"))
            .to_owned()
    };
    assert!(
        line("telemetry:").contains("unavailable") && line("telemetry:").contains("feld init"),
        "{stdout}"
    );
    assert!(
        line("note:").contains(&unprepared.join("skills/feld").display().to_string())
            && line("note:").contains("feld init"),
        "{stdout}"
    );
    assert!(!unprepared.exists());
}

/// A tmux server a person started before the daemon lends every pane its own environment,
/// which has no `CLAUDE_CONFIG_DIR`. The Claude Code Feld starts there still runs on the
/// daemon's configuration root, where the Adapter looks for its transcript.
#[test]
fn owned_claude_code_in_an_existing_server_runs_on_the_daemons_configuration_root() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let launch = fixture.home().join("claude-launch.txt");
    if !compile_runtime(&fixture.home().join("claude"), Some(&launch), None) {
        return;
    }
    assert!(
        fixture
            .tmux(&["new-session", "-d", "-s", "ui", "/bin/sh"])
            .status
            .success()
    );
    let config = fixture.home().join("claude-config");
    fixture.start_daemon_with(&BTreeMap::from([(
        "CLAUDE_CONFIG_DIR".to_owned(),
        config.display().to_string(),
    )]));
    fixture.json(&[
        "actor",
        "create",
        "claude-config",
        "--kind",
        "agent",
        "--runtime",
        "claude-code",
        "--workdir",
        fixture.home().to_str().unwrap(),
        "--tmux-socket",
        fixture.socket.to_str().unwrap(),
        "--json",
    ]);

    let (_, environment) = wait_for_launch(&launch);
    assert_eq!(
        environment.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        Some(config.to_str().unwrap())
    );
}

/// Reads every file under `root` into a map keyed by its path relative to `root`.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let contents = fs::read(&path).unwrap();
                files.insert(path.strip_prefix(root).unwrap().to_owned(), contents);
            }
        }
    }
    files
}

/// Runs `prepare` of the claude-code Plugin installed in `feld_home`, with the
/// configuration root `config` (or `$HOME/.claude` when None), writing when `apply`.
fn prepare(feld_home: &Path, config: Option<&Path>, apply: bool) -> Value {
    let plugin_dir = feld_home.join("plugins/runtime/claude-code");
    let mut command = Command::new(plugin_dir.join("plugin"));
    command
        .arg("prepare")
        .env("FELD_HOME", feld_home)
        .env("HOME", feld_home)
        .env("FELD_PLUGIN_DIR", &plugin_dir);
    match config {
        Some(config) => command.env("CLAUDE_CONFIG_DIR", config),
        None => command.env_remove("CLAUDE_CONFIG_DIR"),
    };
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"apply": apply, "collector": "http://127.0.0.1:4318"})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "prepare failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<Value>(&output.stdout).unwrap()
}

/// Runs one shim of a prepared configuration root as Claude Code would, in an environment
/// that names `feld_home` alone.
fn run_shim(shim: &Path, feld_home: &Path) -> String {
    let output = Command::new(shim)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", feld_home)
        .env("FELD_HOME", feld_home)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", shim.display());
    String::from_utf8(output.stdout).unwrap()
}

/// `prepare` writes the Feld plugin and the helper settings under the `HOME` it is given
/// and nowhere else, and only when it applies. A fixture run must never reach the person's own `~/.claude`, so
/// this checks their settings file is left exactly as it was.
#[test]
fn prepare_writes_only_under_the_home_it_is_given() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    support::install_plugins(home);
    // `feld init` prepared this HOME already; this test starts from one it never saw.
    fs::remove_dir_all(home.join(".claude")).unwrap();
    let real_settings = std::env::var_os("HOME")
        .map(|value| PathBuf::from(value).join(".claude/settings.json"))
        .expect("the test environment has a HOME");
    let before = fs::metadata(&real_settings)
        .and_then(|data| data.modified())
        .ok();

    // A check answers what it would write and writes nothing.
    let checked = prepare(home, None, false);
    assert_eq!(checked["prepared"], false, "{checked}");
    assert_eq!(
        checked["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| (
                entry["key"].as_str().unwrap(),
                entry["action"].as_str().unwrap()
            ))
            .collect::<Vec<_>>(),
        [
            ("skills/feld/", "write"),
            ("otelHeadersHelper", "write"),
            ("statusLine", "write")
        ]
    );
    assert_eq!(
        checked["notes"][0],
        format!(
            "the Feld plugin is not under {}; run `feld init`",
            home.join(".claude/skills/feld").display()
        ),
        "{checked}"
    );
    assert!(!home.join(".claude").exists());

    // The first preparation installs the plugin and asks for /reload-plugins, and for a
    // restart of a Claude Code already running so that it sends its metrics.
    let first = prepare(home, None, true);
    assert_eq!(
        first["notes"],
        json!([
            "run /reload-plugins in a Claude Code that is already open",
            "restart a Claude Code started before this init to send its metrics"
        ]),
        "{first}"
    );
    // Setting the otelHeadersHelper means a Claude Code already running has none.
    assert_eq!(first["telemetryPrepared"], false, "{first}");
    assert!(
        first["telemetryReason"]
            .as_str()
            .unwrap()
            .contains("feld env"),
        "{first}"
    );

    let settings: Value =
        serde_json::from_slice(&fs::read(home.join(".claude/settings.json")).unwrap()).unwrap();
    let installed = home.join(".claude/skills/feld");
    assert_eq!(
        settings["otelHeadersHelper"],
        format!("'{}'", installed.join("bin/otel-headers").display())
    );
    assert_eq!(
        settings["statusLine"]["command"],
        format!("'{}'", installed.join("bin/status-line").display())
    );
    assert_eq!(settings.get("env"), None, "{settings}");
    assert!(installed.join("hooks/hooks.json").is_file());
    assert!(installed.join("monitors/monitors.json").is_file());
    assert!(installed.join("skills/feld/SKILL.md").is_file());
    assert!(installed.join(".claude-plugin/plugin.json").is_file());

    // Preparing again changes nothing, so a Claude Code running by then keeps its telemetry.
    let again = prepare(home, None, true);
    assert_eq!(again["notes"].as_array().unwrap(), &Vec::<Value>::new());
    assert_eq!(again["telemetryPrepared"], true, "{again}");

    // A SKILL.md a person edited is theirs, and the check says the plugin is written into
    // only with their consent.
    let skill_md = installed.join("skills/feld/SKILL.md");
    let current_skill = fs::read(&skill_md).unwrap();
    fs::write(&skill_md, b"# feld\n").unwrap();
    let edited = prepare(home, None, false);
    assert_eq!(edited["entries"][0]["action"], "replace", "{edited}");

    // One another feld version wrote, which the prepare record knows, is Feld's, and the
    // check says so of the plugin alone.
    let record_path = home.join("prepared/claude-code.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    let digest = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(b"# feld\n"));
    record["roots"][home.join(".claude").display().to_string()]["files"]["skills/feld/SKILL.md"] =
        json!(digest);
    fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    let other = prepare(home, None, false);
    assert_eq!(
        other["notes"].as_array().unwrap(),
        &vec![json!(format!(
            "the Feld plugin under {} is from another feld version; run `feld init`",
            installed.display()
        ))],
        "{other}"
    );
    fs::write(&skill_md, current_skill).unwrap();

    // A Claude Code with its own configuration root reads its settings and plugins from
    // there, so that is where the preparation writes.
    let config = home.join("elsewhere");
    let moved = prepare(home, Some(&config), true);
    assert_eq!(moved["notes"], first["notes"], "{moved}");
    let moved_settings: Value =
        serde_json::from_slice(&fs::read(config.join("settings.json")).unwrap()).unwrap();
    assert_eq!(
        moved_settings["otelHeadersHelper"],
        format!(
            "'{}'",
            config.join("skills/feld/bin/otel-headers").display()
        )
    );
    assert!(
        config
            .join("skills/feld/.claude-plugin/plugin.json")
            .is_file()
    );

    let after = fs::metadata(&real_settings)
        .and_then(|data| data.modified())
        .ok();
    assert_eq!(before, after, "prepare touched {}", real_settings.display());
}

/// Two daemons, each with its own Feld home, prepare one configuration root, as an
/// isolated daemon attaching from a person's shell does. What they write is the same, names
/// neither home, and takes out the telemetry environment an earlier Feld wrote into the
/// settings. The helpers run the Plugin of the Feld home their environment names, so the
/// Claude Code of that configuration root never reaches the other daemon.
#[test]
fn prepare_is_the_same_from_every_feld_home() {
    let temporary = tempfile::tempdir().unwrap();
    let first_home = temporary.path().join("first");
    let second_home = temporary.path().join("second");
    support::install_plugins(&first_home);
    support::install_plugins(&second_home);
    let config = temporary.path().join("claude");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("settings.json"),
        serde_json::to_vec(&json!({
            "model": "opus",
            "env": {
                "KEEP": "mine",
                "OTEL_EXPORTER_OTLP_HEADERS": "x-team=mine",
                "CLAUDE_CODE_ENABLE_TELEMETRY": "1",
                "OTEL_METRICS_EXPORTER": "otlp",
                "OTEL_LOGS_EXPORTER": "otlp",
                "OTEL_EXPORTER_OTLP_PROTOCOL": "http/protobuf",
                "OTEL_EXPORTER_OTLP_ENDPOINT": "http://127.0.0.1:4318",
                "OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE": "delta",
                "OTEL_METRIC_EXPORT_INTERVAL": "10000",
                "CLAUDE_CODE_OTEL_HEADERS_HELPER_DEBOUNCE_MS": "60000"
            }
        }))
        .unwrap(),
    )
    .unwrap();

    prepare(&first_home, Some(&config), true);
    let from_first = tree(&config);
    prepare(&second_home, Some(&config), true);
    let from_second = tree(&config);
    assert_eq!(
        from_first.keys().collect::<Vec<_>>(),
        from_second.keys().collect::<Vec<_>>()
    );
    for (path, contents) in &from_first {
        assert_eq!(
            contents,
            &from_second[path],
            "{} differs between Feld homes",
            path.display()
        );
        let text = String::from_utf8_lossy(contents);
        for home in [&first_home, &second_home] {
            assert!(
                !text.contains(&home.display().to_string()),
                "{} names {}",
                path.display(),
                home.display()
            );
        }
    }
    let settings: Value =
        serde_json::from_slice(&fs::read(config.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["model"], "opus");
    assert_eq!(
        settings["env"],
        json!({"KEEP": "mine", "OTEL_EXPORTER_OTLP_HEADERS": "x-team=mine"})
    );

    // The helpers run the Plugin of the Feld home they are given.
    let bin = config.join("skills/feld/bin");
    let other = temporary.path().join("other");
    let fake = other.join("plugins/runtime/claude-code/plugin");
    fs::create_dir_all(fake.parent().unwrap()).unwrap();
    fs::write(&fake, "#!/bin/sh\necho \"other $*\"\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    }
    assert_eq!(
        run_shim(&bin.join("status-line"), &other),
        "other helper status-line\n"
    );
    // A Feld home without the Plugin leaves Claude Code as if Feld were absent.
    let empty = temporary.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert_eq!(run_shim(&bin.join("otel-headers"), &empty), "{}\n");
    assert_eq!(run_shim(&bin.join("status-line"), &empty), "\n");
    assert_eq!(run_shim(&bin.join("hook"), &empty), "");
    // The monitor waits silently, since Claude Code tells the model whenever it ends.
    let mut monitor = Command::new(bin.join("monitor"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &empty)
        .env("FELD_HOME", &empty)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_secs(1));
    assert!(monitor.try_wait().unwrap().is_none(), "the monitor ended");
    monitor.kill().unwrap();
    let output = monitor.wait_with_output().unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
}
