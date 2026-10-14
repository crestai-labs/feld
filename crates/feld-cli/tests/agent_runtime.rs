use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

const FELD: &str = env!("CARGO_BIN_EXE_feld");

mod support;

use support::compile_runtime;

struct Fixture {
    temporary: TempDir,
    socket: PathBuf,
}

impl Fixture {
    fn new() -> Option<Self> {
        if !feld_core::test_require::tmux() {
            return None;
        }
        let temporary = tempfile::tempdir().unwrap();
        let socket = temporary.path().join("runtime,socket.sock");
        support::install_plugins(temporary.path());
        Some(Self { temporary, socket })
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

    fn feld(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().unwrap()
    }

    fn json(&self, arguments: &[&str]) -> Value {
        let output = self.feld(arguments);
        assert!(
            output.status.success(),
            "feld {} failed:\nstdout: {}\nstderr: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Runs tmux on the fixture socket. The server inherits this environment, so the Feld
    /// variables of a Session the tests may run inside stay out of the panes it opens.
    fn tmux(&self, arguments: &[&str]) -> Output {
        support::without_outside_variables(&mut Command::new("tmux"))
            .arg("-S")
            .arg(&self.socket)
            .args(arguments)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
        let _ = self.tmux(&["kill-server"]);
    }
}

fn collector_child(parent: u32) -> Option<u32> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,command="])
        .output()
        .expect("list processes");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let ppid = fields.next()?.parse::<u32>().ok()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            (ppid == parent && command.contains("collector-run")).then_some(pid)
        })
        .next()
}

#[test]
fn real_tmux_attach_rotates_tokens_and_ends_with_the_runtime() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let status = fixture.json(&["daemon", "start", "--json"]);
    let api = status["daemon"]["api"].as_str().unwrap();
    let claude = fixture.home().join("claude");
    if !compile_runtime(&claude, None) {
        return;
    }
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "runtime",
                "-c",
                fixture.home().to_str().unwrap(),
                "--",
                // As under a person's shell, Claude Code is a child of the pane's shell, in
                // front of it with a process group of its own, and the shell stays after it.
                "exec /bin/sh -c 'set -m; ./claude; exec sleep 300'",
            ])
            .status
            .success()
    );
    assert!(
        fixture
            .tmux(&["set-option", "-p", "-t", "runtime", "remain-on-exit", "on"])
            .status
            .success()
    );
    let socket = fixture.socket.to_str().unwrap();
    let first = fixture.json(&[
        "actor",
        "attach",
        "attached",
        "--tmux-pane",
        "runtime",
        "--tmux-socket",
        socket,
        "--runtime",
        "claude-code",
        "--json",
    ]);
    assert_eq!(first["session"]["ownership"], "attached");
    assert_eq!(first["session"]["startKind"], "attach");
    assert_eq!(first["session"]["observation"]["kind"], "running");
    let session = first["session"]["id"].as_str().unwrap();
    let first_token = first["token"].as_str().unwrap();
    let local_token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let client = reqwest::blocking::Client::new();
    assert_eq!(
        client
            .post(format!(
                "{api}/v1/workspaces/default/actors/attached/attach"
            ))
            .bearer_auth(first_token)
            .json(&json!({
                "placement": first["session"]["placement"],
                "runtime": "claude-code"
            }))
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::FORBIDDEN
    );
    fixture.json(&[
        "actor",
        "create",
        "job",
        "--kind",
        "job",
        "--tmux-socket",
        socket,
        "--json",
        "--",
        "true",
    ]);
    for (path, runtime, status, message) in [
        (
            "Bad_WS/actors/attached",
            "claude-code",
            reqwest::StatusCode::BAD_REQUEST,
            "Workspace keys must match ^[a-z0-9][a-z0-9-]*$",
        ),
        (
            "nows/actors/attached",
            "claude-code",
            reqwest::StatusCode::NOT_FOUND,
            "Workspace 'nows' was not found",
        ),
        (
            "default/actors/job",
            "claude-code",
            reqwest::StatusCode::CONFLICT,
            "Actor 'job' is a Job; attach takes an agent Actor",
        ),
        (
            "default/actors/attached",
            "nope",
            reqwest::StatusCode::BAD_REQUEST,
            "runtime 'nope' is not installed; `feld daemon status` lists the installed Runtime Adapters",
        ),
    ] {
        let refused = client
            .post(format!("{api}/v1/workspaces/{path}/attach"))
            .bearer_auth(local_token.trim())
            .json(&json!({
                "placement": first["session"]["placement"],
                "runtime": runtime
            }))
            .send()
            .unwrap();
        assert_eq!(refused.status(), status, "{path}");
        assert_eq!(refused.json::<Value>().unwrap()["message"], message);
    }
    let mut unknown_provider = first["session"]["placement"].clone();
    unknown_provider["provider"] = json!("nope");
    let refused_attach = client
        .post(format!(
            "{api}/v1/workspaces/default/actors/attached/attach"
        ))
        .bearer_auth(local_token.trim())
        .json(&json!({"placement": unknown_provider, "runtime": "claude-code"}))
        .send()
        .unwrap();
    let refused_create = client
        .post(format!("{api}/v1/workspaces/default/actors"))
        .bearer_auth(local_token.trim())
        .json(&json!({
            "name": "unknown-provider", "kind": "job",
            "launchSpec": {
                "command": ["true"], "provider": "nope", "node": "local",
                "workdir": unknown_provider["workdir"]
            }
        }))
        .send()
        .unwrap();
    assert_eq!(refused_attach.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(refused_create.status(), reqwest::StatusCode::BAD_REQUEST);
    let message = refused_attach.json::<Value>().unwrap()["message"].clone();
    assert_eq!(
        message,
        "provider 'nope' is not installed; `feld daemon status` lists the installed Execution Providers"
    );
    assert_eq!(refused_create.json::<Value>().unwrap()["message"], message);
    assert_eq!(
        client
            .post(format!("{api}/v1/self/runtime"))
            .bearer_auth(local_token.trim())
            .json(&json!({
                "sessionId": "forbidden",
                "transcriptPath": "/tmp/forbidden",
                "model": "claude-test"
            }))
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::FORBIDDEN
    );

    let second = fixture.json(&[
        "actor",
        "attach",
        "attached",
        "--tmux-pane",
        "runtime",
        "--tmux-socket",
        socket,
        "--runtime",
        "claude-code",
        "--json",
    ]);
    assert_eq!(second["session"]["id"], session);
    assert_eq!(second["reattached"], true);
    let second_token = second["token"].as_str().unwrap();
    assert_ne!(first_token, second_token);
    assert_eq!(
        client
            .get(format!("{api}/v1/self/messages"))
            .bearer_auth(first_token)
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert!(
        client
            .get(format!("{api}/v1/self/messages"))
            .bearer_auth(second_token)
            .send()
            .unwrap()
            .status()
            .is_success()
    );

    let reset = chrono::Utc::now().timestamp() + 3600;
    let mut status_helper = support::runtime_helper(fixture.home(), "status-line")
        .env("FELD_SESSION_TOKEN", second_token)
        .env("FELD_ACTOR", "attached")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        status_helper.stdin.take().unwrap(),
        "{}",
        json!({
            "context_window":{"context_window_size":200000,"used_percentage":42.0},
            "rate_limits":{"five_hour":{"used_percentage":17.0,"resets_at":reset}}
        })
    )
    .unwrap();
    let status_output = status_helper.wait_with_output().unwrap();
    assert!(status_output.status.success());
    assert!(String::from_utf8_lossy(&status_output.stdout).contains("attached · context 42%"));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let metrics: Value = client
            .get(format!("{api}/v1/sessions/{session}/metrics"))
            .bearer_auth(local_token.trim())
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap();
        if metrics["metrics"].as_array().unwrap().iter().any(|metric| {
            metric["metric"] == "context_window_utilization" && metric["status"] == "reported"
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "status gauges were not projected: {metrics}"
        );
        thread::sleep(Duration::from_millis(25));
    }
    let runtime_metrics: Value = client
        .get(format!("{api}/v1/runtimes/claude-code/metrics"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        runtime_metrics["quota"]["status"], "reported",
        "{runtime_metrics}"
    );
    assert!(
        runtime_metrics["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|metric| {
                metric["metric"] == "context_window_utilization"
                    && metric["status"] == "reported"
                    && metric["accounts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|account| account["series"][0]["session"] == session)
            }),
        "{runtime_metrics}"
    );
    let binned: Value = client
        .get(format!("{api}/v1/sessions/{session}/metrics?bucket=60"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert!(
        binned["metrics"].as_array().unwrap().iter().any(|metric| {
            metric["metric"] == "context_window_utilization" && metric["status"] == "reported"
        }),
        "{binned}"
    );
    assert_eq!(
        client
            .get(format!("{api}/v1/sessions/{session}/metrics?bucket=59"))
            .bearer_auth(local_token.trim())
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::BAD_REQUEST
    );

    let pending: Value = client
        .get(format!("{api}/v1/sessions/{session}/transcript"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(pending["state"], "pending");

    let transcript_directory = fixture.home().join("records");
    fs::create_dir(&transcript_directory).unwrap();
    let transcript = transcript_directory.join("transcript.jsonl");
    let identity = json!({
        "sessionId": "claude-runtime-id",
        "transcriptPath": transcript,
        "model": "claude-test"
    });
    assert_eq!(
        client
            .post(format!("{api}/v1/self/runtime"))
            .bearer_auth(first_token)
            .json(&identity)
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    for _ in 0..2 {
        assert!(
            client
                .post(format!("{api}/v1/self/runtime"))
                .bearer_auth(second_token)
                .json(&identity)
                .send()
                .unwrap()
                .status()
                .is_success()
        );
    }
    let conflict = client
        .post(format!("{api}/v1/self/runtime"))
        .bearer_auth(second_token)
        .json(&json!({
            "sessionId": "claude-other-id",
            "transcriptPath": transcript,
            "model": "claude-test"
        }))
        .send()
        .unwrap();
    assert_eq!(conflict.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(
        conflict.json::<Value>().unwrap()["message"],
        format!(
            "Session '{session}' has recorded Runtime session id 'claude-runtime-id' with transcript '{0}'; the request names session id 'claude-other-id' with transcript '{0}'",
            transcript.display()
        )
    );
    let missing: Value = client
        .get(format!("{api}/v1/sessions/{session}/transcript"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(missing["state"], "missing");
    let unavailable = fixture.json(&["session", "get", session, "--json"]);
    assert_eq!(unavailable["session"]["turn"], "unavailable");
    assert_eq!(
        unavailable["session"].get("lastEntryAt"),
        Some(&Value::Null)
    );
    // Claude Code creates the workdir's `projects/` directory at the first input, so an
    // unborn parent directory is the same missing state and no OS error reaches the Console.
    fs::remove_dir(&transcript_directory).unwrap();
    let unborn: Value = client
        .get(format!("{api}/v1/sessions/{session}/transcript"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(unborn["state"], "missing");
    assert_eq!(unborn["detail"], "transcript file does not exist");
    fs::write(&transcript_directory, b"").unwrap();
    let lost: Value = client
        .get(format!("{api}/v1/sessions/{session}/transcript"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(lost["state"], "lost");
    fs::remove_file(&transcript_directory).unwrap();
    fs::create_dir(&transcript_directory).unwrap();

    fs::write(
        &transcript,
        format!(
            "{{\"type\":\"user\",\"timestamp\":\"2026-09-15T01:00:00Z\",\"unknown\":\"ordinary\"}}\n{{\"type\":\"progress\",\"timestamp\":\"2026-09-15T01:00:01Z\",\"unknown\":\"{}\"}}\n{{\"partial\":",
            first_token
        ),
    )
    .unwrap();
    let readable: Value = client
        .get(format!("{api}/v1/sessions/{session}/transcript"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(readable["state"], "readable");
    assert_eq!(readable["records"].as_array().unwrap().len(), 2);
    // The Adapter's cursor is the Console's cursor: what a record carries reads the
    // records after it, and the Runtime's own record comes back under `raw`, redacted.
    let first_cursor = readable["records"][0]["cursor"].as_str().unwrap();
    let appended: Value = client
        .get(format!(
            "{api}/v1/sessions/{session}/transcript?after={first_cursor}"
        ))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(appended["records"].as_array().unwrap().len(), 1);
    assert_eq!(appended["nextAfter"], readable["nextAfter"]);
    assert_eq!(appended["records"][0]["raw"]["unknown"], "[REDACTED]");
    // A cursor that names no entry is refused rather than read as the end.
    for window in ["after", "before"] {
        let unknown = client
            .get(format!(
                "{api}/v1/sessions/{session}/transcript?{window}=garbage"
            ))
            .bearer_auth(local_token.trim())
            .send()
            .unwrap();
        assert_eq!(
            unknown.status(),
            reqwest::StatusCode::BAD_REQUEST,
            "{window}"
        );
        let refusal: Value = unknown.json().unwrap();
        assert!(
            refusal["message"].as_str().unwrap().contains("'garbage'"),
            "{refusal}"
        );
    }

    // `feld session log` reads the transcript of an agent-runtime Session. The Conversation
    // view, its default, leaves out the records of kind `other`.
    let all = fixture.json(&["session", "log", "attached", "--view", "all", "--json"]);
    assert_eq!(all["records"], readable["records"]);
    let conversation = fixture.json(&["session", "log", session, "--json"]);
    let kept: Vec<_> = readable["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|record| record["kind"] != "other")
        .cloned()
        .collect();
    assert!(kept.len() < 2, "{readable}");
    assert_eq!(readable["records"][1]["kind"], "other", "{readable}");
    assert_eq!(conversation["records"].as_array().unwrap(), &kept);
    // The Session's reads carry the turn its last entry that carries one gives.
    let carried = kept.iter().rev().find(|record| {
        [
            "input",
            "notification",
            "response",
            "thinking",
            "tool-call",
            "tool-result",
        ]
        .contains(&record["kind"].as_str().unwrap())
    });
    let (turn, at) = match carried {
        Some(record) if record["kind"] == "response" => ("waiting-input", record["at"].clone()),
        Some(record) => ("responding", record["at"].clone()),
        None => ("unavailable", Value::Null),
    };
    let current = fixture.json(&["session", "get", session, "--json"]);
    let listed = fixture.json(&["actor", "list", "--json"]);
    let attached = listed["actors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|actor| actor["name"] == "attached")
        .unwrap();
    for read in [&current["session"], &attached["currentSession"]] {
        assert_eq!(read["turn"], turn, "{read}");
        assert_eq!(read.get("lastEntryAt"), Some(&at), "{read}");
    }
    // A tail counts the records of the view, and its cursor passes the ones left out.
    let conversation_tail = fixture.json(&["session", "log", session, "--tail", "1", "--json"]);
    assert_eq!(conversation_tail["records"].as_array().unwrap(), &kept);
    assert_eq!(conversation_tail["nextAfter"], readable["nextAfter"]);
    let tail = fixture.json(&[
        "session", "log", session, "--view", "all", "--tail", "1", "--json",
    ]);
    assert_eq!(
        tail["records"].as_array().unwrap(),
        &readable["records"].as_array().unwrap()[1..]
    );
    let after = fixture.json(&[
        "session",
        "log",
        session,
        "--view",
        "all",
        "--after",
        first_cursor,
        "--json",
    ]);
    assert_eq!(after["records"], appended["records"]);
    // `--follow --json` prints each entry as a line of its own, the shape of an element of
    // `records`.
    let followed = fixture.home().join("follow.jsonl");
    let mut follower = fixture
        .command(&[
            "session", "log", session, "--view", "all", "--follow", "--json",
        ])
        .stdout(fs::File::create(&followed).unwrap())
        .spawn()
        .unwrap();
    let expected = readable["records"].as_array().unwrap();
    assert!(expected.len() > 1, "{readable}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::read_to_string(&followed).unwrap().lines().count() < expected.len()
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(50));
    }
    let _ = follower.kill();
    let _ = follower.wait();
    let followed = fs::read_to_string(&followed).unwrap();
    let entries: Vec<Value> = followed
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(&entries, expected, "{followed}");
    let printed = fixture.feld(&["session", "log", session, "--view", "all"]);
    assert!(printed.status.success());
    let printed = String::from_utf8(printed.stdout).unwrap();
    let printed: Vec<_> = printed.lines().collect();
    assert_eq!(
        printed[0],
        format!("Transcript {session} (agent-runtime · claude-code)")
    );
    assert_eq!(printed.len(), 4, "{printed:?}");
    assert_eq!(
        printed[3],
        format!("next cursor: {}", readable["nextAfter"].as_str().unwrap())
    );

    let subagents = transcript_directory.join("transcript/subagents");
    fs::create_dir_all(&subagents).unwrap();
    fs::write(
        subagents.join("agent-child-7.jsonl"),
        b"{\"type\":\"fork-context-ref\",\"agentId\":\"child-7\",\"parentSessionId\":\"parent\"}\n{\"type\":\"assistant\",\"agentId\":\"child-7\",\"attributionAgent\":\"Explore\",\"timestamp\":\"2026-09-15T01:00:02Z\"}\n",
    )
    .unwrap();
    let session_read: Value = client
        .get(format!("{api}/v1/sessions/{session}"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(session_read["subAgents"][0]["startKind"], "sub-agent");
    assert_eq!(session_read["subAgents"][0]["parentSession"], session);
    assert!(session_read["subAgents"][0]["placement"].is_null());
    let subagent_id = session_read["subAgents"][0]["id"].as_str().unwrap();
    // A Sub Agent Session is read by its own id too.
    let subagent_read = client
        .get(format!("{api}/v1/sessions/{subagent_id}"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .unwrap();
    assert_eq!(
        subagent_read["subAgentSession"],
        session_read["subAgents"][0]
    );
    // The Actor's listing shows it without its parent read on its own.
    let listing = |query: &str| {
        client
            .get(format!(
                "{api}/v1/sessions?workspace=default&actor=attached{query}"
            ))
            .bearer_auth(local_token.trim())
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<Value>()
            .unwrap()
    };
    assert_eq!(
        listing("&subAgents")["subAgents"],
        session_read["subAgents"]
    );
    assert!(listing("").get("subAgents").is_none());
    let missing = client
        .get(format!("{api}/v1/sessions/{session}_subagent_missing"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap();
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
    let subagent_transcript: Value = client
        .get(format!("{api}/v1/sessions/{subagent_id}/transcript"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(subagent_transcript["records"].as_array().unwrap().len(), 2);
    // Its metrics are its parent's.
    let subagent_metrics: Value = client
        .get(format!("{api}/v1/sessions/{subagent_id}/metrics"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(subagent_metrics["session"], subagent_id);
    assert!(
        subagent_metrics["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|metric| metric["status"] == "n/a"
                && metric["source"] == "Included in the parent Session"),
        "{subagent_metrics}"
    );
    // The CLI reads a Sub Agent Session by the id its parent's read gives it.
    assert_eq!(
        fixture.json(&["session", "get", subagent_id, "--json"]),
        subagent_read
    );
    let printed = fixture.feld(&["session", "get", subagent_id]);
    assert!(printed.status.success(), "{printed:?}");
    let printed = String::from_utf8(printed.stdout).unwrap();
    assert!(
        printed.starts_with(&format!("Sub Agent Session {subagent_id}\n")),
        "{printed}"
    );
    assert!(printed.contains(session), "{printed}");
    let subagent_log = fixture.json(&["session", "log", subagent_id, "--view", "all", "--json"]);
    assert_eq!(subagent_log["records"], subagent_transcript["records"]);
    let printed = fixture.feld(&["session", "log", subagent_id, "--view", "all"]);
    assert!(printed.status.success(), "{printed:?}");
    assert!(
        String::from_utf8(printed.stdout)
            .unwrap()
            .starts_with(&format!(
                "Transcript {subagent_id} (agent-runtime · claude-code)\n"
            ))
    );
    assert_eq!(
        fixture.json(&["session", "metrics", subagent_id, "--json"]),
        subagent_metrics
    );
    let printed = fixture.feld(&["session", "metrics", subagent_id]);
    assert!(printed.status.success(), "{printed:?}");
    assert!(
        String::from_utf8(printed.stdout)
            .unwrap()
            .starts_with(&format!("Metrics {subagent_id} (Claude Code"))
    );
    let missing = format!("{session}_subagent_missing");
    assert!(
        !fixture
            .feld(&["session", "metrics", &missing])
            .status
            .success()
    );
    let missing_get = fixture.feld(&["session", "get", &missing]);
    assert!(!missing_get.status.success());
    assert!(
        String::from_utf8(missing_get.stderr)
            .unwrap()
            .contains(&format!("Sub Agent Session '{missing}' was not found"))
    );
    let mut parent_records = fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    writeln!(parent_records, "true}}").unwrap();
    writeln!(parent_records, "{{\"type\":\"assistant\",\"timestamp\":\"2026-09-15T01:00:03Z\",\"message\":{{\"content\":[{{\"type\":\"tool_use\",\"id\":\"agent-async\",\"name\":\"Agent\"}},{{\"type\":\"tool_use\",\"id\":\"agent-sync\",\"name\":\"Agent\"}}]}}}}").unwrap();
    writeln!(parent_records, "{{\"type\":\"user\",\"timestamp\":\"2026-09-15T01:00:03Z\",\"message\":{{\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"agent-async\"}}]}},\"toolUseResult\":{{\"isAsync\":true,\"status\":\"async_launched\",\"agentId\":\"child-7\"}}}}").unwrap();
    writeln!(parent_records, "{{\"type\":\"queue-operation\",\"operation\":\"enqueue\",\"timestamp\":\"2026-09-15T01:00:04Z\",\"content\":\"<task-notification><task-id>child-7</task-id><tool-use-id>agent-async</tool-use-id><status>completed</status></task-notification>\"}}").unwrap();
    writeln!(parent_records, "{{\"type\":\"user\",\"timestamp\":\"2026-09-15T01:00:05Z\",\"message\":{{\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"agent-sync\"}}]}},\"toolUseResult\":{{\"agentId\":\"sync-8\",\"status\":\"completed\"}}}}").unwrap();
    // A background agent the parent stopped with TaskStop reports `killed`, and one a
    // person stopped is announced by description alone; both end the Sub Agent Session.
    writeln!(parent_records, "{{\"type\":\"assistant\",\"timestamp\":\"2026-09-15T01:00:06Z\",\"message\":{{\"content\":[{{\"type\":\"tool_use\",\"id\":\"agent-killed\",\"name\":\"Task\"}},{{\"type\":\"tool_use\",\"id\":\"agent-stopped\",\"name\":\"Agent\"}}]}}}}").unwrap();
    writeln!(parent_records, "{{\"type\":\"user\",\"timestamp\":\"2026-09-15T01:00:06Z\",\"message\":{{\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"agent-killed\"}}]}},\"toolUseResult\":{{\"isAsync\":true,\"status\":\"async_launched\",\"agentId\":\"killed-10\"}}}}").unwrap();
    writeln!(parent_records, "{{\"type\":\"user\",\"timestamp\":\"2026-09-15T01:00:06Z\",\"message\":{{\"content\":[{{\"type\":\"tool_result\",\"tool_use_id\":\"agent-stopped\"}}]}},\"toolUseResult\":{{\"isAsync\":true,\"status\":\"async_launched\"}}}}").unwrap();
    writeln!(parent_records, "{{\"type\":\"queue-operation\",\"operation\":\"enqueue\",\"timestamp\":\"2026-09-15T01:00:07Z\",\"content\":\"<task-notification><task-id>killed-10</task-id><tool-use-id>agent-killed</tool-use-id><status>killed</status></task-notification>\"}}").unwrap();
    writeln!(parent_records, "{{\"type\":\"user\",\"timestamp\":\"2026-09-15T01:00:08Z\",\"origin\":{{\"kind\":\"task-notification\"}},\"promptSource\":\"system\",\"message\":{{\"role\":\"user\",\"content\":\"Background agent \\\"Stopped job\\\" was stopped by the user.\"}}}}").unwrap();
    fs::write(
        subagents.join("agent-killed-10.jsonl"),
        b"{\"type\":\"user\",\"agentId\":\"killed-10\",\"timestamp\":\"2026-09-15T01:00:06Z\"}\n",
    )
    .unwrap();
    fs::write(
        subagents.join("agent-stopped-11.jsonl"),
        b"{\"type\":\"user\",\"agentId\":\"stopped-11\",\"timestamp\":\"2026-09-15T01:00:06Z\"}\n",
    )
    .unwrap();
    fs::write(
        subagents.join("agent-stopped-11.meta.json"),
        b"{\"agentType\":\"general-purpose\",\"description\":\"Stopped job\",\"toolUseId\":\"agent-stopped\",\"stoppedByUser\":true}\n",
    )
    .unwrap();
    fs::write(
        subagents.join("agent-sync-8.jsonl"),
        b"{\"type\":\"user\",\"agentId\":\"sync-8\",\"timestamp\":\"2026-09-15T01:00:02Z\"}\n",
    )
    .unwrap();
    fs::write(
        subagents.join("agent-open-9.jsonl"),
        b"{\"type\":\"user\",\"agentId\":\"open-9\",\"timestamp\":\"2026-09-15T01:00:06Z\"}\n",
    )
    .unwrap();
    fs::write(subagents.join("agent-unavailable.jsonl"), b"{bad}\n").unwrap();
    let completed: Value = client
        .get(format!("{api}/v1/sessions/{session}"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    // The last of the Session's own entries is a Runtime notification, which the turn
    // reads as a response under way; the Sub Agents' entries are theirs.
    assert_eq!(completed["session"]["turn"], "responding", "{completed}");
    assert_eq!(completed["session"]["lastEntryAt"], "2026-09-15T01:00:08Z");
    let listed = fixture.json(&["actor", "list", "--json"]);
    let attached = listed["actors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|actor| actor["name"] == "attached")
        .unwrap();
    assert_eq!(attached["currentSession"]["turn"], "responding", "{listed}");
    assert_eq!(
        attached["currentSession"]["lastEntryAt"],
        "2026-09-15T01:00:08Z"
    );
    assert_eq!(
        completed["subAgents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|agent| agent["id"].as_str().unwrap().ends_with("_child-7"))
            .unwrap()["end"],
        "2026-09-15T01:00:04Z"
    );
    assert_eq!(
        completed["subAgents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|agent| agent["subAgent"] == "sync-8")
            .unwrap()["end"],
        "2026-09-15T01:00:05Z"
    );
    let killed = completed["subAgents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["subAgent"] == "killed-10")
        .unwrap();
    assert_eq!(killed["end"], "2026-09-15T01:00:07Z");
    assert_eq!(killed["outcome"], "ended");
    let stopped = completed["subAgents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["id"].as_str().unwrap().ends_with("_stopped-11"))
        .unwrap();
    assert_eq!(stopped["subAgent"], "stopped-11");
    assert_eq!(stopped["end"], "2026-09-15T01:00:08Z");
    // A Sub Agent transcript file the Adapter cannot read still has a Sub Agent Session: the
    // Adapter says so with one `state: lost` record, and the parent's own records stay
    // readable.
    let unavailable = completed["subAgents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["subAgent"] == "unavailable")
        .unwrap_or_else(|| panic!("the unreadable Sub Agent is missing: {completed}"));
    assert_eq!(unavailable["consoleState"], "lost");
    assert!(unavailable["consoleDetail"].is_string(), "{unavailable}");
    assert!(unavailable["start"].is_null());
    assert_eq!(unavailable["outcome"], "unknown");
    let unavailable_console: Value = client
        .get(format!(
            "{api}/v1/sessions/{}/transcript",
            unavailable["id"].as_str().unwrap()
        ))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(unavailable_console["state"], "lost");
    assert!(
        unavailable_console["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let described = fixture.json(&["actor", "get", "attached", "--json"]);
    assert_eq!(
        described["actor"]["currentSession"]["model"],
        json!(["Claude Code", "claude-test"])
    );
    assert!(described["actor"]["launchSpec"].is_null());
    assert!(
        !fixture
            .feld(&["session", "stop", "attached", "--json"])
            .status
            .success()
    );

    let placement = &second["session"]["placement"]["placement"];
    let pid = placement["process"]["pid"].as_u64().unwrap();
    assert!(
        Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let observed = fixture.json(&["session", "get", session, "--json"]);
        if observed["session"]["outcome"] == "ended" {
            assert!(observed["session"]["exitCode"].is_null());
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(25));
    }
    let closed: Value = client
        .get(format!("{api}/v1/sessions/{session}"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    let open_agent = closed["subAgents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["subAgent"] == "open-9")
        .unwrap();
    assert_eq!(open_agent["end"], closed["session"]["end"]);
    // An ended Session's Sub Agent Sessions are projected once: its entries no longer grow.
    fs::write(
        subagents.join("agent-late-12.jsonl"),
        b"{\"type\":\"user\",\"agentId\":\"late-12\",\"timestamp\":\"2026-09-15T01:00:09Z\"}\n",
    )
    .unwrap();
    assert_eq!(listing("&subAgents")["subAgents"], closed["subAgents"]);
    // A range lists the Sub Agent Sessions of the Sessions it lists alone.
    let outside = listing("&subAgents&end=2000-01-01T00:00:00Z");
    assert_eq!(outside["sessions"], json!([]));
    assert_eq!(outside["subAgents"], json!([]));

    // A complete line the Adapter cannot read loses the whole record column rather than
    // showing part of it.
    writeln!(parent_records, "{{not-json}}").unwrap();
    let malformed = client
        .get(format!("{api}/v1/sessions/{session}/transcript"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap();
    assert_eq!(malformed.status(), reqwest::StatusCode::OK);
    let malformed: Value = malformed.json().unwrap();
    assert_eq!(malformed["state"], "lost");
    assert!(malformed["records"].as_array().unwrap().is_empty());

    let index = fs::read_dir(fixture.home().join("self-index"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::metadata(index).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(
        fixture
            .tmux(&["has-session", "-t", "runtime"])
            .status
            .success()
    );

    drop(parent_records);
    fs::write(
        &transcript,
        (1..=1_205)
            .map(|sequence| {
                format!(
                    "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"record {sequence}\"}}}}\n"
                )
            })
            .collect::<String>(),
    )
    .unwrap();
    let tail: Value = client
        .get(format!(
            "{api}/v1/sessions/{session}/transcript?tail=true&limit=200"
        ))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    let body =
        |page: &Value, index: usize| page["records"][index]["raw"]["message"]["content"].clone();
    let cursor = |page: &Value, index: usize| {
        page["records"][index]["cursor"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(body(&tail, 0), "record 1006");
    assert_eq!(body(&tail, 199), "record 1205");
    assert_eq!(tail["previousBefore"], cursor(&tail, 0));
    assert_eq!(tail["nextAfter"], cursor(&tail, 199));
    assert_eq!(tail["hasOlder"], true);
    fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap()
        .write_all(
            b"{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\"new\"}}\n",
        )
        .unwrap();
    let before = cursor(&tail, 0);
    let older: Value = client
        .get(format!(
            "{api}/v1/sessions/{session}/transcript?before={before}&limit=200"
        ))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(body(&older, 0), "record 806");
    assert_eq!(body(&older, 199), "record 1005");
    assert_eq!(older["previousBefore"], cursor(&older, 0));
    assert_eq!(older["hasOlder"], true);
    let after = tail["nextAfter"].as_str().unwrap().to_owned();
    let appended: Value = client
        .get(format!(
            "{api}/v1/sessions/{session}/transcript?after={after}&limit=200"
        ))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(appended["records"].as_array().unwrap().len(), 1);
    assert_eq!(body(&appended, 0), "new");
    assert_eq!(appended["nextAfter"], cursor(&appended, 0));
}

#[test]
fn status_line_always_prints_one_line_without_a_collector() {
    let temporary = tempfile::tempdir().unwrap();
    support::install_plugins(temporary.path());
    let mut child = support::runtime_helper(temporary.path(), "status-line")
        .env("FELD_SESSION_TOKEN", "offline-token")
        .env("FELD_ACTOR", "offline")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"not-json\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "offline\n");
}

#[test]
fn self_attach_names_the_actor_and_child_cli_uses_the_self_index() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let status = fixture.json(&["daemon", "start", "--json"]);
    let api = status["daemon"]["api"].as_str().unwrap().to_owned();
    let workdir = fixture.home().join("My Agent!!");
    fs::create_dir(&workdir).unwrap();
    let result = fixture.home().join("attach.json");
    let attach_error = fixture.home().join("attach.err");
    let listing = fixture.home().join("listing.json");
    let status_line = fixture.home().join("status-line.txt");
    let script = workdir.join("claude");
    // Nothing prepared Claude Code's configuration root, which `CLAUDE_CONFIG_DIR` names
    // here, and the attach writes nothing there.
    // The Claude Code being attached has not loaded the Feld plugin, so no hook will ever
    // deliver its Runtime identity. What it does give its child processes is its session id
    // and, under its configuration root, the transcript file named after it.
    let config = fixture.home().join("claude-config");
    let runtime_session = "7c1d2e3f-4a5b-6c7d-8e9f-0a1b2c3d4e5f";
    let project = config.join("projects/-Users-me-My-Agent");
    fs::create_dir_all(&project).unwrap();
    let transcript = project.join(format!("{runtime_session}.jsonl"));
    fs::write(
        &transcript,
        b"{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n",
    )
    .unwrap();
    let child = format!(
        "HOME='{}' CLAUDE_CODE_SESSION_ID={runtime_session} CLAUDE_CONFIG_DIR='{}' {} --home {} actor attach --self --runtime claude-code --json > {} 2> {} && echo '{{}}' | env -u FELD_ACTOR -u FELD_SESSION_TOKEN FELD_HOME={} {} > {} && {} --home {} actor list --json > {}",
        fixture.home().display(),
        config.display(),
        FELD,
        fixture.home().display(),
        result.display(),
        attach_error.display(),
        fixture.home().display(),
        fixture
            .home()
            .join("plugins/runtime/claude-code/bin/status-line")
            .display(),
        status_line.display(),
        FELD,
        fixture.home().display(),
        listing.display(),
    );
    if !compile_runtime(&script, Some(&child)) {
        return;
    }
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "self",
                "-c",
                workdir.to_str().unwrap(),
                "--",
                "exec ./claude",
            ])
            .status
            .success()
    );
    // Two `feld` runs inside a tmux pane, while the rest of this file's tmux tests run
    // beside them.
    let deadline = Instant::now() + Duration::from_secs(30);
    while fs::metadata(&listing).map_or(true, |metadata| metadata.len() == 0) {
        assert!(
            Instant::now() < deadline,
            "self attach did not finish; stdout: {}; stderr: {}",
            fs::read_to_string(&result).unwrap_or_default(),
            fs::read_to_string(&attach_error).unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(25));
    }
    let attached: Value =
        serde_json::from_slice(&fs::read(result).unwrap()).unwrap_or_else(|error| {
            panic!(
                "decode self attach: {error}; stderr: {}",
                fs::read_to_string(&attach_error).unwrap_or_default()
            )
        });
    assert_eq!(attached["session"]["actor"], "my-agent-1");
    let listing: Value = serde_json::from_slice(&fs::read(listing).unwrap()).unwrap();
    assert_eq!(listing["actors"][0]["name"], "my-agent-1");
    // The status line of the attached Claude Code names its Actor, which the self index
    // keeps beside the token.
    assert_eq!(fs::read_to_string(&status_line).unwrap(), "my-agent-1\n");
    // The listings count the attached Session, as the tables do, and the history starts
    // with its first record.
    let runtimes = fixture.json(&["--json", "runtime", "list"]);
    assert_eq!(runtimes["runtimes"][0]["name"], "claude-code");
    assert_eq!(runtimes["runtimes"][0]["runningSessions"], 1);
    let printed = fixture.feld(&["runtime", "list"]);
    let printed = String::from_utf8_lossy(&printed.stdout);
    assert!(printed.contains("claude-code  Claude Code  1"), "{printed}");
    let providers = fixture.json(&["--json", "provider", "list"]);
    let tmux = providers["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["name"] == "tmux")
        .unwrap();
    assert_eq!(tmux["nodes"], json!(["local"]));
    assert_eq!(tmux["runningSessions"], 1);
    let status = fixture.json(&["--json", "daemon", "status"]);
    let records = fixture.json(&["--json", "record", "list"]);
    assert_eq!(
        status["historyStart"], records["records"][0]["at"],
        "{records}"
    );
    // The attach itself delivered the Runtime identity: the Console reads the transcript
    // without waiting for a hook, and the model stays unknown until one reports it.
    let session = attached["session"]["id"].as_str().unwrap();
    let local_token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let client = reqwest::blocking::Client::new();
    let page: Value = client
        .get(format!("{api}/v1/sessions/{session}/transcript"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(page["state"], "readable", "{page}");
    assert_eq!(page["records"].as_array().unwrap().len(), 2);
    assert_eq!(page["records"][1]["raw"]["type"], "assistant");
    assert_eq!(
        listing["actors"][0]["currentSession"]["model"],
        json!(["Claude Code", null])
    );

    // The attach was accepted without the preparation, said to run `feld init`, and left
    // the configuration root as it was.
    let notes = fs::read_to_string(&attach_error).unwrap();
    assert!(
        notes.contains("note: the Feld plugin is not under") && notes.contains("feld init"),
        "{notes}"
    );
    assert!(!config.join("settings.json").exists());
    assert!(!config.join("skills").exists());
    // This Claude Code started without `feld env`, so its Runtime metrics say so.
    let metrics: Value = reqwest::blocking::Client::new()
        .get(format!("{api}/v1/sessions/{session}/metrics"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .json()
        .unwrap();
    let projection = |name: &str| {
        metrics["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|metric| metric["metric"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(projection("token_usage")["status"], "unavailable");
    assert_eq!(
        projection("token_usage")["source"],
        "Runtime did not start with this daemon's telemetry environment; start it again from a shell that ran `eval \"$(feld env)\"`"
    );
    assert_eq!(projection("estimated_cost")["status"], "unavailable");
    assert_eq!(
        projection("context_window_utilization")["source"],
        "No correlated Runtime usage record"
    );
}

/// An attach from outside that names the pane in its own words (a session name, not `%n`)
/// writes the self index entry `--self` writes, so the Runtime's children find the token.
#[test]
fn outside_attach_writes_the_self_index_entry_of_self_attach() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fixture.json(&["daemon", "start", "--json"]);
    let token = fixture.home().join("token");
    let again = fixture.home().join("again.json");
    let again_error = fixture.home().join("again.err");
    let home = fixture.home().display();
    // The child of the Runtime waits until it resolves a token, then attaches with `--self`.
    let child = format!(
        "until HOME='{home}' {FELD} --home '{home}' self token > '{token}' 2> /dev/null; do sleep 0.1; done; \
         HOME='{home}' {FELD} --home '{home}' actor attach --self --runtime claude-code --json > '{again}' 2> '{again_error}'",
        token = token.display(),
        again = again.display(),
        again_error = again_error.display(),
    );
    let claude = fixture.home().join("claude");
    if !compile_runtime(&claude, Some(&child)) {
        return;
    }
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "outside",
                "-c",
                fixture.home().to_str().unwrap(),
                "--",
                "exec ./claude",
            ])
            .status
            .success()
    );
    let attached = fixture.json(&[
        "actor",
        "attach",
        "outside",
        "--tmux-pane",
        "outside",
        "--tmux-socket",
        fixture.socket.to_str().unwrap(),
        "--runtime",
        "claude-code",
        "--json",
    ]);

    let deadline = Instant::now() + Duration::from_secs(30);
    while fs::metadata(&again).map_or(true, |metadata| metadata.len() == 0) {
        assert!(
            Instant::now() < deadline,
            "the child did not resolve its token and attach again; token: {:?}; stderr: {}",
            fs::read_to_string(&token).ok(),
            fs::read_to_string(&again_error).unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(
        fs::read_to_string(&token).unwrap().trim(),
        attached["token"].as_str().unwrap()
    );
    let again: Value = serde_json::from_slice(&fs::read(&again).unwrap()).unwrap();
    assert_eq!(again["session"]["actor"], "outside");
    // The token the child resolved shows the keys agree; the sweep of the other entries
    // of the Session leaves one either way.
    let entries: Vec<PathBuf> = fs::read_dir(fixture.home().join("self-index"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    assert_eq!(entries.len(), 1, "{entries:?}");
    let index: Value = serde_json::from_slice(&fs::read(&entries[0]).unwrap()).unwrap();
    assert_eq!(index["sessionId"], again["session"]["id"]);
}

#[test]
fn owned_prepare_failure_closes_the_unlaunched_session_as_gone() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    // A `claude` on the daemon's PATH, so that the start reaches the Runtime's prepare.
    let claude = fixture.home().join("claude");
    if !compile_runtime(&claude, None) {
        return;
    }
    let path = format!(
        "{}:{}",
        fixture.home().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = fixture
        .command(&["daemon", "start", "--json"])
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(output.status.success());
    fixture.json(&[
        "actor",
        "create",
        "prepare-failure",
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
    fs::write(fixture.home().join("sessions"), "blocks runtime directory").unwrap();
    // The Session never began, so the start is reported as a rejection even though its
    // records committed.
    let output = fixture.feld(&["actor", "start", "prepare-failure", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let started: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(started["session"]["observation"]["kind"], "gone");
    assert_eq!(started["session"]["outcome"], "exited");
    assert!(started["session"]["exitCode"].is_null());
    assert!(started["session"]["lifecycle"]["startFailed"].is_string());
    assert!(
        started["session"]["observation"]["detail"]
            .as_str()
            .unwrap()
            .contains("the Claude Code Runtime could not be prepared")
    );
}

#[test]
fn collector_unavailability_at_owned_start_survives_daemon_restart() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let initial = fixture.json(&["daemon", "start", "--json"]);
    let address = initial["daemon"]["otlp"]
        .as_str()
        .unwrap()
        .strip_prefix("http://")
        .unwrap()
        .to_owned();
    fixture.json(&["daemon", "stop", "--json"]);
    let occupied = TcpListener::bind(address).unwrap();

    let claude = fixture.home().join("claude");
    if !compile_runtime(&claude, None) {
        return;
    }
    let path = format!(
        "{}:{}",
        fixture.home().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = fixture
        .command(&["daemon", "start", "--json"])
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let unavailable: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(unavailable["daemon"]["otlp"].is_null());
    fixture.json(&[
        "actor",
        "create",
        "no-collector",
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
    let started = fixture.json(&["actor", "start", "no-collector", "--json"]);
    let session = started["session"]["id"].as_str().unwrap().to_owned();
    let source_before = session_metric_source(&fixture, &session, "token_usage");
    assert!(source_before.contains("Collector was unavailable"));

    fixture.json(&["daemon", "stop", "--json"]);
    drop(occupied);
    let restarted = fixture.json(&["daemon", "start", "--json"]);
    assert!(restarted["daemon"]["otlp"].is_string());
    let source_after = session_metric_source(&fixture, &session, "token_usage");
    assert_eq!(source_after, source_before);
}

#[test]
fn child_only_collector_death_marks_new_sessions_until_restart_succeeds() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let claude = fixture.home().join("claude");
    if !compile_runtime(&claude, None) {
        return;
    }
    let path = format!(
        "{}:{}",
        fixture.home().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = fixture
        .command(&["daemon", "start", "--json"])
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let started: Value = serde_json::from_slice(&output.stdout).unwrap();
    let daemon_pid = started["daemon"]["pid"].as_u64().unwrap() as u32;
    let collector_pid = collector_child(daemon_pid).unwrap();
    let endpoint = started["daemon"]["otlp"].as_str().unwrap().to_owned();
    let address = endpoint.strip_prefix("http://").unwrap();
    assert!(
        Command::new("kill")
            .args(["-KILL", &collector_pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(1);
    let occupied = loop {
        match TcpListener::bind(address) {
            Ok(listener) => break listener,
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(2)),
            Err(error) => panic!("did not acquire dead Collector endpoint: {error}"),
        }
    };
    let status_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let status = fixture.json(&["daemon", "status", "--json"]);
        if status["otlp"].is_null() {
            break;
        }
        assert!(Instant::now() < status_deadline);
        thread::sleep(Duration::from_millis(10));
    }

    fixture.json(&[
        "actor",
        "create",
        "collector-down",
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
    let down = fixture.json(&["actor", "start", "collector-down", "--json"]);
    let down_session = down["session"]["id"].as_str().unwrap();
    assert!(
        session_metric_source(&fixture, down_session, "token_usage")
            .contains("Collector was unavailable")
    );

    drop(occupied);
    let restart_deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let status = fixture.json(&["daemon", "status", "--json"]);
        if status["otlp"] == endpoint {
            break;
        }
        assert!(
            Instant::now() < restart_deadline,
            "Collector did not restart: {status}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    fixture.json(&[
        "actor",
        "create",
        "collector-restored",
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
    let restored = fixture.json(&["actor", "start", "collector-restored", "--json"]);
    let restored_session = restored["session"]["id"].as_str().unwrap();
    assert_eq!(
        session_metric_source(&fixture, restored_session, "token_usage"),
        "No correlated Runtime usage record"
    );
}

fn session_metric_source(fixture: &Fixture, session: &str, metric: &str) -> String {
    let status = fixture.json(&["daemon", "status", "--json"]);
    let api = status["api"].as_str().unwrap();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let response: Value = reqwest::blocking::Client::new()
        .get(format!(
            "{api}/v1/sessions/{session}/metrics?metric={metric}"
        ))
        .bearer_auth(token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    response["metrics"][0]["source"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn attach_reports_a_committed_session_when_cache_write_fails() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fixture.json(&["daemon", "start", "--json"]);
    let claude = fixture.home().join("claude");
    if !compile_runtime(&claude, None) {
        return;
    }
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "cache-failure",
                "-c",
                fixture.home().to_str().unwrap(),
                "--",
                "exec ./claude",
            ])
            .status
            .success()
    );
    fs::write(fixture.home().join("tokens"), "blocks token directory").unwrap();
    let output = fixture.feld(&[
        "actor",
        "attach",
        "cache-failure",
        "--tmux-pane",
        "cache-failure",
        "--tmux-socket",
        fixture.socket.to_str().unwrap(),
        "--runtime",
        "claude-code",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("attach committed for Session")
    );
    let actor = fixture.json(&["actor", "get", "cache-failure", "--json"]);
    assert_eq!(
        actor["actor"]["currentSession"]["observation"]["kind"],
        "running"
    );
}

#[test]
fn parallel_reattach_keeps_the_latest_token_in_the_self_index() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let status = fixture.json(&["daemon", "start", "--json"]);
    let api = status["daemon"]["api"].as_str().unwrap();
    let claude = fixture.home().join("claude");
    if !compile_runtime(&claude, None) {
        return;
    }
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "parallel",
                "-c",
                fixture.home().to_str().unwrap(),
                "--",
                "exec ./claude",
            ])
            .status
            .success()
    );
    let attach_args = [
        "actor",
        "attach",
        "parallel",
        "--tmux-pane",
        "parallel",
        "--tmux-socket",
        fixture.socket.to_str().unwrap(),
        "--runtime",
        "claude-code",
        "--json",
    ];
    fixture.json(&attach_args);
    let first = fixture
        .command(&attach_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = fixture
        .command(&attach_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert!(first.wait_with_output().unwrap().status.success());
    assert!(second.wait_with_output().unwrap().status.success());

    let index_path = fs::read_dir(fixture.home().join("self-index"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .unwrap();
    let index: Value = serde_json::from_slice(&fs::read(index_path).unwrap()).unwrap();
    let token = fs::read_to_string(index["tokenFileLocation"].as_str().unwrap()).unwrap();
    let response = reqwest::blocking::Client::new()
        .post(format!("{api}/v1/self/runtime"))
        .bearer_auth(token.trim())
        .json(&json!({
            "sessionId": "parallel-runtime",
            "transcriptPath": "/tmp/parallel.jsonl",
            "model": "claude-test"
        }))
        .send()
        .unwrap();
    assert!(response.status().is_success());
}

#[test]
fn reattach_after_the_runtime_ends_leaves_no_entry_of_the_closed_session() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fixture.json(&["daemon", "start", "--json"]);
    let claude = fixture.home().join("claude");
    if !compile_runtime(&claude, None) {
        return;
    }
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "again",
                "-c",
                fixture.home().to_str().unwrap(),
                "--",
                "exec ./claude",
            ])
            .status
            .success()
    );
    assert!(
        fixture
            .tmux(&["set-option", "-t", "again", "remain-on-exit", "on"])
            .status
            .success()
    );
    let attach = |name: &str| {
        fixture.json(&[
            "actor",
            "attach",
            name,
            "--tmux-pane",
            "again",
            "--tmux-socket",
            fixture.socket.to_str().unwrap(),
            "--runtime",
            "claude-code",
            "--json",
        ])
    };
    let first = attach("before");
    let session = first["session"]["id"].as_str().unwrap();
    let pid = first["session"]["placement"]["placement"]["process"]["pid"]
        .as_u64()
        .unwrap();
    assert!(
        Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let observed = fixture.json(&["session", "get", session, "--json"]);
        if observed["session"]["outcome"] == "ended" {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(25));
    }

    // The same pane runs a second Runtime, as a person starts one again after `/exit`.
    assert!(
        fixture
            .tmux(&["respawn-pane", "-t", "again", "--", "exec ./claude"])
            .status
            .success()
    );
    let second = attach("after");
    let entries = |directory: &str| {
        fs::read_dir(fixture.home().join(directory))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        entries("tokens"),
        [format!(
            "{}.token",
            second["session"]["id"].as_str().unwrap()
        )]
    );
    let index = entries("self-index");
    assert_eq!(index.len(), 2, "{index:?}");
    let json = index.iter().find(|name| name.ends_with(".json")).unwrap();
    let index: Value =
        serde_json::from_slice(&fs::read(fixture.home().join("self-index").join(json)).unwrap())
            .unwrap();
    assert_eq!(index["sessionId"], second["session"]["id"]);
}

#[test]
fn otel_headers_accepts_an_owned_token_without_a_session_id() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let output = support::runtime_helper(fixture.home(), "otel-headers")
        .env("FELD_SESSION_TOKEN", "token-only")
        .output()
        .unwrap();
    assert!(output.status.success());
    let headers: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        headers["Authorization"],
        format!(
            "Bearer {}",
            feld_core::telemetry::telemetry_token("token-only")
        )
    );
}

/// The Provider answers which process is in front of the pane; the Runtime Adapter's
/// `match` decides whether it is the Runtime the caller asked for. A Claude Code left in
/// the background is not that process, so the attach is refused before the Runtime Adapter
/// prepares the caller's Claude Code configuration.
#[test]
fn attach_refuses_a_pane_whose_foreground_process_is_not_the_runtime() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    fixture.json(&["daemon", "start", "--json"]);
    let claude = fixture.home().join("claude");
    if !compile_runtime(&claude, None) {
        return;
    }
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "background",
                "-c",
                fixture.home().to_str().unwrap(),
                "--",
                "sh",
                "-c",
                "./claude & exec sleep 300",
            ])
            .status
            .success()
    );
    let caller_config = fixture.home().join("caller-config");
    let output = fixture
        .command(&[
            "actor",
            "attach",
            "background",
            "--tmux-pane",
            "background",
            "--tmux-socket",
            fixture.socket.to_str().unwrap(),
            "--runtime",
            "claude-code",
        ])
        .env("CLAUDE_CONFIG_DIR", &caller_config)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("the process in front of the execution is not Claude Code"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!caller_config.join("settings.json").exists());
    assert!(!caller_config.join("skills/feld").exists());
}

/// An attach that points at an execution from outside takes the Runtime identity from
/// that Runtime's own hook. The environment the command was typed in belongs to whoever
/// typed it, which is a Claude Code of its own whenever a person attaches a pane from
/// inside one, and its session id and transcript name another Session's conversation.
#[test]
fn an_external_attach_leaves_the_runtime_identity_to_the_runtimes_own_hook() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    let daemon = fixture.json(&["daemon", "start", "--json"]);
    let api = daemon["daemon"]["api"].as_str().unwrap().to_owned();
    let local_token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let claude = fixture.home().join("claude");
    if !compile_runtime(&claude, None) {
        return;
    }
    // The conversation of the Claude Code the command is typed in, which the attach must
    // leave alone.
    let caller_session = "9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d";
    let caller_config = fixture.home().join("caller-config");
    let caller_project = caller_config.join("projects/-Users-me-elsewhere");
    fs::create_dir_all(&caller_project).unwrap();
    fs::write(
        caller_project.join(format!("{caller_session}.jsonl")),
        b"{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"someone else\"}}\n",
    )
    .unwrap();
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "outside",
                "-c",
                fixture.home().to_str().unwrap(),
                "--",
                "exec ./claude",
            ])
            .status
            .success()
    );

    let output = fixture
        .command(&[
            "actor",
            "attach",
            "outside",
            "--tmux-pane",
            "outside",
            "--tmux-socket",
            fixture.socket.to_str().unwrap(),
            "--runtime",
            "claude-code",
            "--json",
        ])
        .env("CLAUDE_CODE_SESSION_ID", caller_session)
        .env("CLAUDE_CONFIG_DIR", &caller_config)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "attach failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let attached: Value = serde_json::from_slice(&output.stdout).unwrap();
    let session = attached["session"]["id"].as_str().unwrap().to_owned();
    assert_eq!(attached["session"]["startKind"], "attach");

    let read: Value = reqwest::blocking::Client::new()
        .get(format!("{api}/v1/sessions/{session}/transcript"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(read["state"], "pending", "{read}");
    let described: Value = reqwest::blocking::Client::new()
        .get(format!("{api}/v1/sessions/{session}"))
        .bearer_auth(local_token.trim())
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(described["session"]["model"], Value::Null);
}

#[test]
fn attach_target_arguments_are_rejected_by_clap() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    for arguments in [
        vec!["actor", "attach", "--runtime", "claude-code"],
        vec![
            "actor",
            "attach",
            "--self",
            "--tmux-pane",
            "%1",
            "--runtime",
            "claude-code",
        ],
        vec![
            "actor",
            "attach",
            "--tmux-pane",
            "%1",
            "--runtime",
            "claude-code",
        ],
        vec![
            "actor",
            "attach",
            "--self",
            "--tmux-socket",
            "feld",
            "--runtime",
            "claude-code",
        ],
    ] {
        assert_eq!(fixture.feld(&arguments).status.code(), Some(2));
    }
}
