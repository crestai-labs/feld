//! A daemon of another version than `feld`: the commands that talk to it say so on stderr
//! in one line and go on, and `feld doctor` fails the check.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Output};

use fs2::FileExt;
use serde_json::{Value, json};
use tempfile::TempDir;

const FELD: &str = env!("CARGO_BIN_EXE_feld");
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A Feld home whose daemon lock is held by this test, and whose daemon record names a
/// stand-in that answers the status as the daemon of `version` and an empty Actor list.
struct StandIn {
    temp: TempDir,
    _lock: File,
}

impl StandIn {
    fn of_version(version: &str) -> Self {
        let temp = tempfile::tempdir().expect("create a temporary HOME");
        let home = temp.path().join("fh");
        fs::create_dir_all(&home).unwrap();
        let lock = File::create(home.join("daemon.lock")).unwrap();
        lock.lock_exclusive().expect("hold the daemon lock");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}", listener.local_addr().unwrap());
        let pid = std::process::id();
        fs::write(
            home.join("daemon.json"),
            json!({ "instanceId": "instance_test", "pid": pid, "api": api }).to_string(),
        )
        .unwrap();
        fs::write(home.join("api-token"), "token").unwrap();
        let status = json!({
            "state": "running",
            "version": format!("{version} (built 2026-01-01T00:00:00Z)"),
            "instanceId": "instance_test",
            "pid": pid,
            "home": home,
            "api": api,
            "startedAt": "2026-01-01T00:00:00Z",
            "serverTime": "2026-01-01T00:00:00Z",
            "historyStart": null,
            "timezone": "UTC",
            "providers": [],
            "nodes": ["local"],
            "webUi": null,
            "otlp": null,
        });
        std::thread::spawn(move || serve(listener, &status));
        Self { temp, _lock: lock }
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        Command::new(FELD)
            .arg("--home")
            .arg(self.temp.path().join("fh"))
            .env("HOME", self.temp.path())
            .env_remove("FELD_HOME")
            .env_remove("FELD_API")
            .env_remove("FELD_SESSION_TOKEN")
            .env_remove("FELD_WORKSPACE")
            .env_remove("FELD_ACTOR")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .args(arguments)
            .output()
            .expect("run feld")
    }
}

/// Answers each request on its own connection.
fn serve(listener: TcpListener, status: &Value) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        let _ = reader.read_line(&mut request_line);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
        }
        let path = request_line.split_whitespace().nth(1).unwrap_or_default();
        let body = if path.starts_with("/v1/daemon/status") {
            status.to_string()
        } else {
            json!({ "workspace": "default", "actors": [] }).to_string()
        };
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn warning(daemon: &str) -> String {
    format!("warning: daemon {daemon} differs from this CLI {VERSION} \u{b7} next: feld init\n")
}

#[test]
fn a_command_tells_the_daemon_of_another_version_and_goes_on_as_it_is() {
    let current = StandIn::of_version(VERSION);
    let older = StandIn::of_version("0.0.1");
    for arguments in [&["actor", "list"][..], &["actor", "list", "--json"]] {
        let expected = current.feld(arguments);
        let output = older.feld(arguments);
        assert_eq!(stderr(&expected), "", "{arguments:?}");
        assert_eq!(stderr(&output), warning("0.0.1"), "{arguments:?}");
        assert_eq!(
            output.status.code(),
            expected.status.code(),
            "{arguments:?}"
        );
        assert_eq!(output.stdout, expected.stdout, "{arguments:?}");
        assert_eq!(output.status.code(), Some(0));
    }
    let output = older.feld(&["daemon", "status"]);
    assert_eq!(stderr(&output), warning("0.0.1"));
}

#[test]
fn doctor_fails_the_daemon_of_another_version() {
    let older = StandIn::of_version("0.0.1");
    let output = older.feld(&["doctor", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stderr(&output), "", "doctor tells it as a check");
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    let daemon = answer["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "daemon")
        .unwrap();
    assert_eq!(daemon["result"], "fail", "{daemon}");
    assert_eq!(daemon["next"], "feld init", "{daemon}");
    let detail = daemon["detail"].as_str().unwrap();
    assert!(
        detail.starts_with("running \u{b7} 0.0.1 \u{b7} ")
            && detail.ends_with(&format!("differs from feld {VERSION}")),
        "{detail}"
    );

    let current = StandIn::of_version(VERSION);
    let output = current.feld(&["doctor", "--json"]);
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    let daemon = answer["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "daemon")
        .unwrap();
    assert_eq!(daemon["result"], "ok", "{daemon}");
}
