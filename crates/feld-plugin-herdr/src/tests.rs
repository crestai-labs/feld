use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::json;

use feld_core::plugin::protocol::ObservationKind;
use feld_core::plugin::sdk::provider::{
    self, CleanupStatus, NOTHING_TO_KILL, ProviderBackend, ResourceOrigin, WAIT_COMMAND,
    observation,
};
use feld_core::process::ProcessIncarnation;

use crate::commands::*;
use crate::placement::*;
use crate::snapshot::*;

/// A Placement of a `workspace` launch that reached a running program, which every
/// observation below starts from.
fn launched() -> HerdrPlacement {
    HerdrPlacement {
        endpoint: HerdrEndpoint::Session("feldtest".to_owned()),
        unit: HerdrUnit::Workspace,
        split: None,
        borrowed: None,
        workdir: PathBuf::from("/tmp"),
        workspace_id: Some("w1".to_owned()),
        workspace_origin: Some(ResourceOrigin::Created),
        tab_id: Some("w1:t1".to_owned()),
        tab_origin: Some(ResourceOrigin::Created),
        pane_id: Some("w1:p1".to_owned()),
        pane_origin: Some(ResourceOrigin::Created),
        terminal_id: Some("term_abc123".to_owned()),
        pane_process: Some(incarnation(4812, "Mon Sep 21 10:00:00 2026")),
        program: Some(incarnation(4813, "Mon Sep 21 10:00:01 2026")),
        process: None,
    }
}

fn incarnation(pid: u32, start_time: &str) -> ProcessIncarnation {
    ProcessIncarnation {
        pid,
        start_time: start_time.to_owned(),
    }
}

/// The pane the launch above created, as `session.snapshot` answers it.
fn pane(terminal_title: Option<&str>) -> PaneInfo {
    PaneInfo {
        pane_id: "w1:p1".to_owned(),
        tab_id: "w1:t1".to_owned(),
        workspace_id: "w1".to_owned(),
        terminal_id: "term_abc123".to_owned(),
        cwd: None,
        foreground_cwd: None,
        terminal_title: terminal_title.map(str::to_owned),
    }
}

fn reading(panes: Vec<PaneInfo>) -> HerdrReading {
    HerdrReading {
        panes,
        tabs: vec![
            TabInfo {
                tab_id: "w1:t1".to_owned(),
                number: 1,
            },
            TabInfo {
                tab_id: "w2:t3".to_owned(),
                number: 3,
            },
        ],
        workspaces: vec![
            WorkspaceInfo {
                workspace_id: "w1".to_owned(),
                label: "feldtest".to_owned(),
            },
            WorkspaceInfo {
                workspace_id: "w2".to_owned(),
                label: "second".to_owned(),
            },
        ],
    }
}

/// One observation judged against a reading of herdr and a process table that are both
/// given, so that every branch of `provider/herdr.md` is decided without a server.
fn observed(
    placement: &HerdrPlacement,
    reading: std::result::Result<HerdrReading, ReadFailure>,
    processes: &[(u32, Option<ProcessIncarnation>)],
) -> ObservationResult {
    let mut snapshot = Snapshot {
        readings: HashMap::from_iter([(
            serde_json::to_string(&placement.endpoint).unwrap(),
            reading,
        )]),
        processes: processes.iter().cloned().collect(),
    };
    HerdrProvider::new("herdr-that-is-not-here").observe_one(placement, &mut snapshot)
}

/// The process table of a pane whose shell and program are both the recorded ones.
fn both_running(placement: &HerdrPlacement) -> Vec<(u32, Option<ProcessIncarnation>)> {
    [placement.pane_process.clone(), placement.program.clone()]
        .into_iter()
        .flatten()
        .map(|process| (process.pid, Some(process)))
        .collect()
}

#[test]
fn the_launch_script_keeps_the_pane_and_writes_the_exit_code_of_the_argv() {
    let env = launch_env(&[]).unwrap();
    let script = launch_script(
        &env,
        &[
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "echo 'it works'".to_owned(),
        ],
    );
    // The argv core passed is quoted a word at a time.
    assert_eq!(
        script,
        format!(
            concat!(
                r#"unset FELD_LAUNCH_SCRIPT; {}; '/bin/sh' '-c' 'echo '\''it works'\'''; s=$?; "#,
                r#"printf '\033]2;feld exit %d\007' "$s"; exec sleep 2147483647"#,
            ),
            env.source()
        )
    );
    // The line that names the script is short enough for a terminal to take, however
    // long the argv core wrapped is.
    assert!(LAUNCH_TEXT.len() < 64, "{LAUNCH_TEXT}");
    assert!(LAUNCH_TEXT.contains(LAUNCH_SCRIPT), "{LAUNCH_TEXT}");
    assert!(
        script.starts_with(&format!("unset {LAUNCH_SCRIPT};")),
        "{script}"
    );
    // And a shell really does run that script as the argv it was built from, with the
    // environment it takes from the Provider, and write the command's exit code as the
    // pane's terminal title.
    let env = launch_env(&[("FELD_VALUE".to_owned(), "a'b $(touch nope)".to_owned())]).unwrap();
    let shell = Command::new("sh")
        .arg("-c")
        .arg(
            launch_script(
                &env,
                &[
                    "sh".to_owned(),
                    "-c".to_owned(),
                    r#"printf %s "$FELD_VALUE"; exit 7"#.to_owned(),
                ],
            )
            .replace(WAIT_COMMAND, "true"),
        )
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("run the launch script");
    env.hand_over(shell.id(), Instant::now() + Duration::from_secs(10))
        .unwrap();
    let written = shell.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&written.stdout),
        "a'b $(touch nope)\u{1b}]2;feld exit 7\u{7}"
    );
    // An environment variable that is not one is refused before anything is created.
    assert!(
        launch_env(&[("not a name".to_owned(), "x".to_owned())])
            .expect_err("an invalid environment variable name")
            .to_string()
            .contains("invalid environment variable name"),
    );
}

/// The launch script's shell runs the receiver of the environment as a child before it
/// forks for the program, and a fork holds the shell's own argv until it `exec`s. Neither
/// is the program, nor is a child that has ended, and the child that `exec`ed the argv is.
#[test]
fn the_program_is_the_child_that_left_the_launch_script_and_is_not_its_receiver() {
    let env = launch_env(&[]).unwrap();
    // `sh <file>` runs the file, so this holds the argv of the receiver for as long as the
    // file named like the receiver's operation keeps it.
    let directory = tempfile::tempdir().unwrap();
    let receiver = env.receiver();
    std::fs::write(directory.path().join(&receiver[1]), "sleep 5; true\n").unwrap();
    let mut receiving = Command::new("/bin/sh")
        .arg0(&receiver[0])
        .args(&receiver[1..])
        .current_dir(directory.path())
        .spawn()
        .expect("start a process with the argv of the receiver");
    let mut fork = Command::new("sh")
        .args(["-c", "sleep 5; true"])
        .spawn()
        .expect("start a process with the argv of the launch script");
    let script = feld_core::process::snapshot(fork.id())
        .unwrap()
        .expect("the launch script is running")
        .argv;
    let program = Sleeper::new();
    let mut ended = Command::new("true")
        .spawn()
        .expect("start a child that ends");
    while !feld_core::process::field(ended.id(), "stat=")
        .unwrap()
        .is_some_and(|stat| stat.starts_with('Z'))
    {
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(!is_program(receiving.id(), &script, &env).unwrap());
    assert!(!is_program(fork.id(), &script, &env).unwrap());
    assert!(!is_program(ended.id(), &script, &env).unwrap());
    assert!(is_program(program.pid, &script, &env).unwrap());

    for child in [&mut receiving, &mut fork, &mut ended] {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[test]
fn a_create_names_the_unit_and_carries_the_workdir_label_and_launch_script() {
    let mut request = LaunchRequest {
        prepared: launched(),
        resource_name: "feld-default-dev-1".to_owned(),
        command: vec!["true".to_owned()],
        env: vec![("FELD_SESSION_TOKEN".to_owned(), "fst_secret".to_owned())],
    };
    let env = launch_env(&request.env).unwrap();
    let args = create_args(&request, &env).unwrap();
    // The environment stays off the command line, which every user of the machine reads.
    assert!(!args.concat().contains("fst_secret"), "{args:?}");
    assert_eq!(
        args,
        [
            "workspace",
            "create",
            "--cwd",
            "/tmp",
            "--label",
            "feld-default-dev-1",
            "--env",
            &format!(
                "FELD_LAUNCH_SCRIPT={}",
                launch_script(&env, &request.command)
            ),
            "--no-focus",
        ]
    );
    // A tab is added to the workspace the Placement borrowed.
    request.prepared.unit = HerdrUnit::Tab;
    request.prepared.borrowed = Some(Borrowed {
        selector: "w1".to_owned(),
        workspace_id: "w1".to_owned(),
        tab_id: None,
        pane_id: None,
        terminal_id: None,
    });
    assert_eq!(
        create_args(&request, &env).unwrap()[..4],
        ["tab", "create", "--workspace", "w1"]
    );
    // A pane splits the pane it borrowed, and takes the label of the tab it lands in.
    request.prepared.unit = HerdrUnit::Pane;
    request.prepared.split = Some(HerdrSplit::Down);
    request.prepared.borrowed = Some(Borrowed {
        selector: "w1:p2".to_owned(),
        workspace_id: "w1".to_owned(),
        tab_id: Some("w1:t2".to_owned()),
        pane_id: Some("w1:p2".to_owned()),
        terminal_id: Some("term_abc124".to_owned()),
    });
    let args = create_args(&request, &env).unwrap();
    assert_eq!(args[..4], ["pane", "split", "w1:p2", "--direction"]);
    assert_eq!(args[4], "down");
    assert!(!args.contains(&"--label".to_owned()), "{args:?}");
}

#[test]
fn a_pane_holding_the_recorded_incarnation_is_running_where_it_was_recorded() {
    let placement = launched();
    let observed = observed(
        &placement,
        Ok(reading(vec![pane(None)])),
        &both_running(&placement),
    );
    assert_eq!(observed.kind, ObservationKind::Running);
    assert_eq!(
        observed.current,
        Some(CurrentNames {
            workspace_id: "w1".to_owned(),
            tab_id: "w1:t1".to_owned(),
            pane_id: "w1:p1".to_owned(),
            workspace: "feldtest".to_owned(),
            tab_number: Some(1),
        })
    );
}

#[test]
fn a_pane_moved_to_another_tab_is_relocated_with_where_it_is_now() {
    let placement = launched();
    // herdr renames a pane it moves into another workspace and keeps its terminal.
    let moved = PaneInfo {
        pane_id: "w2:p4".to_owned(),
        tab_id: "w2:t3".to_owned(),
        workspace_id: "w2".to_owned(),
        ..pane(None)
    };
    let observed = observed(
        &placement,
        Ok(reading(vec![moved])),
        &both_running(&placement),
    );
    assert_eq!(observed.kind, ObservationKind::Relocated);
    assert!(observed.detail.contains("w2:p4"), "{}", observed.detail);
    assert!(observed.detail.contains("w2:t3"), "{}", observed.detail);
    // The Placement stays as it was recorded; only the reading moved.
    assert_eq!(
        observed.current,
        Some(CurrentNames {
            workspace_id: "w2".to_owned(),
            tab_id: "w2:t3".to_owned(),
            pane_id: "w2:p4".to_owned(),
            workspace: "second".to_owned(),
            tab_number: Some(3),
        })
    );
}

#[test]
fn another_terminal_or_another_shell_under_the_same_pane_id_is_mismatched() {
    let placement = launched();
    // A server restart restores the pane under its own ID with a new terminal.
    let restored = PaneInfo {
        terminal_id: "term_def456".to_owned(),
        ..pane(None)
    };
    assert_eq!(
        observed(
            &placement,
            Ok(reading(vec![restored])),
            &both_running(&placement)
        )
        .kind,
        ObservationKind::Mismatched
    );
    // The same terminal, but the shell recorded for it is gone.
    assert_eq!(
        observed(
            &placement,
            Ok(reading(vec![pane(None)])),
            &[(4812, None), (4813, None)]
        )
        .kind,
        ObservationKind::Mismatched
    );
    // The same pid, started later: the pid was handed to another process.
    assert_eq!(
        observed(
            &placement,
            Ok(reading(vec![pane(None)])),
            &[
                (4812, Some(incarnation(4812, "Mon Sep 21 11:00:00 2026"))),
                (4813, placement.program.clone()),
            ]
        )
        .kind,
        ObservationKind::Mismatched
    );
}

#[test]
fn a_program_that_ended_is_gone_with_the_exit_code_from_the_pane_title() {
    let placement = launched();
    let observed = observed(
        &placement,
        Ok(reading(vec![pane(Some("feld exit 3"))])),
        &[(4812, placement.pane_process.clone()), (4813, None)],
    );
    assert_eq!(observed.kind, ObservationKind::Gone);
    assert_eq!(observed.exit_code, Some(3));
    // A pane whose title says nothing about Feld reports no exit code.
    let quiet = observed_gone_without_a_title(&placement);
    assert_eq!(quiet.kind, ObservationKind::Gone);
    assert_eq!(quiet.exit_code, None);
}

fn observed_gone_without_a_title(placement: &HerdrPlacement) -> ObservationResult {
    observed(
        placement,
        Ok(reading(vec![pane(Some("~/work"))])),
        &[(4812, placement.pane_process.clone()), (4813, None)],
    )
}

#[test]
fn a_pane_that_herdr_no_longer_has_is_lost_while_its_program_runs_and_gone_after() {
    let placement = launched();
    // herdr answers `pane_not_found`, which a reading shows as a pane that is not there.
    let lost = observed(
        &placement,
        Ok(reading(Vec::new())),
        &both_running(&placement),
    );
    assert_eq!(lost.kind, ObservationKind::Lost);
    assert_eq!(lost.exit_code, None);
    let gone = observed(
        &placement,
        Ok(reading(Vec::new())),
        &[(4812, None), (4813, None)],
    );
    assert_eq!(gone.kind, ObservationKind::Gone);
    // The pane is closed, so there is no title to read an exit code from.
    assert_eq!(gone.exit_code, None);
}

#[test]
fn a_server_that_is_not_running_is_judged_by_the_program_alone() {
    let placement = launched();
    let down = || {
        Err(ReadFailure::ServerDown(
            "herdr command failed (api snapshot): server_not_running: ".to_owned(),
        ))
    };
    assert_eq!(
        observed(&placement, down(), &both_running(&placement)).kind,
        ObservationKind::Lost
    );
    let gone = observed(&placement, down(), &[(4813, None)]);
    assert_eq!(gone.kind, ObservationKind::Gone);
    assert_eq!(gone.exit_code, None);
}

#[test]
fn a_reading_that_failed_for_any_other_reason_is_unknown() {
    let placement = launched();
    let unknown = observed(
        &placement,
        Err(ReadFailure::Unreadable("failed to run herdr".to_owned())),
        &both_running(&placement),
    );
    assert_eq!(unknown.kind, ObservationKind::Unknown);
    assert!(unknown.detail.contains("failed to run herdr"));
}

#[test]
fn a_launch_that_was_interrupted_before_it_recorded_the_pane_process_is_lost() {
    let placement = HerdrPlacement {
        pane_process: None,
        program: None,
        ..launched()
    };
    let observation = observed(&placement, Ok(reading(vec![pane(None)])), &[]);
    assert_eq!(observation.kind, ObservationKind::Lost);
    assert!(observation.detail.contains("interrupted"));
}

#[test]
fn a_launch_that_was_interrupted_before_it_held_a_pane_is_lost() {
    let placement = HerdrPlacement {
        pane_id: None,
        terminal_id: None,
        pane_process: None,
        program: None,
        ..launched()
    };
    assert_eq!(
        observed(&placement, Ok(reading(Vec::new())), &[]).kind,
        ObservationKind::Lost
    );
}

/// A Placement whose pane is there and whose program was never recorded is one a
/// launch left half-finished, and `lost` is the kind core recovers a Session from. An
/// `unknown` would leave the Actor holding a Session no observation can ever judge.
#[test]
fn a_pane_whose_program_was_never_recorded_is_lost() {
    let placement = HerdrPlacement {
        program: None,
        ..launched()
    };
    let observation = observed(
        &placement,
        Ok(reading(vec![pane(None)])),
        &both_running(&placement),
    );
    assert_eq!(observation.kind, ObservationKind::Lost);
    assert!(observation.detail.contains("interrupted"));
}

#[test]
fn an_attached_session_is_judged_by_its_runtime_process_first() {
    let placement = HerdrPlacement {
        program: None,
        process: Some(incarnation(4900, "Mon Sep 21 10:00:02 2026")),
        ..launched()
    };
    // The Runtime process is gone, so the pane and its shell are not even read.
    assert_eq!(
        observed(&placement, Ok(reading(vec![pane(None)])), &[(4900, None)]).kind,
        ObservationKind::Gone
    );
    // While it runs, it is the program the rest of the judgement follows.
    let running = observed(
        &placement,
        Ok(reading(vec![pane(None)])),
        &[
            (4812, placement.pane_process.clone()),
            (4900, placement.process.clone()),
        ],
    );
    assert_eq!(running.kind, ObservationKind::Running);
}

#[test]
fn a_whole_sequence_of_placements_is_answered_in_the_order_it_was_given() {
    // Two Placements on the same endpoint share one reading, and the answers stay in
    // the order core asked for them.
    let first = launched();
    let second = HerdrPlacement {
        pane_id: Some("w1:p9".to_owned()),
        terminal_id: Some("term_999999".to_owned()),
        ..launched()
    };
    let mut snapshot = Snapshot {
        readings: HashMap::from_iter([(
            serde_json::to_string(&first.endpoint).unwrap(),
            Ok(reading(vec![pane(None)])),
        )]),
        processes: both_running(&first).into_iter().collect(),
    };
    let provider = HerdrProvider::new("herdr-that-is-not-here");
    let answers: Vec<_> = [first, second]
        .iter()
        .map(|placement| provider.observe_one(placement, &mut snapshot).kind)
        .collect();
    assert_eq!(answers, [ObservationKind::Running, ObservationKind::Lost]);
}

#[test]
fn an_exit_code_is_read_only_from_the_title_the_launch_script_writes() {
    assert_eq!(exit_code_of(Some("feld exit 0")), Some(0));
    assert_eq!(exit_code_of(Some("feld exit 137")), Some(137));
    assert_eq!(exit_code_of(Some("  feld exit 2  ")), Some(2));
    assert_eq!(exit_code_of(Some("claude — feld exit 2")), None);
    assert_eq!(exit_code_of(Some("feld exit")), None);
    assert_eq!(exit_code_of(None), None);
}

#[test]
fn a_command_that_answers_nothing_at_all_succeeded() {
    use std::os::unix::process::ExitStatusExt;

    // `pane run` and the `close` commands print nothing when they worked.
    let silent = Output {
        status: std::process::ExitStatus::from_raw(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    assert_eq!(reply("pane run w1:p1 true", &silent).unwrap(), None);
    // The same command that could not be delivered still answers with its code.
    let refused = Output {
        status: std::process::ExitStatus::from_raw(256),
        stdout: Vec::new(),
        stderr: br#"{"error":{"code":"pane_not_found","message":"pane w9:p9 not found"},"id":"cli:request"}"#.to_vec(),
    };
    let refused = reply("pane run w9:p9 true", &refused).expect_err("a missing pane");
    assert!(refused.to_string().contains("pane_not_found"), "{refused}");
}

#[test]
fn self_answers_the_pane_it_runs_in_and_nothing_outside_herdr() {
    let socket = "/tmp/f/herdr.sock";
    assert_eq!(
        selector_of(Some("1"), Some(socket), Some("w1:p2")),
        Some(json!({"session": socket, "pane": "w1:p2"}))
    );
    // Outside a pane there is no environment to read.
    assert_eq!(selector_of(None, None, None), None);
    // Each of the three is needed: a pane process carries all of them.
    assert_eq!(selector_of(Some("1"), Some(socket), None), None);
    assert_eq!(selector_of(Some("1"), None, Some("w1:p2")), None);
    assert_eq!(selector_of(None, Some(socket), Some("w1:p2")), None);
    assert_eq!(selector_of(Some("0"), Some(socket), Some("w1:p2")), None);
}

#[test]
fn resolve_fills_the_defaults_and_takes_the_endpoint_of_the_caller() {
    // Nothing given, and no caller: the default session, a workspace of its own.
    assert_eq!(
        HerdrProvider::resolve_options(None, None).unwrap(),
        json!({"session": "default", "unit": "workspace"})
    );
    // The caller's own position names the server, and an explicit option wins over it.
    let caller = json!({"session": "/tmp/f/herdr.sock", "pane": "w1:p2"});
    assert_eq!(
        HerdrProvider::resolve_options(Some(&json!({})), Some(&caller)).unwrap(),
        json!({"session": "/tmp/f/herdr.sock", "unit": "workspace"})
    );
    assert_eq!(
        HerdrProvider::resolve_options(Some(&json!({"session": "work"})), Some(&caller)).unwrap(),
        json!({"session": "work", "unit": "workspace"})
    );
    // A pane takes the pane it splits, and the direction defaults to right.
    assert_eq!(
        HerdrProvider::resolve_options(Some(&json!({"unit": "pane", "pane": "w1:p2"})), None)
            .unwrap(),
        json!({"session": "default", "unit": "pane", "pane": "w1:p2", "split": "right"})
    );
    assert_eq!(
        HerdrProvider::resolve_options(
            Some(&json!({"unit": "pane", "pane": "w1:p2", "split": "down"})),
            None
        )
        .unwrap(),
        json!({"session": "default", "unit": "pane", "pane": "w1:p2", "split": "down"})
    );
    // A tab takes the workspace it is added to.
    assert_eq!(
        HerdrProvider::resolve_options(Some(&json!({"unit": "tab", "workspace": "w3"})), None)
            .unwrap(),
        json!({"session": "default", "unit": "tab", "workspace": "w3"})
    );
}

#[test]
fn resolve_refuses_what_herdr_cannot_place() {
    for (options, reason) in [
        (
            json!({"socket": "work"}),
            "does not take the option 'socket'",
        ),
        (json!({"unit": "window"}), "must be workspace, tab, or pane"),
        (
            json!({"unit": "workspace", "workspace": "w1"}),
            "workspace takes no borrowed selector",
        ),
        (json!({"unit": "tab"}), "tab takes the borrowed workspace"),
        (
            json!({"unit": "pane", "workspace": "w1"}),
            "pane takes the borrowed pane only",
        ),
        (
            json!({"unit": "pane", "pane": "w1:p2", "split": "up"}),
            "split must be right or down",
        ),
        (
            json!({"unit": "tab", "workspace": "w1", "split": "down"}),
            "split only for the unit pane",
        ),
        (json!({"session": 7}), "must be a non-empty string"),
    ] {
        let refusal = HerdrProvider::resolve_options(Some(&options), None).expect_err("refused");
        assert!(refusal.contains(reason), "{options}: {refusal}");
    }
}

#[test]
fn a_summary_reads_the_unit_with_the_label_herdr_answers_now() {
    let placement = json!({
        "unit": "workspace",
        "workspaceId": "w3",
        "workspaceOrigin": "created",
    });
    let current = CurrentNames {
        workspace_id: "w3".to_owned(),
        tab_id: "w3:t2".to_owned(),
        pane_id: "w3:p4".to_owned(),
        workspace: "feldtest".to_owned(),
        tab_number: Some(2),
    };
    assert_eq!(
        HerdrProvider::summary(&placement, Some(&current)),
        "workspace w3 (feldtest, created)"
    );
    // Without a reading, the record alone.
    assert_eq!(
        HerdrProvider::summary(&placement, None),
        "workspace w3 (created)"
    );
    // A tab and a pane are named by the tab they are in.
    assert_eq!(
        HerdrProvider::summary(
            &json!({"unit": "tab", "tabId": "w3:t2", "tabOrigin": "created"}),
            Some(&current)
        ),
        "tab w3:t2 (feldtest:2, created)"
    );
    assert_eq!(
        HerdrProvider::summary(
            &json!({"unit": "pane", "paneId": "w3:p4", "paneOrigin": "existing"}),
            Some(&current)
        ),
        "pane w3:p4 (feldtest:2, existing)"
    );
    // A pane moved out of the recorded resource is named by the record alone, not by
    // the label of the resource it sits in now.
    let moved = CurrentNames {
        workspace_id: "w2".to_owned(),
        tab_id: "w2:t3".to_owned(),
        pane_id: "w2:p5".to_owned(),
        workspace: "other".to_owned(),
        tab_number: Some(3),
    };
    assert_eq!(
        HerdrProvider::summary(&placement, Some(&moved)),
        "workspace w3 (created)"
    );
    // Before anything is created, the unit alone.
    assert_eq!(
        HerdrProvider::summary(&json!({"unit": "workspace"}), None),
        "workspace pending"
    );
}

#[test]
fn an_attach_command_opens_the_pane_terminal_of_the_endpoint() {
    let placement = serde_json::to_value(HerdrPlacement {
        endpoint: HerdrEndpoint::Session("feldtest".to_owned()),
        unit: HerdrUnit::Workspace,
        split: None,
        borrowed: None,
        workdir: PathBuf::from("/tmp"),
        workspace_id: Some("w3".to_owned()),
        workspace_origin: Some(ResourceOrigin::Created),
        tab_id: Some("w3:t1".to_owned()),
        tab_origin: Some(ResourceOrigin::Created),
        pane_id: Some("w3:p1".to_owned()),
        pane_origin: Some(ResourceOrigin::Created),
        terminal_id: Some("term_abc123".to_owned()),
        pane_process: None,
        program: None,
        process: None,
    })
    .unwrap();
    assert_eq!(
        HerdrProvider::attach_command(&placement, None).as_deref(),
        Some("herdr --session feldtest terminal attach term_abc123")
    );
    // A socket path names the server through the environment instead.
    let mut on_a_path = placement.clone();
    on_a_path["endpoint"] = json!({"kind": "path", "value": "/tmp/f/herdr.sock"});
    assert_eq!(
        HerdrProvider::attach_command(&on_a_path, None).as_deref(),
        Some("HERDR_SOCKET_PATH=/tmp/f/herdr.sock herdr terminal attach term_abc123")
    );
    // A socket path a shell would split is quoted, so the line reads back as the same argv.
    let mut spaced = placement.clone();
    spaced["endpoint"] = json!({"kind": "path", "value": "/tmp/feld test/herdr.sock"});
    let command = HerdrProvider::attach_command(&spaced, None).unwrap();
    assert_eq!(
        command,
        "HERDR_SOCKET_PATH='/tmp/feld test/herdr.sock' herdr terminal attach term_abc123"
    );
    // A shell reads the line back as the socket path and the argv herdr needs.
    let argv = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "herdr() {{ printf '%s\\n' \"$HERDR_SOCKET_PATH\" \"$@\"; }}; {command}"
        ))
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(argv.stdout).unwrap(),
        "/tmp/feld test/herdr.sock\nterminal\nattach\nterm_abc123\n"
    );
    // A Placement that holds no pane yet has no command.
    let mut placed = placement.clone();
    placed.as_object_mut().unwrap().remove("terminalId");
    assert_eq!(HerdrProvider::attach_command(&placed, None), None);
}

#[test]
fn an_endpoint_tells_a_socket_path_from_a_session_name() {
    assert_eq!(
        endpoint("feldtest"),
        HerdrEndpoint::Session("feldtest".to_owned())
    );
    assert_eq!(
        endpoint("/tmp/f/herdr.sock"),
        HerdrEndpoint::Path(PathBuf::from("/tmp/f/herdr.sock"))
    );
}

#[test]
fn a_prepared_workspace_launch_holds_the_axes_and_no_incarnation() {
    let request = HerdrProvider::prepare_request(None, Path::new("/tmp/work")).unwrap();
    assert_eq!(request.unit, HerdrUnit::Workspace);
    assert_eq!(request.borrowed, None);
    assert_eq!(request.split, None);
    // A workspace of its own borrows nothing, so herdr is not asked anything.
    let placement = HerdrProvider::new("herdr-that-is-not-here")
        .prepare(request)
        .unwrap();
    assert_eq!(placement.endpoint, HerdrEndpoint::Session("default".into()));
    assert_eq!(placement.workspace_id, None);
    assert_eq!(placement.borrowed, None);
    assert_eq!(placement.pane_process, None);
    assert_eq!(
        HerdrProvider::summary(&serde_json::to_value(&placement).unwrap(), None),
        "workspace pending"
    );
}

#[test]
fn a_borrowed_resource_that_does_not_resolve_refuses_the_launch() {
    // A socket no server listens on: the borrowed workspace cannot be resolved, and a
    // launch that cannot be placed is refused rather than answered.
    let nowhere = std::env::temp_dir().join("feld-herdr-absent.sock");
    let options = json!({
        "session": nowhere.display().to_string(),
        "unit": "tab",
        "workspace": "w3",
    });
    let request = HerdrProvider::prepare_request(Some(&options), Path::new("/tmp/work")).unwrap();
    assert_eq!(request.unit, HerdrUnit::Tab);
    assert_eq!(request.borrowed.as_deref(), Some("w3"));
    let refused = HerdrProvider::default()
        .prepare(request)
        .expect_err("a borrowed workspace that does not resolve");
    // Either herdr said so, or herdr is not installed here at all.
    assert!(
        matches!(
            refused,
            HerdrProviderError::Herdr { .. }
                | HerdrProviderError::Io(_)
                | HerdrProviderError::Protocol(_)
        ),
        "{refused}"
    );
}

#[test]
fn a_relative_workdir_is_refused() {
    let refused = HerdrProvider::default()
        .prepare(PrepareRequest {
            endpoint: HerdrEndpoint::Session("feldtest".to_owned()),
            unit: HerdrUnit::Workspace,
            split: None,
            borrowed: None,
            workdir: PathBuf::from("work"),
        })
        .expect_err("a relative workdir");
    assert!(
        refused.to_string().contains("must be absolute"),
        "{refused}"
    );
}

#[test]
fn an_answer_reads_the_result_and_turns_an_error_code_into_a_refusal() {
    use std::os::unix::process::ExitStatusExt;

    let ok = Output {
        status: std::process::ExitStatus::from_raw(0),
        stdout: br#"{"id":"cli:pane:get","result":{"pane":{"pane_id":"w1:p1"}}}"#.to_vec(),
        stderr: Vec::new(),
    };
    assert_eq!(
        text(
            &answer("pane get w1:p1", &ok).unwrap(),
            &["pane", "pane_id"]
        )
        .unwrap(),
        "w1:p1"
    );
    // A command that failed prints its reply on stderr.
    let missing = Output {
        status: std::process::ExitStatus::from_raw(256),
        stdout: Vec::new(),
        stderr: br#"{"error":{"code":"pane_not_found","message":"pane w9:p9 not found"},"id":"cli:pane:get"}"#.to_vec(),
    };
    let refused = answer("pane get w9:p9", &missing).expect_err("a missing pane");
    assert!(refused.to_string().contains("pane_not_found"), "{refused}");
    // A server that is not running answers the same way, with its own code.
    let absent = Output {
        status: std::process::ExitStatus::from_raw(256),
        stdout: Vec::new(),
        stderr: br#"{"id":"cli:workspace:list","error":{"code":"server_not_running","message":"no herdr server is running"}}"#.to_vec(),
    };
    let refused = answer("workspace list", &absent).expect_err("an absent server");
    assert!(
        refused.to_string().contains("server_not_running"),
        "{refused}"
    );
    // Anything that is not a reply at all is a protocol failure.
    let silent = Output {
        status: std::process::ExitStatus::from_raw(256),
        stdout: Vec::new(),
        stderr: b"herdr: command not understood\n".to_vec(),
    };
    let refused = answer("pane get w1:p1", &silent).expect_err("no reply");
    assert!(
        refused.to_string().contains("answered no JSON"),
        "{refused}"
    );
}

/// The selector an `attach` takes: the pane is the one thing it needs, and the server
/// falls back the way a launch's does.
#[test]
fn an_attach_selector_needs_a_pane_and_names_a_server() {
    let attach = HerdrProvider::attach_request(&json!({ "pane": "w1:p2", "session": "work" }))
        .expect("a pane on a named server");
    assert_eq!(attach.pane, "w1:p2");
    assert_eq!(attach.endpoint, HerdrEndpoint::Session("work".to_owned()));
    // A session that looks like a path is the socket of a server, which is what a pane
    // answers about itself.
    assert_eq!(
        HerdrProvider::attach_request(&json!({ "pane": "w1:p2", "session": "/tmp/h.sock" }))
            .expect("a pane on a socket")
            .endpoint,
        HerdrEndpoint::Path(PathBuf::from("/tmp/h.sock"))
    );
    // Outside herdr, with no server named, the attach takes the default session.
    assert_eq!(
        HerdrProvider::attach_request(&json!({ "pane": "w1:p2" }))
            .expect("a pane alone")
            .endpoint,
        HerdrEndpoint::Session(DEFAULT_SESSION.to_owned())
    );
    assert!(
        HerdrProvider::attach_request(&json!({ "session": "work" }))
            .expect_err("no pane")
            .contains("--herdr-pane")
    );
    assert!(
        HerdrProvider::attach_request(&json!({ "pane": "w1:p2", "unit": "tab" }))
            .expect_err("a selector attach does not take")
            .contains("'unit'")
    );
}

/// A process of this machine standing in for a pane's program, started outside this
/// test's own children so that the process table stops answering for it the moment it
/// is signalled.
struct Sleeper {
    pid: u32,
}

impl Sleeper {
    fn new() -> Self {
        let started = Command::new("sh")
            .arg("-c")
            // The program keeps none of this command's pipes, so that reading the
            // pid it answers does not wait for the program itself to end.
            .arg("sleep 30 >/dev/null 2>&1 & printf %s \"$!\"")
            .output()
            .expect("start a program to stop");
        Self {
            pid: String::from_utf8_lossy(&started.stdout)
                .trim()
                .parse()
                .expect("the shell answers the pid it started"),
        }
    }

    fn incarnation(&self) -> ProcessIncarnation {
        self.read().expect("the program is running")
    }

    fn is_running(&self) -> bool {
        self.read().is_some()
    }

    fn read(&self) -> Option<ProcessIncarnation> {
        feld_core::process::incarnation(self.pid).expect("read the program")
    }
}

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-9", &self.pid.to_string()])
            .output();
    }
}

/// Writes a fake herdr and runs it once. macOS inspects an executable the first time it
/// runs, which takes seconds while the machine builds and runs others; running it here
/// keeps that out of the deadline the Provider gives each command.
fn write_fake(binary: &Path, script: &str) {
    let script = script.replacen(
        "#!/bin/sh\n",
        "#!/bin/sh\n[ -z \"$FELD_TEST_WARM_UP\" ] || exit 0\n",
        1,
    );
    std::fs::write(binary, script).expect("write the fake herdr");
    std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o755))
        .expect("make the fake herdr runnable");
    let _ = Command::new(binary).env("FELD_TEST_WARM_UP", "1").status();
}

/// A herdr that answers one prepared reading and writes down every command it is
/// given, so that what the Provider sends to a pane can be read back without a
/// server. The command that carries `ends_on` ends the program, which is what a
/// Runtime that exits on the input it was sent does.
struct FakeHerdr {
    root: tempfile::TempDir,
}

impl FakeHerdr {
    fn answering(reading: &HerdrReading, ends_on: &str, program: u32) -> Self {
        Self::answering_at(reading, ends_on, 1, program)
    }

    /// A herdr whose program ends on the `nth` call that carries `ends_on`, the way a
    /// Runtime that dropped the first column ends on the column sent again.
    fn answering_at(reading: &HerdrReading, ends_on: &str, nth: usize, program: u32) -> Self {
        let root = tempfile::tempdir().expect("create a fake herdr root");
        let at = root.path().display().to_string();
        std::fs::write(
            root.path().join("snapshot.json"),
            json!({ "id": "cli:1", "result": { "snapshot": reading } }).to_string(),
        )
        .expect("write the fake herdr snapshot");
        let script = format!(
            "#!/bin/sh\nall=\"$*\"\nprintf '%s\\n' \"$all\" >> '{at}/log'\ncase \"$all\" in\n\
             *'api snapshot') cat '{at}/snapshot.json' ;;\n\
             *'{ends_on}'*) if [ \"$(grep -c -- '{ends_on}' '{at}/log')\" -ge {nth} ]; then kill -9 {program}; fi ;;\n\
             esac\n"
        );
        write_fake(&root.path().join("herdr"), &script);
        Self { root }
    }

    /// A herdr that answers the readings one launch makes and takes no line: the pane
    /// it creates keeps the shell whose pid it is given, so the launch script never
    /// runs and the launch has to give up.
    fn taking_no_line(shell: u32) -> Self {
        let root = tempfile::tempdir().expect("create a fake herdr root");
        let at = root.path().display().to_string();
        for (name, answer) in [
            (
                "created.json",
                json!({ "id": "cli:1", "result": { "root_pane": pane(None) } }),
            ),
            (
                "process-info.json",
                json!({ "id": "cli:1", "result": { "process_info": {
                    "shell_pid": shell,
                    "foreground_process_group_id": shell,
                } } }),
            ),
            (
                "pane.json",
                json!({ "id": "cli:1", "result": { "pane": pane(None) } }),
            ),
        ] {
            std::fs::write(root.path().join(name), answer.to_string())
                .expect("write a fake herdr answer");
        }
        let script = format!(
            "#!/bin/sh\nall=\"$*\"\nprintf '%s\\n' \"$all\" >> '{at}/log'\ncase \"$all\" in\n\
             *'workspace create'*) cat '{at}/created.json' ;;\n\
             *'pane process-info'*) cat '{at}/process-info.json' ;;\n\
             *'pane get'*) cat '{at}/pane.json' ;;\n\
             esac\n"
        );
        write_fake(&root.path().join("herdr"), &script);
        Self { root }
    }

    /// A herdr that answers one prepared reading, its socket, and the `process_info` of
    /// each pane it is given one for, which is what `candidates` lists. A pane without one
    /// answers nothing, as a pane closed since the reading does.
    fn listing(reading: &HerdrReading, infos: &[(&str, serde_json::Value)]) -> Self {
        let root = tempfile::tempdir().expect("create a fake herdr root");
        let at = root.path().display().to_string();
        let mut answers = vec![
            (
                "snapshot.json".to_owned(),
                json!({ "id": "cli:1", "result": { "snapshot": reading } }),
            ),
            (
                "status.json".to_owned(),
                json!({ "status": "running", "socket": "/tmp/herdr-test.sock" }),
            ),
        ];
        let mut cases = String::new();
        for (index, (pane, info)) in infos.iter().enumerate() {
            let name = format!("process-info-{index}.json");
            cases.push_str(&format!(
                "*'pane process-info --pane {pane}') cat '{at}/{name}' ;;\n"
            ));
            answers.push((
                name,
                json!({ "id": "cli:1", "result": { "process_info": info } }),
            ));
        }
        for (name, answer) in answers {
            std::fs::write(root.path().join(name), answer.to_string())
                .expect("write a fake herdr answer");
        }
        let script = format!(
            "#!/bin/sh\nall=\"$*\"\nprintf '%s\\n' \"$all\" >> '{at}/log'\ncase \"$all\" in\n\
             *'api snapshot') cat '{at}/snapshot.json' ;;\n\
             *'status server --json') cat '{at}/status.json' ;;\n\
             {cases}esac\n"
        );
        write_fake(&root.path().join("herdr"), &script);
        Self { root }
    }

    /// A herdr that refuses every command with one error code, as a server that is not
    /// running or is shutting down does.
    fn refusing(code: &str, message: &str) -> Self {
        let root = tempfile::tempdir().expect("create a fake herdr root");
        let at = root.path().display().to_string();
        let reply = json!({ "id": "cli:1", "error": { "code": code, "message": message } });
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{at}/log'\nprintf '%s\\n' '{reply}' >&2\nexit 1\n"
        );
        write_fake(&root.path().join("herdr"), &script);
        Self { root }
    }

    fn provider(&self) -> HerdrProvider {
        HerdrProvider::new(self.root.path().join("herdr"))
            .settling(Duration::from_millis(200))
            .launching(Duration::from_millis(200))
    }

    /// Every command the Provider ran, in order.
    fn commands(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.path().join("log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// What the Provider put into the pane, in the order it was sent.
    fn inputs(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.path().join("log"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("pane run") || line.contains("pane send-keys"))
            .map(|line| line.replace("--session feldtest ", ""))
            .collect()
    }
}

/// What `place` answers: the three axes with none of the resources created yet.
fn prepared() -> HerdrPlacement {
    HerdrPlacement {
        workspace_id: None,
        workspace_origin: None,
        tab_id: None,
        tab_origin: None,
        pane_id: None,
        pane_origin: None,
        terminal_id: None,
        pane_process: None,
        program: None,
        ..launched()
    }
}

/// A pane whose shell never takes the line the launch types holds no program, and a
/// Placement without one is beyond every observation and every stop. So the launch
/// fails: it closes the resource it created and answers `gone`, which closes the
/// Session rather than leaving the Actor holding one that cannot end.
#[test]
fn a_launch_whose_pane_never_takes_the_line_closes_what_it_created() {
    // The pane's shell is this test process, which never becomes the launch script.
    let herdr = FakeHerdr::taking_no_line(std::process::id());
    let failure = herdr
        .provider()
        .launch(LaunchRequest {
            prepared: prepared(),
            resource_name: "feldtest".to_owned(),
            command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            env: Vec::new(),
        })
        .expect_err("a launch whose line was never taken");
    assert_eq!(failure.observation.kind, ObservationKind::Gone);
    assert!(failure.source.contains("never ran the launch script"));
    assert_eq!(
        failure
            .partial
            .as_ref()
            .and_then(|partial| partial.workspace_id.as_deref()),
        Some("w1")
    );
    assert!(
        herdr
            .commands()
            .iter()
            .any(|command| command.contains("workspace close w1")),
        "the created workspace is closed: {:?}",
        herdr.commands()
    );
}

#[test]
fn a_failed_launch_names_the_command_without_its_arguments() {
    let herdr = FakeHerdr::refusing("server_not_running", "no herdr server is running");
    let failure = herdr
        .provider()
        .launch(LaunchRequest {
            prepared: prepared(),
            resource_name: "feldtest".to_owned(),
            command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
            env: Vec::new(),
        })
        .expect_err("a launch on a server that is not running");
    assert_eq!(failure.observation.kind, ObservationKind::Gone);
    assert_eq!(
        failure.observation.detail,
        "herdr command failed (workspace create): server_not_running: no herdr server is running"
    );
}

/// A server that is shutting down is on its way to having no server, so the program
/// alone decides the observation as it does for a server that is not running.
#[test]
fn a_server_that_is_shutting_down_is_judged_by_the_program_alone() {
    let program = Sleeper::new();
    let placement = placed_on(&program);
    let herdr = FakeHerdr::refusing("server_unavailable", "server is shutting down");
    let lost = herdr.provider().observe(&placement);
    assert_eq!(lost.kind, ObservationKind::Lost, "{}", lost.detail);
    // A pid that another process has taken since answers a different start time.
    let replaced = HerdrPlacement {
        program: Some(ProcessIncarnation {
            start_time: "Thu Jan  1 00:00:00 1970".to_owned(),
            ..program.incarnation()
        }),
        ..placement
    };
    let gone = herdr.provider().observe(&replaced);
    assert_eq!(gone.kind, ObservationKind::Gone, "{}", gone.detail);
    assert_eq!(gone.exit_code, None);
}

/// The Placement of a Session whose program is one real process. The pane's own
/// process is this test, which is alive and matches itself for as long as the test
/// runs.
fn placed_on(program: &Sleeper) -> HerdrPlacement {
    HerdrPlacement {
        pane_process: feld_core::process::incarnation(std::process::id())
            .expect("read this process"),
        program: Some(program.incarnation()),
        ..launched()
    }
}

/// The pane of [`launched`] with the exit code the launch script writes when its
/// program ends, which is what an observation of a stopped Session reads.
fn stopped_pane() -> PaneInfo {
    pane(Some("feld exit 137"))
}

/// The `stopInput` of a Runtime that asks to confirm its exit, which is the column
/// `claude-code` declares.
fn stop_input() -> Vec<String> {
    vec!["/exit".to_owned(), String::new()]
}

#[test]
fn a_stop_input_reaches_the_pane_one_line_at_a_time() {
    let program = Sleeper::new();
    let placement = placed_on(&program);
    // The program answers the confirmation rather than the first input, so the second
    // input is the one that ends it.
    let herdr = FakeHerdr::answering(&reading(vec![stopped_pane()]), "send-keys", program.pid);
    let result = herdr.provider().graceful_stop(&placement, &stop_input());
    assert_eq!(result.attempt, "herdr sent /exit to the pane");
    assert_eq!(result.observed.kind, ObservationKind::Gone);
    assert_eq!(
        herdr.inputs(),
        ["pane run w1:p1 /exit", "pane send-keys w1:p1 enter"]
    );
}

#[test]
fn a_program_that_dropped_the_first_column_ends_on_the_column_sent_again() {
    let program = Sleeper::new();
    let placement = placed_on(&program);
    let herdr =
        FakeHerdr::answering_at(&reading(vec![stopped_pane()]), "send-keys", 2, program.pid);
    let result = herdr.provider().graceful_stop(&placement, &stop_input());
    assert_eq!(result.observed.kind, ObservationKind::Gone);
    assert_eq!(
        herdr.inputs(),
        [
            "pane run w1:p1 /exit",
            "pane send-keys w1:p1 enter",
            "pane run w1:p1 /exit",
            "pane send-keys w1:p1 enter",
        ]
    );
}

#[test]
fn a_program_that_ends_on_the_first_stop_input_is_sent_no_more() {
    let program = Sleeper::new();
    let placement = placed_on(&program);
    let herdr = FakeHerdr::answering(&reading(vec![stopped_pane()]), "/exit", program.pid);
    let result = herdr.provider().graceful_stop(&placement, &stop_input());
    assert_eq!(result.observed.kind, ObservationKind::Gone);
    assert_eq!(herdr.inputs(), ["pane run w1:p1 /exit"]);
}

#[test]
fn only_the_global_options_of_herdr_are_read_as_its_options() {
    for input in [
        "--session",
        "--session=main",
        "--remote",
        "--remote=host",
        "--remote-keybindings",
        "--remote-keybindings=",
    ] {
        assert!(is_global_option(input), "{input}");
    }
    for input in [
        "-x",
        "--help",
        "--",
        "-",
        "--session x",
        "--sessions",
        "/exit",
        "",
    ] {
        assert!(!is_global_option(input), "{input}");
    }
}

/// herdr consumes such an input before `pane run` reads it, so the stop answers why and
/// runs nothing.
#[test]
fn a_stop_input_herdr_reads_as_its_own_option_is_refused_before_herdr_runs() {
    let program = Sleeper::new();
    let placement = placed_on(&program);
    let herdr = FakeHerdr::answering(&reading(vec![stopped_pane()]), "nothing", program.pid);
    let result = herdr
        .provider()
        .graceful_stop(&placement, &["--session=main".to_owned()]);
    assert_eq!(result.observed.kind, ObservationKind::Unknown);
    assert!(
        result
            .observed
            .detail
            .contains("--session=main as its own option"),
        "{}",
        result.observed.detail
    );
    assert!(herdr.inputs().is_empty(), "{:?}", herdr.inputs());
}

#[test]
fn a_program_that_outlives_the_wait_leaves_the_session_open() {
    let program = Sleeper::new();
    let placement = placed_on(&program);
    let herdr = FakeHerdr::answering(&reading(vec![stopped_pane()]), "nothing", program.pid);
    let result = herdr.provider().graceful_stop(&placement, &stop_input());
    assert_eq!(result.observed.kind, ObservationKind::Running);
    assert_eq!(result.observed.detail, STILL_PRESENT);
    // Every input the Adapter declared was sent in every round, because the program
    // took all of them.
    assert_eq!(herdr.inputs().len(), 2 * provider::STOP_INPUT_ROUNDS);
    assert!(program.is_running());
}

#[test]
fn nothing_is_sent_or_signalled_to_a_pane_that_holds_another_incarnation() {
    let program = Sleeper::new();
    let placement = placed_on(&program);
    // The recorded pane ID is there with a terminal this Placement never recorded,
    // which is what a restored pane looks like.
    let restored = PaneInfo {
        terminal_id: "term_restored".to_owned(),
        ..stopped_pane()
    };
    let herdr = FakeHerdr::answering(&reading(vec![restored]), "/exit", program.pid);
    let provider = herdr.provider();
    for result in [
        provider.graceful_stop(&placement, &stop_input()),
        provider.stop(&placement),
        provider.kill(&placement),
    ] {
        assert_eq!(result.attempt, NOTHING_TO_KILL);
        assert_eq!(result.observed.kind, ObservationKind::Mismatched);
    }
    assert!(herdr.inputs().is_empty());
    assert!(program.is_running(), "the program was never signalled");
}

#[test]
fn stop_without_an_input_signals_the_pane_program() {
    let program = Sleeper::new();
    let placement = placed_on(&program);
    let herdr = FakeHerdr::answering(&reading(vec![stopped_pane()]), "nothing", program.pid);
    let result = herdr.provider().stop(&placement);
    assert_eq!(result.attempt, SIGTERM_ATTEMPT);
    assert_eq!(result.observed.kind, ObservationKind::Gone);
    assert!(herdr.inputs().is_empty(), "a signal is not an input");
    assert!(!program.is_running());
}

#[test]
fn kill_ends_the_pane_program_and_reads_its_exit_code() {
    let program = Sleeper::new();
    let placement = placed_on(&program);
    let herdr = FakeHerdr::answering(&reading(vec![stopped_pane()]), "nothing", program.pid);
    let result = herdr.provider().kill(&placement);
    assert_eq!(result.attempt, SIGKILL_ATTEMPT);
    assert_eq!(result.observed.kind, ObservationKind::Gone);
    assert_eq!(result.observed.exit_code, Some(137));
    assert!(!program.is_running());
}

/// The start time is read on both sides of SIGKILL: a pid that answers with another
/// incarnation afterwards is one the OS handed on, and what became of the program is
/// then unknown.
#[test]
fn a_pid_that_changed_across_the_signal_is_unknown() {
    let program = Sleeper::new();
    let placement = placed_on(&program);
    let elsewhere = ProcessIncarnation {
        pid: program.pid,
        start_time: "Mon Sep 21 09:00:00 2026".to_owned(),
    };
    let result = HerdrProvider::new("herdr-that-is-not-here").after_kill(
        &placement,
        &elsewhere,
        observation(ObservationKind::Running, None, String::new(), None),
    );
    assert_eq!(result.attempt, SIGKILL_ATTEMPT);
    assert_eq!(result.observed.kind, ObservationKind::Unknown);
    assert!(result.observed.detail.contains("PID changed"), "{result:?}");
}

/// The Placement of a `pane` launch: the pane was split out of the borrowed pane of a
/// tab and a workspace that were both there already.
fn split() -> HerdrPlacement {
    HerdrPlacement {
        unit: HerdrUnit::Pane,
        split: Some(HerdrSplit::Right),
        borrowed: Some(Borrowed {
            selector: "w1:p1".to_owned(),
            workspace_id: "w1".to_owned(),
            tab_id: Some("w1:t1".to_owned()),
            pane_id: Some("w1:p1".to_owned()),
            terminal_id: Some("term_borrowed".to_owned()),
        }),
        workspace_origin: Some(ResourceOrigin::Existing),
        tab_origin: Some(ResourceOrigin::Existing),
        ..launched()
    }
}

/// Another pane of the same tab, which a created pane has to leave the tab with.
fn sibling() -> PaneInfo {
    PaneInfo {
        pane_id: "w1:p9".to_owned(),
        terminal_id: "term_sibling".to_owned(),
        ..pane(None)
    }
}

#[test]
fn cleanup_closes_the_resource_of_the_unit_and_nothing_a_launch_borrowed() {
    let workspace = launched();
    assert_eq!(unit_resource(&workspace).0, "workspace");
    assert_eq!(unit_resource(&workspace).1, Some("w1"));
    assert!(matches!(
        cleanup_plan(&workspace, &reading(vec![pane(None)]), "w1", true),
        Cleanup::Close
    ));

    let tab = HerdrPlacement {
        unit: HerdrUnit::Tab,
        workspace_origin: Some(ResourceOrigin::Existing),
        ..launched()
    };
    assert_eq!(unit_resource(&tab).0, "tab");
    assert_eq!(unit_resource(&tab).1, Some("w1:t1"));
    assert_eq!(unit_resource(&tab).2, Some(ResourceOrigin::Created));

    // An attach created nothing at all, so cleanup has nothing of its own to close.
    let attached = HerdrPlacement {
        unit: HerdrUnit::Pane,
        pane_origin: Some(ResourceOrigin::Existing),
        ..split()
    };
    assert_eq!(unit_resource(&attached).2, Some(ResourceOrigin::Existing));
}

#[test]
fn a_created_pane_that_is_the_borrowed_tabs_last_one_stays() {
    let placement = split();
    let alone = cleanup_plan(&placement, &reading(vec![pane(None)]), "w1:p1", true);
    let Cleanup::Answer(result) = alone else {
        panic!("the last pane of a borrowed tab is not closed");
    };
    assert_eq!(result.status, CleanupStatus::Retained);
    assert_eq!(
        result.detail,
        "the created pane is the borrowed tab's last pane"
    );

    assert!(matches!(
        cleanup_plan(
            &placement,
            &reading(vec![pane(None), sibling()]),
            "w1:p1",
            true
        ),
        Cleanup::Close
    ));
}

#[test]
fn cleanup_leaves_what_no_longer_answers_as_the_recorded_incarnation() {
    let placement = launched();
    let answer = |reading: HerdrReading, alive: bool| match cleanup_plan(
        &placement, &reading, "w1", alive,
    ) {
        Cleanup::Answer(result) => result,
        Cleanup::Close => panic!("cleanup closed a resource it should have kept"),
    };
    // The pane's own process is another one now.
    assert_eq!(
        answer(reading(vec![pane(None)]), false).status,
        CleanupStatus::Retained
    );
    // The pane is gone while the workspace the launch created is still there.
    assert_eq!(
        answer(reading(Vec::new()), true).status,
        CleanupStatus::Retained
    );
    // The workspace is gone with it, so there is nothing left to close.
    let emptied = HerdrReading {
        workspaces: Vec::new(),
        ..reading(Vec::new())
    };
    assert_eq!(answer(emptied, true).status, CleanupStatus::AlreadyAbsent);
    // The pane moved out of the workspace the launch created.
    let moved = PaneInfo {
        workspace_id: "w2".to_owned(),
        tab_id: "w2:t3".to_owned(),
        ..pane(None)
    };
    assert_eq!(
        answer(reading(vec![moved]), true).status,
        CleanupStatus::Retained
    );
}

#[test]
fn candidates_list_the_panes_of_the_callers_server_with_the_process_in_front() {
    let in_front = std::process::id();
    let gone = Sleeper::new();
    let gone_pid = gone.pid;
    drop(gone);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while feld_core::process::incarnation(gone_pid).unwrap().is_some() {
        assert!(
            std::time::Instant::now() < deadline,
            "the shell is still running"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let other = |pane_id: &str, terminal_id: &str| PaneInfo {
        pane_id: pane_id.to_owned(),
        tab_id: "w2:t3".to_owned(),
        workspace_id: "w2".to_owned(),
        terminal_id: terminal_id.to_owned(),
        cwd: Some("/tmp/other".to_owned()),
        ..pane(None)
    };
    let listed = reading(vec![
        PaneInfo {
            cwd: Some("/tmp".to_owned()),
            foreground_cwd: Some("/tmp/work".to_owned()),
            ..pane(None)
        },
        other("w2:p4", "term_def456"),
        // A pane herdr answers no directory for is one no attach can take.
        PaneInfo {
            pane_id: "w2:p5".to_owned(),
            ..pane(None)
        },
        // Neither is a pane whose shell or front herdr does not answer, one whose shell
        // is no longer running, or one closed since the reading.
        other("w2:p6", "term_6"),
        other("w2:p7", "term_7"),
        other("w2:p8", "term_8"),
        other("w2:p9", "term_9"),
    ]);
    let both = json!({ "shell_pid": in_front, "foreground_process_group_id": in_front });
    let herdr = FakeHerdr::listing(
        &listed,
        &[
            ("w1:p1", both.clone()),
            ("w2:p4", both.clone()),
            ("w2:p5", both),
            ("w2:p6", json!({ "foreground_process_group_id": in_front })),
            ("w2:p7", json!({ "shell_pid": in_front })),
            (
                "w2:p8",
                json!({ "shell_pid": gone_pid, "foreground_process_group_id": in_front }),
            ),
        ],
    );
    let candidates = herdr
        .provider()
        .candidates(Some(&json!({ "session": "feldtest", "pane": "w1:p1" })))
        .expect("the panes are listed");
    let summaries: Vec<_> = candidates.iter().map(|c| c.summary.as_str()).collect();
    assert_eq!(
        summaries,
        ["pane w1:p1 (feldtest:1)", "pane w2:p4 (second:3)"]
    );
    // The directory is the one of the program in front, or the pane's own without one.
    assert_eq!(candidates[0].workdir, "/tmp/work");
    assert_eq!(candidates[1].workdir, "/tmp/other");
    // Each names the pane the way `--herdr-session` and `--herdr-pane` take it again, on
    // the socket the server reports.
    assert_eq!(
        candidates[1].options,
        json!({ "session": "/tmp/herdr-test.sock", "pane": "w2:p4" })
    );
    assert_eq!(candidates[0].process.pid, in_front);
    assert!(
        herdr
            .commands()
            .iter()
            .all(|command| command.starts_with("--session feldtest ")),
        "{:?}",
        herdr.commands()
    );
}

#[test]
fn candidates_that_cannot_reach_the_server_say_why() {
    let herdr = FakeHerdr::refusing("server_not_running", "no server is running");
    let refusal = herdr
        .provider()
        .candidates(Some(&json!({ "session": "feldtest", "pane": "w1:p1" })))
        .expect_err("nothing to list");
    assert!(refusal.contains("server_not_running"), "{refusal}");
}

#[test]
fn a_create_that_answers_nothing_by_the_deadline_may_have_created_its_resource() {
    let failed = |kind| {
        HerdrProviderError::Io(provider::CommandFailed {
            program: "herdr".to_owned(),
            source: std::io::Error::new(kind, "herdr"),
        })
    };
    assert_eq!(
        launch_kind(&failed(std::io::ErrorKind::TimedOut)),
        ObservationKind::Lost
    );
    assert_eq!(
        launch_kind(&failed(std::io::ErrorKind::NotFound)),
        ObservationKind::Gone
    );
}
