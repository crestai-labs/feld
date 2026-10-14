//! End-to-end coverage of the real Claude Code in tmux, both as an owned Session Feld
//! starts and as one a person starts from a shell `feld init` prepared and attaches with
//! `/feld attach`. Everything runs as a person's machine runs it: the `claude` on `PATH` and the
//! Anthropic API it calls, the daemon, Collector, store, and projections, the Adapter's
//! parts beside Claude Code (hook, Plugin Monitor helper, otelHeadersHelper, status line),
//! and the `feld` the model runs from its shell. The test checks where Feld meets the
//! Runtime and leaves the wording of the model's answers alone.
//!
//! It calls the API, so it runs only where `FELD_TEST_REQUIRE` names `claude-code`, with
//! `ANTHROPIC_API_KEY` set. Each test runs in a temporary `FELD_HOME` and `HOME`, a
//! temporary Claude Code configuration root, a tmux socket of its own, and an empty workdir.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use feld_core::test_require;
use serde_json::{Value, json};
use tempfile::TempDir;

mod support;

const FELD: &str = env!("CARGO_BIN_EXE_feld");
/// The lightest model Claude Code names.
const MODEL: &str = "haiku";
/// The Actor whose Session runs Claude Code.
const AGENT: &str = "claude-e2e";
/// The Actor that never starts and whose mailbox takes the reply.
const RECEIVER: &str = "claude-e2e-reply";
/// The Actor an attach declares for the Claude Code started in the workdir `work`: the
/// basename of the cwd with the suffix of the first one running there.
const ATTACHED: &str = "work-1";
/// How long after an attach the otelHeadersHelper has run again: the minute `feld env`
/// sets, and an export interval of 10 s.
const OTEL_HEADERS_REFRESH: Duration = Duration::from_secs(75);

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
    /// `HOME`, which holds the shell settings `feld init` adds `feld env` to.
    user: PathBuf,
    config: PathBuf,
    workdir: PathBuf,
    socket: PathBuf,
    key: String,
}

impl Fixture {
    fn new() -> Option<Self> {
        let key = test_require::claude_code()?;
        if !test_require::tmux() {
            return None;
        }
        let temporary = tempfile::tempdir().unwrap();
        // Claude Code trusts a workdir by its real path, which a temporary directory of
        // macOS reaches through a link.
        let root = fs::canonicalize(temporary.path()).unwrap();
        let home = root.join("home");
        let user = root.join("user");
        let config = root.join("claude");
        let workdir = root.join("work");
        for directory in [&home, &user, &config, &workdir] {
            fs::create_dir(directory).unwrap();
        }
        // The first screens and the trust of the workdir, settled ahead the way Claude Code
        // records them once a person has answered them.
        let approved = &key[key.len().saturating_sub(20)..];
        fs::write(
            config.join(".claude.json"),
            json!({
                "hasCompletedOnboarding": true,
                "theme": "dark",
                "customApiKeyResponses": { "approved": [approved], "rejected": [] },
                "projects": { workdir.display().to_string(): { "hasTrustDialogAccepted": true } },
            })
            .to_string(),
        )
        .unwrap();
        Some(Self {
            socket: root.join("runtime.sock"),
            _temporary: temporary,
            home,
            user,
            config,
            workdir,
            key,
        })
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(&self.home)
            .env("HOME", &self.user)
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

    /// Starts the daemon with the configuration root, the key, and the `feld` under test
    /// first on `PATH`, where the model's shell finds it. The tmux server the daemon starts
    /// on the fixture socket inherits all three, and so does every pane it opens.
    fn start_daemon(&self) {
        let output = self
            .command(&["daemon", "start", "--json"])
            .env("PATH", path_with_feld())
            .env("CLAUDE_CONFIG_DIR", &self.config)
            .env("ANTHROPIC_API_KEY", &self.key)
            .env("DISABLE_AUTOUPDATER", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "daemon start failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Prepares the configuration root and the shell settings with `feld init --yes`, the
    /// way a person does before attaching, which starts the daemon as well. The settings a
    /// person keeps there already, which `feld init` leaves as they are, let the model run
    /// `feld` without stopping at a confirmation.
    fn init(&self) {
        fs::write(
            self.config.join("settings.json"),
            json!({ "model": MODEL, "permissions": { "allow": ["Bash(feld:*)"] } }).to_string(),
        )
        .unwrap();
        let output = self
            .command(&[
                "init",
                "--yes",
                "--runtime",
                "claude-code",
                "--provider",
                "tmux",
            ])
            .env("PATH", path_with_feld())
            .env("SHELL", "/bin/bash")
            .env("CLAUDE_CONFIG_DIR", &self.config)
            .output()
            .unwrap();
        if !output.status.success() {
            self.fail(&format!(
                "feld init failed:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    /// Opens a pane on the fixture socket running an interactive bash in the workdir, which
    /// reads the `feld env` line `feld init` added to `~/.bashrc`, and answers its id. The
    /// pane has the environment of a person's terminal and nothing of the test's own: no
    /// `FELD_*` and no Claude Code it may run inside.
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
            .env("HOME", &self.user)
            .env("SHELL", "/bin/bash")
            .env("FELD_HOME", &self.home)
            .env("CLAUDE_CONFIG_DIR", &self.config)
            .env("ANTHROPIC_API_KEY", &self.key)
            .env("DISABLE_AUTOUPDATER", "1")
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

    /// Types `text` into `pane` and presses Enter.
    fn type_line(&self, pane: &str, text: &str) {
        self.tmux(&["send-keys", "-t", pane, "-l", text]);
        // Claude Code reads a paste and the Enter after it as one input unless they come
        // apart.
        thread::sleep(Duration::from_millis(500));
        self.tmux(&["send-keys", "-t", pane, "Enter"]);
    }

    fn screen(&self, pane: &str) -> String {
        self.tmux(&["capture-pane", "-p", "-J", "-t", pane])
    }

    /// Whether Claude Code in `pane` has drawn its prompt, whose status line the Feld
    /// status line helper writes: `Feld` before the attach.
    fn shows_prompt(&self, pane: &str) -> bool {
        self.screen(pane)
            .lines()
            .any(|line| line.trim_start().starts_with("Feld"))
    }

    /// The transcript files Claude Code wrote under the configuration root.
    fn transcripts(&self) -> Vec<PathBuf> {
        let Ok(projects) = fs::read_dir(self.config.join("projects")) else {
            return Vec::new();
        };
        projects
            .flatten()
            .filter_map(|project| fs::read_dir(project.path()).ok())
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "jsonl")
            })
            .collect()
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

    /// Fails the test with what the panes show and what the daemon logged, so that a
    /// Claude Code stopped at a screen the test does not expect shows it in the output.
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
        panic!(
            "{what}\n{}",
            report.replace(&self.key, "[ANTHROPIC_API_KEY]")
        );
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

    /// The processes the claude-code Plugin of this home runs beside Claude Code.
    fn runtime_helpers(&self) -> String {
        let output = Command::new("pgrep")
            .arg("-fl")
            .arg(self.home.join("plugins/runtime/claude-code/"))
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A Claude Code a failed test left running ends with its Session.
        let _ = self.command(&["session", "kill", AGENT]).output();
        let _ = self.command(&["daemon", "stop"]).output();
        let _ = self.tmux(&["kill-server"]);
        support::stop_runtime_helpers(&self.home);
    }
}

#[test]
fn feld_starts_the_real_claude_code_and_reaches_it_both_ways() {
    let Some(fixture) = Fixture::new() else {
        return;
    };
    support::install_plugins(&fixture.home);
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
        "claude-code",
        "--workdir",
        workdir,
        "--tmux-socket",
        socket,
    ]);

    // 1. Claude Code takes the arguments and the settings the Adapter built, and keeps
    //    running in its pane.
    let settings = json!({
        "model": MODEL,
        // The model runs `feld` without stopping at a confirmation.
        "permissions": { "allow": ["Bash(feld:*)"] },
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
        "claude-code",
        "--runtime-settings",
        &settings,
        "--workdir",
        workdir,
        "--tmux-socket",
        socket,
    ]);
    let session = fixture.session(AGENT)["id"].as_str().unwrap().to_owned();

    // 2. The SessionStart hook delivers the Runtime identity: the model, and where the
    //    transcript file is.
    fixture.wait_for("the Runtime identity", Duration::from_secs(90), || {
        let read = fixture.session(&session);
        if !read["end"].is_null() {
            fixture.fail(&format!("Claude Code ended: {read}"));
        }
        let located = fixture.transcript(&session)["state"] != "pending";
        (read["model"].is_array() && located).then_some(())
    });

    // 3. The Plugin Monitor helper hands a Message to Claude Code and records its read.
    let message = fixture.json(&[
        "message",
        "send",
        AGENT,
        &format!("Run the shell command `feld message send {RECEIVER} pong` and nothing else."),
    ])["message"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.wait_for("the read of the Message", Duration::from_secs(60), || {
        fixture.json(&["message", "list", "--to", AGENT])["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|listed| listed["id"] == message && listed["waiting"] == false)
            .then_some(())
    });

    // 4. The `feld` the model runs from its shell arrives as this Session, and Feld reads
    //    the transcript: the Console has the Message, then what the model did with it.
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

    // 5. The telemetry Claude Code exports every 10 s and the status line both reach this
    //    Session.
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

    // 6. The end input of the Adapter ends this version of Claude Code, and nothing it ran
    //    beside itself is left.
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
fn a_person_attaches_the_real_claude_code_they_started() {
    let Some(fixture) = Fixture::new() else {
        return;
    };

    // 1. `feld init` writes the preparation where the Claude Code the person starts reads
    //    it: the Claude Code plugin and the settings in the configuration root, and the
    //    `feld env` line in the shell settings.
    fixture.init();
    assert!(
        fixture
            .config
            .join("skills/feld/.claude-plugin/plugin.json")
            .is_file()
    );
    let settings: Value =
        serde_json::from_str(&fs::read_to_string(fixture.config.join("settings.json")).unwrap())
            .unwrap();
    assert!(settings["otelHeadersHelper"].is_string(), "{settings}");
    assert!(settings["statusLine"].is_object(), "{settings}");
    fixture.json(&[
        "actor",
        "create",
        RECEIVER,
        "--kind",
        "agent",
        "--no-start",
        "--runtime",
        "claude-code",
        "--workdir",
        fixture.workdir.to_str().unwrap(),
        "--tmux-socket",
        fixture.socket.to_str().unwrap(),
    ]);

    // 2. The person starts Claude Code from a shell that read `feld env`, and gives it one
    //    input, after which the transcript file exists and Claude Code waits again.
    let pane = fixture.open_shell();
    fixture.type_line(&pane, &format!("claude --model {MODEL}"));
    fixture.wait_for("the prompt of Claude Code", Duration::from_secs(60), || {
        fixture.shows_prompt(&pane).then_some(())
    });
    fixture.type_line(&pane, "Reply with the single word ready.");
    fixture.wait_for(
        "the answer to the first input",
        Duration::from_secs(90),
        || {
            fixture
                .transcripts()
                .iter()
                .any(|transcript| {
                    fs::read_to_string(transcript)
                        .unwrap_or_default()
                        .lines()
                        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                        .any(|entry| entry["message"]["stop_reason"] == "end_turn")
                })
                .then_some(())
        },
    );

    // 3. `/feld attach` runs `feld actor attach --self --runtime claude-code` from Claude
    //    Code's shell, which finds the Claude Code process above it, keeps the token in the
    //    self index, and delivers the Runtime identity: the model, and the transcript file
    //    Feld reads the first answer from.
    fixture.type_line(&pane, "/feld attach");
    let session = fixture.wait_for("the attach", Duration::from_secs(120), || {
        let output = fixture
            .command(&["session", "get", ATTACHED, "--json"])
            .output()
            .unwrap();
        output
            .status
            .success()
            .then(|| serde_json::from_slice::<Value>(&output.stdout).unwrap()["session"].clone())
    });
    let attached_at = Instant::now();
    assert_eq!(session["ownership"], "attached", "{session}");
    assert_eq!(session["startKind"], "attach", "{session}");
    let session = session["id"].as_str().unwrap().to_owned();
    fixture.wait_for(
        "the Runtime identity and the transcript read",
        Duration::from_secs(60),
        || {
            let page = fixture.transcript(&session);
            let answered = page["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["kind"] == "response");
            (fixture.session(&session)["model"].is_array()
                && page["state"] == "readable"
                && answered)
                .then_some(())
        },
    );

    // 4. The Plugin Monitor helper, running since Claude Code started, takes the token from
    //    the self index and hands Claude Code a Message, which the model answers with the
    //    `feld` of its shell. The Message waits until Claude Code runs the otelHeadersHelper
    //    again, which `feld env` makes a minute after the last run, so that the telemetry
    //    of its turn carries the token.
    thread::sleep((attached_at + OTEL_HEADERS_REFRESH).saturating_duration_since(Instant::now()));
    let message = fixture.json(&[
        "message",
        "send",
        ATTACHED,
        &format!("Run the shell command `feld message send {RECEIVER} pong` and nothing else."),
    ])["message"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture.wait_for("the read of the Message", Duration::from_secs(90), || {
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

    // 5. The telemetry of that turn and the status line reach this Session.
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

    // 6. Feld refuses to stop what it did not start, and sees Claude Code end when the
    //    person ends it, although the pane's shell stays.
    let stop = fixture
        .command(&["session", "stop", ATTACHED])
        .output()
        .unwrap();
    assert!(
        !stop.status.success(),
        "the stop of an attached Session was accepted: {}",
        String::from_utf8_lossy(&stop.stdout)
    );
    // Claude Code asks how to exit while the Plugin Monitor helper runs, and the default,
    // to stop it and exit, answers. Claude Code 2.1.226 and older word it `Exit anyway`.
    fixture.type_line(&pane, "/exit");
    let mut answered = false;
    fixture.wait_for("the end of Claude Code", Duration::from_secs(60), || {
        let screen = fixture.screen(&pane);
        if !answered && (screen.contains("Exit and stop tasks") || screen.contains("Exit anyway")) {
            fixture.tmux(&["send-keys", "-t", &pane, "Enter"]);
            answered = true;
        }
        let read = fixture.session(&session);
        (!read["end"].is_null()).then_some(())
    });
    let closed = fixture.session(&session);
    assert_eq!(closed["endKind"], "exited", "{closed}");
}
