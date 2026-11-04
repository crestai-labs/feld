//! The process Provider driven as core drives it: one operation per run of the executable,
//! the request on stdin and the answer on stdout.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

fn operation(home: &Path, name: &str, request: &Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_feld-plugin-process"))
        .arg(name)
        .env("FELD_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{name} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Places and launches `script` in `workdir`, and answers the launch.
fn launch(home: &Path, workdir: &Path, script: &str, env: Value) -> Value {
    let placed = operation(
        home,
        "place",
        &json!({"options": {}, "node": "local", "workdir": workdir}),
    );
    operation(
        home,
        "launch",
        &json!({
            "placement": placed["placement"],
            "name": "feld-default-job-1",
            "command": ["/bin/sh", "-c", script],
            "env": env,
            "workdir": workdir,
        }),
    )
}

fn observe(home: &Path, placement: &Value) -> Value {
    operation(home, "observe", &json!({"placements": [placement]}))["results"][0]["observation"]
        .clone()
}

fn observe_until_gone(home: &Path, placement: &Value) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let observed = observe(home, placement);
        if observed["kind"] == "gone" {
            return observed;
        }
        assert!(Instant::now() < deadline, "still {observed}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_program_runs_in_its_workdir_with_its_env_and_its_exit_code_is_observed() {
    let home = tempfile::tempdir().unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let workdir = workdir.path().canonicalize().unwrap();
    let launched = launch(
        home.path(),
        &workdir,
        "pwd > seen; echo \"$GREETING\" >> seen; sleep 1; exit 3",
        json!({"GREETING": "hello"}),
    );
    assert_eq!(launched["observation"]["kind"], "running", "{launched}");
    let placement = launched["placement"].clone();
    assert!(
        launched["summary"]
            .as_str()
            .unwrap()
            .starts_with("process ")
    );
    assert_eq!(launched["attachCommand"], Value::Null);
    let observed = observe_until_gone(home.path(), &placement);
    assert_eq!(observed["exitCode"], 3, "{observed}");
    assert_eq!(
        std::fs::read_to_string(workdir.join("seen")).unwrap(),
        format!("{}\nhello\n", workdir.display())
    );
    let state = Path::new(placement["state"].as_str().unwrap()).to_owned();
    assert!(state.starts_with(home.path().join("providers/process")));
    operation(home.path(), "cleanup", &json!({"placement": placement}));
    assert!(!state.exists());
}

#[test]
fn a_program_that_ends_at_once_closes_with_its_exit_code() {
    let home = tempfile::tempdir().unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let launched = launch(home.path(), workdir.path(), "exit 2", json!({}));
    let observed = match launched["observation"]["kind"].as_str() {
        Some("gone") => launched["observation"].clone(),
        _ => observe_until_gone(home.path(), &launched["placement"]),
    };
    assert_eq!(observed["exitCode"], 2, "{observed}");
}

#[test]
fn stop_sends_sigterm_and_kill_sends_sigkill() {
    let home = tempfile::tempdir().unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let stopped = launch(home.path(), workdir.path(), "exec sleep 300", json!({}));
    let stop = operation(
        home.path(),
        "stop",
        &json!({"placement": stopped["placement"], "input": ["/exit", ""]}),
    );
    assert_eq!(stop["attempt"], "process sent SIGTERM to the program");
    assert_eq!(stop["observation"]["kind"], "gone", "{stop}");
    assert_eq!(stop["observation"]["exitCode"], 143);

    let killed = launch(
        home.path(),
        workdir.path(),
        "trap '' TERM; while :; do sleep 1; done",
        json!({}),
    );
    let stop = operation(
        home.path(),
        "stop",
        &json!({"placement": killed["placement"], "input": null}),
    );
    assert_eq!(stop["observation"]["kind"], "running", "{stop}");
    let kill = operation(
        home.path(),
        "kill",
        &json!({"placement": killed["placement"]}),
    );
    assert_eq!(kill["attempt"], "process killed the program (SIGKILL)");
    assert_eq!(kill["observation"]["kind"], "gone", "{kill}");
    assert_eq!(kill["observation"]["exitCode"], 137);
}

#[test]
fn a_workdir_that_is_gone_refuses_the_place_and_fails_the_launch_as_gone() {
    let home = tempfile::tempdir().unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let missing = workdir.path().join("missing");
    let mut child = Command::new(env!("CARGO_BIN_EXE_feld-plugin-process"))
        .arg("place")
        .env("FELD_HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"options": {}, "node": "local", "workdir": missing})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("is not a directory"));

    // A workdir removed between the place and the launch.
    let launched = operation(
        home.path(),
        "launch",
        &json!({
            "placement": {"workdir": missing},
            "name": "feld-default-job-2",
            "command": ["/bin/sh", "-c", "exit 0"],
            "env": {},
            "workdir": missing,
        }),
    );
    assert_eq!(launched["observation"]["kind"], "gone", "{launched}");
    assert_eq!(launched["placement"], Value::Null);
    assert!(
        launched["observation"]["detail"]
            .as_str()
            .unwrap()
            .contains("cannot be entered")
    );
    let states = std::fs::read_dir(home.path().join("providers/process")).unwrap();
    assert_eq!(states.count(), 0);
}

#[test]
fn self_answers_nothing_and_options_are_refused() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(
        operation(home.path(), "self", &json!({}))["selector"],
        Value::Null
    );
    assert_eq!(
        operation(home.path(), "resolve", &json!({"options": {}}))["options"],
        json!({})
    );
}
