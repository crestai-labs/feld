//! `feld doctor`: one line per check of the environment Feld runs in, written nowhere, and
//! the same checks after a `feld init` that fails.

use std::collections::BTreeMap;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use serde_json::Value;
use tempfile::TempDir;

mod support;

use support::stand_in::write_stand_in;

const FELD: &str = env!("CARGO_BIN_EXE_feld");

/// The checks, in the order `feld doctor` runs them.
const CHECKS: [&str; 11] = [
    "executables",
    "home",
    "store",
    "daemon",
    "provider",
    "runtime",
    "runtime-settings",
    "sandbox",
    "shell",
    "collector",
    "browser",
];

struct Fixture {
    temp: TempDir,
}

impl Fixture {
    /// A `HOME` with no Feld home in it yet, and Claude Code, tmux, and a browser opener on
    /// `PATH`, each telling its version.
    fn new() -> Self {
        warm_up_plugin_executables();
        let fixture = Self {
            temp: tempfile::tempdir().expect("create a temporary HOME"),
        };
        fixture.with_on_path(&[
            ("claude", "echo '2.1.200 (Claude Code)'"),
            ("tmux", "echo 'tmux 3.6a'"),
            ("open", "exit 0"),
            ("xdg-open", "exit 0"),
        ]);
        fixture
    }

    /// The `HOME` of every `feld` the test runs.
    fn user_home(&self) -> &Path {
        self.temp.path()
    }

    /// The Feld home, a directory of `HOME` that `feld init` makes.
    fn home(&self) -> PathBuf {
        self.user_home().join("fh")
    }

    fn path(&self) -> PathBuf {
        self.user_home().join("path")
    }

    /// Puts a stand-in running `script` for each command, and nothing else, on `PATH`.
    fn with_on_path(&self, commands: &[(&str, &str)]) {
        let path = self.path();
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create the PATH directory");
        for (command, script) in commands {
            write_stand_in(&path.join(command), script);
        }
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        self.feld_with(arguments, &[])
    }

    fn feld_with(&self, arguments: &[&str], variables: &[(&str, &str)]) -> Output {
        Command::new(FELD)
            .arg("--home")
            .arg(self.home())
            // Nothing here may reach the real `~/.feld`, `~/.claude` or `~/.zshrc`.
            .env("HOME", self.user_home())
            .env("PATH", self.path())
            .env("SHELL", "/bin/zsh")
            .env_remove("ZDOTDIR")
            .env_remove("BROWSER")
            .env_remove("FELD_HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_HOME")
            .env_remove("NO_COLOR")
            .env_remove("CLICOLOR_FORCE")
            // Neither the Session nor the pane this test runs inside speaks for the fixture.
            .env_remove("FELD_API")
            .env_remove("FELD_SESSION_TOKEN")
            .env_remove("FELD_WORKSPACE")
            .env_remove("FELD_ACTOR")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("OTEL_EXPORTER_OTLP_ENDPOINT")
            .envs(variables.iter().copied())
            .args(arguments)
            .output()
            .expect("run feld")
    }

    fn succeeds(&self, arguments: &[&str]) -> String {
        let output = self.feld(arguments);
        assert!(
            output.status.success(),
            "feld {} failed:\nstdout: {}\nstderr: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// The checks of `feld doctor --json` by name, with the exit code.
    fn doctor_with(&self, variables: &[(&str, &str)]) -> (BTreeMap<String, Value>, i32) {
        let output = self.feld_with(&["doctor", "--json"], variables);
        let answer: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "feld doctor --json printed no JSON ({error}):\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        let checks = answer["checks"].as_array().expect("a list of checks");
        let names: Vec<&str> = checks
            .iter()
            .map(|check| check["name"].as_str().expect("a name"))
            .collect();
        assert_eq!(names, CHECKS);
        let checks = checks
            .iter()
            .map(|check| (check["name"].as_str().unwrap().to_owned(), check.clone()))
            .collect();
        (checks, output.status.code().expect("an exit code"))
    }

    fn doctor(&self) -> (BTreeMap<String, Value>, i32) {
        self.doctor_with(&[])
    }

    /// A Feld home with Claude Code and tmux installed and prepared, and no daemon.
    fn initialized(self) -> Self {
        self.succeeds(&["init", "--runtime", "claude-code", "--provider", "tmux"]);
        self
    }

    /// Everything under `HOME` but `PATH`, with its contents and modification times.
    fn snapshot(&self) -> BTreeMap<PathBuf, (Vec<u8>, std::time::SystemTime)> {
        fn walk(directory: &Path, into: &mut BTreeMap<PathBuf, (Vec<u8>, std::time::SystemTime)>) {
            let Ok(entries) = fs::read_dir(directory) else {
                return;
            };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path).expect("read an entry");
                let contents = if metadata.is_file() {
                    fs::read(&path).unwrap_or_default()
                } else {
                    Vec::new()
                };
                into.insert(path.clone(), (contents, metadata.modified().unwrap()));
                if metadata.is_dir() {
                    walk(&path, into);
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(&self.home(), &mut found);
        found
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
    }
}

/// Runs each Plugin executable beside `feld` once, the way `feld doctor` asks its version.
///
/// macOS checks a new executable on its first run, and each build of the workspace makes
/// these anew. Under the load of a build, that check outlasts how long `feld doctor` waits
/// for a version, so it happens here, before any test waits on it.
fn warm_up_plugin_executables() {
    static WARMED: OnceLock<()> = OnceLock::new();
    WARMED.get_or_init(|| {
        let beside = Path::new(FELD).parent().expect("feld sits in a directory");
        for entry in fs::read_dir(beside).expect("read the directory of feld") {
            let path = entry.expect("read an entry").path();
            let is_plugin = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("feld-plugin-") && !name.contains('.'));
            if is_plugin && path.is_file() {
                let _ = Command::new(&path).arg("--version").output();
            }
        }
    });
}

fn result(checks: &BTreeMap<String, Value>, name: &str) -> String {
    checks[name]["result"].as_str().unwrap().to_owned()
}

fn detail(checks: &BTreeMap<String, Value>, name: &str) -> String {
    checks[name]["detail"].as_str().unwrap().to_owned()
}

fn next(checks: &BTreeMap<String, Value>, name: &str) -> String {
    checks[name]["next"].as_str().unwrap_or_default().to_owned()
}

/// A port nothing listens on now.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn doctor_after_init_yes_shows_each_check_on_a_line_and_writes_nothing() {
    let fixture = Fixture::new();
    let init = fixture.succeeds(&["init", "--yes"]);
    assert!(
        init.lines()
            .any(|line| line.trim() == "note:  run `feld doctor` if something does not work"),
        "{init}"
    );
    let before = fixture.snapshot();
    let output = fixture.feld(&["doctor"]);
    let printed = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = printed
        .lines()
        .filter(|line| !line.trim_start().starts_with("next:"))
        .collect();
    assert_eq!(lines.len(), CHECKS.len(), "{printed}");
    for (line, name) in lines.iter().zip(CHECKS) {
        let words: Vec<&str> = line.split_whitespace().collect();
        assert!(
            ["ok", "warn", "fail"].contains(&words[0]),
            "{line} starts with no result"
        );
        assert_eq!(words[1], name, "{printed}");
    }
    assert_eq!(output.status.code(), Some(0), "{printed}");
    let (checks, code) = fixture.doctor();
    assert_eq!(code, 0);
    for name in [
        "executables",
        "home",
        "store",
        "daemon",
        "provider",
        "runtime",
    ] {
        assert_eq!(result(&checks, name), "ok", "{name}: {:?}", checks[name]);
    }
    assert_eq!(result(&checks, "runtime-settings"), "ok");
    assert_eq!(result(&checks, "collector"), "ok");
    assert_eq!(result(&checks, "browser"), "ok");
    assert_eq!(detail(&checks, "provider"), "tmux 3.6a");
    assert_eq!(detail(&checks, "runtime"), "claude 2.1.200 (Claude Code)");
    assert_eq!(detail(&checks, "store"), "open in the running daemon");
    assert!(detail(&checks, "daemon").starts_with("running"));
    // init added the `feld env` line, which this process has not read.
    assert_eq!(result(&checks, "shell"), "warn");
    assert!(next(&checks, "shell").starts_with("source "));
    assert_eq!(
        fixture.snapshot(),
        before,
        "feld doctor changed the Feld home"
    );
}

#[test]
fn doctor_before_init_names_init_and_leaves_no_feld_home() {
    let fixture = Fixture::new();
    let (checks, code) = fixture.doctor();
    assert_eq!(code, 1);
    assert_eq!(result(&checks, "home"), "fail");
    assert_eq!(next(&checks, "home"), "feld init");
    assert_eq!(result(&checks, "store"), "ok");
    assert_eq!(result(&checks, "daemon"), "warn");
    assert_eq!(result(&checks, "provider"), "fail");
    // With nothing installed, the executables beside feld are what init installs from.
    assert_eq!(result(&checks, "executables"), "ok");
    assert!(!fixture.home().exists(), "feld doctor made the Feld home");
}

#[test]
fn a_port_the_daemon_starts_on_held_by_another_process_fails_the_daemon_check() {
    let fixture = Fixture::new().initialized();
    let port = free_port();
    fixture.succeeds(&["daemon", "start", "--port", &port.to_string()]);
    fixture.succeeds(&["daemon", "stop"]);
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "daemon"), "warn");
    assert_eq!(next(&checks, "daemon"), "feld daemon start");
    let holder = TcpListener::bind(("127.0.0.1", port)).expect("hold the daemon port");
    let (checks, code) = fixture.doctor();
    assert_eq!(code, 1);
    assert_eq!(result(&checks, "daemon"), "fail");
    assert!(
        detail(&checks, "daemon").contains(&format!("port {port} is in use")),
        "{:?}",
        checks["daemon"]
    );
    assert!(
        next(&checks, "daemon").contains(&format!("lsof -nP -iTCP:{port} -sTCP:LISTEN"))
            && next(&checks, "daemon").contains("--port"),
        "{:?}",
        checks["daemon"]
    );

    // init runs the same checks once the daemon fails to start.
    let output = fixture.feld(&["init", "--yes"]);
    let printed = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{printed}");
    let (failed, checked) = printed
        .split_once("\nfeld doctor\n")
        .unwrap_or_else(|| panic!("init ran no checks:\n{printed}"));
    assert!(failed.contains("Feld is not ready."), "{printed}");
    assert!(
        checked.lines().any(|line| line.starts_with("fail  daemon")),
        "{printed}"
    );
    drop(holder);
}

#[test]
fn a_store_at_the_top_of_the_home_is_checked() {
    let fixture = Fixture::new().initialized();
    let store = fixture.home().join("feld.db");
    rusqlite::Connection::open(&store)
        .unwrap()
        .execute_batch("PRAGMA user_version = 99;")
        .unwrap();
    let (checks, code) = fixture.doctor();
    assert_eq!(code, 1);
    assert_eq!(result(&checks, "store"), "fail");
    assert!(
        next(&checks, "store").contains(&store.display().to_string()),
        "{:?}",
        checks["store"]
    );
}

#[test]
fn an_unsupported_store_fails_with_what_to_move_aside() {
    let fixture = Fixture::new().initialized();
    let store = fixture.home().join("admin/feld.db");
    let connection = rusqlite::Connection::open(&store).unwrap();
    connection
        .execute_batch("PRAGMA user_version = 99;")
        .unwrap();
    drop(connection);
    let (checks, code) = fixture.doctor();
    assert_eq!(code, 1);
    assert_eq!(result(&checks, "store"), "fail");
    assert!(
        detail(&checks, "store").contains("store schema version 99 is not supported"),
        "{:?}",
        checks["store"]
    );
    assert!(next(&checks, "store").contains("mv "));
}

#[test]
fn a_provider_missing_from_path_fails_and_an_old_tmux_too() {
    let fixture = Fixture::new().initialized();
    fixture.with_on_path(&[("claude", "echo 2.1.200"), ("open", "exit 0")]);
    let (checks, code) = fixture.doctor();
    assert_eq!(code, 1);
    assert_eq!(result(&checks, "provider"), "fail");
    assert_eq!(detail(&checks, "provider"), "tmux is not on PATH");
    fixture.with_on_path(&[
        ("claude", "echo 2.1.200"),
        ("tmux", "echo 'tmux 3.1c'"),
        ("open", "exit 0"),
    ]);
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "provider"), "fail");
    assert!(next(&checks, "provider").contains("tmux 3.2 or later"));
    fixture.with_on_path(&[
        ("claude", "echo 2.1.200"),
        ("tmux", "exit 1"),
        ("open", "exit 0"),
    ]);
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "provider"), "warn");
    assert_eq!(detail(&checks, "provider"), "tmux answers no version");
}

/// A Provider older than its oldest supported version fails, as one that cannot run a
/// Session does, and a Runtime as old warns, as a missing one does.
#[test]
fn a_product_older_than_its_oldest_supported_version_is_told() {
    let fixture = Fixture::new();
    fixture.succeeds(&[
        "init",
        "--yes",
        "--runtime",
        "claude-code",
        "--runtime",
        "codex",
        "--provider",
        "tmux",
        "--provider",
        "herdr",
    ]);
    let products = |claude: &str, codex: &str, tmux: &str, herdr: &str| {
        fixture.with_on_path(&[
            ("claude", &format!("echo '{claude} (Claude Code)'")),
            ("codex", &format!("echo 'codex-cli {codex}'")),
            ("tmux", &format!("echo 'tmux {tmux}'")),
            ("herdr", &format!("echo 'herdr {herdr}'")),
            ("open", "exit 0"),
        ]);
        fixture.doctor()
    };

    let (checks, code) = products("2.1.150", "0.155.1", "3.1c", "0.8.2");
    assert_eq!(code, 1);
    assert_eq!(result(&checks, "provider"), "fail");
    for told in [
        "tmux 3.1c is not supported; the tmux Provider needs tmux 3.2 or later",
        "herdr 0.8.2 is not supported; the herdr Provider needs herdr 0.9.0 or later",
    ] {
        assert!(
            detail(&checks, "provider").contains(told),
            "{:?}",
            checks["provider"]
        );
    }
    assert!(next(&checks, "provider").contains("install tmux 3.2 or later"));
    assert!(next(&checks, "provider").contains("install herdr 0.9.0 or later"));
    assert_eq!(result(&checks, "runtime"), "warn");
    for told in [
        "Claude Code 2.1.150 is not supported; the Claude Code Runtime Adapter needs Claude Code 2.1.181 or later",
        "Codex 0.155.1 is not supported; the Codex Runtime Adapter needs Codex 0.156.0 or later",
    ] {
        assert!(
            detail(&checks, "runtime").contains(told),
            "{:?}",
            checks["runtime"]
        );
    }
    assert!(next(&checks, "runtime").contains("install Claude Code 2.1.181 or later"));
    assert!(next(&checks, "runtime").contains("install Codex 0.156.0 or later"));

    let (checks, _) = products("2.1.181", "0.156.0", "3.2a", "0.9.0");
    assert_eq!(
        result(&checks, "provider"),
        "ok",
        "{:?}",
        checks["provider"]
    );
    assert_eq!(result(&checks, "runtime"), "ok", "{:?}", checks["runtime"]);
}

#[test]
fn a_runtime_missing_from_path_warns() {
    let fixture = Fixture::new().initialized();
    fixture.with_on_path(&[("tmux", "echo 'tmux 3.6a'"), ("open", "exit 0")]);
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "runtime"), "warn");
    assert_eq!(detail(&checks, "runtime"), "claude is not on PATH");
}

#[test]
fn runtime_settings_warn_once_what_init_prepared_is_gone() {
    let fixture = Fixture::new().initialized();
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "runtime-settings"), "ok");
    fs::remove_dir_all(fixture.user_home().join(".claude/skills/feld"))
        .expect("remove the Feld skill");
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "runtime-settings"), "warn");
    assert_eq!(
        next(&checks, "runtime-settings"),
        "feld init --runtime claude-code"
    );
}

#[test]
fn a_codex_sandbox_open_to_the_network_on_tmux_warns() {
    let fixture = Fixture::new().initialized();
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "sandbox"), "ok");
    assert_eq!(detail(&checks, "sandbox"), "codex is not installed");

    fixture.with_on_path(&[
        ("tmux", "echo 'tmux 3.6a'"),
        ("codex", "echo 'codex-cli 0.160.0'"),
    ]);
    fixture.succeeds(&["init", "--yes", "--runtime", "codex", "--provider", "tmux"]);
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "sandbox"), "ok", "{:?}", checks["sandbox"]);

    let config = fixture.user_home().join(".codex/config.toml");
    let prepared = fs::read_to_string(&config).expect("read config.toml");
    fs::write(
        &config,
        prepared.replace(
            "[sandbox_workspace_write]",
            "[sandbox_workspace_write]\nnetwork_access = true",
        ),
    )
    .expect("open the sandbox to the network");
    let before = fixture.snapshot();
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "sandbox"), "warn");
    assert!(
        detail(&checks, "sandbox").contains("network_access = true on tmux"),
        "{:?}",
        checks["sandbox"]
    );
    assert!(
        next(&checks, "sandbox")
            .starts_with("set sandbox_workspace_write.network_access = false in "),
        "{:?}",
        checks["sandbox"]
    );
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn an_installed_plugin_without_its_executable_beside_feld_fails() {
    let fixture = Fixture::new().initialized();
    // A feld copied alone has no Plugin executables beside it.
    let lone = fixture.user_home().join("lone");
    fs::create_dir_all(&lone).unwrap();
    fs::copy(FELD, lone.join("feld")).unwrap();
    let output = Command::new(lone.join("feld"))
        .args(["--home"])
        .arg(fixture.home())
        .args(["doctor", "--json"])
        .env("HOME", fixture.user_home())
        .env("PATH", fixture.path())
        .env("SHELL", "/bin/zsh")
        .output()
        .expect("run feld");
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    let executables = &answer["checks"][0];
    assert_eq!(executables["result"], "fail", "{executables}");
    assert!(
        executables["detail"]
            .as_str()
            .unwrap()
            .contains("feld-plugin-claude-code, codex, tmux, herdr missing"),
        "{executables}"
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn the_shell_check_reads_the_settings_file_and_this_environment() {
    let fixture = Fixture::new().initialized();
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "shell"), "warn");
    assert!(next(&checks, "shell").starts_with("add `eval \"$(feld env)\"` to "));
    fs::write(fixture.user_home().join(".zshrc"), "eval \"$(feld env)\"\n").unwrap();
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "shell"), "warn");
    assert!(next(&checks, "shell").starts_with("source "));
    let home = fixture.home().display().to_string();
    let (checks, _) = fixture.doctor_with(&[("FELD_HOME", &home)]);
    assert_eq!(result(&checks, "shell"), "ok", "{:?}", checks["shell"]);
    let (checks, _) = fixture.doctor_with(&[("SHELL", "/bin/fish")]);
    assert_eq!(result(&checks, "shell"), "warn");
    assert_eq!(detail(&checks, "shell"), "fish is not zsh or bash");
}

#[test]
fn a_collector_port_in_use_warns() {
    let fixture = Fixture::new().initialized();
    let holder = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", holder.local_addr().unwrap());
    fs::write(fixture.home().join("collector-endpoint"), &endpoint).unwrap();
    let (checks, code) = fixture.doctor();
    assert_eq!(result(&checks, "collector"), "warn");
    assert_eq!(
        detail(&checks, "collector"),
        format!("{endpoint} is in use")
    );
    assert_eq!(code, 0, "a warning leaves the exit code 0");
    drop(holder);
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "collector"), "ok");
}

#[test]
fn the_browser_check_follows_what_feld_ui_opens() {
    let fixture = Fixture::new();
    let (checks, _) = fixture.doctor_with(&[("BROWSER", "/nowhere/browser")]);
    assert_eq!(result(&checks, "browser"), "warn");
    assert!(next(&checks, "browser").contains("feld ui --print-url"));
    let (checks, _) = fixture.doctor_with(&[("BROWSER", "open")]);
    assert_eq!(result(&checks, "browser"), "ok");
    assert_eq!(detail(&checks, "browser"), "BROWSER=open");
    fixture.with_on_path(&[("tmux", "echo 'tmux 3.6a'")]);
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "browser"), "warn");
}

#[test]
fn init_runs_no_checks_for_a_plugin_feld_does_not_ship() {
    let fixture = Fixture::new();
    let output = fixture.feld(&["init", "--yes", "--runtime", "nope"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("feld doctor"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn init_runs_the_checks_when_no_provider_is_on_path() {
    let fixture = Fixture::new();
    fixture.with_on_path(&[("claude", "echo 2.1.200")]);
    let output = fixture.feld(&["init", "--yes"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Execution Provider is on PATH"));
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(printed.contains("feld doctor\n"), "{printed}");
    assert!(
        printed
            .lines()
            .any(|line| line.starts_with("fail  provider")),
        "{printed}"
    );
    let output = fixture.feld(&["init", "--yes", "--json"]);
    let answer: Value = serde_json::from_slice(&output.stdout).expect("the checks as JSON");
    assert_eq!(answer["checks"].as_array().unwrap().len(), CHECKS.len());
}

#[test]
fn a_store_the_daemon_cannot_read_fails_and_a_readable_one_stays_untouched() {
    let fixture = Fixture::new().initialized();
    fixture.succeeds(&["daemon", "start"]);
    fixture.succeeds(&["daemon", "stop"]);
    let store = fixture.home().join("admin/feld.db");
    let wal = fixture.home().join("admin/feld.db-wal");
    let shm = fixture.home().join("admin/feld.db-shm");
    assert!(!wal.exists() && !shm.exists());
    let before = fixture.snapshot();
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "store"), "ok", "{:?}", checks["store"]);
    assert_eq!(
        fixture.snapshot(),
        before,
        "feld doctor wrote beside the store"
    );

    // A store cut after its header still names schema 11 there.
    let contents = fs::read(&store).unwrap();
    fs::write(&store, &contents[..100]).unwrap();
    let (checks, code) = fixture.doctor();
    assert_eq!(code, 1);
    assert_eq!(result(&checks, "store"), "fail", "{:?}", checks["store"]);
    assert!(next(&checks, "store").contains("mv "));
    assert!(!wal.exists() && !shm.exists());
}

/// A store in WAL mode whose log holds schema 99 over the schema 11 of its file, as a writer
/// that ended without a checkpoint leaves it, with the shared memory file removed.
fn store_with_an_unread_log(fixture: &Fixture) -> rusqlite::Connection {
    let store = fixture.home().join("admin/feld.db");
    let connection = rusqlite::Connection::open(&store).unwrap();
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL; CREATE TABLE kept (value INTEGER);
             PRAGMA user_version = 11; PRAGMA wal_checkpoint(TRUNCATE);
             PRAGMA wal_autocheckpoint = 0; PRAGMA user_version = 99;",
        )
        .unwrap();
    assert!(
        fs::metadata(fixture.home().join("admin/feld.db-wal"))
            .unwrap()
            .len()
            > 0
    );
    fs::remove_file(fixture.home().join("admin/feld.db-shm")).unwrap();
    connection
}

#[test]
fn a_store_whose_log_the_daemon_has_not_read_back_is_not_checked_and_not_written() {
    let fixture = Fixture::new().initialized();
    let connection = store_with_an_unread_log(&fixture);
    let before = fixture.snapshot();
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "store"), "warn", "{:?}", checks["store"]);
    assert!(next(&checks, "store").starts_with("feld daemon start"));
    assert!(!fixture.home().join("admin/feld.db-shm").exists());
    assert_eq!(
        fixture.snapshot(),
        before,
        "feld doctor wrote beside the store"
    );
    drop(connection);
}

#[test]
fn a_store_behind_a_daemon_lock_nobody_answers_for_is_not_checked() {
    use fs2::FileExt as _;

    let fixture = Fixture::new().initialized();
    let connection = store_with_an_unread_log(&fixture);
    // The process holding the daemon lock is no daemon that answers.
    let lock = fs::File::create(fixture.home().join("daemon.lock")).unwrap();
    lock.lock_exclusive().unwrap();
    let before = fixture.snapshot();
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "store"), "warn", "{:?}", checks["store"]);
    assert_eq!(
        next(&checks, "store"),
        "fix the daemon first, and run `feld doctor` again"
    );
    assert!(!fixture.home().join("admin/feld.db-shm").exists());
    assert_eq!(
        fixture.snapshot(),
        before,
        "feld doctor wrote beside the store"
    );
    lock.unlock().unwrap();
    drop(connection);
}

/// Inside a sandbox like Codex's `workspace-write`, which denies TCP to loopback and writes
/// to the Feld home but its spool, `feld doctor` names the daemon holding the lock, warns
/// that it could not check it or the Collector, and writes no spool request.
#[cfg(target_os = "macos")]
#[test]
fn doctor_inside_a_sandbox_warns_what_it_cannot_check_without_the_spool() {
    let fixture = Fixture::new().initialized();
    fixture.succeeds(&["daemon", "start"]);
    let home = fixture.home().canonicalize().unwrap();
    let spool = home.join("spool");
    let listing = || -> Vec<PathBuf> {
        let mut names: Vec<PathBuf> = fs::read_dir(&spool)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    };
    let before = listing();
    let profile = format!(
        "(version 1)(allow default)\
         (deny network-outbound (remote ip \"localhost:*\"))\
         (deny file-write* (subpath \"{}\"))\
         (allow file-write* (subpath \"{}\"))",
        home.display(),
        spool.display()
    );
    let output = Command::new("/usr/bin/sandbox-exec")
        .args(["-p", &profile, FELD, "--home"])
        .arg(&home)
        .args(["doctor", "--json"])
        .env("HOME", fixture.user_home())
        .env("PATH", fixture.path())
        .env("SHELL", "/bin/zsh")
        .env_remove("FELD_HOME")
        .env_remove("FELD_API")
        .env_remove("FELD_SESSION_TOKEN")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .output()
        .expect("run feld in a sandbox");
    let answer: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("feld doctor in a sandbox: {output:?}"));
    let check = |name: &str| {
        answer["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["name"] == name)
            .unwrap()
            .clone()
    };
    let daemon = check("daemon");
    assert_eq!(daemon["result"], "warn", "{daemon}");
    assert!(
        daemon["detail"]
            .as_str()
            .unwrap()
            .contains("holds the daemon lock"),
        "{daemon}"
    );
    assert_eq!(
        daemon["next"],
        "run `feld doctor` outside the sandbox to check it"
    );
    let collector = check("collector");
    assert_eq!(collector["result"], "warn", "{collector}");
    assert_eq!(collector["next"], daemon["next"]);
    assert_eq!(check("store")["result"], "warn");
    assert_eq!(listing(), before, "feld doctor wrote to the spool");
}

/// An installed shipped Plugin of another version than `feld` fails the home check, which
/// `feld init` brings to this version.
#[test]
fn an_installed_plugin_of_another_version_fails_the_home_check() {
    let fixture = Fixture::new().initialized();
    let (checks, _) = fixture.doctor();
    assert_eq!(result(&checks, "home"), "ok", "{:?}", checks["home"]);
    let directory = fixture.home().join("plugins/provider/tmux");
    write_stand_in(&directory.join("plugin"), "echo 'feld-plugin-tmux 0.0.1'");
    let (checks, code) = fixture.doctor();
    assert_eq!(code, 1);
    assert_eq!(result(&checks, "home"), "fail");
    assert_eq!(next(&checks, "home"), "feld init");
    assert!(
        detail(&checks, "home").ends_with(&format!(
            "feld-plugin-tmux 0.0.1 differs from feld {}",
            env!("CARGO_PKG_VERSION")
        )),
        "{:?}",
        checks["home"]
    );
}
