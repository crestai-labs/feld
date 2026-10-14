//! End-to-end coverage of the real Codex in tmux, both as an owned Session Feld starts and
//! as one a person starts from a shell `feld init` prepared and attaches from outside with
//! `feld actor attach --tmux-pane`. Everything runs as a person's machine runs it: the `codex` on `PATH` and the OpenAI API
//! it calls, the daemon, Collector, store, spool, and projections, the Adapter's parts
//! beside Codex (hook, queue helper, the rollout gauges), and the `feld` the model runs
//! from its shell inside Codex's `workspace-write` sandbox. The test checks where Feld
//! meets the Runtime and leaves the wording of the model's answers alone.
//!
//! It calls the API, so it runs only where `FELD_TEST_REQUIRE` names `codex`, with
//! `OPENAI_API_KEY` set. It runs in a temporary `FELD_HOME`, a temporary Codex
//! configuration root, a tmux socket of its own, and an empty workdir.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use feld_core::test_require;
use serde_json::{Value, json};
use tempfile::TempDir;

mod support;

const FELD: &str = env!("CARGO_BIN_EXE_feld");
/// The lightest model Codex names.
const MODEL: &str = "gpt-6-luna";
/// The Actor whose Session runs Codex.
const AGENT: &str = "codex-e2e";
/// The Actor that never starts and whose mailbox takes the reply.
const RECEIVER: &str = "codex-e2e-reply";
/// The Actor an attach declares for the Codex a person started.
const ATTACHED: &str = "codex-e2e-attached";

/// `PATH` with the `feld` under test first, where a pane's shell and the model's find it.
fn path_with_feld() -> String {
    format!(
        "{}:{}",
        Path::new(FELD).parent().unwrap().display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

struct Fixture {
    _temporary: TempDir,
    home: PathBuf,
    config: PathBuf,
    workdir: PathBuf,
    socket: PathBuf,
    key: String,
}

impl Fixture {
    fn new() -> Option<Self> {
        let key = test_require::codex()?;
        if !test_require::tmux() {
            return None;
        }
        let temporary = tempfile::tempdir().unwrap();
        // Codex trusts a workdir by its real path, which a temporary directory of macOS
        // reaches through a link.
        let root = fs::canonicalize(temporary.path()).unwrap();
        let home = root.join("home");
        let workdir = root.join("work");
        for directory in [&home, &workdir] {
            fs::create_dir(directory).unwrap();
        }
        // `feld init` prepares the configuration root under the fixture HOME.
        let config = home.join(".codex");
        Some(Self {
            socket: root.join("runtime.sock"),
            _temporary: temporary,
            home,
            config,
            workdir,
            key,
        })
    }

    /// The sign-in and the trust of the workdir, settled ahead the way Codex records them
    /// once a person has answered them.
    fn settle_first_screens(&self) {
        let mut settings = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.config.join("config.toml"))
            .unwrap();
        write!(
            settings,
            "\n[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
            self.workdir.display()
        )
        .unwrap();
        let mut login = Command::new("codex")
            .args(["login", "--with-api-key"])
            .env("HOME", &self.home)
            .env("CODEX_HOME", &self.config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        login
            .stdin
            .take()
            .unwrap()
            .write_all(self.key.as_bytes())
            .unwrap();
        let output = login.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "codex login failed:\nstdout: {}\nstderr: {}",
            self.redact(&String::from_utf8_lossy(&output.stdout)),
            self.redact(&String::from_utf8_lossy(&output.stderr))
        );
    }

    fn redact(&self, text: &str) -> String {
        text.replace(&self.key, "[OPENAI_API_KEY]")
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(&self.home)
            .env("HOME", &self.home)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .args(arguments);
        command
    }

    fn json(&self, arguments: &[&str]) -> Value {
        let output = self.command(arguments).arg("--json").output().unwrap();
        if !output.status.success() {
            self.fail(&format!(
                "feld {} failed:\nstdout: {}\nstderr: {}",
                arguments.join(" "),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// Starts the daemon with the configuration root and the `feld` under test first on
    /// `PATH`, where the model's shell finds it. The tmux server the daemon starts on the
    /// fixture socket inherits both, and so does every pane it opens. The key stays in the
    /// configuration root the sign-in wrote it to.
    fn start_daemon(&self) {
        let output = self
            .command(&["daemon", "start", "--json"])
            .env("PATH", path_with_feld())
            .env("CODEX_HOME", &self.config)
            .env_remove("OPENAI_API_KEY")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "daemon start failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Prepares the configuration root and the shell settings with `feld init --yes`, the
    /// way a person does before attaching, which starts the daemon as well.
    fn init(&self) {
        support::plugin_executable("codex");
        support::plugin_executable("tmux");
        let output = self
            .command(&["init", "--yes", "--runtime", "codex", "--provider", "tmux"])
            .env("PATH", path_with_feld())
            .env("SHELL", "/bin/bash")
            .env("CODEX_HOME", &self.config)
            .env_remove("OPENAI_API_KEY")
            .output()
            .unwrap();
        if !output.status.success() {
            self.fail(&format!(
                "feld init failed:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        support::warm_up_plugins(&self.home);
        support::warm_up_runtime_parts(&self.config);
    }

    /// Opens a pane on the fixture socket running an interactive bash in the workdir, which
    /// reads the `feld env` line `feld init` added to `~/.bashrc`, and answers its id. The
    /// pane has the environment of a person's terminal and nothing of the test's own: no
    /// `FELD_*`, no key, and no Runtime it may run inside.
    fn open_shell(&self) -> String {
        let mut command = Command::new("tmux");
        command.env_clear();
        for variable in ["USER", "LOGNAME", "LANG", "LC_ALL", "TMPDIR"] {
            if let Some(value) = std::env::var_os(variable) {
                command.env(variable, value);
            }
        }
        let output = command
            .env("PATH", path_with_feld())
            .env("HOME", &self.home)
            .env("SHELL", "/bin/bash")
            .env("FELD_HOME", &self.home)
            .env("CODEX_HOME", &self.config)
            .arg("-S")
            .arg(&self.socket)
            .args([
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .args(["-x", "200", "-y", "50", "-c"])
            .arg(&self.workdir)
            .args(["bash", "-i"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "tmux new-session failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// The environment the shell in `pane` holds, as `env` prints it.
    fn shell_environment(&self, pane: &str) -> String {
        let written = self.workdir.join(".shell-environment");
        self.type_line(pane, &format!("env > {}", written.display()));
        let environment = self.wait_for("the shell's environment", Duration::from_secs(15), || {
            fs::read_to_string(&written)
                .ok()
                .filter(|text| text.ends_with('\n'))
        });
        fs::remove_file(&written).unwrap();
        environment
    }

    /// Types `text` into `pane` and presses Enter.
    fn type_line(&self, pane: &str, text: &str) {
        self.tmux(&["send-keys", "-t", pane, "-l", text]);
        // Codex reads a paste and the Enter after it as one input unless they come apart.
        thread::sleep(Duration::from_millis(500));
        self.tmux(&["send-keys", "-t", pane, "Enter"]);
    }

    /// Whether Codex in `pane` has drawn its input line.
    fn shows_prompt(&self, pane: &str) -> bool {
        self.tmux(&["capture-pane", "-p", "-J", "-t", pane])
            .lines()
            .any(|line| line.starts_with("› "))
    }

    /// Whether a rollout file Codex wrote under the configuration root has a finished turn.
    fn has_finished_turn(&self) -> bool {
        fn rollouts(directory: &Path, found: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    rollouts(&path, found);
                } else if path
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
                {
                    found.push(path);
                }
            }
        }
        let mut found = Vec::new();
        rollouts(&self.config.join("sessions"), &mut found);
        found.iter().any(|rollout| {
            fs::read_to_string(rollout)
                .unwrap_or_default()
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .any(|record| record["payload"]["type"] == "task_complete")
        })
    }

    fn tmux(&self, arguments: &[&str]) -> String {
        let output = Command::new("tmux")
            .arg("-S")
            .arg(&self.socket)
            .args(arguments)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Polls `ready` until it answers, failing with what the pane shows and what the daemon
    /// logged once `timeout` has passed.
    fn wait_for<T>(
        &self,
        what: &str,
        timeout: Duration,
        mut ready: impl FnMut() -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(value) = ready() {
                return value;
            }
            if Instant::now() > deadline {
                self.fail(&format!("{what} did not happen within {timeout:?}"));
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    /// Fails the test with what the panes show and what the daemon logged, so that a Codex
    /// stopped at a screen the test does not expect shows it in the output.
    fn fail(&self, what: &str) -> ! {
        let mut report = String::new();
        for pane in self.tmux(&["list-panes", "-a", "-F", "#{pane_id}"]).lines() {
            report.push_str(&format!(
                "--- pane {pane} ---\n{}\n",
                self.tmux(&["capture-pane", "-p", "-J", "-S", "-200", "-t", pane])
            ));
        }
        for actor in [AGENT, ATTACHED] {
            let metrics = self
                .command(&["session", "metrics", actor, "--json"])
                .output()
                .unwrap();
            if metrics.status.success() {
                report.push_str(&format!(
                    "--- metrics of {actor} ---\n{}\n",
                    String::from_utf8_lossy(&metrics.stdout)
                ));
            }
        }
        report.push_str(&format!(
            "--- daemon log ---\n{}\n",
            fs::read_to_string(self.home.join("daemon.log")).unwrap_or_default()
        ));
        panic!("{what}\n{}", self.redact(&report));
    }

    fn session(&self, session: &str) -> Value {
        self.json(&["session", "get", session])["session"].clone()
    }

    fn transcript(&self, session: &str) -> Value {
        self.json(&["session", "log", session, "--tail", "1000"])
    }

    fn metric(&self, session: &str, name: &str) -> Value {
        self.json(&["session", "metrics", session])["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|metric| metric["metric"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("metric {name} is missing"))
    }

    /// The processes the codex Plugin of this home runs beside Codex.
    fn runtime_helpers(&self) -> String {
        let output = Command::new("pgrep")
            .arg("-fl")
            .arg(self.home.join("plugins/runtime/codex/"))
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A Codex a failed test left running ends with its Session.
        let _ = self.command(&["session", "kill", AGENT]).output();
        let _ = self.command(&["daemon", "stop"]).output();
        let _ = self.tmux(&["kill-server"]);
        support::stop_runtime_helpers(&self.home);
    }
}

#[test]
fn feld_starts_the_real_codex_and_reaches_it_both_ways() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    // `feld init` puts the spool among the roots the sandbox may write.
    support::install_plugins_with_codex(&fixture.home);
    fixture.settle_first_screens();
    fixture.start_daemon();
    let workdir = fixture.workdir.to_str().unwrap();
    let socket = fixture.socket.to_str().unwrap();
    fixture.json(&[
        "actor",
        "create",
        RECEIVER,
        "--kind",
        "agent",
        "--no-start",
        "--runtime",
        "codex",
        "--workdir",
        workdir,
        "--tmux-socket",
        socket,
    ]);

    // 1. Codex takes the arguments and the settings the Adapter built, and keeps running in
    //    its pane. Its first turn starts from the initial prompt.
    let settings = json!({
        "model": MODEL,
        "model_reasoning_effort": "low",
        // The model runs `feld` without stopping at a confirmation, inside the sandbox.
        "approval_policy": "never",
        "sandbox_mode": "workspace-write",
    })
    .to_string();
    fixture.json(&[
        "actor",
        "create",
        AGENT,
        "--kind",
        "agent",
        "--start",
        "--runtime",
        "codex",
        "--runtime-settings",
        &settings,
        "--workdir",
        workdir,
        "--tmux-socket",
        socket,
    ]);
    let session = fixture.session(AGENT)["id"].as_str().unwrap().to_owned();

    // 2. The SessionStart hook of the first turn delivers the Runtime identity: the model,
    //    and where the rollout file is.
    fixture.wait_for("the Runtime identity", Duration::from_secs(90), || {
        let read = fixture.session(&session);
        if !read["end"].is_null() {
            fixture.fail(&format!("Codex ended: {read}"));
        }
        let located = fixture.transcript(&session)["state"] != "pending";
        (read["model"].is_array() && located).then_some(())
    });

    // 3. The queue helper puts a Message in Codex's queue and records its read once a turn
    //    has taken it.
    let message = fixture.json(&[
        "message",
        "send",
        AGENT,
        &format!("Run the shell command `feld message send {RECEIVER} pong` and nothing else."),
    ])["message"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.wait_for("the read of the Message", Duration::from_secs(120), || {
        fixture.json(&["message", "list", "--to", AGENT])["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|listed| listed["id"] == message && listed["waiting"] == false)
            .then_some(())
    });

    // 4. The `feld` the model runs inside the sandbox arrives through the spool as this
    //    Session, and Feld reads the rollout file: the Console has the Message, then what
    //    the model did with it.
    let reply = fixture.wait_for("the reply", Duration::from_secs(180), || {
        fixture.json(&["message", "list", "--from", AGENT, "--to", RECEIVER])["messages"]
            .as_array()
            .unwrap()
            .first()
            .cloned()
    });
    assert_eq!(reply["fromSessionId"], session, "{reply}");
    fixture.wait_for(
        "the Message and the answer in the Console",
        Duration::from_secs(60),
        || {
            let page = fixture.transcript(&session);
            let entries = page["records"].as_array().unwrap();
            let delivered = entries.iter().position(|entry| {
                entry["kind"] == "notification"
                    && entry["text"]
                        .as_str()
                        .unwrap_or_default()
                        .contains(&message)
            })?;
            entries[delivered + 1..]
                .iter()
                .any(|entry| matches!(entry["kind"].as_str(), Some("tool-call" | "response")))
                .then_some(())
        },
    );

    // 5. The telemetry Codex exports every 10 s and the gauges the Stop hook reads from the
    //    rollout file both reach this Session.
    fixture.wait_for(
        "the token usage and the context window",
        Duration::from_secs(90),
        || {
            let tokens = fixture.metric(&session, "token_usage");
            let context = fixture.metric(&session, "context_window_utilization");
            let reported = |metric: &Value| {
                metric["status"] == "reported" && !metric["series"].as_array().unwrap().is_empty()
            };
            (reported(&tokens)
                && tokens["series"][0]["value"]["total"].as_f64() > Some(0.0)
                && reported(&context))
            .then_some(())
        },
    );

    // 6. The end input of the Adapter ends this version of Codex, and the queue helper it
    //    ran beside itself is not left.
    fixture.json(&["session", "stop", AGENT]);
    fixture.wait_for("the stop", Duration::from_secs(60), || {
        let read = fixture.session(&session);
        (!read["end"].is_null()).then_some(read)
    });
    let closed = fixture.session(&session);
    assert_eq!(closed["outcome"], "stopped", "{closed}");
    fixture.wait_for("the end of the helpers", Duration::from_secs(15), || {
        fixture.runtime_helpers().trim().is_empty().then_some(())
    });
}

#[test]
fn a_person_attaches_the_real_codex_they_started() {
    let Some(fixture) = Fixture::new() else {
        return;
    };

    // 1. `feld init` writes the preparation where the Codex the person starts reads it:
    //    the hooks, the `feld` skill, and the spool among the roots the sandbox may write,
    //    and the `feld env` line in the shell settings.
    fixture.init();
    assert!(fixture.config.join("hooks.json").is_file());
    assert!(fixture.config.join("skills/feld/SKILL.md").is_file());
    let settings = fs::read_to_string(fixture.config.join("config.toml")).unwrap();
    assert!(
        settings.contains(&fixture.home.join("spool/shared").display().to_string()),
        "{settings}"
    );
    fixture.settle_first_screens();
    let workdir = fixture.workdir.to_str().unwrap();
    let socket = fixture.socket.to_str().unwrap();
    fixture.json(&[
        "actor",
        "create",
        RECEIVER,
        "--kind",
        "agent",
        "--no-start",
        "--runtime",
        "codex",
        "--workdir",
        workdir,
        "--tmux-socket",
        socket,
    ]);

    // 2. The person starts Codex from a shell that read `feld env`, which holds the API of
    //    the daemon and the exporter of the Collector, with the thread in the TUI's own process, and gives it one input, after which the rollout file exists and
    //    Codex waits again. The hooks run before anyone answers their trust.
    let pane = fixture.open_shell();
    let shell = fixture.shell_environment(&pane);
    let exports = fixture.json(&["env"]);
    for (key, value) in exports.as_object().unwrap() {
        let line = format!("{key}={}", value.as_str().unwrap());
        assert!(
            shell.lines().any(|held| held == line),
            "{line} is not in {shell}"
        );
    }
    assert!(exports.get("FELD_API").is_some(), "{exports}");
    assert!(
        exports.get("OTEL_EXPORTER_OTLP_ENDPOINT").is_some(),
        "{exports}"
    );
    fixture.type_line(
        &pane,
        &format!(
            "codex --no-daemon --model {MODEL} -c model_reasoning_effort=low \
             --sandbox workspace-write --ask-for-approval never --dangerously-bypass-hook-trust"
        ),
    );
    fixture.wait_for("the prompt of Codex", Duration::from_secs(60), || {
        fixture.shows_prompt(&pane).then_some(())
    });
    fixture.type_line(&pane, "Reply with the single word ready.");
    fixture.wait_for(
        "the answer to the first input",
        Duration::from_secs(120),
        || fixture.has_finished_turn().then_some(()),
    );

    // 3. The attach from outside finds Codex in front of the pane, keeps the token in the
    //    self index, and delivers the Runtime identity from the rollout file Codex holds
    //    open, so the Session learns where it is while Codex waits for its next input.
    let attached = fixture.json(&[
        "actor",
        "attach",
        ATTACHED,
        "--runtime",
        "codex",
        "--tmux-pane",
        &pane,
        "--tmux-socket",
        socket,
    ])["session"]
        .clone();
    assert_eq!(attached["ownership"], "attached", "{attached}");
    assert_eq!(attached["startKind"], "attach", "{attached}");
    let session = attached["id"].as_str().unwrap().to_owned();
    fixture.wait_for("the rollout file located", Duration::from_secs(60), || {
        (fixture.transcript(&session)["state"] != "pending").then_some(())
    });

    // 4. The queue helper the attach started with its token puts a Message into Codex's
    //    queue without a hook firing, and the model answers with the `feld` of its
    //    sandboxed shell.
    let message = fixture.json(&[
        "message",
        "send",
        ATTACHED,
        &format!("Run the shell command `feld message send {RECEIVER} pong` and nothing else."),
    ])["message"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.wait_for("the read of the Message", Duration::from_secs(120), || {
        fixture.json(&["message", "list", "--to", ATTACHED])["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|listed| listed["id"] == message && listed["waiting"] == false)
            .then_some(())
    });
    let reply = fixture.wait_for("the reply", Duration::from_secs(180), || {
        fixture.json(&["message", "list", "--from", ATTACHED, "--to", RECEIVER])["messages"]
            .as_array()
            .unwrap()
            .first()
            .cloned()
    });
    assert_eq!(reply["fromSessionId"], session, "{reply}");

    // 5. Codex took the header of its telemetry when it started, before the attach, so the
    //    token usage has no Session to reach; the gauges the Stop hook reads from the
    //    rollout file reach this one.
    fixture.wait_for("the context window", Duration::from_secs(90), || {
        let context = fixture.metric(&session, "context_window_utilization");
        (context["status"] == "reported" && !context["series"].as_array().unwrap().is_empty())
            .then_some(())
    });
    let tokens = fixture.metric(&session, "token_usage");
    assert_eq!(tokens["status"], "unavailable", "{tokens}");

    // 6. Feld refuses to stop what it did not start, and sees Codex end when the person
    //    ends it, although the pane's shell stays. The queue helper ends with it.
    let stop = fixture
        .command(&["session", "stop", ATTACHED])
        .output()
        .unwrap();
    assert!(
        !stop.status.success(),
        "the stop of an attached Session was accepted: {}",
        String::from_utf8_lossy(&stop.stdout)
    );
    fixture.type_line(&pane, "/quit");
    fixture.wait_for("the end of Codex", Duration::from_secs(60), || {
        (!fixture.session(&session)["end"].is_null()).then_some(())
    });
    let closed = fixture.session(&session);
    assert_eq!(closed["endKind"], "exited", "{closed}");
    fixture.wait_for("the end of the helpers", Duration::from_secs(15), || {
        fixture.runtime_helpers().trim().is_empty().then_some(())
    });
}
