use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Barrier};
use std::thread;

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use feld_core::model::{Execution, Placement, Requester};
use feld_daemon::store::Store;
use feld_daemon::store::{RequestIdentity, ScheduledAdmission, ScheduledAdmissionRequest};
use reqwest::blocking::Client;
use serde_json::{Value, json};
use tempfile::TempDir;

mod support;

struct Fixture {
    temp: TempDir,
    socket: PathBuf,
    bin: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::installing(support::install_plugins)
    }

    /// A Fixture whose home has the Plugins `install` puts there.
    fn installing(install: fn(&Path)) -> Self {
        let temp = tempfile::tempdir().expect("create temporary Feld home");
        let socket = temp.path().join("tmux.sock");
        // A stub on PATH keeps the claude-code Runtime from starting the real
        // Claude Code, which would read and write the developer's ~/.claude.
        let bin = temp.path().join("bin");
        fs::create_dir_all(&bin).expect("create stub bin directory");
        let claude = bin.join("claude");
        fs::write(&claude, "#!/bin/sh\nexec sleep 300\n").expect("write claude stub");
        fs::set_permissions(&claude, fs::Permissions::from_mode(0o755))
            .expect("make the claude stub executable");
        install(temp.path());
        Self { temp, socket, bin }
    }

    fn home(&self) -> &Path {
        self.temp.path()
    }

    fn socket(&self) -> &str {
        self.socket.to_str().expect("tmux socket path is UTF-8")
    }

    fn command(&self) -> Command {
        let path = match std::env::var_os("PATH") {
            Some(path) => {
                let mut entries = vec![self.bin.clone()];
                entries.extend(std::env::split_paths(&path));
                std::env::join_paths(entries).expect("join PATH entries")
            }
            None => self.bin.clone().into_os_string(),
        };
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(self.home())
            .current_dir(self.home())
            .env("HOME", self.home())
            .env("PATH", path)
            .env_remove("TMUX");
        command
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        self.command().args(arguments).output().expect("run feld")
    }

    /// `feld` as a tmux client runs it, which is what a Provider's `self` answers from.
    fn feld_in_tmux(&self, arguments: &[&str]) -> Output {
        self.command()
            .env(
                "TMUX",
                format!("{},{},0", self.socket(), std::process::id()),
            )
            .env("TMUX_PANE", "%0")
            .args(arguments)
            .output()
            .expect("run feld")
    }

    fn json(&self, arguments: &[&str]) -> Value {
        json_output(arguments, self.feld(arguments))
    }

    fn start(&self) -> Value {
        self.json(&["daemon", "start", "--json"])
    }

    fn api(&self) -> (String, String) {
        let status = self.json(&["daemon", "status", "--json"]);
        let token = fs::read_to_string(self.home().join("admin/api-token")).unwrap();
        (status["api"].as_str().unwrap().to_owned(), token)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
        let _ = Command::new("tmux")
            .arg("-S")
            .arg(&self.socket)
            .arg("kill-server")
            .output();
    }
}

fn json_output(arguments: &[&str], output: Output) -> Value {
    assert!(
        output.status.success(),
        "feld {} failed:\nstdout: {}\nstderr: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse feld JSON output")
}

fn launch(workdir: &str, command: &[&str]) -> Value {
    json!({
        "command": command,
        "workdir": workdir,
        "provider": "tmux",
        "node": "local",
        "providerOptions": {"socket": "feld", "unit": "session"}
    })
}

#[test]
fn skipped_ticks_are_half_open_in_api_and_visible_in_actor_get_and_list_records() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.json(&[
        "actor",
        "create",
        "skip-visible",
        "--kind",
        "job",
        "--cron",
        "0 0 1 1 *",
        "--timezone",
        "UTC",
        "--json",
        "--",
        "sleep",
        "30",
    ]);
    let store = Store::open(&fixture.home().join("admin/feld.db")).unwrap();
    let actor = store.get_actor("default", "skip-visible").unwrap().unwrap();
    let launch = actor.launch_spec.unwrap();
    let execution = Execution {
        runtime: None,
        execution_type: "program".to_owned(),
        label: "sleep program".to_owned(),
        command: Some(vec!["sleep".to_owned(), "30".to_owned()]),
        ai: false,
        telemetry_collector_available: None,
        telemetry_runtime_prepared: None,
    };
    let placement = Placement {
        provider: "tmux".to_owned(),
        node: "local".to_owned(),
        workdir: launch.workdir.clone().unwrap(),
        placement: json!({
            "endpoint": {"kind": "name", "value": "feld"},
            "unit": "session"
        }),
    };
    let current = store
        .admit_start(
            "default",
            "skip-visible",
            &launch,
            &execution,
            &placement,
            RequestIdentity {
                requester: &Requester::Admin,
                session_id: None,
            },
        )
        .unwrap();
    let schedule = store
        .get_schedule("default", "skip-visible")
        .unwrap()
        .unwrap();
    // The skip mark only counts ticks accepted strictly after the current Session
    // start, so derive the tick times from the Session that just started.
    let session_start = DateTime::parse_from_rfc3339(&current.session.start)
        .unwrap()
        .with_timezone(&Utc);
    let skipped_at = session_start + chrono::Duration::milliseconds(10);
    let after_skipped_at = skipped_at + chrono::Duration::milliseconds(1);
    let accepted = skipped_at.to_rfc3339_opts(SecondsFormat::Millis, true);
    let after_accepted = after_skipped_at.to_rfc3339_opts(SecondsFormat::Millis, true);
    let tokyo = FixedOffset::east_opt(9 * 3600).unwrap();
    let accepted_in_tokyo = skipped_at
        .with_timezone(&tokyo)
        .to_rfc3339_opts(SecondsFormat::Millis, false);
    let after_accepted_in_tokyo = after_skipped_at
        .with_timezone(&tokyo)
        .to_rfc3339_opts(SecondsFormat::Millis, false);
    assert!(matches!(
        store
            .admit_scheduled(
                "default",
                "skip-visible",
                ScheduledAdmissionRequest {
                    launch: &launch,
                    schedule: &schedule,
                    planned: "2026-09-16T02:01:00Z",
                    accepted: &accepted,
                    execution: &execution,
                    placement: &placement,
                },
            )
            .unwrap(),
        ScheduledAdmission::Skipped(_)
    ));

    let (api, token) = fixture.api();
    let client = Client::new();
    let skips: Value = client
        .get(format!("{api}/v1/records"))
        .bearer_auth(&token)
        .query(&[
            ("workspace", "default"),
            ("actor", "skip-visible"),
            ("kind", "schedule.skipped"),
            ("start", accepted_in_tokyo.as_str()),
            ("end", after_accepted_in_tokyo.as_str()),
        ])
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    let skipped = skips["records"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["kind"], "schedule.skipped");
    assert_eq!(skipped[0]["actor"]["name"], "skip-visible");
    assert_eq!(skipped[0]["sessionId"], current.session.id);
    assert_eq!(skipped[0]["data"]["planned"], "2026-09-16T02:01:00Z");
    assert_eq!(skipped[0]["data"]["currentSessionId"], current.session.id);
    let empty: Value = client
        .get(format!("{api}/v1/records"))
        .bearer_auth(&token)
        .query(&[
            ("actor", "skip-visible"),
            ("kind", "schedule.skipped"),
            ("start", accepted.as_str()),
            ("end", accepted.as_str()),
        ])
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert!(empty["records"].as_array().unwrap().is_empty());

    let got = fixture.json(&["actor", "get", "skip-visible", "--json"]);
    assert_eq!(got["actor"]["schedule"]["skippedTicks"], 1);
    let list = fixture.feld(&["actor", "list"]);
    assert!(list.status.success());
    let list = String::from_utf8(list.stdout).unwrap();
    let skip_row: Vec<_> = list
        .lines()
        .find(|line| line.starts_with("skip-visible "))
        .unwrap()
        .split_whitespace()
        .collect();
    assert_eq!(skip_row[8], "skip");
    let records_of_session: Value = client
        .get(format!("{api}/v1/records"))
        .bearer_auth(&token)
        .query(&[("session", current.session.id.as_str())])
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert!(
        records_of_session["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["kind"] == "schedule.skipped")
    );
    let skipped_in_range: Value = client
        .get(format!(
            "{api}/v1/records?kind=schedule.skipped&start={accepted}&end={after_accepted}"
        ))
        .bearer_auth(token)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(skipped_in_range["records"].as_array().unwrap().len(), 1);
}

#[test]
fn cli_resolves_workspace_and_records_actor_schedule_without_sessions() {
    let fixture = Fixture::new();
    let started = fixture.start();
    let daemon_timezone = started["daemon"]["timezone"].clone();
    fixture.json(&["workspace", "create", "env-space", "--json"]);
    fixture.json(&["workspace", "create", "flag-space", "--json"]);

    let env_actor = fixture
        .command()
        .env("FELD_WORKSPACE", "env-space")
        .args([
            "actor", "create", "env-job", "--kind", "job", "--json", "--", "echo", "env",
        ])
        .output()
        .unwrap();
    let env_actor = json_output(&["actor", "create", "env-job"], env_actor);
    assert_eq!(env_actor["actor"]["workspace"], "env-space");

    let flag_actor = fixture
        .command()
        .env("FELD_WORKSPACE", "env-space")
        .args([
            "--workspace",
            "flag-space",
            "actor",
            "create",
            "flag-job",
            "--kind",
            "job",
            "--json",
            "--",
            "echo",
            "flag",
        ])
        .output()
        .unwrap();
    let flag_actor = json_output(&["actor", "create", "flag-job"], flag_actor);
    assert_eq!(flag_actor["actor"]["workspace"], "flag-space");

    // Yearly crons keep the daemon from ticking `scheduled` while the test runs.
    let job = fixture.json(&[
        "actor",
        "create",
        "scheduled",
        "--kind",
        "job",
        "--cron",
        "0 0 1 1 *",
        "--json",
        "--",
        "echo",
        "tick",
    ]);
    assert_eq!(job["actor"]["schedule"]["timezone"], daemon_timezone);
    assert!(job["actor"]["schedule"]["nextRun"].is_string());
    assert_eq!(job["actor"]["currentSession"], Value::Null);
    assert_eq!(job["actor"]["sessions"], json!([]));

    fixture.json(&["actor", "pause", "scheduled", "--json"]);
    let paused_recreated = fixture.json(&[
        "actor",
        "create",
        "scheduled",
        "--kind",
        "job",
        "--cron",
        "0 0 2 1 *",
        "--json",
        "--",
        "printf",
        "new",
    ]);
    assert_eq!(paused_recreated["actor"]["schedule"]["enabled"], false);
    assert_eq!(paused_recreated["actor"]["schedule"]["cron"], "0 0 2 1 *");
    fixture.json(&["actor", "resume", "scheduled", "--json"]);
    // A Schedule takes effect from the next tick, a launchSpec from the next start.
    fixture.json(&[
        "actor", "create", "applies", "--kind", "job", "--json", "--", "true",
    ]);
    let applies = |arguments: &[&str]| {
        let mut all = vec!["actor", "update", "applies"];
        all.extend_from_slice(arguments);
        let output = fixture.feld(&all);
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("applies:")
                    .map(str::trim)
                    .map(str::to_owned)
            })
            .unwrap()
    };
    assert_eq!(applies(&["--cron", "0 0 1 1 *"]), "next tick");
    assert_eq!(applies(&["--", "false"]), "next start");
    assert_eq!(
        applies(&["--cron", "0 0 2 1 *", "--", "true"]),
        "next start or tick"
    );
    fixture.json(&["actor", "delete", "applies", "--json"]);
    // A relative workdir resolves against the directory the CLI runs in, and has to be a
    // directory a start could enter.
    // It is stored canonical, with `..` and symlinks resolved.
    fs::create_dir(fixture.home().join("relative-workdir")).unwrap();
    fs::create_dir(fixture.home().join("rejected-workdir")).unwrap();
    let updated = fixture.json(&[
        "actor",
        "update",
        "scheduled",
        "--workdir",
        "rejected-workdir/../relative-workdir",
        "--json",
    ]);
    assert_eq!(
        updated["actor"]["launchSpec"]["workdir"],
        fs::canonicalize(fixture.home().join("relative-workdir"))
            .unwrap()
            .to_str()
            .unwrap()
    );
    // One update commits the launchSpec and the Schedule together, or neither.
    let rejected_update = fixture.feld(&[
        "actor",
        "update",
        "scheduled",
        "--workdir",
        "rejected-workdir",
        "--cron",
        "invalid",
        "--json",
    ]);
    assert_eq!(rejected_update.status.code(), Some(1));
    assert!(rejected_update.stdout.is_empty());
    let after_rejected = fixture.json(&["actor", "get", "scheduled", "--json"]);
    assert!(
        after_rejected["actor"]["launchSpec"]["workdir"]
            .as_str()
            .unwrap()
            .ends_with("/relative-workdir")
    );
    assert_eq!(after_rejected["actor"]["schedule"]["cron"], "0 0 2 1 *");
    for arguments in [
        vec![
            "actor",
            "create",
            "ambiguous-runtime",
            "--kind",
            "job",
            "--runtime",
            "claude-code",
            "--json",
            "--",
            "echo",
        ],
        vec![
            "actor",
            "create",
            "ambiguous-settings",
            "--kind",
            "job",
            "--runtime-settings",
            "{}",
            "--json",
            "--",
            "echo",
        ],
        vec![
            "actor",
            "update",
            "scheduled",
            "--runtime",
            "claude-code",
            "--json",
            "--",
            "echo",
        ],
        vec![
            "actor",
            "update",
            "scheduled",
            "--runtime-settings",
            "{}",
            "--json",
            "--",
            "echo",
        ],
    ] {
        let ambiguous = fixture.feld(&arguments);
        assert_eq!(ambiguous.status.code(), Some(2));
        assert!(ambiguous.stdout.is_empty());
        let error: Value = serde_json::from_slice(&ambiguous.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "operational");
    }
    // A Job starts a command, so an update to a runtime is refused and keeps the launchSpec.
    let refused = fixture.feld(&[
        "actor",
        "update",
        "scheduled",
        "--runtime",
        "claude-code",
        "--json",
    ]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty());
    let kept = fixture.json(&["actor", "get", "scheduled", "--json"]);
    assert!(kept["actor"]["launchSpec"].get("runtime").is_none());
    assert!(kept["actor"]["launchSpec"]["command"].is_array());
    let command_launch = fixture.json(&[
        "actor",
        "update",
        "scheduled",
        "--json",
        "--",
        "echo",
        "again",
    ]);
    assert_eq!(command_launch["actor"]["launchSpec"]["command"][1], "again");

    let created_only = fixture.json(&[
        "actor",
        "create",
        "created-agent",
        "--kind",
        "agent",
        "--no-start",
        "--runtime",
        "claude-code",
        "--json",
    ]);
    assert_eq!(created_only["actor"]["kind"], "agent");
    let started_agent = fixture.feld(&[
        "actor",
        "create",
        "start-later",
        "--kind",
        "agent",
        "--runtime",
        "claude-code",
        "--tmux-socket",
        fixture.socket(),
        "--json",
    ]);
    assert!(started_agent.status.success());
    let started_agent: Value = serde_json::from_slice(&started_agent.stdout).unwrap();
    assert_eq!(started_agent["session"]["actor"], "start-later");
    assert_eq!(
        started_agent["session"]["execution"]["type"],
        "agent-runtime"
    );
    let job_start = fixture.feld(&[
        "actor",
        "create",
        "start-job-later",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        fixture.socket(),
        "--json",
        "--",
        "true",
    ]);
    assert!(job_start.status.success());
    let job_start: Value = serde_json::from_slice(&job_start.stdout).unwrap();
    assert_eq!(job_start["actor"]["name"], "start-job-later");
    assert!(
        job_start["actor"]["id"]
            .as_str()
            .unwrap()
            .starts_with("actor_")
    );
    // The start answers with what the Provider saw right after the launch, which on a slow
    // machine can still be the running `true`.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        fixture.json(&["actor", "list", "--refresh", "--json"]);
        let got = fixture.json(&["actor", "get", "start-job-later", "--json"]);
        if got["actor"]["latestSession"]["outcome"] == "succeeded" {
            assert_eq!(
                got["actor"]["latestSession"]["id"],
                job_start["session"]["id"]
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the Job did not succeed: {got}"
        );
        thread::sleep(std::time::Duration::from_millis(25));
    }

    let jobs = fixture.json(&["actor", "list", "--kind", "job", "--json"]);
    assert!(
        jobs["actors"]
            .as_array()
            .unwrap()
            .iter()
            .all(|actor| actor["kind"] == "job")
    );
    let human_list = fixture.feld(&["actor", "list"]);
    assert!(human_list.status.success());
    let human_list = String::from_utf8(human_list.stdout).unwrap();
    let mut lines = human_list.lines();
    assert_eq!(lines.next(), Some("Workspace: default"));
    let header = lines.next().expect("human actor list prints a header");
    assert!(header.starts_with("NAME "));
    assert_eq!(
        header.split_whitespace().collect::<Vec<_>>(),
        vec![
            "NAME",
            "KIND",
            "SESSION",
            "GEN",
            "START",
            "OUTCOME",
            "OBSERVED",
            "WAITING",
            "MARKS",
            "NEXT_RUN",
            "GRANTS",
            "ACCEPT_FROM"
        ]
    );
    let scheduled = lines
        .find(|line| line.starts_with("scheduled "))
        .expect("scheduled Job appears in human actor list");
    let fields: Vec<_> = scheduled.split_whitespace().collect();
    assert_eq!(fields.len(), 12);
    assert_eq!(
        &fields[..9],
        &["scheduled", "job", "-", "-", "-", "-", "-", "0", "-"]
    );
    assert_ne!(fields[9], "-");
    assert_eq!(fields[10], "-");
    assert_eq!(fields[11], "*");
    let got = fixture.json(&["actor", "get", "scheduled", "--json"]);
    assert_eq!(got["actor"]["name"], "scheduled");

    fixture.json(&["daemon", "restart", "--json"]);
    let restored = fixture.json(&["actor", "get", "scheduled", "--json"]);
    assert_eq!(restored["actor"]["schedule"]["cron"], "0 0 2 1 *");
    let schedule_removed = fixture.json(&[
        "actor",
        "create",
        "scheduled",
        "--kind",
        "job",
        "--json",
        "--",
        "echo",
        "without-schedule",
    ]);
    assert_eq!(schedule_removed["actor"]["schedule"], Value::Null);
    let workspaces = fixture.json(&["workspace", "list", "--json"]);
    let counts: Vec<_> = workspaces["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|workspace| {
            (
                workspace["key"].as_str().unwrap(),
                workspace["actorCount"].as_u64().unwrap(),
            )
        })
        .collect();
    assert!(counts.contains(&("env-space", 1)));
    assert!(counts.contains(&("flag-space", 1)));
    assert!(counts.contains(&("default", 4)));
}

#[test]
fn pause_and_resume_name_the_schedule_a_job_does_not_have() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.json(&[
        "actor", "create", "hello", "--kind", "job", "--json", "--", "echo", "hello",
    ]);

    for command in ["pause", "resume"] {
        let output = fixture.feld(&["actor", command, "hello"]);
        assert_eq!(output.status.code(), Some(1));
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(
            message.contains(
                "Actor 'hello' has no Schedule; add one with `feld actor update hello --cron ...`"
            ),
            "feld actor {command} hello said: {message}"
        );
    }
}

#[test]
fn actor_list_shows_current_sessions_and_enabled_schedules_unless_all() {
    let fixture = Fixture::new();
    fixture.start();
    let job = |name: &str, options: &[&str], command: &[&str]| {
        let mut arguments = vec!["actor", "create", name, "--kind", "job"];
        arguments.extend_from_slice(options);
        arguments.extend_from_slice(&["--tmux-socket", fixture.socket(), "--json", "--"]);
        arguments.extend_from_slice(command);
        fixture.json(&arguments);
    };
    job("busy", &["--start"], &["sleep", "30"]);
    job("finished", &["--start"], &["true"]);
    job("idle", &[], &["true"]);
    job("hourly", &["--cron", "0 * * * *"], &["true"]);
    job("paused", &["--cron", "0 * * * *"], &["true"]);
    fixture.json(&["actor", "pause", "paused", "--json"]);

    let names = |arguments: &[&str]| -> (Vec<String>, Value) {
        let listed = fixture.json(arguments);
        let mut names: Vec<String> = listed["actors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|actor| actor["name"].as_str().unwrap().to_owned())
            .collect();
        names.sort();
        (names, listed["hiddenCount"].clone())
    };
    assert_eq!(
        names(&["actor", "list", "--json"]),
        (vec!["busy".to_owned(), "hourly".to_owned()], json!(3))
    );
    assert_eq!(
        names(&["actor", "list", "--kind", "agent", "--json"]),
        (vec![], json!(0))
    );
    assert_eq!(
        names(&["actor", "list", "-a", "--json"]),
        (
            ["busy", "finished", "hourly", "idle", "paused"]
                .map(str::to_owned)
                .to_vec(),
            json!(0)
        )
    );

    let human = fixture.feld(&["actor", "list"]);
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    assert_eq!(
        human.lines().last(),
        Some("3 stopped Actors not shown (use -a)")
    );
    let all = fixture.feld(&["actor", "list", "--all"]);
    let all = String::from_utf8(all.stdout).unwrap();
    assert_eq!(all.lines().count(), 2 + 5);
    assert!(!all.contains("not shown"));
}

#[test]
fn deleted_actor_keeps_its_id_and_frees_its_name() {
    let fixture = Fixture::new();
    fixture.start();
    let created = fixture.json(&[
        "actor",
        "create",
        "gone",
        "--kind",
        "job",
        "--cron",
        "0 0 1 1 *",
        "--json",
        "--",
        "true",
    ]);
    let id = created["actor"]["id"].as_str().unwrap().to_owned();
    assert!(created["actor"]["schedule"]["nextRun"].is_string());

    let deleted = fixture.json(&["actor", "delete", "gone", "--json"]);
    assert_eq!(deleted["actor"]["id"], id);
    assert!(deleted["actor"]["deletedAt"].is_string());

    assert_eq!(
        fixture.feld(&["actor", "get", "gone"]).status.code(),
        Some(1)
    );
    let by_id = fixture.json(&["actor", "get", &id, "--json"]);
    assert_eq!(by_id["actor"]["name"], "gone");
    assert!(by_id["actor"]["deletedAt"].is_string());
    assert_eq!(by_id["actor"]["schedule"]["cron"], "0 0 1 1 *");
    assert_eq!(by_id["actor"]["schedule"]["nextRun"], Value::Null);
    let listed_ids = |arguments: &[&str]| -> Vec<Value> {
        fixture.json(arguments)["actors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|actor| actor["id"].clone())
            .collect()
    };
    assert!(!listed_ids(&["actor", "list", "--json"]).contains(&json!(id)));
    assert!(!listed_ids(&["actor", "list", "--include-deleted", "--json"]).contains(&json!(id)));
    assert!(
        listed_ids(&["actor", "list", "-a", "--include-deleted", "--json"]).contains(&json!(id))
    );

    let recreated = fixture.json(&[
        "actor", "create", "gone", "--kind", "job", "--json", "--", "true",
    ]);
    assert_ne!(recreated["actor"]["id"], id);
    assert_eq!(recreated["actor"]["deletedAt"], Value::Null);
}

/// A hyphenated option no command defines is an argument error (exit 2), also where it reads
/// like `--<provider>-<key>`, and the Actor stays.
#[test]
fn unknown_hyphenated_option_is_refused_and_leaves_the_actor() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.json(&[
        "actor", "create", "kept", "--kind", "job", "--json", "--", "true",
    ]);

    for arguments in [
        &["actor", "delete", "kept", "--dry-run", "true", "--json"][..],
        &["actor", "delete", "kept", "--dry-run=true"],
        &["actor", "delete", "kept", "--dry-run"],
        &["actor", "delete", "kept", "--dry-run", "true", "--help"],
        &[
            "--workspace",
            "default",
            "actor",
            "ls",
            "--not-a-real-option",
        ],
        &["actor", "list", "--not-a-real-option"],
        &["actor", "list", "--not-a-real-option", "x", "--json"],
        &["workspace", "list", "--not-a-real-option", "x"],
    ] {
        let output = fixture.feld(arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("unexpected argument '--"), "{stderr}");
    }

    let kept = fixture.json(&["actor", "get", "kept", "--json"]);
    assert_eq!(kept["actor"]["deletedAt"], Value::Null);
}

/// A Provider option without its value is an argument error (exit 2), also where the next
/// argument is another option, and no Actor is declared. A value that starts with `-` is
/// given inline.
#[test]
fn provider_option_without_a_value_is_refused_and_declares_nothing() {
    let fixture = Fixture::new();
    fixture.start();

    for arguments in [
        &[
            "actor",
            "create",
            "lacking",
            "--kind",
            "job",
            "--tmux-socket",
            "--json",
            "--",
            "true",
        ][..],
        &[
            "--json",
            "actor",
            "create",
            "lacking",
            "--kind",
            "job",
            "--tmux-socket",
            "--no-start",
            "--",
            "true",
        ],
        &[
            "actor",
            "create",
            "lacking",
            "--kind",
            "job",
            "--tmux-socket",
            "-x",
            "--json",
            "--",
            "true",
        ],
        &[
            "actor",
            "create",
            "lacking",
            "--kind",
            "job",
            "--json",
            "--tmux-socket",
        ],
    ] {
        let output = fixture.feld(arguments);
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("'--tmux-socket <VALUE>'"),
            "{error}"
        );
    }
    let output = fixture.feld(&[
        "actor",
        "create",
        "lacking",
        "--kind",
        "job",
        "--tmux-socket",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("Error: ")
    );
    assert_eq!(
        fixture.feld(&["actor", "get", "lacking"]).status.code(),
        Some(1)
    );

    let inline = fixture.json(&[
        "actor",
        "create",
        "inline",
        "--kind",
        "job",
        "--tmux-socket=-x",
        "--json",
        "--",
        "true",
    ]);
    assert_eq!(
        inline["actor"]["launchSpec"]["providerOptions"]["socket"],
        "-x"
    );
}

#[test]
fn actor_with_a_running_session_is_not_deleted() {
    let fixture = Fixture::new();
    fixture.start();
    let started = fixture.json(&[
        "actor",
        "create",
        "busy",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        fixture.socket(),
        "--json",
        "--",
        "sleep",
        "30",
    ]);
    assert_eq!(started["session"]["end"], Value::Null);

    assert_eq!(
        fixture.feld(&["actor", "delete", "busy"]).status.code(),
        Some(1)
    );
    let kept = fixture.json(&["actor", "get", "busy", "--json"]);
    assert_eq!(kept["actor"]["id"], started["actor"]["id"]);
    assert_eq!(kept["actor"]["deletedAt"], Value::Null);
}

/// `glossary.md`: a rename keeps the `id`, the Sessions and the mailbox, and waits for the
/// current Session to end.
#[test]
fn rename_keeps_the_id_sessions_and_mailbox_of_a_stopped_actor() {
    let fixture = Fixture::new();
    fixture.start();
    let started = fixture.json(&[
        "actor",
        "create",
        "worker",
        "--kind",
        "job",
        "--start",
        "--tmux-socket",
        fixture.socket(),
        "--json",
        "--",
        "sleep",
        "30",
    ]);
    let id = started["actor"]["id"].as_str().unwrap().to_owned();
    let session = started["session"]["id"].as_str().unwrap().to_owned();
    fixture.json(&[
        "message",
        "send",
        "worker",
        "kept across the rename",
        "--json",
    ]);

    let running = fixture.feld(&["actor", "update", "worker", "--name", "crew", "--json"]);
    assert_eq!(running.status.code(), Some(1));
    let running: Value = serde_json::from_slice(&running.stderr).unwrap();
    assert!(
        running["error"]["message"]
            .as_str()
            .unwrap()
            .contains("stop it before the rename"),
        "{running}"
    );
    assert_eq!(
        fixture.json(&["actor", "get", "worker", "--json"])["actor"]["id"],
        id
    );

    fixture.json(&["session", "stop", "worker", "--json"]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while fixture.json(&["session", "get", &session, "--json"])["session"]["end"].is_null() {
        assert!(
            std::time::Instant::now() < deadline,
            "the stopped Session closes"
        );
        thread::sleep(std::time::Duration::from_millis(50));
    }
    fixture.json(&[
        "actor", "create", "taken", "--kind", "job", "--json", "--", "true",
    ]);
    let taken = fixture.feld(&["actor", "update", "worker", "--name", "taken", "--json"]);
    assert_eq!(taken.status.code(), Some(1));
    let taken: Value = serde_json::from_slice(&taken.stderr).unwrap();
    assert!(
        taken["error"]["message"]
            .as_str()
            .unwrap()
            .contains("409 Conflict"),
        "{taken}"
    );

    let renamed = fixture.feld(&["actor", "update", "worker", "--name", "crew"]);
    assert!(
        renamed.status.success(),
        "{}",
        String::from_utf8_lossy(&renamed.stderr)
    );
    let renamed = String::from_utf8(renamed.stdout).unwrap();
    assert!(renamed.starts_with("Actor updated.\n"), "{renamed}");
    assert!(renamed.contains("renamed:"), "{renamed}");
    assert!(renamed.contains("worker -> crew"), "{renamed}");

    assert_eq!(
        fixture.feld(&["actor", "get", "worker"]).status.code(),
        Some(1)
    );
    let crew = fixture.json(&["actor", "get", "crew", "--json"]);
    assert_eq!(crew["actor"]["id"], id);
    let by_id = fixture.json(&["actor", "get", &id, "--json"]);
    assert_eq!(by_id["actor"]["name"], "crew");
    assert_eq!(by_id["actor"]["sessions"][0]["id"], session);
    let mailbox = fixture.json(&["message", "list", "--to", "crew", "--json"]);
    assert!(
        mailbox.to_string().contains("kept across the rename"),
        "{mailbox}"
    );

    fixture.json(&["message", "send", "crew", "after the rename", "--json"]);
    let next = fixture.json(&["actor", "start", "crew", "--json"]);
    assert_eq!(next["session"]["generation"], 2);
    assert_eq!(
        fixture
            .feld(&["message", "send", "worker", "lost"])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn create_replaces_a_live_definition_and_update_changes_launch_and_schedule_together() {
    let fixture = Fixture::new();
    fixture.start();
    let first = fixture.json(&[
        "actor",
        "create",
        "nightly",
        "--kind",
        "job",
        "--cron",
        "0 1 * * *",
        "--timezone",
        "UTC",
        "--json",
        "--",
        "echo",
        "one",
    ]);
    fixture.json(&["actor", "pause", "nightly", "--json"]);

    let replaced = fixture.json(&[
        "actor",
        "create",
        "nightly",
        "--kind",
        "job",
        "--cron",
        "0 2 * * *",
        "--timezone",
        "UTC",
        "--json",
        "--",
        "echo",
        "two",
    ]);
    assert_eq!(replaced["actor"]["id"], first["actor"]["id"]);
    assert_eq!(
        replaced["actor"]["launchSpec"]["command"],
        json!(["echo", "two"])
    );
    assert_eq!(replaced["actor"]["schedule"]["cron"], "0 2 * * *");
    assert_eq!(replaced["actor"]["schedule"]["enabled"], false);

    let unscheduled = fixture.json(&[
        "actor", "create", "nightly", "--kind", "job", "--json", "--", "echo", "three",
    ]);
    assert_eq!(unscheduled["actor"]["id"], first["actor"]["id"]);
    assert_eq!(unscheduled["actor"]["schedule"], Value::Null);

    let updated = fixture.json(&[
        "actor",
        "update",
        "nightly",
        "--cron",
        "0 3 * * *",
        "--json",
        "--",
        "echo",
        "four",
    ]);
    assert_eq!(updated["actor"]["id"], first["actor"]["id"]);
    assert_eq!(
        updated["actor"]["launchSpec"]["command"],
        json!(["echo", "four"])
    );
    assert_eq!(updated["actor"]["schedule"]["cron"], "0 3 * * *");
    assert_eq!(updated["actor"]["schedule"]["enabled"], true);
}

#[test]
fn actor_api_validates_and_commits_whole_definitions_atomically() {
    let fixture = Fixture::new();
    fixture.start();
    let (api, token) = fixture.api();
    let client = Client::new();
    let actor_url = format!("{api}/v1/workspaces/default/actors");

    let unauthorized = client
        .get(&actor_url)
        .send()
        .expect("call Actor API without token");
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let created = client
        .post(&actor_url)
        .bearer_auth(&token)
        .json(&json!({
            "name": "atomic-job",
            "kind": "job",
            "launchSpec": launch("/tmp/atomic-one", &["echo", "one"]),
            "schedule": {"cron": "0 * * * *", "timezone": "UTC"}
        }))
        .send()
        .unwrap();
    assert!(created.status().is_success());

    let mismatch = client
        .post(&actor_url)
        .bearer_auth(&token)
        .json(&json!({
            "name": "atomic-job",
            "kind": "agent",
            "launchSpec": {
                "runtime": "claude-code",
                "workdir": "/tmp/changed",
                "provider": "tmux",
                "node": "local",
                "providerOptions": {"socket": "feld", "unit": "session"}
            }
        }))
        .send()
        .unwrap();
    assert_eq!(mismatch.status(), reqwest::StatusCode::CONFLICT);
    let unchanged = client
        .get(format!("{actor_url}/atomic-job"))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<Value>()
        .unwrap();
    assert_eq!(unchanged["actor"]["kind"], "job");
    assert_eq!(unchanged["actor"]["launchSpec"]["command"][1], "one");
    assert_eq!(unchanged["actor"]["schedule"]["cron"], "0 * * * *");

    let invalid_patch = client
        .patch(format!("{actor_url}/atomic-job"))
        .bearer_auth(&token)
        .json(&json!({"launchSpec": {"command": null}}))
        .send()
        .unwrap();
    assert_eq!(invalid_patch.status(), reqwest::StatusCode::BAD_REQUEST);
    // A Job starts a command, so a patch that leaves it a runtime is refused.
    let switched = client
        .patch(format!("{actor_url}/atomic-job"))
        .bearer_auth(&token)
        .json(&json!({"launchSpec": {"runtime": "claude-code", "command": null}}))
        .send()
        .unwrap();
    assert_eq!(switched.status(), reqwest::StatusCode::BAD_REQUEST);
    let unswitched: Value = client
        .get(format!("{actor_url}/atomic-job"))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(unswitched["actor"]["launchSpec"]["command"][1], "one");
    assert!(unswitched["actor"]["launchSpec"].get("runtime").is_none());

    // An Agent starts a runtime and a Job a command; the other pairs are refused before
    // anything is written.
    for (kind, unstartable) in [
        ("agent", launch("/tmp/agent-command", &["echo", "agent"])),
        (
            "job",
            json!({
                "runtime": "claude-code", "workdir": "/tmp/job-runtime",
                "provider": "tmux", "node": "local"
            }),
        ),
    ] {
        let response = client
            .post(&actor_url)
            .bearer_auth(&token)
            .json(&json!({"name": "unstartable", "kind": kind, "launchSpec": unstartable}))
            .send()
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        let missing = client
            .get(format!("{actor_url}/unstartable"))
            .bearer_auth(&token)
            .send()
            .unwrap();
        assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
    }

    for invalid in [
        // The borrowed selector a tmux unit takes is the Provider's rule, and its `resolve`
        // refuses the ones it cannot place.
        json!({
            "runtime": "claude-code", "workdir": "/tmp/a", "provider": "tmux",
            "node": "local", "providerOptions": {"unit": "window"}
        }),
        json!({
            "runtime": "claude-code", "workdir": "/tmp/a", "provider": "tmux",
            "node": "local", "providerOptions": {"pane": "%3"}
        }),
        json!({
            "runtime": "claude-code", "workdir": "/tmp/a", "provider": "docker",
            "node": "local", "providerOptions": {}
        }),
        json!({
            "runtime": "claude-code", "runtimeSettings": {"statusLine": {}},
            "workdir": "/tmp/a", "provider": "tmux", "node": "local"
        }),
        json!({
            "command": ["echo"], "workdir": "relative", "provider": "tmux",
            "node": "local"
        }),
        json!({
            "runtime": "claude-code", "command": ["echo"], "workdir": "/tmp/a",
            "provider": "tmux", "node": "local"
        }),
    ] {
        let kind = if invalid.get("runtime").is_some() {
            "agent"
        } else {
            "job"
        };
        let response = client
            .post(&actor_url)
            .bearer_auth(&token)
            .json(&json!({"name": "invalid", "kind": kind, "launchSpec": invalid}))
            .send()
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    }
    let configured = client
        .post(&actor_url)
        .bearer_auth(&token)
        .json(&json!({
            "name": "configured-agent",
            "kind": "agent",
            "launchSpec": {
                "runtime": "claude-code",
                "runtimeSettings": {"permissions": {"defaultMode": "acceptEdits"}},
                "workdir": "/tmp/configured",
                "provider": "tmux",
                "node": "local",
                "providerOptions": {
                    "socket": "/tmp/custom.sock",
                    "unit": "window",
                    "session": "main"
                }
            }
        }))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .unwrap();
    assert_eq!(
        configured["actor"]["launchSpec"]["runtimeSettings"]["permissions"]["defaultMode"],
        "acceptEdits"
    );
    let options = &configured["actor"]["launchSpec"]["providerOptions"];
    assert_eq!(options["socket"], "/tmp/custom.sock");
    assert_eq!(options["unit"], "window");
    assert_eq!(options["session"], "main");
    let updated_unit = fixture.json(&[
        "actor",
        "update",
        "configured-agent",
        "--tmux-unit",
        "session",
        "--json",
    ]);
    // `providerOptions` moves as a whole, so the options the update gave are all that stay
    // and the Provider fills the socket it left out.
    let updated_options = &updated_unit["actor"]["launchSpec"]["providerOptions"];
    assert_eq!(updated_options["unit"], "session");
    assert_eq!(updated_options["socket"], "feld");
    assert!(updated_options.get("session").is_none());
    let agent_schedule = client
        .patch(format!("{actor_url}/configured-agent"))
        .bearer_auth(&token)
        .json(&json!({"schedule": {"cron": "0 * * * *"}}))
        .send()
        .unwrap();
    assert_eq!(agent_schedule.status(), reqwest::StatusCode::BAD_REQUEST);

    client
        .post(&actor_url)
        .bearer_auth(&token)
        .json(&json!({
            "name": "manual-job", "kind": "job",
            "launchSpec": launch("/tmp/manual", &["true"])
        }))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    let created_schedule = client
        .patch(format!("{actor_url}/manual-job"))
        .bearer_auth(&token)
        .json(&json!({"schedule": {"cron": "30 * * * *"}}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .unwrap();
    assert_eq!(created_schedule["actor"]["schedule"]["cron"], "30 * * * *");
    assert!(created_schedule["actor"]["schedule"]["nextRun"].is_string());
    let read_schedule = client
        .get(format!("{actor_url}/manual-job"))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<Value>()
        .unwrap();
    assert_eq!(read_schedule["actor"]["schedule"]["cron"], "30 * * * *");
    assert_eq!(read_schedule["actor"]["schedule"]["enabled"], true);
    for schedule in [
        json!({"cron": "bad cron", "timezone": "UTC"}),
        json!({"cron": "0 0 31 2 *", "timezone": "UTC"}),
        json!({"cron": "0 * * * *", "timezone": "Not/AZone"}),
    ] {
        let response = client
            .post(&actor_url)
            .bearer_auth(&token)
            .json(&json!({
                "name": "invalid-schedule", "kind": "job",
                "launchSpec": launch("/tmp/schedule", &["true"]), "schedule": schedule
            }))
            .send()
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    }
    let impossible_patch = client
        .patch(format!("{actor_url}/manual-job"))
        .bearer_auth(&token)
        .json(&json!({"schedule": {"cron": "0 0 31 2 *"}}))
        .send()
        .unwrap();
    assert_eq!(impossible_patch.status(), reqwest::StatusCode::BAD_REQUEST);
    let unchanged_schedule: Value = client
        .get(format!("{actor_url}/manual-job"))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        unchanged_schedule["actor"]["schedule"]["cron"],
        "30 * * * *"
    );
}

#[test]
fn concurrent_create_responses_match_their_committed_actor() {
    let fixture = Fixture::new();
    fixture.start();
    let (api, token) = fixture.api();
    let actor_url = format!("{api}/v1/workspaces/default/actors");
    let client = Client::new();
    let writer_count = 16;
    let barrier = Arc::new(Barrier::new(writer_count));
    let handles: Vec<_> = (0..writer_count)
        .map(|number| {
            let url = actor_url.clone();
            let token = token.clone();
            let client = client.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for round in 0..8 {
                    let value = number * 8 + round;
                    let argument = value.to_string();
                    let workdir = format!("/tmp/create-{value}");
                    let cron = format!("{} * * * *", value % 60);
                    let response = client
                        .post(&url)
                        .bearer_auth(&token)
                        .json(&json!({
                            "name": "create-race", "kind": "job",
                            "launchSpec": launch(&workdir, &["echo", &argument]),
                            "schedule": {"cron": cron, "timezone": "UTC"}
                        }))
                        .send()
                        .unwrap()
                        .error_for_status()
                        .unwrap()
                        .json::<Value>()
                        .unwrap();
                    assert_eq!(response["actor"]["launchSpec"]["workdir"], workdir);
                    assert_eq!(response["actor"]["launchSpec"]["command"][1], argument);
                    assert_eq!(response["actor"]["schedule"]["cron"], cron);
                    assert!(response["actor"]["schedule"]["nextRun"].is_string());
                }
            })
        })
        .collect();
    handles
        .into_iter()
        .for_each(|handle| handle.join().unwrap());
}

#[test]
fn concurrent_patch_responses_match_their_committed_actor() {
    let fixture = Fixture::new();
    fixture.start();
    let (api, token) = fixture.api();
    let actor_url = format!("{api}/v1/workspaces/default/actors");
    let client = Client::new();
    client
        .post(&actor_url)
        .bearer_auth(&token)
        .json(&json!({
            "name": "patch-race", "kind": "job",
            "launchSpec": launch("/tmp/patch-initial", &["echo", "initial"]),
            "schedule": {"cron": "0 * * * *", "timezone": "UTC"}
        }))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();

    let writer_count = 16;
    let barrier = Arc::new(Barrier::new(writer_count));
    let handles: Vec<_> = (0..writer_count)
        .map(|number| {
            let url = format!("{actor_url}/patch-race");
            let token = token.clone();
            let client = client.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for round in 0..8 {
                    let value = number * 8 + round;
                    let argument = value.to_string();
                    let workdir = format!("/tmp/patch-{value}");
                    let response = client
                        .patch(&url)
                        .bearer_auth(&token)
                        .json(&json!({"launchSpec": {
                            "workdir": workdir,
                            "command": ["echo", argument]
                        }}))
                        .send()
                        .unwrap()
                        .error_for_status()
                        .unwrap()
                        .json::<Value>()
                        .unwrap();
                    assert_eq!(response["actor"]["launchSpec"]["workdir"], workdir);
                    assert_eq!(response["actor"]["launchSpec"]["command"][1], argument);
                    assert_eq!(response["actor"]["schedule"]["cron"], "0 * * * *");
                    assert!(response["actor"]["schedule"]["nextRun"].is_string());
                }
            })
        })
        .collect();
    handles
        .into_iter()
        .for_each(|handle| handle.join().unwrap());
}

/// Without `--provider`, the prefix of the Provider's own options names the Provider, and the
/// options the command line leaves out are filled from where the caller runs.
#[test]
fn provider_options_follow_the_option_prefix_and_the_callers_own_position() {
    let fixture = Fixture::new();
    fixture.start();

    let arguments = [
        "actor",
        "create",
        "from-caller",
        "--kind",
        "agent",
        "--no-start",
        "--runtime",
        "claude-code",
        "--json",
    ];
    let from_caller = json_output(&arguments, fixture.feld_in_tmux(&arguments));
    let options = &from_caller["actor"]["launchSpec"]["providerOptions"];
    assert_eq!(from_caller["actor"]["launchSpec"]["provider"], "tmux");
    assert_eq!(options["socket"], fixture.socket());
    assert_eq!(options["unit"], "session");

    let borrowed = fixture.json(&[
        "actor",
        "create",
        "borrowed",
        "--kind",
        "agent",
        "--no-start",
        "--runtime",
        "claude-code",
        "--tmux-unit",
        "window",
        "--tmux-session",
        "main",
        "--json",
    ]);
    let options = &borrowed["actor"]["launchSpec"]["providerOptions"];
    assert_eq!(borrowed["actor"]["launchSpec"]["provider"], "tmux");
    assert_eq!(options["socket"], "feld");
    assert_eq!(options["unit"], "window");
    assert_eq!(options["session"], "main");

    // An option no installed Provider takes, and one tmux does not define, are both refused.
    for rejected in [
        vec![
            "actor",
            "create",
            "elsewhere",
            "--kind",
            "agent",
            "--no-start",
            "--runtime",
            "claude-code",
            "--herdr-pane",
            "w1:p2",
        ],
        vec![
            "actor",
            "create",
            "unknown-option",
            "--kind",
            "agent",
            "--no-start",
            "--runtime",
            "claude-code",
            "--tmux-sockett",
            "feld",
        ],
    ] {
        let output = fixture.feld(&rejected);
        assert!(
            !output.status.success(),
            "feld {} was refused",
            rejected.join(" ")
        );
    }
}

/// An update that switches the Provider replaces the options with the new Provider's own:
/// `--provider` alone takes its defaults, and an option without `--provider` names the Provider
/// by its prefix.
#[test]
fn update_switching_the_provider_resolves_the_new_providers_options() {
    let fixture = Fixture::installing(support::install_plugins_with_herdr);
    fixture.start();
    fixture.json(&[
        "actor",
        "create",
        "switched",
        "--kind",
        "agent",
        "--no-start",
        "--runtime",
        "claude-code",
        "--tmux-unit",
        "window",
        "--tmux-session",
        "main",
        "--json",
    ]);

    let to_herdr = fixture.json(&[
        "actor",
        "update",
        "switched",
        "--provider",
        "herdr",
        "--json",
    ]);
    assert_eq!(to_herdr["actor"]["launchSpec"]["provider"], "herdr");
    assert_eq!(
        to_herdr["actor"]["launchSpec"]["providerOptions"],
        json!({"session": "default", "unit": "workspace"})
    );

    let to_tmux = fixture.json(&[
        "actor",
        "update",
        "switched",
        "--tmux-socket",
        "work",
        "--json",
    ]);
    assert_eq!(to_tmux["actor"]["launchSpec"]["provider"], "tmux");
    assert_eq!(
        to_tmux["actor"]["launchSpec"]["providerOptions"],
        json!({"socket": "work", "unit": "session"})
    );
    assert_eq!(
        fixture.json(&["actor", "get", "switched", "--json"])["actor"]["launchSpec"],
        to_tmux["actor"]["launchSpec"]
    );
}
