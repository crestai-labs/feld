use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use feld_core::plugin::protocol::{ObservationKind, ObserveRequest, StopRequest};
use feld_core::plugin::sdk::provider::{self, CleanupStatus, ProviderBackend, ResourceOrigin};
use feld_core::process::quote;

use crate::commands::*;
use crate::placement::*;
use crate::snapshot::*;

/// A program that stays in the foreground of a pane, which is all this Provider needs
/// of a Runtime: which product it is belongs to the Runtime Adapter's `match`.
fn sleeping_program(directory: &Path, name: &str) -> Option<PathBuf> {
    let source = directory.join(format!("{name}.c"));
    let executable = directory.join(name);
    fs::write(
        &source,
        "#include <unistd.h>\nint main(void) { sleep(300); return 0; }\n",
    )
    .unwrap();
    let compiled = Command::new("cc")
        .args([source.as_os_str(), "-o".as_ref(), executable.as_os_str()])
        .status()
        .is_ok_and(|status| status.success());
    if !compiled {
        feld_core::test_require::step_aside("tmux", "no C compiler to build the program with");
        return None;
    }
    Some(executable)
}

fn real_tmux() -> String {
    String::from_utf8(
        Command::new("which")
            .arg("tmux")
            .output()
            .expect("locate tmux")
            .stdout,
    )
    .expect("tmux path is UTF-8")
    .trim()
    .to_owned()
}

/// A tmux that runs `script` after `#!/bin/sh`.
fn tmux_wrapper(directory: &Path, script: &str) -> PathBuf {
    let wrapper = directory.join("tmux-wrapper");
    fs::write(
        &wrapper,
        format!("#!/bin/sh\n[ -z \"$FELD_TEST_WARM_UP\" ] || exit 0\n{script}"),
    )
    .expect("write tmux wrapper");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))
        .expect("make tmux wrapper executable");
    // macOS inspects an executable the first time it runs, which takes seconds while the
    // machine builds and runs others; running it here keeps that out of the deadline the
    // Provider gives each command.
    let _ = Command::new(&wrapper)
        .env("FELD_TEST_WARM_UP", "1")
        .status();
    wrapper
}

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);

struct TestServer {
    endpoint: TmuxEndpoint,
}

impl TestServer {
    fn new() -> Option<Self> {
        if !feld_core::test_require::tmux() {
            return None;
        }
        Some(Self {
            endpoint: TmuxEndpoint::Name(format!(
                "feld-provider-test-{}-{}",
                std::process::id(),
                NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
            )),
        })
    }

    fn new_with_path() -> Option<Self> {
        if !feld_core::test_require::tmux() {
            return None;
        }
        Some(Self {
            endpoint: TmuxEndpoint::Path(std::env::temp_dir().join(format!(
                "feld-provider-test-{}-{}.sock",
                std::process::id(),
                NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
            ))),
        })
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut command = Command::new("tmux");
        self.endpoint.append_args(&mut command);
        command.args(args).output().expect("run tmux")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let mut command = Command::new("tmux");
        self.endpoint.append_args(&mut command);
        let _ = command.arg("kill-server").output();
    }
}

fn prepare(server: &TestServer, unit: TmuxUnit, borrowed: Option<&str>) -> PreparedLaunch {
    TmuxProvider::default()
        .prepare(PrepareRequest {
            endpoint: server.endpoint.clone(),
            unit,
            borrowed: borrowed.map(str::to_owned),
            workdir: std::env::temp_dir(),
        })
        .expect("prepare launch")
}

/// How many times a test launches again when tmux could not open a pty. macOS refuses
/// one now and then while other tmux servers on the machine open and close theirs.
const PTY_ATTEMPTS: usize = 5;

fn launch(
    provider: &TmuxProvider,
    prepared: PreparedLaunch,
    name: &str,
    command: &[&str],
    env: Vec<(String, String)>,
) -> LaunchResult {
    let mut prepared = prepared;
    let mut attempts = 1;
    loop {
        let failure = match provider.launch(LaunchRequest {
            prepared: prepared.clone(),
            resource_name: name.to_owned(),
            command: command.iter().map(|value| (*value).to_owned()).collect(),
            env: env.clone(),
        }) {
            Ok(launched) => return launched,
            Err(failure) => failure,
        };
        assert!(
            failure.source.contains("fork failed") && attempts < PTY_ATTEMPTS,
            "launch: {failure:?}"
        );
        attempts += 1;
        // The failed launch closed what it created, so the tmux server it prepared on
        // may be gone with it.
        prepared = provider
            .prepare(PrepareRequest {
                endpoint: prepared.endpoint,
                unit: prepared.unit,
                borrowed: prepared.borrowed,
                workdir: prepared.workdir,
            })
            .expect("prepare launch");
    }
}

fn eventually_gone(provider: &TmuxProvider, placement: &TmuxPlacement) -> ObservationResult {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let observed = provider.observe(placement);
        if observed.kind == ObservationKind::Gone {
            return observed;
        }
        assert!(Instant::now() < deadline, "last observation: {observed:?}");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_summary_adds_the_name_tmux_answers_to_the_stable_id() {
    let placement = serde_json::json!({
        "unit": "pane",
        "paneId": "%3",
        "paneOrigin": "existing",
    });
    let current = CurrentNames {
        session_id: "$1".to_owned(),
        window_id: "@7".to_owned(),
        pane_id: "%3".to_owned(),
        session: "main".to_owned(),
        window: "editor".to_owned(),
        window_index: 1,
        pane_index: 0,
    };
    assert_eq!(
        TmuxProvider::summary(&placement, Some(&current)),
        "pane %3 (main:1, existing)"
    );
    // tmux answers no name for a resource it no longer holds, and none for one that is
    // not created yet; the stable ID stands alone.
    assert_eq!(
        TmuxProvider::summary(&placement, None),
        "pane %3 (existing)"
    );
    assert_eq!(
        TmuxProvider::summary(
            &serde_json::json!({"unit": "session", "sessionId": "$1"}),
            Some(&current)
        ),
        "session $1 (main)"
    );
    // A pane moved out of the recorded session reads no name for it: the name tmux answers
    // is the session that holds the pane now, not the recorded one.
    assert_eq!(
        TmuxProvider::summary(
            &serde_json::json!({"unit": "session", "sessionId": "$2", "sessionOrigin": "created"}),
            Some(&current)
        ),
        "session $2 (created)"
    );
}

#[test]
fn the_attach_command_quotes_the_stable_id_for_the_shell() {
    let placement = serde_json::json!({
        "endpoint": {"kind": "path", "value": "/tmp/feld test/tmux"},
        "unit": "session",
        "sessionId": "$0",
    });
    let command = TmuxProvider::attach_command(&placement, None).unwrap();
    assert_eq!(
        command,
        "tmux -S '/tmp/feld test/tmux' attach-session -t '$0'"
    );
    // A shell reads the line back as the argv tmux needs, with `$0` left as it is.
    let argv = Command::new("sh")
        .args(["-c", &format!("printf '%s\\n' {command}")])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(argv.stdout).unwrap(),
        "tmux\n-S\n/tmp/feld test/tmux\nattach-session\n-t\n$0\n"
    );
    let named = serde_json::json!({
        "endpoint": {"kind": "name", "value": "feld"},
        "unit": "window",
        "windowId": "@7",
    });
    assert_eq!(
        TmuxProvider::attach_command(&named, None).as_deref(),
        Some("tmux -L feld attach-session -t '@7'")
    );
}

#[test]
fn the_attach_command_targets_the_pane_once_it_left_the_recorded_unit() {
    let placement = serde_json::json!({
        "endpoint": {"kind": "name", "value": "feld"},
        "unit": "session",
        "sessionId": "$2",
    });
    let moved = CurrentNames {
        session_id: "$0".to_owned(),
        window_id: "@2".to_owned(),
        pane_id: "%2".to_owned(),
        session: "holder".to_owned(),
        window: "sh".to_owned(),
        window_index: 1,
        pane_index: 0,
    };
    assert_eq!(
        TmuxProvider::attach_command(&placement, Some(&moved)).as_deref(),
        Some("tmux -L feld attach-session -t '%2'")
    );
    let inside = CurrentNames {
        session_id: "$2".to_owned(),
        ..moved
    };
    assert_eq!(
        TmuxProvider::attach_command(&placement, Some(&inside)).as_deref(),
        Some("tmux -L feld attach-session -t '$2'")
    );
    assert_eq!(
        TmuxProvider::attach_command(&placement, None).as_deref(),
        Some("tmux -L feld attach-session -t '$2'")
    );
}

#[test]
fn the_launch_command_takes_the_environment_from_the_provider_and_quotes_the_argv() {
    let env = launch_env(&[("FELD_VALUE".to_owned(), "a'b $(touch nope)".to_owned())]).unwrap();
    let command = launch_shell_command(
        &PathBuf::from("tmux"),
        &TmuxEndpoint::Name("feld test".to_owned()),
        "gate",
        &env,
        &[
            "printf".to_owned(),
            "%s".to_owned(),
            "hello world".to_owned(),
        ],
    );
    // The environment stays off the command line, which every user of the machine reads.
    assert!(!command.contains("touch nope"), "{command}");
    assert!(command.starts_with(&format!("{}; ", env.source())));
    assert!(command.contains("'tmux' -L 'feld test' wait-for 'gate'; exec"));
    assert!(command.ends_with("'printf' '%s' 'hello world'"));
    // A shell takes the environment from the Provider.
    let shell = Command::new("sh")
        .arg("-c")
        .arg(format!("{}; printf %s \"$FELD_VALUE\"", env.source()))
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    env.hand_over(shell.id(), Instant::now() + Duration::from_secs(10))
        .unwrap();
    let ran = shell.wait_with_output().unwrap();
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "a'b $(touch nope)");
    assert!(launch_env(&[("BAD-NAME".into(), "x".into())]).is_err());
}

/// A shell whose launch environment never arrives stops before the program.
#[test]
fn the_launch_command_stops_without_its_environment() {
    let env = launch_env(&[("FELD_VALUE".to_owned(), "x".to_owned())]).unwrap();
    let source = env.source();
    drop(env);
    let ran = Command::new("sh")
        .arg("-c")
        .arg(format!("{source}; printf ran"))
        .output()
        .unwrap();
    assert!(!ran.status.success());
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "");
}

#[test]
fn session_launch_preserves_zero_and_nonzero_exit_status() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();

    let zero = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "zero",
        &["sh", "-c", "exit 0"],
        vec![],
    );
    let zero_observed = eventually_gone(&provider, &zero.placement);
    assert_eq!(zero_observed.exit_code, Some(0));
    assert_eq!(zero.placement.session_origin, ResourceOrigin::Created);
    assert!(provider.cleanup(&zero.placement).cleaned);

    let nonzero = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "nonzero",
        &["sh", "-c", "exit 23"],
        vec![],
    );
    let nonzero_observed = eventually_gone(&provider, &nonzero.placement);
    assert_eq!(nonzero_observed.exit_code, Some(23));
    assert!(provider.cleanup(&nonzero.placement).cleaned);
}

#[test]
fn a_dead_pane_waits_for_the_exit_status_tmux_records_after_it() {
    let Some(server) = TestServer::new() else {
        return;
    };
    // The wrapper answers `list-panes` as tmux does between the end of file on the
    // pane's terminal and the `SIGCHLD` of its process: dead, with no exit status yet.
    let temp = tempfile::tempdir().expect("temporary wrapper directory");
    let real_tmux = real_tmux();
    let wrapper = tmux_wrapper(
        temp.path(),
        &format!(
            "case \" $* \" in\n  *\" list-panes \"*)\n    {} \"$@\" | perl -pe 's/\\xee\\x80\\x801\\xee\\x80\\x80\\d+\\xee\\x80\\x80\\xee\\x80\\x80/\\xee\\x80\\x801\\xee\\x80\\x80\\xee\\x80\\x80\\xee\\x80\\x80/'\n    ;;\n  *)\n    exec {} \"$@\"\n    ;;\nesac\n",
            quote(&real_tmux),
            quote(&real_tmux),
        ),
    );
    let provider = TmuxProvider::new(&wrapper);

    let ended = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "unreaped",
        &["sh", "-c", "exit 7"],
        vec![],
    );
    assert_eq!(
        eventually_gone(&provider, &ended.placement).exit_code,
        Some(7)
    );
    assert!(provider.cleanup(&ended.placement).cleaned);
}

#[test]
fn a_signal_ends_a_pane_without_pane_dead_signal() {
    let Some(server) = TestServer::new() else {
        return;
    };
    // The wrapper answers as tmux 3.2 does, which has no `pane_dead_signal`.
    let temp = tempfile::tempdir().expect("temporary wrapper directory");
    let real_tmux = real_tmux();
    let wrapper = tmux_wrapper(
        temp.path(),
        &format!(
            "case \" $* \" in\n  *\" list-panes \"*|*\" display-message \"*)\n    {} \"$@\" | perl -pe 's/(\\xee\\x80\\x801\\xee\\x80\\x80\\xee\\x80\\x80)[a-z0-9]+/$1/'\n    ;;\n  *)\n    exec {} \"$@\"\n    ;;\nesac\n",
            quote(&real_tmux),
            quote(&real_tmux),
        ),
    );
    let provider = TmuxProvider::new(&wrapper);

    // tmux 3.2 wraps the line it draws over several rows of a narrow pane. Later tmux cuts
    // it at the pane's width, and answers `pane_dead_signal` instead.
    let version = String::from_utf8(
        Command::new(&real_tmux)
            .arg("-V")
            .output()
            .expect("tmux version")
            .stdout,
    )
    .expect("tmux version is UTF-8");
    let draws_wrapped = version.trim().strip_prefix("tmux 3.").is_some_and(|minor| {
        minor
            .trim_end_matches(|character: char| !character.is_ascii_digit())
            .parse::<u32>()
            .is_ok_and(|minor| minor < 3)
    });
    let widths = if draws_wrapped {
        vec![None, Some("20")]
    } else {
        vec![None]
    };
    for width in widths {
        let stopped = launch(
            &provider,
            prepare(&server, TmuxUnit::Session, None),
            "signalled",
            &["sleep", "300"],
            vec![],
        );
        if let Some(width) = width {
            let window = stopped.placement.window_id.as_str();
            assert!(
                server
                    .run(&["resize-window", "-t", window, "-x", width])
                    .status
                    .success()
            );
            let shown = server.run(&["display-message", "-p", "-t", window, "#{pane_width}"]);
            assert_eq!(String::from_utf8_lossy(&shown.stdout).trim(), width);
        }
        let started = Instant::now();
        let stop = provider.stop(&stopped.placement);
        assert_eq!(stop.observed.kind, ObservationKind::Gone, "{width:?}");
        assert_eq!(stop.observed.exit_code, Some(143), "{width:?}");
        assert!(started.elapsed() < provider.settle_timeout, "{width:?}");
        assert!(provider.cleanup(&stopped.placement).cleaned);
    }
}

#[test]
fn signal_exit_codes_come_from_names_and_numbers() {
    assert_eq!(signal_exit_code("term"), Some(143));
    assert_eq!(signal_exit_code("kill"), Some(137));
    assert_eq!(signal_exit_code("15"), Some(143));
    assert_eq!(signal_exit_code("hup"), None);
    assert_eq!(
        drawn_dead_signal("$ \n\nPane is dead (signal term, Sat Oct  3 02:44:48 2026)\n\n"),
        Some("term")
    );
    assert_eq!(
        drawn_dead_signal("Pane is dead (status 7, Sat Oct  3 02:44:48 2026)"),
        None
    );
    assert_eq!(
        drawn_dead_signal("Pane is dead (signal term, then)\nlater output"),
        None
    );
}

#[test]
fn launches_without_a_utf8_locale() {
    let Some(server) = TestServer::new() else {
        return;
    };
    // tmux replaces control characters in its output with `_` unless the locale is UTF-8
    // or the client runs inside tmux.
    let temp = tempfile::tempdir().expect("temporary wrapper directory");
    let wrapper = tmux_wrapper(
        temp.path(),
        &format!(
            "exec env -u LC_ALL -u LC_CTYPE -u LANG -u TMUX {} \"$@\"\n",
            quote(&real_tmux())
        ),
    );
    let provider = TmuxProvider::new(&wrapper);

    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "no-locale",
        &["true"],
        vec![],
    );
    assert_eq!(
        eventually_gone(&provider, &launched.placement).exit_code,
        Some(0)
    );
    assert!(provider.cleanup(&launched.placement).cleaned);
}

#[test]
fn launches_on_a_socket_path_endpoint() {
    let Some(server) = TestServer::new_with_path() else {
        return;
    };
    let provider = TmuxProvider::default();
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "path-endpoint",
        &["true"],
        vec![],
    );
    assert_eq!(
        eventually_gone(&provider, &launched.placement).exit_code,
        Some(0)
    );
    assert!(provider.cleanup(&launched.placement).cleaned);
}

#[test]
fn attach_names_the_pane_and_socket_it_could_not_find() {
    let Some(server) = TestServer::new() else {
        return;
    };
    assert!(
        server
            .run(&["new-session", "-d", "-s", "present", "--", "sleep", "300"])
            .status
            .success()
    );
    let provider = TmuxProvider::default();
    let TmuxEndpoint::Name(socket) = &server.endpoint else {
        unreachable!("TestServer::new names its socket");
    };
    for pane in ["%999", "absent:1", "present:7", "present:7.0"] {
        let error = provider
            .attach(AttachRequest {
                endpoint: server.endpoint.clone(),
                pane: pane.to_owned(),
            })
            .expect_err("attach to a missing pane");
        assert_eq!(
            error.to_string(),
            format!("pane {pane} was not found on tmux socket {socket}")
        );
    }
}

#[test]
fn prepare_names_the_borrowed_resource_and_socket_it_could_not_find() {
    let Some(server) = TestServer::new() else {
        return;
    };
    assert!(
        server
            .run(&["new-session", "-d", "-s", "present", "--", "sleep", "300"])
            .status
            .success()
    );
    let provider = TmuxProvider::default();
    let TmuxEndpoint::Name(socket) = &server.endpoint else {
        unreachable!("TestServer::new names its socket");
    };
    for (unit, kind, borrowed) in [
        (TmuxUnit::Pane, "window", "@999"),
        (TmuxUnit::Pane, "window", "absent:1"),
        (TmuxUnit::Pane, "window", "present:7"),
        (TmuxUnit::Window, "session", "nosuch"),
    ] {
        let error = provider
            .prepare(PrepareRequest {
                endpoint: server.endpoint.clone(),
                unit,
                borrowed: Some(borrowed.to_owned()),
                workdir: std::env::temp_dir(),
            })
            .expect_err("prepare over a missing borrowed resource");
        assert_eq!(
            error.to_string(),
            format!("{kind} {borrowed} was not found on tmux socket {socket}")
        );
    }
}

#[test]
fn attach_tracks_the_runtime_process_and_preserves_borrowed_resources() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let temporary = tempfile::tempdir().unwrap();
    let Some(_program) = sleeping_program(temporary.path(), "runtime") else {
        return;
    };
    // As under a person's shell, the Runtime is a child of the pane's shell, in front of
    // it with a process group of its own, and the shell stays after it.
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "attached",
                "-c",
                temporary.path().to_str().unwrap(),
                "--",
                "exec /bin/sh -c 'set -m; ./runtime; exec sleep 300'",
            ])
            .status
            .success()
    );
    assert!(
        server
            .run(&["set-option", "-p", "-t", "attached", "remain-on-exit", "on"])
            .status
            .success()
    );
    let provider = TmuxProvider::default();
    let placement = provider
        .attach(AttachRequest {
            endpoint: server.endpoint.clone(),
            pane: "attached".to_owned(),
        })
        .expect("attach Runtime")
        .placement;
    assert_eq!(placement.session_origin, ResourceOrigin::Existing);
    assert_eq!(placement.window_origin, ResourceOrigin::Existing);
    assert_eq!(placement.pane_origin, ResourceOrigin::Existing);
    assert_eq!(provider.observe(&placement).kind, ObservationKind::Running);
    assert_eq!(provider.cleanup(&placement).status, CleanupStatus::Retained);
    provider
        .signal(placement.process.as_ref().unwrap().pid, "KILL")
        .unwrap();
    assert_eq!(eventually_gone(&provider, &placement).exit_code, None);
    assert!(
        server
            .run(&["has-session", "-t", "attached"])
            .status
            .success()
    );
}

/// A Runtime that outlives its pane is `lost`, even after the pane's shell has gone.
#[test]
fn an_attached_runtime_that_outlives_its_pane_is_lost() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let temporary = tempfile::tempdir().unwrap();
    let Some(_program) = sleeping_program(temporary.path(), "runtime") else {
        return;
    };
    // `nohup` keeps the Runtime through the hangup that ends the shell with its pane.
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "attached",
                "-c",
                temporary.path().to_str().unwrap(),
                "--",
                "exec /bin/sh -c 'set -m; nohup ./runtime; exec sleep 300'",
            ])
            .status
            .success()
    );
    // Another session keeps the tmux server up once the attached pane is gone.
    assert!(
        server
            .run(&["new-session", "-d", "-s", "keep", "sleep 300"])
            .status
            .success()
    );
    let provider = TmuxProvider::default();
    let placement = provider
        .attach(AttachRequest {
            endpoint: server.endpoint.clone(),
            pane: "attached".to_owned(),
        })
        .expect("attach Runtime")
        .placement;
    let runtime = placement.process.clone().unwrap();
    assert_ne!(runtime, placement.pane_process);
    assert!(
        server
            .run(&["kill-pane", "-t", &placement.pane_id])
            .status
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while provider
        .process_incarnation(placement.pane_process.pid)
        .unwrap()
        .is_some_and(|current| current == placement.pane_process)
    {
        assert!(
            Instant::now() < deadline,
            "the pane's shell is still running"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let observed = provider.observe(&placement);
    assert_eq!(observed.kind, ObservationKind::Lost, "{observed:?}");
    provider.signal(runtime.pid, "KILL").unwrap();
    assert_eq!(eventually_gone(&provider, &placement).exit_code, None);
}

/// An attach to the pane of an `owned` Session takes the process the launch recorded as
/// its program, which is how core refuses it to another Actor.
#[test]
fn attach_to_a_launched_pane_takes_its_program() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "owned",
        &["sleep", "30"],
        vec![],
    );
    let program = launched.placement.program.clone().expect("owned program");
    assert_eq!(program, launched.placement.pane_process);
    assert_eq!(launched.placement.process, None);
    let attached = provider
        .attach(AttachRequest {
            endpoint: server.endpoint.clone(),
            pane: launched.placement.pane_id.clone(),
        })
        .expect("attach the launched pane")
        .placement;
    assert_eq!(attached.process, Some(program));
    assert_eq!(attached.program, None);
    provider.kill(&launched.placement);
    eventually_gone(&provider, &launched.placement);
    assert!(provider.cleanup(&launched.placement).cleaned);
}

#[test]
fn attach_reports_the_program_a_pane_runs_by_absolute_path() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let temporary = tempfile::tempdir().unwrap();
    let Some(program) = sleeping_program(temporary.path(), "runtime") else {
        return;
    };
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "absolute",
                "--",
                &format!("exec {}", program.to_str().unwrap()),
            ])
            .status
            .success()
    );
    let provider = TmuxProvider::default();
    let placement = provider
        .attach(AttachRequest {
            endpoint: server.endpoint.clone(),
            pane: "absolute".to_owned(),
        })
        .unwrap()
        .placement;
    assert_eq!(provider.observe(&placement).kind, ObservationKind::Running);
    assert_eq!(
        foreground_argv(&placement),
        vec![program.to_string_lossy().into_owned()]
    );
}

/// What the Runtime Adapter's `match` is handed: the process in the pane's foreground,
/// never a child the pane's program left in the background.
fn foreground_argv(placement: &TmuxPlacement) -> Vec<String> {
    let pid = placement
        .process
        .as_ref()
        .expect("attach reports the foreground process")
        .pid;
    feld_core::process::snapshot(pid)
        .expect("read the foreground process")
        .expect("the foreground process is running")
        .argv
}

#[test]
fn attach_reports_the_foreground_process_rather_than_a_background_child() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let temporary = tempfile::tempdir().unwrap();
    let Some(_program) = sleeping_program(temporary.path(), "runtime") else {
        return;
    };
    let command = "./runtime & exec sleep 300";
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "background",
                "-c",
                temporary.path().to_str().unwrap(),
                "--",
                "sh",
                "-c",
                command
            ])
            .status
            .success()
    );
    let provider = TmuxProvider::default();
    let placement = provider
        .attach(AttachRequest {
            endpoint: server.endpoint.clone(),
            pane: "background".to_owned(),
        })
        .expect("attach resolves the pane")
        .placement;
    assert_eq!(
        foreground_argv(&placement),
        vec!["sleep".to_owned(), "300".to_owned()]
    );
}

#[test]
fn graceful_stop_sends_exit_to_the_recorded_pane() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "graceful",
        &["sh", "-c", "read line; [ \"$line\" = /exit ]"],
        vec![],
    );
    let result = provider.graceful_stop(&launched.placement, &["/exit".to_owned()]);
    assert_eq!(result.attempt, "tmux sent /exit to the pane");
    let gone = if result.observed.kind == ObservationKind::Gone {
        result.observed
    } else {
        eventually_gone(&provider, &launched.placement)
    };
    assert_eq!(gone.exit_code, Some(0));
    assert!(provider.cleanup(&launched.placement).cleaned);
}

/// An input that starts with `-` reaches the pane as typed, not as a `send-keys` flag.
#[test]
fn graceful_stop_types_an_input_that_starts_with_a_dash() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "dash",
        &[
            "sh",
            "-c",
            "read line; [ \"$line\" = -R ] || exit 1; read item; [ \"$item\" = '- item' ]",
        ],
        vec![],
    );
    let result =
        provider.graceful_stop(&launched.placement, &["-R".to_owned(), "- item".to_owned()]);
    assert_eq!(result.attempt, "tmux sent -R and - item to the pane");
    let gone = if result.observed.kind == ObservationKind::Gone {
        result.observed
    } else {
        eventually_gone(&provider, &launched.placement)
    };
    assert_eq!(gone.exit_code, Some(0));
    assert!(provider.cleanup(&launched.placement).cleaned);
}

/// A Runtime that asks to confirm its exit, the way Claude Code does while the Feld
/// Monitor runs beside it: the first input leaves it asking, and the next input the
/// Adapter declares answers it.
#[test]
fn graceful_stop_answers_a_runtime_that_asks_to_confirm() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "confirm",
        &[
            "sh",
            "-c",
            "read line; [ \"$line\" = /exit ] || exit 1; read confirm; [ -z \"$confirm\" ]",
        ],
        vec![],
    );
    let result = provider.graceful_stop(&launched.placement, &["/exit".to_owned(), String::new()]);
    assert_eq!(result.attempt, "tmux sent /exit to the pane");
    let gone = if result.observed.kind == ObservationKind::Gone {
        result.observed
    } else {
        eventually_gone(&provider, &launched.placement)
    };
    assert_eq!(gone.exit_code, Some(0));
    assert!(provider.cleanup(&launched.placement).cleaned);
}

/// A Runtime that was still starting when the stop came, the way Claude Code drops what
/// is typed into it before its prompt is ready: the first column is lost, and the column
/// heard again ends it.
#[test]
fn graceful_stop_answers_a_runtime_that_dropped_the_first_column() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "starting",
        &[
            "sh",
            "-c",
            "read dropped; read dropped; read line; [ \"$line\" = /exit ] || exit 1; read confirm; [ -z \"$confirm\" ]",
        ],
        vec![],
    );
    let result = provider.graceful_stop(&launched.placement, &["/exit".to_owned(), String::new()]);
    let gone = if result.observed.kind == ObservationKind::Gone {
        result.observed
    } else {
        eventually_gone(&provider, &launched.placement)
    };
    assert_eq!(gone.exit_code, Some(0));
    assert!(provider.cleanup(&launched.placement).cleaned);
}

#[test]
fn socket_replacement_is_lost_and_cleanup_preserves_the_new_server() {
    let Some(server) = TestServer::new_with_path() else {
        return;
    };
    let provider = TmuxProvider::default();
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "same-name",
        &["sleep", "300"],
        vec![],
    );
    let socket = match &server.endpoint {
        TmuxEndpoint::Path(path) => path,
        TmuxEndpoint::Name(_) => unreachable!(),
    };
    let old_socket = socket.with_extension("old.sock");
    fs::rename(socket, &old_socket).expect("move old server socket");
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "same-name",
                "--",
                "exec sleep 300"
            ])
            .status
            .success()
    );

    assert_eq!(
        provider.observe(&launched.placement).kind,
        ObservationKind::Lost
    );
    let cleanup = provider.cleanup(&launched.placement);
    assert_eq!(cleanup.status, CleanupStatus::Retained);
    assert!(
        server
            .run(&["has-session", "-t", "same-name"])
            .status
            .success()
    );

    let old_endpoint = TmuxEndpoint::Path(old_socket);
    let mut command = Command::new("tmux");
    old_endpoint.append_args(&mut command);
    assert!(
        command
            .arg("kill-server")
            .status()
            .expect("kill old server")
            .success()
    );
}

#[test]
fn launch_failure_reports_partial_placement_and_cleanup() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    let failure = provider
        .launch(LaunchRequest {
            prepared: prepare(&server, TmuxUnit::Session, None),
            resource_name: "invalid-env".to_owned(),
            command: vec!["sleep".to_owned(), "300".to_owned()],
            env: vec![("INVALID-NAME".to_owned(), "value".to_owned())],
        })
        .expect_err("invalid environment must fail after resource creation");

    assert_eq!(failure.observation.kind, ObservationKind::Gone);
    assert!(failure.partial.is_some());
    assert_eq!(
        failure.cleanup.as_ref().map(|cleanup| cleanup.status),
        Some(CleanupStatus::Removed)
    );
}

#[test]
fn an_unreadable_create_answer_closes_the_created_resource() {
    let Some(server) = TestServer::new() else {
        return;
    };
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "borrowed",
                "--",
                "exec sleep 300"
            ])
            .status
            .success()
    );
    // The wrapper answers the create as tmux 3.4 does for a control character
    // separator: the separator printed as the four characters `\037`.
    let temp = tempfile::tempdir().expect("temporary wrapper directory");
    let real_tmux = real_tmux();
    let wrapper = tmux_wrapper(
        temp.path(),
        &format!(
            "case \" $* \" in\n  *\" -P \"*)\n    {} \"$@\" | perl -pe 's/\\xee\\x80\\x80/\\\\037/g'\n    ;;\n  *)\n    exec {} \"$@\"\n    ;;\nesac\n",
            quote(&real_tmux),
            quote(&real_tmux),
        ),
    );
    let provider = TmuxProvider::new(&wrapper);
    let windows = |server: &TestServer| {
        String::from_utf8_lossy(
            &server
                .run(&["list-windows", "-a", "-F", "#{session_name}"])
                .stdout,
        )
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>()
    };

    for (unit, borrowed) in [
        (TmuxUnit::Session, None),
        (TmuxUnit::Window, Some("borrowed")),
        (TmuxUnit::Pane, Some("borrowed:^")),
    ] {
        let failure = provider
            .launch(LaunchRequest {
                prepared: prepare(&server, unit, borrowed),
                resource_name: "unreadable".to_owned(),
                command: vec!["sleep".to_owned(), "300".to_owned()],
                env: vec![],
            })
            .expect_err("an unreadable create answer must fail the launch");
        assert_eq!(failure.observation.kind, ObservationKind::Gone, "{unit:?}");
        assert!(failure.partial.is_some());
        assert_eq!(
            failure.cleanup.as_ref().map(|cleanup| cleanup.status),
            Some(CleanupStatus::Removed),
            "{unit:?}"
        );
        assert_eq!(windows(&server), ["borrowed"], "{unit:?}");
        let panes = server.run(&["list-panes", "-t", "borrowed:^", "-F", "#{pane_id}"]);
        assert_eq!(String::from_utf8_lossy(&panes.stdout).lines().count(), 1);
    }
}

#[test]
fn a_failed_respawn_launches_in_a_resource_created_anew() {
    let Some(server) = TestServer::new() else {
        return;
    };
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "borrowed",
                "--",
                "exec sleep 300"
            ])
            .status
            .success()
    );
    // The wrapper fails the first respawn as tmux does when it cannot fork, and writes down
    // the pane it targeted.
    let temp = tempfile::tempdir().expect("temporary wrapper directory");
    let failed = temp.path().join("failed");
    let wrapper = tmux_wrapper(
        temp.path(),
        &format!(
            "case \" $* \" in\n  *\" respawn-pane \"*)\n    if [ ! -e {failed} ]; then\n      printf '%s\\n' \"$@\" | sed -n '/^-t$/{{n;p;q;}}' > {failed}\n      echo 'respawn pane failed: fork failed: Resource temporarily unavailable' >&2\n      exit 1\n    fi\n    ;;\nesac\nexec {} \"$@\"\n",
            quote(&real_tmux()),
            failed = quote(&failed.to_string_lossy()),
        ),
    );
    let provider = TmuxProvider::new(&wrapper);
    let server_pid = |server: &TestServer| {
        String::from_utf8_lossy(&server.run(&["display-message", "-p", "#{pid}"]).stdout)
            .trim()
            .to_owned()
    };
    let before = server_pid(&server);

    for (unit, borrowed) in [
        (TmuxUnit::Session, None),
        (TmuxUnit::Window, Some("borrowed")),
        (TmuxUnit::Pane, Some("borrowed:^")),
    ] {
        let _ = fs::remove_file(&failed);
        let launched = provider
            .launch(LaunchRequest {
                prepared: prepare(&server, unit, borrowed),
                resource_name: "respawned".to_owned(),
                command: vec!["sleep".to_owned(), "300".to_owned()],
                env: vec![],
            })
            .unwrap_or_else(|failure| panic!("{unit:?}: {failure:?}"));
        let failed_pane = fs::read_to_string(&failed).expect("the first respawn failed");
        assert!(failed_pane.starts_with('%'), "{failed_pane}");
        assert_ne!(launched.placement.pane_id, failed_pane.trim(), "{unit:?}");
        assert_eq!(
            launched.observation.kind,
            ObservationKind::Running,
            "{unit:?}"
        );
        let panes = server.run(&["list-panes", "-a", "-F", "#{pane_id}"]);
        let panes = String::from_utf8_lossy(&panes.stdout);
        assert!(
            !panes.lines().any(|pane| pane == failed_pane.trim()),
            "{unit:?}: {panes}"
        );
        assert_eq!(server_pid(&server), before, "{unit:?}");
        let _ = provider.kill(&launched.placement);
        provider.cleanup(&launched.placement);
    }
}

#[test]
fn an_unreadable_create_answer_keeps_a_pane_that_moved_away() {
    let Some(server) = TestServer::new() else {
        return;
    };
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "borrowed",
                "--",
                "exec sleep 300"
            ])
            .status
            .success()
    );
    // The created pane joins the borrowed window between the create and its answer, which
    // the wrapper prints as tmux 3.4 does for a control character separator.
    let temp = tempfile::tempdir().expect("temporary wrapper directory");
    let real_tmux = real_tmux();
    let wrapper = tmux_wrapper(
        temp.path(),
        &format!(
            "real={}\ncase \" $* \" in\n  *\" -P \"*)\n    answer=$(\"$real\" \"$@\")\n    pane=$(printf '%s' \"$answer\" | perl -ne 'print $1 if /(%\\d+)/')\n    \"$real\" \"$2\" \"$3\" join-pane -d -s \"$pane\" -t borrowed:^\n    printf '%s\\n' \"$answer\" | perl -pe 's/\\xee\\x80\\x80/\\\\037/g'\n    ;;\n  *)\n    exec \"$real\" \"$@\"\n    ;;\nesac\n",
            quote(&real_tmux),
        ),
    );
    let provider = TmuxProvider::new(&wrapper);

    for (unit, borrowed) in [
        (TmuxUnit::Session, None),
        (TmuxUnit::Window, Some("borrowed")),
    ] {
        let failure = provider
            .launch(LaunchRequest {
                prepared: prepare(&server, unit, borrowed),
                resource_name: "moved".to_owned(),
                command: vec!["sleep".to_owned(), "300".to_owned()],
                env: vec![],
            })
            .expect_err("an unreadable create answer must fail the launch");
        assert_eq!(failure.observation.kind, ObservationKind::Lost, "{unit:?}");
        assert!(failure.partial.is_none());
        assert_eq!(
            failure.cleanup.as_ref().map(|cleanup| cleanup.status),
            Some(CleanupStatus::Retained),
            "{unit:?}"
        );
        assert!(
            server
                .run(&["has-session", "-t", "borrowed"])
                .status
                .success()
        );
    }
    let panes = server.run(&["list-panes", "-t", "borrowed:^", "-F", "#{pane_id}"]);
    assert_eq!(String::from_utf8_lossy(&panes.stdout).lines().count(), 3);
}

#[test]
fn an_unreadable_create_answer_keeps_the_session_of_a_restarted_server() {
    let Some(server) = TestServer::new() else {
        return;
    };
    // Between the create and its answer the server ends, and a server started again on
    // the socket makes a session of the same name, numbered `$0`, `@0` and `%0` as well.
    let temp = tempfile::tempdir().expect("temporary wrapper directory");
    let real_tmux = real_tmux();
    let wrapper = tmux_wrapper(
        temp.path(),
        &format!(
            "real={}\ncase \" $* \" in\n  *\" -P \"*)\n    answer=$(\"$real\" \"$@\")\n    pid=$(\"$real\" \"$2\" \"$3\" display-message -p '#{{pid}}')\n    \"$real\" \"$2\" \"$3\" kill-server\n    while kill -0 \"$pid\" 2>/dev/null; do sleep 0.01; done\n    \"$real\" \"$@\" > /dev/null\n    printf '%s\\n' \"$answer\" | perl -pe 's/\\xee\\x80\\x80/\\\\037/g'\n    ;;\n  *)\n    exec \"$real\" \"$@\"\n    ;;\nesac\n",
            quote(&real_tmux),
        ),
    );
    let provider = TmuxProvider::new(&wrapper);

    let prepared = prepare(&server, TmuxUnit::Session, None);
    assert!(prepared.server.is_none());
    let failure = provider
        .launch(LaunchRequest {
            prepared,
            resource_name: "restarted".to_owned(),
            command: vec!["sleep".to_owned(), "300".to_owned()],
            env: vec![],
        })
        .expect_err("an unreadable create answer must fail the launch");
    assert_eq!(failure.observation.kind, ObservationKind::Lost);
    assert!(failure.partial.is_none());
    assert_eq!(
        failure.cleanup.as_ref().map(|cleanup| cleanup.status),
        Some(CleanupStatus::Retained)
    );
    let sessions = server.run(&["list-sessions", "-F", "#{session_id} #{session_name}"]);
    assert_eq!(
        String::from_utf8_lossy(&sessions.stdout).trim(),
        "$0 restarted"
    );
}

#[test]
fn an_answer_is_read_through_any_one_separator() {
    let fields = ["$0", "@1", "%2", "100", "1700000000", "101"].map(str::to_owned);
    assert!(answers_fields(
        "$0\\037@1\\037%2\\037100\\0371700000000\\037101",
        &fields
    ));
    assert!(answers_fields(
        "$0\u{e000}@1\u{e000}%2\u{e000}100\u{e000}1700000000\u{e000}101",
        &fields
    ));
    assert!(!answers_fields(
        "$0\\037@1\\037%2\\037999\\0371700000000\\037101",
        &fields
    ));
    assert!(!answers_fields("$0@1%2\\037100", &fields));
}

#[test]
fn a_dead_pane_reads_with_or_without_its_pid() {
    let pane = |pid: &str| {
        PaneSnapshot::parse(
            &[
                "$0",
                "@1",
                "%2",
                "100",
                "1700000000",
                pid,
                "1",
                "0",
                "",
                "main",
                "zsh",
                "0",
                "0",
                "/tmp",
                "/tmp/tmux.sock",
            ]
            .join("\u{e000}"),
        )
        .unwrap()
    };
    // tmux 3.7 and earlier keep the PID of a dead pane; tmux 3.8 answers none.
    assert_eq!(pane("101").pane_pid, Some(101));
    let dead = pane("");
    assert_eq!(dead.pane_pid, None);
    assert!(dead.dead);
    assert_eq!(dead.exit_code(), Some(0));
    assert!(dead.live_pid().is_err());
}

#[test]
fn a_placement_the_launch_never_reached_reads_without_a_decode_error() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    let placed = serde_json::to_value(prepare(&server, TmuxUnit::Session, None)).unwrap();

    let observed = provider::observe(
        &provider,
        &ObserveRequest {
            placements: vec![placed.clone()],
        },
    );
    let observation = &observed.results[0].observation;
    assert_eq!(observation.kind, ObservationKind::Lost);
    assert_eq!(
        observation.detail.as_deref(),
        Some("the launch never recorded an incarnation")
    );
    let error = provider::stop(
        &provider,
        &StopRequest {
            placement: placed,
            input: None,
        },
    )
    .expect_err("a placed Placement has nothing to stop");
    assert_eq!(
        format!("{error:#}"),
        "the launch never recorded an incarnation"
    );
}

#[test]
fn a_launch_whose_server_stopped_leaves_a_partial_placement_without_a_decode_error() {
    let Some(server) = TestServer::new() else {
        return;
    };
    // The server stops right after the pane is created, before the launch records the
    // pane's process, so cleanup finds a different server and keeps the Placement.
    let temp = tempfile::tempdir().expect("temporary wrapper directory");
    let real_tmux = real_tmux();
    let wrapper = tmux_wrapper(
        temp.path(),
        &format!(
            "real={}\nsocket={}\ncase \" $* \" in\n  *\" remain-on-exit \"*)\n    pid=$(\"$real\" -L \"$socket\" display-message -p '#{{pid}}')\n    \"$real\" -L \"$socket\" kill-server\n    while kill -0 \"$pid\" 2>/dev/null; do sleep 0.01; done\n    exit 1\n    ;;\n  *)\n    exec \"$real\" \"$@\"\n    ;;\nesac\n",
            quote(&real_tmux),
            quote(&server.endpoint.label()),
        ),
    );
    let provider = TmuxProvider::new(&wrapper);
    let failure = provider
        .launch(LaunchRequest {
            prepared: prepare(&server, TmuxUnit::Session, None),
            resource_name: "server-stopped".to_owned(),
            command: vec!["sleep".to_owned(), "300".to_owned()],
            env: vec![],
        })
        .expect_err("the launch must fail once its server stops");
    assert_ne!(
        failure.observation.kind,
        ObservationKind::Gone,
        "{failure:?}"
    );
    let partial = serde_json::to_value(failure.partial.expect("partial placement")).unwrap();

    let unfinished = "the launch did not finish recording an incarnation";
    let observed = provider::observe(
        &provider,
        &ObserveRequest {
            placements: vec![partial.clone()],
        },
    );
    let observation = &observed.results[0].observation;
    assert_eq!(observation.kind, ObservationKind::Lost);
    assert_eq!(observation.detail.as_deref(), Some(unfinished));
    for input in [None, Some(vec!["/exit".to_owned(), String::new()])] {
        let error = provider::stop(
            &provider,
            &StopRequest {
                placement: partial.clone(),
                input,
            },
        )
        .expect_err("a partial Placement has nothing to stop");
        assert_eq!(format!("{error:#}"), unfinished);
    }
}

#[test]
fn duplicate_session_rejection_is_gone_without_partial_resources() {
    let Some(server) = TestServer::new() else {
        return;
    };
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "duplicate",
                "--",
                "exec sleep 300"
            ])
            .status
            .success()
    );
    let provider = TmuxProvider::default();
    let failure = provider
        .launch(LaunchRequest {
            prepared: prepare(&server, TmuxUnit::Session, None),
            resource_name: "duplicate".to_owned(),
            command: vec!["true".to_owned()],
            env: vec![],
        })
        .expect_err("duplicate session must be rejected");

    assert_eq!(failure.observation.kind, ObservationKind::Gone);
    assert!(failure.partial.is_none());
    assert!(failure.cleanup.is_none());
    assert!(failure.source.contains("duplicate session"));
}

#[test]
fn disappearing_borrowed_session_is_gone_before_create() {
    let Some(server) = TestServer::new() else {
        return;
    };
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "borrowed",
                "--",
                "exec sleep 300"
            ])
            .status
            .success()
    );
    assert!(
        server
            .run(&["new-session", "-d", "-s", "keeper", "--", "exec sleep 300"])
            .status
            .success()
    );
    let provider = TmuxProvider::default();
    let prepared = prepare(&server, TmuxUnit::Window, Some("borrowed"));
    assert!(
        server
            .run(&["kill-session", "-t", "borrowed"])
            .status
            .success()
    );

    let failure = provider
        .launch(LaunchRequest {
            prepared,
            resource_name: "never-created".to_owned(),
            command: vec!["true".to_owned()],
            env: vec![],
        })
        .expect_err("missing borrowed session must fail verification");
    assert_eq!(failure.observation.kind, ObservationKind::Gone);
    assert!(failure.partial.is_none());
    assert!(failure.cleanup.is_none());
}

#[test]
fn disappearing_workdir_is_gone_before_create() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    let workdir = tempfile::tempdir().expect("temporary workdir");
    let prepared = provider
        .prepare(PrepareRequest {
            endpoint: server.endpoint.clone(),
            unit: TmuxUnit::Session,
            borrowed: None,
            workdir: workdir.path().to_owned(),
        })
        .expect("prepare with existing workdir");
    workdir.close().expect("remove prepared workdir");

    let failure = provider
        .launch(LaunchRequest {
            prepared,
            resource_name: "never-created".to_owned(),
            command: vec!["true".to_owned()],
            env: vec![],
        })
        .expect_err("missing workdir must fail before create");
    assert_eq!(failure.observation.kind, ObservationKind::Gone);
    assert!(failure.partial.is_none());
    assert!(failure.cleanup.is_none());
}

#[test]
fn create_rechecks_the_prepared_server_and_cleans_only_its_new_window() {
    let Some(server) = TestServer::new_with_path() else {
        return;
    };
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "borrowed",
                "--",
                "exec sleep 300"
            ])
            .status
            .success()
    );
    let temp = tempfile::tempdir().expect("temporary wrapper directory");
    let trigger = temp.path().join("trigger");
    let done = temp.path().join("done");
    let socket = match &server.endpoint {
        TmuxEndpoint::Path(path) => path.clone(),
        TmuxEndpoint::Name(_) => unreachable!(),
    };
    let old_socket = socket.with_extension("prepared.sock");
    let real_tmux = real_tmux();
    let script = format!(
        "real={}\n\"$real\" \"$@\"\nstatus=$?\ncase \" $* \" in\n  *\" display-message \"*)\n    if [ -f {} ] && [ ! -f {} ]; then\n      mv {} {}\n      : > {}\n      \"$real\" -S {} new-session -d -s borrowed -- 'exec sleep 300'\n    fi\n    ;;\nesac\nexit $status\n",
        quote(&real_tmux),
        quote(&trigger.to_string_lossy()),
        quote(&done.to_string_lossy()),
        quote(&socket.to_string_lossy()),
        quote(&old_socket.to_string_lossy()),
        quote(&done.to_string_lossy()),
        quote(&socket.to_string_lossy()),
    );
    let wrapper = tmux_wrapper(temp.path(), &script);
    let provider = TmuxProvider::new(&wrapper);
    let prepared = provider
        .prepare(PrepareRequest {
            endpoint: server.endpoint.clone(),
            unit: TmuxUnit::Window,
            borrowed: Some("borrowed".to_owned()),
            workdir: std::env::temp_dir(),
        })
        .expect("prepare against old server");
    fs::write(&trigger, b"swap").expect("arm socket replacement");

    let failure = provider
        .launch(LaunchRequest {
            prepared,
            resource_name: "created-window".to_owned(),
            command: vec!["sleep".to_owned(), "300".to_owned()],
            env: vec![],
        })
        .expect_err("create response from replacement server must fail");
    assert_eq!(failure.observation.kind, ObservationKind::Gone);
    assert_eq!(
        failure.cleanup.as_ref().map(|cleanup| cleanup.status),
        Some(CleanupStatus::Removed)
    );
    let windows = server.run(&["list-windows", "-t", "borrowed", "-F", "#{window_id}"]);
    assert!(windows.status.success());
    assert_eq!(String::from_utf8_lossy(&windows.stdout).lines().count(), 1);

    let old_endpoint = TmuxEndpoint::Path(old_socket);
    let mut command = Command::new("tmux");
    old_endpoint.append_args(&mut command);
    assert!(
        command
            .arg("kill-server")
            .status()
            .expect("kill old server")
            .success()
    );
}

#[test]
fn stop_and_kill_signal_the_recorded_process() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();

    let stopped = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "stop",
        &["sleep", "300"],
        vec![],
    );
    assert_eq!(stopped.observation.kind, ObservationKind::Running);
    let stop = provider.stop(&stopped.placement);
    assert_eq!(stop.observed.kind, ObservationKind::Gone);
    assert_eq!(stop.observed.exit_code, Some(143));
    provider.cleanup(&stopped.placement);

    let killed = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "kill",
        &["sleep", "300"],
        vec![],
    );
    let kill = provider.kill(&killed.placement);
    assert_eq!(kill.observed.kind, ObservationKind::Gone);
    assert_eq!(kill.observed.exit_code, Some(137));
    provider.cleanup(&killed.placement);
}

#[test]
fn detects_process_mismatch_and_server_restart() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "identity",
        &["sleep", "300"],
        vec![],
    );

    let replacement = server.run(&[
        "respawn-pane",
        "-k",
        "-t",
        &launched.placement.pane_id,
        "--",
        "exec sleep 300",
    ]);
    assert!(replacement.status.success());
    assert_eq!(
        provider.observe(&launched.placement).kind,
        ObservationKind::Mismatched
    );

    let killed = server.run(&["kill-server"]);
    assert!(killed.status.success());
    // `kill-server` returns while the server process is still exiting, and tmux cannot
    // tell the Provider about the server meanwhile.
    let deadline = Instant::now() + Duration::from_secs(3);
    let lost = loop {
        let observed = provider.observe(&launched.placement);
        if observed.kind != ObservationKind::Unknown || Instant::now() >= deadline {
            break observed;
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(lost.kind, ObservationKind::Lost, "{lost:?}");
}

/// The moment right after `/exit`: the process has been reaped and the process table no
/// longer lists its pid, but tmux has not yet marked the pane dead.
#[test]
fn a_pane_whose_process_the_process_table_no_longer_lists_is_exiting() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "exiting",
        &["sleep", "300"],
        vec![],
    );
    let mut snapshot = Snapshot::default();
    snapshot
        .processes
        .insert(launched.placement.pane_process.pid, None);
    let observed = provider
        .observe_checked(&launched.placement, &mut snapshot)
        .expect("observe");
    assert_eq!(observed.kind, ObservationKind::Running);
    assert_eq!(observed.detail, EXITING);
}

#[test]
fn graceful_stop_closes_every_pane_that_ends_on_exit() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    for _ in 0..10 {
        let launched = launch(
            &provider,
            prepare(&server, TmuxUnit::Session, None),
            "exit",
            &["sh", "-c", "read line; [ \"$line\" = /exit ]"],
            vec![],
        );
        let result = provider.graceful_stop(&launched.placement, &["/exit".to_owned()]);
        assert_eq!(result.observed.kind, ObservationKind::Gone, "{result:?}");
        assert_eq!(result.observed.exit_code, Some(0));
        provider.cleanup(&launched.placement);
    }
}

#[test]
fn a_server_that_answers_no_sessions_counts_as_no_server() {
    let Some(server) = TestServer::new() else {
        return;
    };
    // The wrapper answers `list-sessions` as a server does between closing its last
    // session and exiting: successfully, with no sessions.
    let temp = tempfile::tempdir().expect("temporary wrapper directory");
    let wrapper = tmux_wrapper(
        temp.path(),
        &format!(
            "case \" $* \" in\n  *\" list-sessions -F #{{pid}} \"*)\n    exit 0\n    ;;\nesac\nexec {} \"$@\"\n",
            quote(&real_tmux()),
        ),
    );
    let provider = TmuxProvider::new(&wrapper);
    let prepared = provider
        .prepare(PrepareRequest {
            endpoint: server.endpoint.clone(),
            unit: TmuxUnit::Session,
            borrowed: None,
            workdir: std::env::temp_dir(),
        })
        .expect("prepare launch");
    assert_eq!(prepared.server, None);
    let launched = launch(&provider, prepared, "after-last", &["sleep", "300"], vec![]);
    assert!(provider.cleanup(&launched.placement).cleaned);
}

#[test]
fn a_server_that_exits_during_list_sessions_counts_as_no_server() {
    let Some(server) = TestServer::new() else {
        return;
    };
    // The wrapper answers `list-sessions` as a server does that exits while the client
    // asks it.
    let temp = tempfile::tempdir().expect("temporary wrapper directory");
    let wrapper = tmux_wrapper(
        temp.path(),
        &format!(
            "case \" $* \" in\n  *\" list-sessions -F #{{pid}} \"*)\n    echo 'server exited unexpectedly' >&2\n    exit 1\n    ;;\nesac\nexec {} \"$@\"\n",
            quote(&real_tmux()),
        ),
    );
    let provider = TmuxProvider::new(&wrapper);
    let prepared = provider
        .prepare(PrepareRequest {
            endpoint: server.endpoint.clone(),
            unit: TmuxUnit::Session,
            borrowed: None,
            workdir: std::env::temp_dir(),
        })
        .expect("prepare launch");
    assert_eq!(prepared.server, None);
    let launched = launch(&provider, prepared, "after-exit", &["sleep", "300"], vec![]);
    assert!(provider.cleanup(&launched.placement).cleaned);
}

#[test]
fn a_mismatch_that_outlasts_the_wait_after_a_signal_is_reported() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider {
        settle_timeout: Duration::from_millis(200),
        ..TmuxProvider::default()
    };
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "replaced",
        &["sleep", "300"],
        vec![],
    );
    let replacement = server.run(&[
        "respawn-pane",
        "-k",
        "-t",
        &launched.placement.pane_id,
        "--",
        "exec sleep 300",
    ]);
    assert!(replacement.status.success());
    let observed = provider.observe_after_signal(&launched.placement);
    assert_eq!(observed.kind, ObservationKind::Mismatched);
    assert_eq!(
        observed.detail,
        "the pane now contains a different process PID"
    );
}

#[test]
fn reports_a_joined_pane_as_relocated() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    assert!(
        server
            .run(&["new-session", "-d", "-s", "source", "--", "exec sleep 300"])
            .status
            .success()
    );
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "destination",
                "--",
                "exec sleep 300"
            ])
            .status
            .success()
    );
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Pane, Some("source:^")),
        "relocated-pane",
        &["sleep", "300"],
        vec![],
    );

    let moved = server.run(&[
        "join-pane",
        "-d",
        "-s",
        &launched.placement.pane_id,
        "-t",
        "destination:^",
    ]);
    assert!(
        moved.status.success(),
        "{}",
        String::from_utf8_lossy(&moved.stderr)
    );
    assert_eq!(
        provider.observe(&launched.placement).kind,
        ObservationKind::Relocated
    );
    provider.kill(&launched.placement);
    assert!(provider.cleanup(&launched.placement).cleaned);
}

#[test]
fn reports_where_a_pane_broken_out_of_its_session_runs_now() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    assert!(
        server
            .run(&["new-session", "-d", "-s", "holder", "--", "exec sleep 300"])
            .status
            .success()
    );
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "broken-out",
        &["sleep", "300"],
        vec![],
    );
    let moved = server.run(&[
        "break-pane",
        "-d",
        "-s",
        &launched.placement.pane_id,
        "-t",
        "holder:",
    ]);
    assert!(
        moved.status.success(),
        "{}",
        String::from_utf8_lossy(&moved.stderr)
    );

    // The created session lost its last pane, so tmux removed it.
    let observed = provider.observe(&launched.placement);
    assert_eq!(observed.kind, ObservationKind::Relocated);
    let current = observed.current.as_ref().unwrap();
    assert_eq!(current.session, "holder");
    assert_eq!(
        observed.detail,
        format!(
            "the recorded pane is now in window {} (holder:{}) of session {} (holder)",
            current.window_id, current.window_index, current.session_id
        )
    );
    let placement = serde_json::to_value(&launched.placement).unwrap();
    assert_eq!(
        TmuxProvider::summary(&placement, Some(current)),
        format!("session {} (created)", launched.placement.session_id)
    );

    // The attach command opens the moved pane, not the active window of the session that
    // holds it now. A control-mode client attaches the same way without a terminal.
    let command = TmuxProvider::attach_command(&placement, Some(current)).unwrap();
    let mut client = Command::new("sh")
        .args([
            "-c",
            &format!(
                "exec {}",
                command.replace("attach-session", "-C attach-session")
            ),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let reached = loop {
        let clients = server.run(&["list-clients", "-F", "#{window_id} #{pane_id}"]);
        let clients = String::from_utf8_lossy(&clients.stdout).trim().to_owned();
        if !clients.is_empty() || Instant::now() >= deadline {
            break clients;
        }
        thread::sleep(Duration::from_millis(20));
    };
    let _ = client.kill();
    let _ = client.wait();
    assert_eq!(
        reached,
        format!("{} {}", current.window_id, launched.placement.pane_id)
    );
    provider.kill(&launched.placement);
}

#[test]
fn an_exiting_relocated_pane_still_names_where_it_runs_now() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    assert!(
        server
            .run(&["new-session", "-d", "-s", "holder", "--", "exec sleep 300"])
            .status
            .success()
    );
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Session, None),
        "exiting-moved",
        &["sleep", "300"],
        vec![],
    );
    assert!(
        server
            .run(&[
                "break-pane",
                "-d",
                "-s",
                &launched.placement.pane_id,
                "-t",
                "holder:"
            ])
            .status
            .success()
    );
    let mut snapshot = Snapshot::default();
    snapshot
        .processes
        .insert(launched.placement.pane_process.pid, None);
    let observed = provider
        .observe_checked(&launched.placement, &mut snapshot)
        .expect("observe");
    assert_eq!(observed.kind, ObservationKind::Relocated);
    let current = observed.current.as_ref().unwrap();
    assert_eq!(
        observed.detail,
        format!(
            "{EXITING}; the recorded pane is now in window {} (holder:{}) of session {} (holder)",
            current.window_id, current.window_index, current.session_id
        )
    );
    provider.kill(&launched.placement);
}

#[test]
fn reports_linked_membership_as_relocated_and_returns_that_location() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    assert!(
        server
            .run(&["new-session", "-d", "-s", "source", "--", "exec sleep 300"])
            .status
            .success()
    );
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "linked-destination",
                "--",
                "exec sleep 300"
            ])
            .status
            .success()
    );
    let launched = launch(
        &provider,
        prepare(&server, TmuxUnit::Pane, Some("source:^")),
        "linked-pane",
        &["sleep", "300"],
        vec![],
    );
    let linked = server.run(&[
        "link-window",
        "-s",
        &launched.placement.window_id,
        "-t",
        "linked-destination:",
    ]);
    assert!(
        linked.status.success(),
        "{}",
        String::from_utf8_lossy(&linked.stderr)
    );

    let observed = provider.observe(&launched.placement);
    assert_eq!(observed.kind, ObservationKind::Relocated);
    assert_ne!(
        observed.current.as_ref().map(|current| &current.session_id),
        Some(&launched.placement.session_id)
    );
    provider.kill(&launched.placement);
    assert!(provider.cleanup(&launched.placement).cleaned);
}

#[test]
fn window_and_pane_units_preserve_borrowed_resources() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    let setup = server.run(&[
        "new-session",
        "-d",
        "-s",
        "borrowed",
        "--",
        "exec sleep 300",
    ]);
    assert!(setup.status.success());

    let window_prepared = prepare(&server, TmuxUnit::Window, Some("borrowed"));
    assert!(
        window_prepared
            .borrowed
            .as_deref()
            .is_some_and(|borrowed| borrowed.starts_with('$'))
    );
    let window = launch(
        &provider,
        window_prepared,
        "owned-window",
        &["true"],
        vec![],
    );
    eventually_gone(&provider, &window.placement);
    assert!(provider.cleanup(&window.placement).cleaned);
    assert!(
        window
            .placement
            .borrowed
            .as_deref()
            .is_some_and(|borrowed| borrowed.starts_with('$'))
    );
    assert!(
        server
            .run(&["has-session", "-t", "borrowed"])
            .status
            .success()
    );

    let pane_prepared = prepare(&server, TmuxUnit::Pane, Some("borrowed:^"));
    assert!(
        pane_prepared
            .borrowed
            .as_deref()
            .is_some_and(|borrowed| borrowed.starts_with('@'))
    );
    let pane = launch(&provider, pane_prepared, "owned-pane", &["true"], vec![]);
    eventually_gone(&provider, &pane.placement);
    assert!(provider.cleanup(&pane.placement).cleaned);
    assert!(
        pane.placement
            .borrowed
            .as_deref()
            .is_some_and(|borrowed| borrowed.starts_with('@'))
    );
    assert!(
        server
            .run(&["has-session", "-t", "borrowed"])
            .status
            .success()
    );
}

#[test]
fn cleanup_closes_a_created_pane_moved_out_of_the_borrowed_window() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let provider = TmuxProvider::default();
    assert!(
        server
            .run(&[
                "new-session",
                "-d",
                "-s",
                "borrowed",
                "--",
                "exec sleep 300"
            ])
            .status
            .success()
    );
    let panes = |window: &str| {
        let listed = server.run(&["list-panes", "-t", window, "-F", "#{pane_id}"]);
        String::from_utf8_lossy(&listed.stdout)
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };

    // Broken out into a window of its own, the pane closes with the window tmux made.
    let broken = launch(
        &provider,
        prepare(&server, TmuxUnit::Pane, Some("borrowed:^")),
        "broken-out",
        &["sleep", "300"],
        vec![],
    );
    let moved = server.run(&[
        "break-pane",
        "-d",
        "-P",
        "-F",
        "#{window_id}",
        "-s",
        &broken.placement.pane_id,
    ]);
    assert!(
        moved.status.success(),
        "{}",
        String::from_utf8_lossy(&moved.stderr)
    );
    let made = String::from_utf8_lossy(&moved.stdout).trim().to_owned();
    provider.kill(&broken.placement);
    eventually_gone(&provider, &broken.placement);
    assert!(provider.cleanup(&broken.placement).cleaned);
    assert!(!server.run(&["has-session", "-t", &made]).status.success());
    assert_eq!(panes(&broken.placement.window_id).len(), 1);

    // Joined into a window a person made, the pane closes and the window stays.
    assert!(
        server
            .run(&[
                "new-window",
                "-d",
                "-t",
                "borrowed:",
                "-n",
                "mine",
                "--",
                "exec sleep 300"
            ])
            .status
            .success()
    );
    let joined = launch(
        &provider,
        prepare(&server, TmuxUnit::Pane, Some("borrowed:^")),
        "joined",
        &["sleep", "300"],
        vec![],
    );
    let moved = server.run(&[
        "join-pane",
        "-d",
        "-s",
        &joined.placement.pane_id,
        "-t",
        "borrowed:mine",
    ]);
    assert!(
        moved.status.success(),
        "{}",
        String::from_utf8_lossy(&moved.stderr)
    );
    provider.kill(&joined.placement);
    eventually_gone(&provider, &joined.placement);
    assert!(provider.cleanup(&joined.placement).cleaned);
    assert_eq!(panes("borrowed:mine").len(), 1);
    assert!(!panes("borrowed:mine").contains(&joined.placement.pane_id));
    assert_eq!(panes(&joined.placement.window_id).len(), 1);
}

#[test]
fn a_server_the_provider_starts_keeps_no_session_variables() {
    let Some(server) = TestServer::new() else {
        return;
    };
    let TmuxEndpoint::Name(socket) = &server.endpoint else {
        unreachable!();
    };
    // The launch runs in this test binary started again with the variables a daemon started
    // inside a Session would carry.
    let launched = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::launches_on_the_socket_it_is_given",
            "--ignored",
            "--quiet",
        ])
        .env("FELD_TEST_TMUX_SOCKET", socket)
        .env("FELD_SESSION_TOKEN", "review-sentinel")
        .env("FELD_SESSION_ID", "session_sentinel")
        .env("FELD_ACTOR", "sentinel")
        .env("FELD_WORKSPACE", "sentinel")
        .env("FELD_HOME", "/tmp/feld-sentinel")
        .output()
        .unwrap();
    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stdout)
    );
    let shown = server.run(&["show-environment", "-g"]);
    let names = String::from_utf8_lossy(&shown.stdout)
        .lines()
        .filter_map(|line| line.split('=').next())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert!(names.iter().any(|name| name == "FELD_HOME"), "{names:?}");
    for name in [
        "FELD_SESSION_TOKEN",
        "FELD_SESSION_ID",
        "FELD_ACTOR",
        "FELD_WORKSPACE",
    ] {
        assert!(!names.iter().any(|shown| shown == name), "{names:?}");
    }
}

#[test]
#[ignore = "run by a_server_the_provider_starts_keeps_no_session_variables"]
fn launches_on_the_socket_it_is_given() {
    let socket = std::env::var("FELD_TEST_TMUX_SOCKET").unwrap();
    let server = TestServer {
        endpoint: TmuxEndpoint::Name(socket),
    };
    launch(
        &TmuxProvider::default(),
        prepare(&server, TmuxUnit::Session, None),
        "sentinel",
        &["sleep", "300"],
        vec![],
    );
    // The server belongs to the test that started this one.
    std::mem::forget(server);
}

#[test]
fn a_create_that_answers_nothing_by_the_deadline_may_have_created_its_resource() {
    let failed = |kind| {
        TmuxProviderError::Io(provider::CommandFailed {
            program: "tmux".to_owned(),
            source: std::io::Error::new(kind, "tmux"),
        })
    };
    assert_eq!(
        create_run_failure_kind(&failed(std::io::ErrorKind::TimedOut)),
        ObservationKind::Lost
    );
    assert_eq!(
        create_run_failure_kind(&failed(std::io::ErrorKind::NotFound)),
        ObservationKind::Gone
    );
}
