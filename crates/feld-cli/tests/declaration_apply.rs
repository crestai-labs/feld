//! `feld diff` and `feld apply` against a running daemon: the changes, a second apply that
//! changes nothing, the fields a file leaves out, the prune scope, and the env file read at
//! launch.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

mod support;

struct Fixture {
    temp: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("create temporary Feld home");
        support::install_plugins(temp.path());
        let fixture = Self { temp };
        fixture.json(&["daemon", "start", "--json"]);
        fixture
    }

    fn home(&self) -> &Path {
        self.temp.path()
    }

    fn command(&self) -> Command {
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(self.home())
            .current_dir(self.home())
            .env("HOME", self.home())
            .env("WORKDIR", self.workdir())
            .env_remove("TMUX");
        command
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        self.command().args(arguments).output().expect("run feld")
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
        serde_json::from_slice(&output.stdout).expect("JSON output")
    }

    fn workdir(&self) -> PathBuf {
        self.home().join("work")
    }

    fn socket(&self) -> PathBuf {
        self.home().join("tmux.sock")
    }

    /// Writes the declaration file with the Actors given as YAML list items.
    fn declare(&self, actors: &str) -> String {
        let path = self.workdir().join("feld.yaml");
        fs::write(
            &path,
            format!(
                "version: 1\nname: harness\ndefaults:\n  workdir: ${{WORKDIR}}\n  envFile: secrets.env\n  providerOptions:\n    socket: {}\nactors:\n{actors}",
                self.socket().display()
            ),
        )
        .expect("write declaration file");
        path.display().to_string()
    }

    fn actor_names(&self) -> Vec<String> {
        self.json(&["actor", "list", "-a", "--json"])["actors"]
            .as_array()
            .expect("actors")
            .iter()
            .map(|actor| actor["name"].as_str().expect("name").to_owned())
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
        let _ = Command::new("tmux")
            .arg("-S")
            .arg(self.socket())
            .arg("kill-server")
            .output();
    }
}

const WRITER: &str = r#"  - name: writer
    kind: job
    command: [sh, -c, 'printf "%s" "$SECRET" > out.txt']
    env:
      GREETING: hi
    schedule:
      cron: "0 9 * * *"
      timezone: Asia/Tokyo
    grants: [read]
"#;

const REVIEWER: &str = r#"  - name: reviewer
    kind: agent
    runtime: claude-code
    providerOptions: {}
    acceptFrom: [writer]
"#;

fn actions(report: &Value) -> Vec<(String, String)> {
    report["changes"]
        .as_array()
        .expect("changes")
        .iter()
        .map(|change| {
            (
                change["action"].as_str().expect("action").to_owned(),
                change["actor"].as_str().expect("actor").to_owned(),
            )
        })
        .collect()
}

fn fields(report: &Value, actor: &str) -> Vec<String> {
    report["changes"]
        .as_array()
        .expect("changes")
        .iter()
        .find(|change| change["actor"] == actor)
        .map(|change| {
            change["fields"]
                .as_array()
                .expect("fields")
                .iter()
                .map(|field| field["field"].as_str().expect("field").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn pair(action: &str, actor: &str) -> (String, String) {
    (action.to_owned(), actor.to_owned())
}

#[test]
fn apply_matches_the_file_once_and_prunes_only_its_own_actors_when_asked() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.workdir()).expect("create workdir");
    fs::write(fixture.workdir().join("secrets.env"), "SECRET=s3cr3t\n").expect("write env file");
    let file = fixture.declare(&format!("{WRITER}{REVIEWER}"));

    let diff = fixture.json(&["diff", "-f", &file, "--json"]);
    assert_eq!(
        actions(&diff),
        [pair("create", "writer"), pair("create", "reviewer")]
    );
    assert!(fixture.actor_names().is_empty(), "a diff changes nothing");
    let human = fixture.feld(&["diff", "-f", &file]);
    let human = String::from_utf8_lossy(&human.stdout);
    assert!(human.contains("+ writer (job)"), "{human}");
    assert!(human.contains("env.GREETING: set"), "{human}");
    assert!(human.contains("providerOptions.socket:"), "{human}");

    let applied = fixture.json(&["apply", "-f", &file, "--json"]);
    assert_eq!(
        actions(&applied),
        [pair("create", "writer"), pair("create", "reviewer")]
    );
    let again = fixture.json(&["diff", "-f", &file, "--json"]);
    assert!(actions(&again).is_empty(), "{again}");
    assert!(again.get("prunable").is_none(), "{again}");

    let writer = fixture.json(&["actor", "get", "writer", "--json"]);
    assert_eq!(writer["actor"]["declaredIn"], "harness");
    assert_eq!(writer["actor"]["grants"], serde_json::json!(["read"]));
    assert_eq!(
        writer["actor"]["launchSpec"]["envFile"],
        fixture.workdir().join("secrets.env").display().to_string()
    );
    assert!(!writer.to_string().contains("s3cr3t"));
    let reviewer = fixture.json(&["actor", "get", "reviewer", "--json"]);
    assert_eq!(reviewer["actor"]["acceptFrom"], serde_json::json!(["writer"]));

    // What the file leaves out stays: a paused Schedule, and the grants given by hand.
    fixture.json(&["actor", "pause", "writer", "--json"]);
    assert!(actions(&fixture.json(&["diff", "-f", &file, "--json"])).is_empty());
    let reviewer_with_grants = REVIEWER.replace("    acceptFrom: [writer]\n", "");
    fixture.json(&[
        "actor", "update", "reviewer", "--grant", "read", "--json",
    ]);
    let file = fixture.declare(&format!("{WRITER}{reviewer_with_grants}"));
    assert!(
        actions(&fixture.json(&["diff", "-f", &file, "--json"])).is_empty(),
        "grants and acceptFrom left out are kept"
    );
    // Written as `null` and `[]`, they are declared.
    let reviewer_cleared = format!("{reviewer_with_grants}    acceptFrom: null\n    grants: []\n");
    let file = fixture.declare(&format!("{WRITER}{reviewer_cleared}"));
    let cleared = fixture.json(&["diff", "-f", &file, "--json"]);
    assert_eq!(actions(&cleared), [pair("replace", "reviewer")]);
    assert_eq!(fields(&cleared, "reviewer"), ["acceptFrom", "grants"]);
    fixture.json(&["apply", "-f", &file, "--json"]);
    let reviewer = fixture.json(&["actor", "get", "reviewer", "--json"]);
    assert_eq!(reviewer["actor"]["acceptFrom"], Value::Null);
    assert_eq!(reviewer["actor"]["grants"], serde_json::json!([]));

    // An Actor the file no longer declares stays, and is listed, until --prune.
    fixture.json(&[
        "actor", "create", "manual", "--kind", "job", "--json", "--", "true",
    ]);
    let file = fixture.declare(WRITER);
    let kept = fixture.json(&["apply", "-f", &file, "--json"]);
    assert!(actions(&kept).is_empty(), "{kept}");
    assert_eq!(
        kept["prunable"],
        serde_json::json!([{ "actor": "reviewer", "kind": "agent" }])
    );
    assert!(fixture.actor_names().contains(&"reviewer".to_owned()));
    let human = fixture.feld(&["diff", "-f", &file]);
    let human = String::from_utf8_lossy(&human.stdout);
    assert!(human.contains("--prune deletes them"), "{human}");
    assert!(human.contains("  reviewer (agent)"), "{human}");

    let pruned_diff = fixture.json(&["diff", "-f", &file, "--prune", "--json"]);
    assert_eq!(actions(&pruned_diff), [pair("delete", "reviewer")]);
    let pruned = fixture.json(&["apply", "-f", &file, "--prune", "--json"]);
    assert_eq!(actions(&pruned), [pair("delete", "reviewer")]);
    let names = fixture.actor_names();
    assert!(names.contains(&"manual".to_owned()), "{names:?}");
    assert!(!names.contains(&"reviewer".to_owned()), "{names:?}");

    if !feld_core::test_require::tmux() {
        return;
    }
    fixture.json(&["actor", "start", "writer", "--json"]);
    let out = fixture.workdir().join("out.txt");
    let deadline = Instant::now() + Duration::from_secs(20);
    while fs::read_to_string(&out).ok().as_deref() != Some("s3cr3t") {
        assert!(Instant::now() < deadline, "the job never read the env file");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn an_unset_variable_and_an_actor_of_another_kind_are_refused() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.workdir()).expect("create workdir");
    let file = fixture
        .declare("  - name: writer\n    kind: job\n    command: [echo, \"${UNSET_IN_TEST}\"]\n");
    let unset = fixture.feld(&["diff", "-f", &file]);
    assert_eq!(unset.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unset.stderr).contains("${UNSET_IN_TEST} is not set"));

    fixture.json(&[
        "actor", "create", "reviewer", "--kind", "job", "--json", "--", "true",
    ]);
    let file = fixture.declare(REVIEWER);
    for command in ["diff", "apply"] {
        let refused = fixture.feld(&[command, "-f", &file]);
        assert_eq!(refused.status.code(), Some(1), "{command}");
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains("kind cannot change"),
            "{command}: {}",
            String::from_utf8_lossy(&refused.stderr)
        );
    }
}
