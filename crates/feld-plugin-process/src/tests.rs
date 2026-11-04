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
