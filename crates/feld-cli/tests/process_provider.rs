//! A Job on the process Provider, end to end through the daemon: it is the Provider a Job
//! takes when it names none, its output reaches `feld session log` as a tmux Job's does, and
//! it outlives a daemon restart.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

mod support;

struct Fixture {
    temporary: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().expect("create process provider test directory");
        support::install_plugins(temporary.path());
        let fixture = Self { temporary };
        fixture.json(&["daemon", "start", "--json"]);
        fixture
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

    fn latest(&self, actor: &str, outcome: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.json(&["actor", "list", "--refresh", "--json"]);
            let session =
                self.json(&["actor", "get", actor, "--json"])["actor"]["latestSession"].clone();
            if session["outcome"] == outcome {
                return session;
            }
            assert!(
                Instant::now() < deadline,
                "the Session never became {outcome}: {session}"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["session", "kill", "sleeper"]);
        let _ = self.feld(&["daemon", "stop"]);
    }
}

#[test]
fn a_job_runs_on_the_process_provider_and_its_output_reaches_the_log() {
    let fixture = Fixture::new();
    let workdir = fixture.home().join("work");
    fs::create_dir_all(&workdir).unwrap();
    let workdir = workdir.canonicalize().unwrap();
    let created = fixture.json(&[
        "actor",
        "create",
        "printer",
        "--kind",
        "job",
        "--start",
        "--workdir",
        workdir.to_str().unwrap(),
        "--json",
        "--",
        "sh",
        "-c",
        "echo one; echo two >&2; pwd; exit 4",
    ]);
    assert_eq!(created["actor"]["launchSpec"]["provider"], "process");
    let session = fixture.latest("printer", "failed");
    assert_eq!(session["exitCode"], 4, "{session}");
    let id = session["id"].as_str().unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    let lines = loop {
        let page = fixture.json(&["session", "log", id, "--json"]);
        let lines: Vec<(String, String)> = page["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| {
                (
                    line["stream"].as_str().unwrap().to_owned(),
                    line["text"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        if lines.len() == 3 {
            break lines;
        }
        assert!(Instant::now() < deadline, "output did not arrive: {page}");
        thread::sleep(Duration::from_millis(20));
    };
    let mut sorted = lines.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        [
            ("stderr".to_owned(), "two".to_owned()),
            ("stdout".to_owned(), workdir.display().to_string()),
            ("stdout".to_owned(), "one".to_owned()),
        ],
        "{lines:?}"
    );
}

#[test]
fn a_job_on_the_process_provider_outlives_a_daemon_restart_and_stops() {
    let fixture = Fixture::new();
    let started = fixture.json(&[
        "actor", "create", "sleeper", "--kind", "job", "--start", "--json", "--", "sleep", "60",
    ]);
    assert_eq!(started["session"]["outcome"], "running", "{started}");
    let summary = started["session"]["placement"]["placement"]["summary"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(summary.starts_with("process "), "{started}");

    fixture.json(&["daemon", "stop", "--json"]);
    fixture.json(&["daemon", "start", "--json"]);
    let session = fixture.latest("sleeper", "running");
    assert_eq!(session["id"], started["session"]["id"]);

    let stopped = fixture.json(&["session", "stop", "sleeper", "--json"]);
    assert_eq!(stopped["session"]["exitCode"], 143, "{stopped}");
}
