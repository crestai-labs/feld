use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use serde_json::json;

use feld_core::plugin::sdk::provider::ProviderBackend;

use crate::backend::ProcessProvider;
use crate::supervisor::{ExitRecord, exit_code, read_record, record};

#[test]
fn the_options_are_an_empty_object_and_any_key_is_refused() {
    assert_eq!(
        ProcessProvider::resolve_options(None, None).unwrap(),
        json!({})
    );
    assert_eq!(
        ProcessProvider::resolve_options(Some(&json!({})), None).unwrap(),
        json!({})
    );
    assert_eq!(
        ProcessProvider::resolve_options(Some(&json!({"socket": "feld"})), None).unwrap_err(),
        "process does not take the option 'socket'"
    );
}

#[test]
fn a_signal_reads_as_128_and_its_number() {
    assert_eq!(exit_code(ExitStatus::from_raw(3 << 8)), 3);
    assert_eq!(exit_code(ExitStatus::from_raw(libc::SIGTERM)), 143);
    assert_eq!(exit_code(ExitStatus::from_raw(libc::SIGKILL)), 137);
}

#[test]
fn the_exit_record_is_read_back_whole_or_not_at_all() {
    let state = tempfile::tempdir().unwrap();
    assert_eq!(read_record(state.path()).unwrap(), None);
    record(state.path(), 7).unwrap();
    assert_eq!(
        read_record(state.path()).unwrap(),
        Some(ExitRecord { exit_code: 7 })
    );
}

#[test]
fn the_summary_names_the_program_pid() {
    assert_eq!(
        ProcessProvider::summary(&json!({"program": {"pid": 4812, "startTime": "x"}}), None),
        "process 4812"
    );
    assert_eq!(
        ProcessProvider::summary(&json!({"workdir": "/"}), None),
        "process"
    );
    assert_eq!(ProcessProvider::attach_command(&json!({}), None), None);
}

/// A launch that cannot read the program's start time ends the program and its supervisor,
/// rather than record a Placement nothing could stop.
#[test]
fn a_program_whose_start_time_cannot_be_read_is_ended_with_the_launch() {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use feld_core::plugin::protocol::ObservationKind;
    use feld_core::plugin::sdk::provider::LaunchRequest;

    use crate::backend::Prepared;

    let directory = tempfile::tempdir().unwrap();
    let pid_file = directory.path().join("program.pid");
    // A supervisor that starts a program in its own process group and answers its pid.
    let supervisor = directory.path().join("supervisor");
    std::fs::write(
        &supervisor,
        format!(
            "#!/bin/sh\ncat >/dev/null\nsleep 300 </dev/null >/dev/null 2>&1 &\necho $! > '{}'\necho \"{{\\\"pid\\\":$!}}\"\nwait\n",
            pid_file.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&supervisor, std::fs::Permissions::from_mode(0o755)).unwrap();
    let provider = ProcessProvider {
        executable: supervisor,
        state_root: directory.path().join("states"),
        settle_timeout: Duration::from_millis(200),
        program_incarnation: |_| Err(std::io::Error::other("ps was refused")),
    };
    let failure = provider
        .launch(LaunchRequest {
            prepared: Prepared {
                workdir: PathBuf::from("/"),
            },
            resource_name: "feld-default-job-1".to_owned(),
            command: vec!["true".to_owned()],
            env: Vec::new(),
        })
        .unwrap_err();
    assert_eq!(failure.observation.kind, ObservationKind::Gone);
    assert!(
        failure
            .source
            .contains("read the program's start time: ps was refused"),
        "{}",
        failure.source
    );
    let program: u32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while feld_core::process::field(program, "pid=")
        .unwrap()
        .is_some()
    {
        assert!(
            Instant::now() < deadline,
            "the program {program} is still running"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        std::fs::read_dir(directory.path().join("states"))
            .unwrap()
            .count(),
        0
    );
}
