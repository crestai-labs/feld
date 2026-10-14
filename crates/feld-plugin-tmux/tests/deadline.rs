//! A tmux client that does not answer, met by core's deadline while the Provider waits on it.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use feld_core::plugin::invoke::{PluginEnvironment, PluginProcess};
use feld_core::plugin::manifest::EXECUTABLE_FILE;

fn executable(path: &Path, script: &str) {
    std::fs::write(
        path,
        format!("#!/bin/sh\n[ -z \"$FELD_TEST_WARM_UP\" ] || exit 0\n{script}\n"),
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    // macOS inspects an executable the first time it runs, which takes seconds while the
    // machine builds and runs others. Running it once here keeps that out of the deadline.
    let _ = std::process::Command::new(path)
        .env("FELD_TEST_WARM_UP", "1")
        .status();
}

#[test]
fn a_tmux_client_goes_with_the_plugin_core_kills_at_its_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let bin = directory.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let pids = directory.path().join("pids");
    executable(
        &bin.join("tmux"),
        &format!("sleep 300 &\necho $$ $! >> '{}'\nwait", pids.display()),
    );
    executable(
        &directory.path().join(EXECUTABLE_FILE),
        &format!(
            "PATH='{}':\"$PATH\" exec '{}' \"$@\"",
            bin.display(),
            env!("CARGO_BIN_EXE_feld-plugin-tmux")
        ),
    );
    let _ = std::process::Command::new(env!("CARGO_BIN_EXE_feld-plugin-tmux"))
        .stdin(std::process::Stdio::null())
        .output();
    let process = PluginProcess {
        name: "tmux".to_owned(),
        directory: directory.path().to_path_buf(),
        environment: PluginEnvironment {
            home: directory.path().to_path_buf(),
            api: None,
        },
        // Well inside the deadline the Provider gives one tmux command.
        deadline: Duration::from_secs(3),
    };
    let error = process
        .call_json(
            "candidates",
            &serde_json::json!({ "selector": { "socket": directory.path().join("socket") } }),
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("no answer within 3 seconds"),
        "{error:#}"
    );
    // What the Plugin started is no longer anyone's, so whoever adopts it reaps it a moment
    // later.
    let deadline = Instant::now() + Duration::from_secs(5);
    for pid in std::fs::read_to_string(&pids).unwrap().split_whitespace() {
        let pid: u32 = pid.parse().unwrap();
        while feld_core::process::field(pid, "pid=").unwrap().is_some() {
            assert!(Instant::now() < deadline, "process {pid} is still there");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
