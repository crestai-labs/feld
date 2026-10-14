//! `cli.md` Common conventions: the errors a first run meets carry the step a person takes next, on the
//! line after `Error:` and as `error.next` under `--json`, with the exit code they had.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

mod support;

struct Fixture {
    temp: TempDir,
}

impl Fixture {
    /// A Feld home with claude-code and the Providers `providers` installed, and no daemon.
    fn new(providers: &[&str]) -> Self {
        let fixture = Self {
            temp: tempfile::tempdir().expect("create temporary Feld home"),
        };
        match providers {
            ["tmux"] => support::install_plugins(fixture.home()),
            ["tmux", "herdr"] => support::install_plugins_with_herdr(fixture.home()),
            _ => unreachable!("no fixture installs {providers:?}"),
        }
        fixture
    }

    fn home(&self) -> &Path {
        self.temp.path()
    }

    /// `feld` outside every tmux and herdr pane, so that no Provider answers where it runs.
    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(self.home())
            .env("HOME", self.home())
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .args(arguments);
        for (variable, _) in std::env::vars_os() {
            if variable.to_string_lossy().starts_with("HERDR_") {
                command.env_remove(variable);
            }
        }
        command
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().expect("run feld")
    }

    /// Starts the daemon with `PATH` holding only a stand-in for each of `commands` beside
    /// the system directories, as a daemon started where the products are not installed.
    fn start_daemon_with_only(&self, commands: &[&str]) {
        let path = self.home().join("path");
        fs::create_dir_all(&path).unwrap();
        for command in commands {
            let stand_in = path.join(command);
            fs::write(&stand_in, "#!/bin/sh\nexit 1\n").unwrap();
            fs::set_permissions(&stand_in, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let output = self
            .command(&["daemon", "start"])
            .env("PATH", format!("{}:/usr/bin:/bin", path.display()))
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The `next:` line that follows the `Error:` line, after checking the exit code.
fn next_line(output: &Output, code: i32) -> String {
    let said = stderr(output);
    assert_eq!(output.status.code(), Some(code), "{said}");
    let lines: Vec<&str> = said.lines().collect();
    let error = lines
        .iter()
        .position(|line| line.starts_with("Error: "))
        .unwrap_or_else(|| panic!("no Error line: {said}"));
    lines
        .get(error + 1)
        .and_then(|line| line.strip_prefix("next: "))
        .unwrap_or_else(|| panic!("no next line after the Error line: {said}"))
        .to_owned()
}

/// `error.next` of a `--json` error, after checking the exit code.
fn json_next(output: &Output, code: i32) -> String {
    assert_eq!(output.status.code(), Some(code), "{}", stderr(output));
    let error: Value = serde_json::from_slice(&output.stderr).expect("a JSON error");
    error["error"]["next"]
        .as_str()
        .unwrap_or_else(|| panic!("no error.next: {error}"))
        .to_owned()
}

/// 1: With no daemon, `feld ui` and `feld actor list` name `feld daemon start`, and
/// `feld init` before there is a Feld home.
#[test]
fn a_stopped_daemon_points_at_daemon_start() {
    let fixture = Fixture::new(&["tmux"]);
    for arguments in [&["ui", "--print-url"][..], &["actor", "list"][..]] {
        let output = fixture.feld(arguments);
        assert!(stderr(&output).starts_with("Error: no Feld daemon is running for "));
        assert_eq!(next_line(&output, 2), "feld daemon start", "{arguments:?}");
        let json = fixture.feld(&[&["--json"][..], arguments].concat());
        assert_eq!(json_next(&json, 2), "feld daemon start", "{arguments:?}");
    }

    let nowhere = PathBuf::from(fixture.home()).join("not-a-home");
    let output = support::feld()
        .arg("--home")
        .arg(&nowhere)
        .env("HOME", fixture.home())
        .args(["actor", "list"])
        .output()
        .unwrap();
    assert_eq!(next_line(&output, 2), "feld init");
}

/// 3 and 4: A start whose Provider or Runtime is not on the daemon's `PATH` is refused,
/// naming the install, and for a Runtime its login.
#[test]
fn a_start_without_the_product_on_the_daemon_path_names_its_install() {
    let fixture = Fixture::new(&["tmux", "herdr"]);
    fixture.start_daemon_with_only(&["tmux"]);

    let output = fixture.feld(&[
        "actor",
        "create",
        "worker",
        "--kind",
        "agent",
        "--runtime",
        "claude-code",
        "--provider",
        "tmux",
    ]);
    assert!(
        stderr(&output).contains("`claude` is not on the PATH of the Feld daemon"),
        "{}",
        stderr(&output)
    );
    assert_eq!(
        next_line(&output, 1),
        "install Claude Code so that `claude` is on PATH, and log in with it"
    );

    let output = fixture.feld(&[
        "--json",
        "actor",
        "create",
        "job",
        "--kind",
        "job",
        "--start",
        "--provider",
        "herdr",
        "--",
        "true",
    ]);
    assert_eq!(
        json_next(&output, 1),
        "install herdr so that `herdr` is on PATH, or give the Actor another Provider with --provider"
    );
}

/// 5: A Workspace that is not there names `feld workspace create`.
#[test]
fn a_missing_workspace_names_workspace_create() {
    let fixture = Fixture::new(&["tmux"]);
    fixture.start_daemon_with_only(&["tmux"]);
    let expected = "feld workspace create missing, or choose one of `feld workspace list`";
    let output = fixture.feld(&["actor", "list", "--workspace", "missing"]);
    assert_eq!(next_line(&output, 1), expected);
    let output = fixture.feld(&["--json", "actor", "list", "--workspace", "missing"]);
    assert_eq!(json_next(&output, 1), expected);
}

/// 6 and 7: Two installed Providers and nothing to tell them apart name `--provider`, and an
/// attach of `--self` outside a pane names the pane to run it in.
#[test]
fn an_unresolved_provider_names_the_provider_or_the_pane() {
    let single = Fixture::new(&["tmux"]);
    single.start_daemon_with_only(&["tmux"]);
    let attach = ["actor", "attach", "--self", "--runtime", "claude-code"];
    let output = single.feld(&attach);
    assert!(
        stderr(&output).starts_with("Error: --self requires a tmux environment"),
        "{}",
        stderr(&output)
    );
    assert_eq!(
        next_line(&output, 1),
        "run it inside the tmux pane where Claude Code runs; `feld doctor` checks the rest"
    );

    let both = Fixture::new(&["tmux", "herdr"]);
    both.start_daemon_with_only(&["tmux", "herdr"]);
    let output = both.feld(&["actor", "create", "job", "--kind", "job", "--", "true"]);
    assert_eq!(
        next_line(&output, 1),
        "add `--provider herdr` or `--provider tmux`"
    );
    let output = both.feld(&[&["--json"][..], &attach].concat());
    assert_eq!(
        json_next(&output, 1),
        "run it inside the herdr or tmux pane where Claude Code runs; `feld doctor` checks the rest"
    );
}
