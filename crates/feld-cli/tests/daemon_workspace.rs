use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use chrono::SubsecRound;
use fs2::FileExt;
use serde_json::{Value, json};
use tempfile::TempDir;

mod support;

struct Fixture {
    temp: TempDir,
    timezone: Option<String>,
}

impl Fixture {
    fn new() -> Self {
        Self::build(None)
    }

    /// A fixture whose daemon and CLI both run in one named timezone, so that the
    /// human-readable output has a timezone the test knows.
    fn in_timezone(timezone: &str) -> Self {
        Self::build(Some(timezone.to_owned()))
    }

    fn build(timezone: Option<String>) -> Self {
        let fixture = Self {
            temp: tempfile::tempdir().expect("create temporary Feld home"),
            timezone,
        };
        support::install_plugins(fixture.home());
        fixture
    }

    fn home(&self) -> &Path {
        self.temp.path()
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().expect("run feld")
    }

    fn spawn(&self, arguments: &[&str]) -> Child {
        self.command(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn feld")
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = support::feld();
        // The fixture owns HOME too, so nothing this runs reads or writes the real one.
        command
            .arg("--home")
            .arg(self.home())
            .env("HOME", self.home())
            .args(arguments);
        if let Some(timezone) = &self.timezone {
            command.env("TZ", timezone);
        }
        command
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
        serde_json::from_slice(&output.stdout).expect("parse feld JSON output")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
    }
}

#[test]
fn daemon_and_workspaces_survive_a_restart_through_the_real_api() {
    let fixture = Fixture::new();
    let unreachable = fixture.feld(&["workspace", "list"]);
    assert_eq!(unreachable.status.code(), Some(2));

    let started = fixture.json(&["--json", "daemon", "start"]);
    assert_eq!(started["alreadyRunning"], false);
    assert_eq!(started["daemon"]["state"], "running");
    assert!(
        started["daemon"]["api"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1:")
    );
    assert!(
        started["daemon"]["otlp"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1:")
    );
    let second_start = fixture.json(&["daemon", "start", "--json"]);
    assert_eq!(second_start["alreadyRunning"], true);
    assert_eq!(
        second_start["daemon"]["instanceId"],
        started["daemon"]["instanceId"]
    );

    let home_mode = fs::metadata(fixture.home()).unwrap().permissions().mode() & 0o777;
    let token_mode = fs::metadata(fixture.home().join("admin/api-token"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(home_mode, 0o700);
    assert_eq!(token_mode, 0o600);

    // The daemon answers on the socket of the Feld home as well, the one place the parts
    // beside a Runtime send a Session token to.
    let socket = fixture.home().join("daemon.sock");
    let socket_mode = fs::metadata(&socket).unwrap().permissions().mode() & 0o777;
    assert_eq!(socket_mode, 0o600);
    let over_socket: Value = reqwest::blocking::Client::builder()
        .unix_socket(socket.as_path())
        .build()
        .unwrap()
        .get(format!(
            "{}/v1/daemon/status",
            started["daemon"]["api"].as_str().unwrap()
        ))
        .bearer_auth(
            fs::read_to_string(fixture.home().join("admin/api-token"))
                .unwrap()
                .trim(),
        )
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(over_socket["instanceId"], started["daemon"]["instanceId"]);

    let initial = fixture.json(&["workspace", "list", "--json"]);
    assert_eq!(initial["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(initial["workspaces"][0]["key"], "default");
    let default_created_at = initial["workspaces"][0]["createdAt"].clone();

    let created = fixture.json(&["--json", "workspace", "create", "research"]);
    assert_eq!(created["workspace"]["key"], "research");
    let research_created_at = created["workspace"]["createdAt"].clone();
    let first_token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();

    let rejected = fixture.feld(&["workspace", "create", "research"]);
    assert_eq!(rejected.status.code(), Some(1));
    let invalid = fixture.feld(&["workspace", "create", "Invalid"]);
    assert_eq!(invalid.status.code(), Some(1));

    let first_instance = started["daemon"]["instanceId"].clone();
    let restarted = fixture.json(&["daemon", "restart", "--json"]);
    assert_eq!(restarted["stopped"], true);
    assert_ne!(restarted["daemon"]["instanceId"], first_instance);
    assert_eq!(restarted["daemon"]["otlp"], started["daemon"]["otlp"]);
    // A restart keeps the local token, and a stop followed by a start issues a new one.
    let second_token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    assert_eq!(second_token, first_token);
    fixture.json(&["daemon", "stop", "--json"]);
    assert!(!socket.exists(), "a stopped daemon leaves no socket");
    fixture.json(&["daemon", "start", "--json"]);
    let third_token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    assert_ne!(third_token, first_token);

    let printed = fixture.feld(&["daemon", "restart"]);
    assert!(printed.status.success());
    let printed = String::from_utf8(printed.stdout).unwrap();
    let mut lines = printed.lines();
    assert_eq!(
        lines.next(),
        Some("Daemon stopped. Sessions keep running on their providers.")
    );
    assert_eq!(lines.next(), Some("Daemon started."));

    let restored = fixture.json(&["--json", "workspace", "list"]);
    assert_eq!(restored["workspaces"].as_array().unwrap().len(), 2);
    assert_eq!(restored["workspaces"][0]["createdAt"], default_created_at);
    assert_eq!(restored["workspaces"][1]["createdAt"], research_created_at);
}

/// `auth.md` local token: a restart with no daemon to stop issues a new local token rather
/// than keep the one a daemon that died left behind.
#[test]
fn a_restart_without_a_running_daemon_issues_a_new_local_token() {
    let fixture = Fixture::new();
    let started = fixture.json(&["daemon", "start", "--json"]);
    let left = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let pid = started["daemon"]["pid"].as_u64().unwrap().to_string();
    assert!(
        Command::new("kill")
            .args(["-9", &pid])
            .status()
            .unwrap()
            .success()
    );
    let mut gone = false;
    for _ in 0..200 {
        if fixture.feld(&["daemon", "status"]).status.code() == Some(1) {
            gone = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(gone, "the killed daemon still answered");
    assert_eq!(
        fs::read_to_string(fixture.home().join("admin/api-token")).unwrap(),
        left
    );

    let restarted = fixture.json(&["daemon", "restart", "--json"]);
    assert_eq!(restarted["stopped"], false);
    let issued = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    assert_ne!(issued, left);
}

/// `cli.md` daemon start: a port named with `--port` stays the daemon's loopback port across
/// stop, start, and restart, and a port another process holds fails the start or restart.
#[test]
fn a_named_port_stays_the_daemon_port_and_a_held_port_fails() {
    let fixture = Fixture::new();
    let free_port = || {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let port = free_port();
    let api = format!("http://127.0.0.1:{port}");
    let started = fixture.json(&["--json", "daemon", "start", "--port", &port.to_string()]);
    assert_eq!(started["daemon"]["api"], api);
    let restarted = fixture.json(&["--json", "daemon", "restart"]);
    assert_eq!(restarted["daemon"]["api"], api);
    fixture.json(&["--json", "daemon", "stop"]);
    let again = fixture.json(&["--json", "daemon", "start"]);
    assert_eq!(again["daemon"]["api"], api);

    let other = free_port();
    let running_elsewhere = fixture.feld(&["daemon", "start", "--port", &other.to_string()]);
    assert_eq!(running_elsewhere.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&running_elsewhere.stderr)
            .contains(&format!("feld daemon restart --port {other}"))
    );

    // A process listening on every address holds the loopback port too.
    let holder = TcpListener::bind(("0.0.0.0", other)).unwrap();
    let held = fixture.feld(&["daemon", "restart", "--port", &other.to_string()]);
    assert_eq!(held.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&held.stderr).contains(&format!("port {other} is in use")));
    let status = fixture.json(&["--json", "daemon", "status"]);
    assert_eq!(status["api"], api);
    assert_eq!(status["instanceId"], again["daemon"]["instanceId"]);

    let moved = fixture.json(&[
        "--json",
        "daemon",
        "restart",
        "--port",
        &free_port().to_string(),
    ]);
    fixture.json(&["--json", "daemon", "stop"]);
    let moved_port = moved["daemon"]["api"]
        .as_str()
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .parse::<u16>()
        .unwrap();
    drop(holder);
    let blocker = TcpListener::bind(("127.0.0.1", moved_port)).unwrap();
    let blocked = fixture.feld(&["daemon", "start"]);
    assert_eq!(blocked.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&blocked.stderr).contains(&format!("port {moved_port} is in use"))
    );
    drop(blocker);
}

/// `cli.md` daemon start: the daemon answers on `[::1]` and `localhost` at its port as on
/// `127.0.0.1`, so no other process takes the name, and a start fails while another
/// process holds the port on `[::1]`.
#[test]
fn the_daemon_answers_on_every_loopback_address_and_a_port_held_on_ipv6_fails() {
    // A host without IPv6 has no `[::1]` to answer on; any other failure is the test's.
    if let Err(error) = TcpListener::bind(("::1", 0))
        && (error.kind() == std::io::ErrorKind::AddrNotAvailable
            || matches!(
                error.raw_os_error(),
                Some(libc::EAFNOSUPPORT | libc::EPROTONOSUPPORT | libc::EADDRNOTAVAIL)
            ))
    {
        return;
    }
    let fixture = Fixture::new();
    let started = fixture.json(&["--json", "daemon", "start"]);
    let api = started["daemon"]["api"].as_str().unwrap();
    let port = api.rsplit(':').next().unwrap().to_owned();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let client = reqwest::blocking::Client::new();
    for host in ["127.0.0.1", "[::1]", "localhost"] {
        let status: Value = client
            .get(format!("http://{host}:{port}/v1/daemon/status"))
            .bearer_auth(token.trim())
            .send()
            .unwrap_or_else(|error| panic!("call the API on {host}: {error}"))
            .error_for_status()
            .unwrap_or_else(|error| panic!("the API on {host} answers: {error}"))
            .json()
            .expect("decode daemon status");
        assert_eq!(
            status["instanceId"], started["daemon"]["instanceId"],
            "{host}"
        );
    }
    fixture.json(&["--json", "daemon", "stop"]);

    let holder = TcpListener::bind(("::1", port.parse::<u16>().unwrap())).unwrap();
    let held = fixture.feld(&["daemon", "start", "--port", &port]);
    assert_eq!(held.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&held.stderr).contains(&format!("port {port} is in use on ::1")),
        "{}",
        String::from_utf8_lossy(&held.stderr)
    );
    drop(holder);
}

/// `cli.md` daemon start: the default Feld home, `~/.feld`, listens on the default port
/// 7381 and fails while another process holds it, and another home, as every other test
/// uses, leaves that port alone. A restart keeps the port of the daemon it stops, as one
/// that listened before the default port did.
#[test]
fn the_default_home_listens_on_the_default_port_and_others_leave_it() {
    let fixture = Fixture::new();
    let default_home = fixture.home().join(".feld");
    let in_default_home = |arguments: &[&str]| {
        support::feld()
            .arg("--home")
            .arg(&default_home)
            .env("HOME", fixture.home())
            .args(arguments)
            .output()
            .expect("run feld")
    };
    // A daemon a person runs on this host may hold the port already; it fails the start
    // all the same. A listener on the wildcard address holds the loopback port too.
    let holder = feld_daemon::daemon::port_free(7381)
        .is_ok_and(|free| free)
        .then(|| TcpListener::bind(("0.0.0.0", 7381)).ok())
        .flatten();

    let isolated = fixture.json(&["--json", "daemon", "start"]);
    assert_ne!(isolated["daemon"]["api"], "http://127.0.0.1:7381");

    let held = in_default_home(&["daemon", "start"]);
    assert_eq!(held.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&held.stderr);
    assert!(stderr.contains("port 7381 is in use"), "{stderr}");
    assert!(stderr.contains("--port"), "{stderr}");

    let Some(holder) = holder else {
        return;
    };
    drop(holder);
    let started = in_default_home(&["--json", "daemon", "start"]);
    let restarted = in_default_home(&["--json", "daemon", "restart"]);
    let stopped = in_default_home(&["daemon", "stop"]);
    for output in [&started, &restarted, &stopped] {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let started: Value = serde_json::from_slice(&started.stdout).unwrap();
    let restarted: Value = serde_json::from_slice(&restarted.stdout).unwrap();
    assert_eq!(started["daemon"]["api"], "http://127.0.0.1:7381");
    assert_eq!(restarted["daemon"]["api"], "http://127.0.0.1:7381");

    // A daemon listening on another port without one named, as one that a version before
    // the default port started, keeps that port across a restart.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let api = format!("http://127.0.0.1:{port}");
    assert!(
        in_default_home(&["daemon", "start", "--port", &port.to_string()])
            .status
            .success()
    );
    let record_path = default_home.join("daemon.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    record.as_object_mut().unwrap().remove("port");
    fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    let carried = in_default_home(&["--json", "daemon", "restart"]);
    assert!(carried.status.success());
    let carried: Value = serde_json::from_slice(&carried.stdout).unwrap();
    assert_eq!(carried["daemon"]["api"], api);
    assert!(in_default_home(&["daemon", "stop"]).status.success());
    let moved = in_default_home(&["--json", "daemon", "start"]);
    let stopped = in_default_home(&["daemon", "stop"]);
    assert!(stopped.status.success());
    let moved: Value = serde_json::from_slice(&moved.stdout).unwrap();
    assert_eq!(moved["daemon"]["api"], "http://127.0.0.1:7381");
}

/// `daemon-api.md` name grammar: the CLI accepts every Workspace key the daemon accepts.
#[test]
fn workspace_option_follows_the_daemon_key_grammar() {
    let fixture = Fixture::new();
    fixture.json(&["daemon", "start", "--json"]);
    fixture.json(&["--json", "workspace", "create", "a-"]);
    let listed = fixture.json(&["--json", "--workspace", "a-", "actor", "list"]);
    assert_eq!(listed["actors"].as_array().unwrap().len(), 0);

    for option in ["--workspace=A", "--workspace=-a"] {
        let rejected = fixture.feld(&[option, "actor", "list"]);
        assert_eq!(rejected.status.code(), Some(2), "{option}");
    }

    // An empty FELD_WORKSPACE reads as unset, so the Workspace falls back to `default`.
    let workspace_of = |value: &str| {
        let output = fixture
            .command(&["actor", "list"])
            .env("FELD_WORKSPACE", value)
            .output()
            .expect("run feld");
        assert!(output.status.success(), "FELD_WORKSPACE={value:?}");
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_owned()
    };
    assert_eq!(workspace_of(""), "Workspace: default");
    assert_eq!(workspace_of("a-"), "Workspace: a-");
}

/// `cli.md`: a bare `feld` shows the help, which leaves the hidden internal commands out,
/// even when the environment or `--json` gives it an option.
#[test]
fn a_bare_feld_shows_the_help_without_the_internal_commands() {
    let fixture = Fixture::new();
    let bare = |arguments: &[&str]| {
        support::feld()
            .args(arguments)
            .env("FELD_HOME", fixture.home())
            .env("HOME", fixture.home())
            .env("FELD_WORKSPACE", "default")
            .output()
            .expect("run feld")
    };
    for arguments in [&[][..], &["--workspace", "default"][..]] {
        let output = bare(arguments);
        assert_eq!(output.status.code(), Some(2), "feld {arguments:?}");
        assert!(output.stdout.is_empty());
        let help = String::from_utf8(output.stderr).unwrap();
        assert!(help.contains("Usage: feld [OPTIONS] <COMMAND>"), "{help}");
        assert!(help.contains("actor"), "{help}");
        for internal in ["collector-run", "program-capture"] {
            assert!(!help.contains(internal), "{help}");
        }
    }
    let output = bare(&["--json"]);
    assert_eq!(output.status.code(), Some(2));
    let error: Value = serde_json::from_slice(&output.stderr).expect("parse JSON error");
    assert_eq!(error["error"]["kind"], "operational");
    assert!(
        !error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("collector-run")
    );
}

/// The Usage line of an argument error names what the command line gave, so an option the
/// environment fills stays out of it, in the JSON message too. A value the environment gives
/// that the option refuses is still the error.
#[test]
fn an_argument_error_leaves_the_environment_out_of_its_usage() {
    let fixture = Fixture::new();
    let run = |arguments: &[&str], environment: &[(&str, &str)]| {
        let output = support::feld()
            .args(arguments)
            .env("FELD_HOME", fixture.home())
            .env("HOME", fixture.home())
            .env("FELD_WORKSPACE", "default")
            .env("FELD_API_PORT", "4000")
            .envs(environment.iter().copied())
            .output()
            .expect("run feld");
        assert_eq!(output.status.code(), Some(2), "feld {arguments:?}");
        assert!(output.stdout.is_empty());
        output.stderr
    };
    for (arguments, usage) in [
        (&["actor", "get"][..], "Usage: feld actor get <ACTOR>\n"),
        (
            &["session", "log", "x", "--tail", "1", "--after", "1"][..],
            "Usage: feld session log --tail <N> <TARGET>\n",
        ),
        (
            &["daemon", "start", "x"][..],
            "Usage: feld daemon start [OPTIONS]\n",
        ),
    ] {
        let human = String::from_utf8(run(arguments, &[])).unwrap();
        let json: Value = serde_json::from_slice(&run(&[&["--json"], arguments].concat(), &[]))
            .expect("parse JSON error");
        let json = json["error"]["message"].as_str().unwrap().to_owned();
        for message in [human, json] {
            assert!(message.contains(usage), "{message}");
            for option in ["--home", "--workspace", "--port"] {
                assert!(!message.contains(option), "{message}");
            }
        }
    }

    for (arguments, environment, error) in [
        (
            &["actor", "get", "a"][..],
            ("FELD_WORKSPACE", "bad key!"),
            "invalid value 'bad key!' for '--workspace <KEY>'",
        ),
        (
            &["daemon", "start"][..],
            ("FELD_API_PORT", "0"),
            "invalid value '0' for '--port <PORT>'",
        ),
    ] {
        let message = String::from_utf8(run(arguments, &[environment])).unwrap();
        assert!(message.contains(error), "{message}");
    }
}

/// `cli.md`: `--bucket` shapes the series `--json` prints, so it is an argument error without
/// `--json` or below 60 seconds, whichever way the output is asked for.
#[test]
fn bucket_is_an_argument_error_without_json_or_below_a_minute() {
    let fixture = Fixture::new();
    for command in [
        &["session", "metrics", "hello"][..],
        &["runtime", "metrics"][..],
    ] {
        for extra in [
            &["--bucket", "60"][..],
            &["--bucket", "5"][..],
            &["--json", "--bucket", "5"][..],
            &["--json", "--bucket", "abc"][..],
        ] {
            let arguments = [command, extra].concat();
            let output = fixture.feld(&arguments);
            assert_eq!(output.status.code(), Some(2), "feld {arguments:?}");
        }
    }

    // `--json` before the subcommand counts as well as after it.
    fixture.json(&["daemon", "start", "--json"]);
    for arguments in [
        &["--json", "runtime", "metrics", "--bucket", "60"][..],
        &["runtime", "metrics", "--json", "--bucket", "60"][..],
    ] {
        let metrics = fixture.json(arguments);
        assert!(metrics["runtimes"].is_array(), "feld {arguments:?}");
    }
}

/// `glossary.md`: a Workspace with a live Actor, and the default Workspace, refuse `delete`;
/// a deleted key names a new Workspace, and the deleted one keeps its own history.
#[test]
fn workspace_delete_refuses_live_actors_and_frees_the_key() {
    let fixture = Fixture::new();
    fixture.json(&["daemon", "start", "--json"]);
    let first = fixture.json(&["--json", "workspace", "create", "research"]);
    let first_id = first["workspace"]["id"].as_str().unwrap().to_owned();
    let research = |arguments: &[&str]| {
        let mut all = vec!["--workspace", "research"];
        all.extend_from_slice(arguments);
        fixture.feld(&all)
    };
    let worker = research(&[
        "--json", "actor", "create", "worker", "--kind", "job", "--", "true",
    ]);
    assert!(
        worker.status.success(),
        "{}",
        String::from_utf8_lossy(&worker.stderr)
    );

    let refused = fixture.feld(&["--json", "workspace", "delete", "research"]);
    assert_eq!(refused.status.code(), Some(1));
    let refused: Value = serde_json::from_slice(&refused.stderr).unwrap();
    assert_eq!(refused["error"]["kind"], "rejected");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("live Actors")
    );

    assert!(research(&["actor", "delete", "worker"]).status.success());
    let deleted = fixture.json(&["--json", "workspace", "delete", "research"]);
    assert_eq!(deleted["workspace"]["id"], first_id.as_str());
    assert!(deleted["workspace"]["deletedAt"].is_string());
    let kept = fixture.feld(&["--json", "workspace", "delete", "default"]);
    assert_eq!(kept.status.code(), Some(1));
    let kept: Value = serde_json::from_slice(&kept.stderr).unwrap();
    assert_eq!(
        kept["error"]["message"],
        "daemon rejected the request (409 Conflict): Workspace 'default' is the default Workspace and cannot be deleted"
    );
    assert!(fixture.feld(&["actor", "list"]).status.success());
    assert_eq!(
        fixture
            .feld(&["workspace", "delete", "research"])
            .status
            .code(),
        Some(1)
    );

    let live = fixture.json(&["--json", "workspace", "list"]);
    assert_eq!(live["workspaces"].as_array().unwrap().len(), 1);
    let all = fixture.json(&["--json", "workspace", "list", "--include-deleted"]);
    assert_eq!(all["workspaces"].as_array().unwrap().len(), 2);
    let table = fixture.feld(&["workspace", "list", "--include-deleted"]);
    assert!(String::from_utf8_lossy(&table.stdout).starts_with("KEY"));
    assert!(
        String::from_utf8_lossy(&table.stdout)
            .lines()
            .next()
            .unwrap()
            .ends_with("DELETED")
    );
    let got = fixture.json(&["--json", "workspace", "get", "research"]);
    assert_eq!(
        got["workspace"]["deletedAt"],
        deleted["workspace"]["deletedAt"]
    );
    let rejected = research(&["actor", "create", "late", "--kind", "job", "--", "true"]);
    assert_eq!(rejected.status.code(), Some(1));
    assert_eq!(research(&["actor", "list"]).status.code(), Some(1));

    let second = fixture.json(&["--json", "workspace", "create", "research"]);
    assert_ne!(second["workspace"]["id"], first_id.as_str());
    assert!(second["workspace"]["deletedAt"].is_null());
    let actors = research(&["--json", "actor", "list", "--include-deleted"]);
    let actors: Value = serde_json::from_slice(&actors.stdout).unwrap();
    assert_eq!(actors["actors"].as_array().unwrap().len(), 0);
    let all = fixture.json(&["--json", "workspace", "list", "--include-deleted"]);
    let keys: Vec<_> = all["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|workspace| {
            (
                workspace["key"].as_str().unwrap(),
                workspace["deletedAt"].is_null(),
            )
        })
        .collect();
    assert_eq!(
        keys,
        [("default", true), ("research", false), ("research", true)]
    );
    let human = fixture.feld(&["workspace", "delete", "research"]);
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).starts_with("Workspace deleted.\n"));
}

/// A store that fails answers 500 with the whole cause chain and leaves the same chain in
/// the daemon log.
#[test]
fn a_store_failure_keeps_its_cause_in_the_response_and_the_daemon_log() {
    let fixture = Fixture::new();
    fixture.json(&["daemon", "start", "--json"]);
    fixture.json(&["--json", "workspace", "create", "research"]);
    rusqlite::Connection::open(fixture.home().join("admin/feld.db"))
        .unwrap()
        .execute_batch("ALTER TABLE workspaces RENAME TO workspaces_aside")
        .unwrap();

    let failed = fixture.feld(&["--json", "workspace", "delete", "research"]);
    assert_eq!(failed.status.code(), Some(1));
    let failed: Value = serde_json::from_slice(&failed.stderr).unwrap();
    let message = failed["error"]["message"].as_str().unwrap();
    assert!(
        message.starts_with("daemon rejected the request (500 Internal Server Error): ")
            && message.contains("no such table: workspaces"),
        "{message}"
    );
    let log = fs::read_to_string(fixture.home().join("daemon.log")).unwrap();
    assert!(
        log.lines().any(
            |line| line.starts_with("Feld API: ") && line.contains("no such table: workspaces")
        ),
        "{log}"
    );
}

/// `cli.md`: the human-readable output reads a committed time in the daemon's local
/// timezone, to the second, while `--json` keeps the committed value.
#[test]
fn human_output_reads_times_in_the_daemon_local_timezone() {
    let fixture = Fixture::in_timezone("Asia/Tokyo");
    let started = fixture.json(&["daemon", "start", "--json"]);
    assert_eq!(started["daemon"]["timezone"], "Asia/Tokyo");

    let listing = fixture.json(&["workspace", "list", "--json"]);
    let committed = listing["workspaces"][0]["createdAt"].as_str().unwrap();
    assert!(committed.ends_with('Z'), "committed value stays UTC");

    let output = fixture.feld(&["workspace", "list"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let shown = stdout
        .lines()
        .nth(1)
        .and_then(|row| row.split_whitespace().nth(2))
        .expect("the CREATED column of the default Workspace");
    assert!(
        shown.ends_with("+09:00"),
        "human-readable output shows the local offset: {stdout}"
    );
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(shown).unwrap(),
        chrono::DateTime::parse_from_rfc3339(committed)
            .unwrap()
            .trunc_subsecs(0),
        "the shown time is the committed instant, to the second"
    );
}

#[test]
fn api_requires_the_local_token_and_supports_workspace_lookup() {
    let fixture = Fixture::new();
    let started = fixture.json(&["daemon", "start", "--json"]);
    let api = started["daemon"]["api"].as_str().unwrap();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let client = reqwest::blocking::Client::new();

    let unauthorized = client
        .get(format!("{api}/v1/daemon/status"))
        .send()
        .expect("call unauthenticated API");
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let default: Value = client
        .get(format!("{api}/v1/workspaces/default"))
        .bearer_auth(token.trim())
        .send()
        .expect("call authenticated API")
        .error_for_status()
        .expect("Workspace lookup succeeds")
        .json()
        .expect("decode Workspace lookup");
    assert_eq!(default["workspace"]["key"], "default");

    // The scheme of the credentials is case-insensitive.
    let lowercase = client
        .get(format!("{api}/v1/workspaces/default"))
        .header("Authorization", format!("bearer {}", token.trim()))
        .send()
        .expect("call API with a lowercase scheme");
    assert_eq!(lowercase.status(), reqwest::StatusCode::OK);
}

#[test]
fn api_answers_every_refusal_as_an_error() {
    let fixture = Fixture::new();
    let started = fixture.json(&["daemon", "start", "--json"]);
    let api = started["daemon"]["api"].as_str().unwrap();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let client = reqwest::blocking::Client::new();
    let refused = |response: reqwest::blocking::Response, status: reqwest::StatusCode| {
        assert_eq!(response.status(), status);
        let body: Value = response.json().expect("the refusal is JSON");
        assert!(body["message"].is_string(), "{body}");
    };

    refused(
        client
            .post(format!("{api}/v1/workspaces"))
            .bearer_auth(token.trim())
            .json(&json!({"key": "extra", "label": "unknown"}))
            .send()
            .unwrap(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY,
    );
    refused(
        client
            .post(format!("{api}/v1/workspaces/default/actors"))
            .bearer_auth(token.trim())
            .json(&json!({
                "name": "caller", "kind": "job",
                "launchSpec": {"command": ["true"], "provider": "tmux", "node": "local"},
                "caller": "not an object",
            }))
            .send()
            .unwrap(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY,
    );
    refused(
        client
            .get(format!("{api}/v1/nothing"))
            .bearer_auth(token.trim())
            .send()
            .unwrap(),
        reqwest::StatusCode::NOT_FOUND,
    );
    refused(
        client
            .put(format!("{api}/v1/workspaces"))
            .bearer_auth(token.trim())
            .send()
            .unwrap(),
        reqwest::StatusCode::METHOD_NOT_ALLOWED,
    );
}

#[test]
fn daemon_status_reports_when_the_daemon_started() {
    let fixture = Fixture::new();
    let started = fixture.json(&["daemon", "start", "--json"]);
    let api = started["daemon"]["api"].as_str().unwrap();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();

    let status: Value = reqwest::blocking::Client::new()
        .get(format!("{api}/v1/daemon/status"))
        .bearer_auth(token.trim())
        .send()
        .expect("call daemon status")
        .error_for_status()
        .expect("daemon status succeeds")
        .json()
        .expect("decode daemon status");

    let started_at = chrono::DateTime::parse_from_rfc3339(status["startedAt"].as_str().unwrap())
        .expect("startedAt is RFC 3339");
    let server_time = chrono::DateTime::parse_from_rfc3339(status["serverTime"].as_str().unwrap())
        .expect("serverTime is RFC 3339");
    assert!(
        started_at <= server_time,
        "startedAt {started_at} is after serverTime {server_time}"
    );
    assert_eq!(
        fixture.json(&["daemon", "status", "--json"])["startedAt"],
        status["startedAt"]
    );
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// `cli.md`: the commands that start the daemon make the Feld home, and the others only
/// read it, answering that no daemon runs when it does not exist.
#[test]
fn only_the_daemon_start_makes_the_feld_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let feld = |arguments: &[&str]| {
        support::feld()
            .arg("--home")
            .arg(&home)
            .env("HOME", temp.path())
            .args(arguments)
            .output()
            .expect("run feld")
    };

    let status = feld(&["daemon", "status"]);
    assert_eq!(status.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&status.stdout).starts_with("Daemon is not running."));
    assert!(feld(&["daemon", "stop"]).status.success());
    assert!(!home.exists());

    assert!(feld(&["daemon", "start"]).status.success());
    assert_eq!(mode(&home), 0o700);
    assert_eq!(mode(&home.join("daemon.lock")), 0o600);
    assert_eq!(mode(&home.join("daemon.lifecycle.lock")), 0o600);
    assert!(feld(&["daemon", "stop"]).status.success());
}

/// The CLI reads a running daemon from a Feld home it may not write, as a Runtime's
/// sandbox leaves it.
#[test]
fn the_cli_reads_a_running_daemon_from_a_read_only_feld_home() {
    let fixture = Fixture::new();
    fixture.json(&["daemon", "start", "--json"]);
    fixture.json(&[
        "--json", "actor", "create", "worker", "--kind", "job", "--", "true",
    ]);
    let mut send = fixture
        .command(&["--json", "message", "send", "worker"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn feld");
    send.stdin.take().unwrap().write_all(b"hello").unwrap();
    let sent = send.wait_with_output().unwrap();
    assert!(sent.status.success());
    let sent: Value = serde_json::from_slice(&sent.stdout).unwrap();
    let message = sent["message"]["id"].as_str().unwrap();

    let files =
        ["daemon.lock", "daemon.json", "admin/api-token"].map(|name| fixture.home().join(name));
    for file in &files {
        fs::set_permissions(file, fs::Permissions::from_mode(0o400)).unwrap();
    }
    fs::set_permissions(fixture.home(), fs::Permissions::from_mode(0o500)).unwrap();
    let outputs = [
        vec!["daemon", "status"],
        vec!["actor", "list"],
        vec!["message", "get", message],
    ]
    .map(|arguments| (arguments.join(" "), fixture.feld(&arguments)));
    fs::set_permissions(fixture.home(), fs::Permissions::from_mode(0o700)).unwrap();
    for file in &files {
        fs::set_permissions(file, fs::Permissions::from_mode(0o600)).unwrap();
    }

    for (command, output) in outputs {
        assert!(
            output.status.success(),
            "feld {command} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        fixture.json(&["daemon", "start", "--json"])["alreadyRunning"],
        true
    );
}

/// `cli.md`: a reader of stdout that closes early (`| head`) ends the command quietly with 0,
/// for the human output and `--json` alike.
#[test]
fn a_closed_stdout_ends_the_command_quietly() {
    let fixture = Fixture::new();
    fixture.json(&["daemon", "start", "--json"]);
    for arguments in [
        &["daemon", "status"][..],
        &["daemon", "status", "--json"][..],
    ] {
        let (reader, writer) = std::io::pipe().expect("create stdout pipe");
        drop(reader);
        let output = fixture
            .command(arguments)
            .stdout(writer)
            .output()
            .expect("run feld");
        assert_eq!(
            output.status.code(),
            Some(0),
            "feld {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "feld {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn daemon_status_reports_the_version_and_the_restart_endpoint_starts_a_new_instance() {
    let fixture = Fixture::new();
    let started = fixture.json(&["daemon", "start", "--json"]);
    let api = started["daemon"]["api"].as_str().unwrap();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let before = &started["daemon"]["instanceId"];

    let version = support::feld().arg("--version").output().unwrap().stdout;
    let version = String::from_utf8(version).unwrap();
    assert_eq!(
        Some(started["daemon"]["version"].as_str().unwrap()),
        version.trim().strip_prefix("feld "),
        "the daemon reports what `feld --version` prints"
    );

    let accepted: Value = reqwest::blocking::Client::new()
        .post(format!("{api}/v1/daemon/restart"))
        .bearer_auth(token.trim())
        .send()
        .expect("call daemon restart")
        .error_for_status()
        .expect("daemon restart succeeds")
        .json()
        .expect("decode daemon restart");
    assert_eq!(accepted["accepted"], true);
    assert_eq!(&accepted["instanceId"], before);

    for _ in 0..200 {
        let status = fixture.feld(&["daemon", "status", "--json"]);
        if status.status.success() {
            let status: Value = serde_json::from_slice(&status.stdout).unwrap();
            if &status["instanceId"] != before {
                assert_eq!(status["state"], "running");
                // The new daemon keeps the local token, so a caller holding it reads on.
                let read = reqwest::blocking::Client::new()
                    .get(format!("{api}/v1/daemon/status"))
                    .bearer_auth(token.trim())
                    .send()
                    .expect("read the restarted daemon");
                assert!(read.status().is_success(), "{}", read.status());
                return;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("the daemon did not come back as a new instance");
}

#[test]
fn a_restart_that_fails_leaves_the_daemon_ready_to_restart_again() {
    let fixture = Fixture::new();
    let started = fixture.json(&["daemon", "start", "--json"]);
    let api = started["daemon"]["api"].as_str().unwrap();
    let before = &started["daemon"]["instanceId"];
    let token_path = fixture.home().join("admin/api-token");
    let token = fs::read_to_string(&token_path).unwrap();
    let restart = || {
        reqwest::blocking::Client::new()
            .post(format!("{api}/v1/daemon/restart"))
            .bearer_auth(token.trim())
            .send()
    };

    // Without the token file the restart process cannot reach the daemon and gives up.
    let aside = fixture.home().join("api-token.aside");
    fs::rename(&token_path, &aside).unwrap();
    restart()
        .expect("call daemon restart")
        .error_for_status()
        .expect("daemon restart is accepted");
    let log = fixture.home().join("daemon.log");
    let mut failed = false;
    for _ in 0..200 {
        if fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("Error:")
        {
            failed = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(failed, "the restart process did not fail");
    fs::rename(&aside, &token_path).unwrap();

    // A request made before the daemon has seen the failed process end joins it, so the
    // restart is asked again until the daemon comes back as a new instance.
    for attempt in 0..200 {
        if attempt % 20 == 0 {
            let _ = restart();
        }
        let status = fixture.feld(&["daemon", "status", "--json"]);
        if status.status.success() {
            let status: Value = serde_json::from_slice(&status.stdout).unwrap();
            if &status["instanceId"] != before {
                return;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("the daemon did not restart after a failed restart");
}

/// The daemon log rotates at 10 MiB into `daemon.log.1` to `daemon.log.4`, when the daemon
/// starts and while it runs, and the running daemon writes on into the new current file.
#[test]
fn the_daemon_log_rotates_into_four_generations() {
    const LIMIT: usize = 10 * 1024 * 1024;
    let fixture = Fixture::new();
    let log = fixture.home().join("daemon.log");
    let generation = |n: usize| fixture.home().join(format!("daemon.log.{n}"));
    let read = |path: &Path| fs::read_to_string(path).unwrap_or_default();
    let fill = |marker: &str| {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .unwrap();
        writeln!(file, "{marker}").unwrap();
        file.write_all(&vec![b'.'; LIMIT]).unwrap();
    };
    for n in 1..=4 {
        fs::write(generation(n), format!("gen-{n}\n")).unwrap();
        fs::set_permissions(generation(n), fs::Permissions::from_mode(0o600)).unwrap();
    }
    fill("before-start");

    // Starting rotates a full log, and the oldest generation goes.
    fixture.json(&["daemon", "start", "--json"]);
    assert!(read(&generation(1)).starts_with("before-start"));
    assert_eq!(read(&generation(2)), "gen-1\n");
    assert_eq!(read(&generation(3)), "gen-2\n");
    assert_eq!(read(&generation(4)), "gen-3\n");
    assert!(!generation(5).exists());

    // A log that fills while the daemon runs rotates too.
    fill("while-running");
    let mut rotated = false;
    for _ in 0..200 {
        if read(&generation(2)).starts_with("before-start") {
            rotated = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(rotated, "the running daemon did not rotate its log");
    assert!(read(&generation(1)).contains("while-running"));
    assert_eq!(read(&generation(3)), "gen-1\n");
    assert_eq!(read(&generation(4)), "gen-2\n");

    // The same daemon writes on into the new current file, and the daemon it restarts
    // into keeps the generations.
    let started = fixture.json(&["daemon", "status", "--json"]);
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    reqwest::blocking::Client::new()
        .post(format!(
            "{}/v1/daemon/restart",
            started["api"].as_str().unwrap()
        ))
        .bearer_auth(token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    let mut written = false;
    for _ in 0..200 {
        if read(&log).contains("Feld daemon: restarting") {
            written = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(written, "the daemon did not write into the current log");
    let mut restarted = false;
    for _ in 0..200 {
        let status = fixture.feld(&["daemon", "status", "--json"]);
        if status.status.success() {
            let status: Value = serde_json::from_slice(&status.stdout).unwrap();
            if status["instanceId"] != started["instanceId"] {
                restarted = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(restarted, "the daemon did not restart");
    assert!(read(&generation(1)).contains("while-running"));
    assert!(read(&generation(2)).starts_with("before-start"));
    for path in [
        log.clone(),
        generation(1),
        generation(2),
        generation(3),
        generation(4),
    ] {
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{}", path.display());
    }
}

/// A pairing code as the CLI shows it: `XXXX-XXXX` of the 32 characters that read apart.
fn assert_pairing_code(code: &str) {
    let (head, tail) = code
        .split_once('-')
        .unwrap_or_else(|| panic!("code {code}"));
    for part in [head, tail] {
        assert_eq!(part.len(), 4, "code {code}");
        assert!(
            part.chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
                && !part.contains(['0', '1', 'I', 'L', 'O']),
            "code {code}"
        );
    }
}

/// Trades a pairing code for a browser token the way the Web UI does, and gives the cookie
/// and the key.
fn pair_browser(client: &reqwest::blocking::Client, api: &str, code: &str) -> (String, String) {
    let response = client
        .post(format!("{api}/v1/ui/pair"))
        .header(reqwest::header::ORIGIN, api)
        .json(&serde_json::json!({ "code": code }))
        .send()
        .unwrap();
    assert_eq!(response.status(), 200);
    let cookie = response.headers()[reqwest::header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_owned();
    let paired: serde_json::Value = response.json().unwrap();
    (
        cookie.split(';').next().unwrap().to_owned(),
        paired["key"].as_str().unwrap().to_owned(),
    )
}

#[test]
fn browser_pairing_from_the_cli() {
    let fixture = Fixture::new();
    let started = fixture.json(&["daemon", "start", "--json"]);
    let api = started["daemon"]["api"].as_str().unwrap();
    let client = reqwest::blocking::Client::new();

    let empty = fixture.feld(&["browser", "list"]);
    assert!(empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stdout).contains("No browsers are signed in."));

    let issued = fixture.feld(&["ui", "pair", "--name", "laptop"]);
    assert!(issued.status.success());
    let shown = String::from_utf8(issued.stdout).unwrap();
    assert!(shown.starts_with("Sign-in code issued."));
    assert!(shown.contains("  name:       laptop"));
    assert!(!shown.contains("http"));
    let issued = fixture.json(&["--json", "ui", "pair", "--name", "laptop"]);
    assert_eq!(issued["name"], "laptop");
    assert!(issued["expiresAt"].is_string());
    let code = issued["code"].as_str().unwrap();
    assert_pairing_code(code);
    let laptop = pair_browser(&client, api, code);

    let default_name = fixture.json(&["--json", "ui", "pair"]);
    assert_eq!(default_name["name"], "browser");

    let printed = fixture.feld(&["ui", "--print-url"]);
    let url = String::from_utf8(printed.stdout).unwrap();
    let local = pair_browser(&client, api, url.trim().rsplit_once("#code=").unwrap().1);

    let listing = fixture.json(&["--json", "browser", "list"]);
    let names: Vec<&str> = listing["browsers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|browser| browser["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["laptop", "local"]);
    let table = String::from_utf8(fixture.feld(&["browser", "ls"]).stdout).unwrap();
    assert!(table.starts_with("ID"), "{table}");
    assert!(!table.contains("TAILSCALE"), "{table}");
    assert!(table.contains("laptop"));

    let workspaces = |(cookie, key): &(String, String)| {
        client
            .get(format!("{api}/v1/workspaces"))
            .header(reqwest::header::COOKIE, cookie)
            .header("X-Feld-Browser-Key", key)
            .send()
            .unwrap()
            .status()
    };
    assert_eq!(workspaces(&laptop), 200);

    let revoked = fixture.feld(&["browser", "revoke", "laptop"]);
    assert!(revoked.status.success());
    let revoked = String::from_utf8(revoked.stdout).unwrap();
    assert!(revoked.starts_with("Browser signed out."));
    assert!(revoked.contains("  name:       laptop"));
    assert_eq!(workspaces(&laptop), 401);
    assert_eq!(workspaces(&local), 200);

    let local_id = fixture.json(&["--json", "browser", "list"])["browsers"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let revoked = fixture.json(&["--json", "browser", "revoke", &local_id]);
    assert_eq!(revoked["browsers"][0]["id"], local_id.as_str());
    assert!(revoked["browsers"][0]["revokedAt"].is_string());
    assert_eq!(workspaces(&local), 401);

    let missing = fixture.feld(&["--json", "browser", "revoke", "laptop"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    let missing: Value = serde_json::from_slice(&missing.stderr).unwrap();
    assert_eq!(missing["error"]["kind"], "rejected");
}

#[test]
fn web_ui_assets_bootstrap_without_exposing_tokens() {
    let fixture = Fixture::new();
    let started = fixture.json(&["daemon", "start", "--json"]);
    let api = started["daemon"]["api"].as_str().unwrap();
    let web_ui = started["daemon"]["webUi"].as_str().unwrap();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    assert_eq!(web_ui, format!("{api}/ui/"));

    let printed = fixture.feld(&["ui", "--print-url"]);
    assert!(printed.status.success());
    let url = String::from_utf8(printed.stdout).unwrap();
    let code = url
        .trim()
        .strip_prefix(&format!("{api}/ui/pair?workspace=default#code="))
        .unwrap_or_else(|| panic!("unexpected URL {url}"));
    assert_pairing_code(code);
    assert!(!url.contains(token.trim()));

    let conflicting = fixture
        .command(&["ui", "--print-url"])
        .env("FELD_SESSION_TOKEN", "arbitrary-session-secret")
        .output()
        .unwrap();
    assert!(conflicting.status.success());
    let conflicting_url = String::from_utf8(conflicting.stdout).unwrap();
    assert!(conflicting_url.contains("#code="));
    assert!(!conflicting_url.contains("arbitrary-session-secret"));

    let browser_failure = fixture
        .command(&["--json", "ui"])
        .env("BROWSER", "feld-browser-that-does-not-exist")
        .output()
        .unwrap();
    assert_eq!(browser_failure.status.code(), Some(2));
    assert!(browser_failure.stdout.is_empty());
    let failure: Value = serde_json::from_slice(&browser_failure.stderr).unwrap();
    assert_eq!(failure["error"]["kind"], "operational");

    let pages = fixture.home().join("admin/ui-pair");
    fs::create_dir_all(&pages).unwrap();
    let expired = pages.join("1-1-1.html");
    fs::write(&expired, "expired").unwrap();
    assert!(fixture.feld(&["ui", "--print-url"]).status.success());
    assert!(!expired.exists());
    fs::write(&expired, "expired").unwrap();
    let argv = fixture.home().join("browser-argv");
    let browser = fixture.home().join("browser");
    fs::write(
        &browser,
        format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", argv.display()),
    )
    .unwrap();
    fs::set_permissions(&browser, fs::Permissions::from_mode(0o700)).unwrap();
    support::warm_up(&mut Command::new(&browser));
    fs::remove_file(&argv).unwrap();
    let opened = fixture
        .command(&["--json", "ui"])
        .env("BROWSER", &browser)
        .output()
        .unwrap();
    assert!(opened.status.success());
    let opened: Value = serde_json::from_slice(&opened.stdout).unwrap();
    assert_eq!(opened["opened"], true);
    let opened_url = opened["url"].as_str().unwrap();
    let code = opened_url.split("#code=").nth(1).unwrap();
    let argv = support::wait_for_written(&argv);
    assert!(!argv.contains(code), "{argv}");
    let page = Path::new(argv.trim());
    assert_eq!(page.parent().unwrap(), pages);
    assert_eq!(mode(page), 0o600);
    assert!(fs::read_to_string(page).unwrap().contains(opened_url));
    assert!(!expired.exists());

    let invalid_workspace =
        fixture.feld(&["--json", "--workspace", "Bad Key", "ui", "--print-url"]);
    assert_eq!(invalid_workspace.status.code(), Some(2));
    assert!(invalid_workspace.stdout.is_empty());
    let invalid: Value = serde_json::from_slice(&invalid_workspace.stderr).unwrap();
    assert_eq!(invalid["error"]["kind"], "operational");

    let missing_workspace =
        fixture.feld(&["--json", "--workspace", "missing", "ui", "--print-url"]);
    assert_eq!(missing_workspace.status.code(), Some(1));
    assert!(missing_workspace.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&missing_workspace.stderr)
            .contains("Workspace 'missing' was not found")
    );

    let client = reqwest::blocking::Client::new();
    let html = client
        .get(web_ui)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        html.headers()[reqwest::header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(
        html.headers()[reqwest::header::REFERRER_POLICY],
        "no-referrer"
    );
    let html = html.text().unwrap();
    assert!(html.contains("Feld Actor Activity"));
    assert!(!html.contains(token.trim()));

    let javascript = client
        .get(format!("{api}/ui/app.js"))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        javascript.headers()[reqwest::header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    let javascript = javascript.text().unwrap();
    assert!(javascript.contains("/v1/events"));
    assert!(javascript.contains("feld-agents"));

    let styles = client
        .get(format!("{api}/ui/app.css"))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        styles.headers()[reqwest::header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );

    // The page's own routes answer with the page, which draws them.
    let daemon_page = client
        .get(format!("{api}/ui/daemon"))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        daemon_page.headers()[reqwest::header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    // The page's address keeps the query it was asked with, which names what it shows.
    let no_redirects = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let redirect = no_redirects
        .get(format!("{api}/ui?workspace=*&session=session_1"))
        .send()
        .unwrap();
    assert_eq!(redirect.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        redirect.headers()[reqwest::header::LOCATION],
        "/ui/?workspace=*&session=session_1"
    );
    let missing_file = client.get(format!("{api}/ui/missing.js")).send().unwrap();
    assert_eq!(missing_file.status(), reqwest::StatusCode::NOT_FOUND);

    let missing = client.get(format!("{api}/ui/../api-token")).send().unwrap();
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
    let unauthorized_events = client.get(format!("{api}/v1/events")).send().unwrap();
    assert_eq!(
        unauthorized_events.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
}

#[test]
fn events_stream_reports_resource_identity_without_record_values() {
    let fixture = Fixture::new();
    let started = fixture.json(&["daemon", "start", "--json"]);
    let api = started["daemon"]["api"].as_str().unwrap().to_owned();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let event_api = api.clone();
    let event_token = token.clone();
    let reader = thread::spawn(move || {
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .get(format!("{event_api}/v1/events"))
            .bearer_auth(event_token.trim())
            .send()
            .unwrap()
            .error_for_status()
            .unwrap();
        ready_tx.send(()).unwrap();
        let mut reader = BufReader::new(response);
        let mut body = String::new();
        let mut data_events = 0;
        while data_events < 2 {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.starts_with("data:") {
                data_events += 1;
            }
            body.push_str(&line);
        }
        body
    });
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let created: Value = reqwest::blocking::Client::new()
        .post(format!("{api}/v1/workspaces"))
        .bearer_auth(token.trim())
        .json(&serde_json::json!({ "key": "events-test" }))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(created["workspace"]["key"], "events-test");
    let body = reader.join().unwrap();
    assert!(body.contains("event: resource"));
    assert!(body.contains(r#""resource":"daemon""#));
    assert!(body.contains(r#""resource":"workspace","id":"events-test""#));
    assert!(!body.contains(r#""key":"events-test""#));
    assert!(!body.contains(token.trim()));
}

#[test]
fn events_stream_filters_workspaces_while_unscoped_stream_sees_all() {
    let fixture = Fixture::new();
    let started = fixture.json(&["daemon", "start", "--json"]);
    let api = started["daemon"]["api"].as_str().unwrap().to_owned();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let client = reqwest::blocking::Client::new();
    for key in ["ignored-events", "target-events"] {
        client
            .post(format!("{api}/v1/workspaces"))
            .bearer_auth(token.trim())
            .json(&serde_json::json!({ "key": key }))
            .send()
            .unwrap()
            .error_for_status()
            .unwrap();
    }
    // macOS checks a freshly copied executable on its first run, which takes seconds under
    // the suite's load. Running the tmux Plugin once here keeps that out of the Actor
    // creations the readers wait for.
    Command::new(fixture.home().join("plugins/provider/tmux/plugin"))
        .stdin(Stdio::null())
        .output()
        .unwrap();

    // Each reader stops at the last Actor, created in the selected Workspace.
    let start_reader = |workspace: Option<&str>| {
        let api = api.clone();
        let token = token.clone();
        let workspace = workspace.map(str::to_owned);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let reader = thread::spawn(move || {
            let suffix = workspace
                .as_deref()
                .map(|key| format!("?workspace={key}"))
                .unwrap_or_default();
            let response = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .get(format!("{api}/v1/events{suffix}"))
                .bearer_auth(token.trim())
                .send()
                .unwrap()
                .error_for_status()
                .unwrap();
            ready_tx.send(()).unwrap();
            let mut reader = BufReader::new(response);
            let mut events = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(data) = line.strip_prefix("data:") {
                    let event = serde_json::from_str::<Value>(data.trim()).unwrap();
                    let last = event["resource"] == "actor" && event["id"] == "target-actor";
                    events.push(event);
                    if last {
                        return events;
                    }
                }
            }
        });
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        reader
    };

    let all = start_reader(None);
    let scoped = start_reader(Some("target-events"));
    client
        .post(format!("{api}/v1/workspaces"))
        .bearer_auth(token.trim())
        .json(&serde_json::json!({ "key": "other-events" }))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    client
        .delete(format!("{api}/v1/workspaces/other-events"))
        .bearer_auth(token.trim())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    for (workspace, name) in [
        ("ignored-events", "ignored-actor"),
        ("target-events", "target-actor"),
    ] {
        client
            .post(format!("{api}/v1/workspaces/{workspace}/actors"))
            .bearer_auth(token.trim())
            .json(&serde_json::json!({
                "name": name,
                "kind": "job",
                "launchSpec": {
                    "command": ["true"],
                    "workdir": "/tmp",
                    "provider": "tmux",
                    "node": "local",
                    "providerOptions": {"socket": "feld", "unit": "session"}
                }
            }))
            .send()
            .unwrap()
            .error_for_status()
            .unwrap();
    }

    let ids = |events: &[Value], resource: &str| {
        events
            .iter()
            .filter(|event| event["resource"] == resource)
            .map(|event| event["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let all = all.join().unwrap();
    let scoped = scoped.join().unwrap();
    assert_eq!(ids(&all, "actor"), ["ignored-actor", "target-actor"]);
    assert_eq!(ids(&scoped, "actor"), ["target-actor"]);
    // A Workspace's creation and deletion reach every Workspace's stream.
    for events in [&all, &scoped] {
        assert_eq!(ids(events, "workspace"), ["other-events", "other-events"]);
    }
}

#[test]
fn occupied_stable_collector_endpoint_disables_telemetry_without_blocking_daemon() {
    let fixture = Fixture::new();
    let first = fixture.json(&["daemon", "start", "--json"]);
    let endpoint = first["daemon"]["otlp"]
        .as_str()
        .unwrap()
        .strip_prefix("http://")
        .unwrap()
        .to_owned();
    fixture.json(&["daemon", "stop", "--json"]);
    let occupied = TcpListener::bind(&endpoint).unwrap();
    let restarted = fixture.json(&["daemon", "start", "--json"]);
    assert!(restarted["daemon"]["otlp"].is_null());
    assert_eq!(
        fixture.json(&["daemon", "status", "--json"])["state"],
        "running"
    );
    drop(occupied);
}

#[test]
fn hard_daemon_death_reaps_its_collector_before_immediate_restart() {
    let fixture = Fixture::new();
    let first = fixture.json(&["daemon", "start", "--json"]);
    let daemon_pid = first["daemon"]["pid"].as_u64().unwrap() as u32;
    let collector_pid = collector_child(daemon_pid).expect("find managed Collector child");
    let endpoint = first["daemon"]["otlp"].clone();

    assert!(
        Command::new("kill")
            .args(["-KILL", &daemon_pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let restarted = fixture.json(&["daemon", "start", "--json"]);
    assert_eq!(restarted["daemon"]["otlp"], endpoint);
    assert_ne!(
        restarted["daemon"]["pid"].as_u64().unwrap(),
        daemon_pid as u64
    );
    assert!(!process_exists(collector_pid));
}

#[test]
fn daemon_supervises_a_collector_that_dies_alone() {
    let fixture = Fixture::new();
    let first = fixture.json(&["daemon", "start", "--json"]);
    let daemon_pid = first["daemon"]["pid"].as_u64().unwrap() as u32;
    let old_collector = collector_child(daemon_pid).expect("find managed Collector child");
    let endpoint = first["daemon"]["otlp"].clone();
    let api = first["daemon"]["api"].as_str().unwrap();
    let token = fs::read_to_string(fixture.home().join("admin/api-token")).unwrap();
    let client = reqwest::blocking::Client::new();

    assert!(
        Command::new("kill")
            .args(["-KILL", &old_collector.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut saw_unavailable = false;
    loop {
        let status: Value = client
            .get(format!("{api}/v1/daemon/status"))
            .bearer_auth(token.trim())
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap();
        saw_unavailable |= status["otlp"].is_null();
        if saw_unavailable && status["otlp"] == endpoint {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Collector was not republished after child-only death: {status}"
        );
        thread::sleep(std::time::Duration::from_millis(10));
    }
    let new_collector = collector_child(daemon_pid).expect("find restarted Collector child");
    assert_ne!(new_collector, old_collector);
    assert_eq!(
        fixture.json(&["daemon", "status", "--json"])["pid"],
        daemon_pid
    );
}

/// Continues a stopped process when it drops, so the fixture can stop its daemon even after
/// a failed assertion.
struct Continue(u32);

impl Drop for Continue {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-CONT", &self.0.to_string()])
            .status();
    }
}

#[test]
fn a_daemon_that_does_not_answer_is_reported_with_its_pid() {
    let fixture = Fixture::new();
    let started = fixture.json(&["daemon", "start", "--json"]);
    let pid = started["daemon"]["pid"].as_u64().unwrap() as u32;
    let api = started["daemon"]["api"].as_str().unwrap().to_owned();
    assert!(
        Command::new("kill")
            .args(["-STOP", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    let resume = Continue(pid);

    let status = fixture.feld(&["daemon", "status", "--json"]);
    assert_eq!(status.status.code(), Some(2));
    let status: Value = serde_json::from_slice(&status.stdout).expect("parse status JSON");
    assert_eq!(status["state"], "unresponsive");
    assert_eq!(status["pid"], pid);
    assert_eq!(status["api"], api.as_str());
    assert_eq!(status["home"], fixture.home().to_str().unwrap());

    let human = fixture.feld(&["daemon", "status"]);
    assert_eq!(human.status.code(), Some(2));
    let human = String::from_utf8_lossy(&human.stdout);
    assert!(human.contains("state:      unresponsive"), "{human}");
    assert!(human.contains(&format!("kill {pid}")), "{human}");

    for arguments in [
        &["actor", "list"][..],
        &["daemon", "stop"][..],
        &["daemon", "restart"][..],
    ] {
        let begun = std::time::Instant::now();
        let output = fixture.feld(arguments);
        assert!(begun.elapsed() < Duration::from_secs(15), "{arguments:?}");
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("did not answer"), "{error}");
        assert!(error.contains(&api), "{error}");
        assert!(
            error.contains(&format!("`kill {pid}`")) && error.contains("`feld daemon start`"),
            "{error}"
        );
    }
    assert!(process_exists(pid));

    drop(resume);
    assert_eq!(fixture.json(&["daemon", "status", "--json"])["pid"], pid);
}

fn collector_child(parent: u32) -> Option<u32> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,command="])
        .output()
        .expect("list processes");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let ppid = fields.next()?.parse::<u32>().ok()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            (ppid == parent && command.contains("collector-run")).then_some(pid)
        })
        .next()
}

fn process_exists(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn concurrent_start_and_restart_requests_converge_on_a_running_daemon() {
    let fixture = Fixture::new();
    let starts: Vec<_> = (0..8)
        .map(|_| fixture.spawn(&["daemon", "start", "--json"]))
        .collect();
    let start_outputs: Vec<_> = starts
        .into_iter()
        .map(|child| child.wait_with_output().expect("wait for concurrent start"))
        .collect();
    assert!(start_outputs.iter().all(|output| output.status.success()));
    let start_instances: Vec<_> = start_outputs
        .iter()
        .map(|output| {
            serde_json::from_slice::<Value>(&output.stdout).expect("parse concurrent start output")
                ["daemon"]["instanceId"]
                .clone()
        })
        .collect();
    assert!(
        start_instances
            .iter()
            .all(|instance| instance == &start_instances[0])
    );

    let restarts: Vec<_> = (0..8)
        .map(|_| fixture.spawn(&["--json", "daemon", "restart"]))
        .collect();
    let restart_outputs: Vec<_> = restarts
        .into_iter()
        .map(|child| {
            child
                .wait_with_output()
                .expect("wait for concurrent restart")
        })
        .collect();
    for output in restart_outputs {
        assert!(
            output.status.success(),
            "concurrent restart failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let response: Value =
            serde_json::from_slice(&output.stdout).expect("parse concurrent restart output");
        assert_eq!(response["daemon"]["state"], "running");
    }
    let final_status = fixture.json(&["daemon", "status", "--json"]);
    assert_eq!(final_status["state"], "running");
}

#[test]
fn json_execution_errors_use_stderr_and_preserve_exit_codes() {
    let fixture = Fixture::new();

    for arguments in [
        ["--json", "workspace", "create"],
        ["workspace", "create", "--json"],
    ] {
        let parsing = fixture.feld(&arguments);
        assert_eq!(parsing.status.code(), Some(2));
        assert!(parsing.stdout.is_empty());
        let parsing_json: Value =
            serde_json::from_slice(&parsing.stderr).expect("parse argument JSON error");
        assert_eq!(parsing_json["error"]["kind"], "operational");
    }

    let human_parsing = fixture.feld(&["workspace", "create"]);
    assert_eq!(human_parsing.status.code(), Some(2));
    assert!(human_parsing.stdout.is_empty());
    let human_message = String::from_utf8_lossy(&human_parsing.stderr);
    assert!(human_message.contains("Usage:"));
    assert!(human_message.contains("required arguments"));

    let unreachable = fixture.feld(&["workspace", "list", "--json"]);
    assert_eq!(unreachable.status.code(), Some(2));
    assert!(unreachable.stdout.is_empty());
    let unreachable_json: Value =
        serde_json::from_slice(&unreachable.stderr).expect("parse operational JSON error");
    assert_eq!(unreachable_json["error"]["kind"], "operational");

    fixture.json(&["daemon", "start", "--json"]);
    let rejected = fixture.feld(&["--json", "workspace", "create", "Invalid"]);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(rejected.stdout.is_empty());
    let rejected_json: Value =
        serde_json::from_slice(&rejected.stderr).expect("parse rejected JSON error");
    assert_eq!(rejected_json["error"]["kind"], "rejected");
}

#[test]
fn self_outside_a_session_says_why_it_has_none() {
    let fixture = Fixture::new();

    for arguments in [
        vec!["self", "token"],
        vec!["self", "token", "--json"],
        vec!["self", "get"],
        vec!["self", "get", "--json"],
    ] {
        let json = arguments.contains(&"--json");
        let output = fixture.command(&arguments).output().expect("run feld");
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = if json {
            let error: Value =
                serde_json::from_slice(&output.stderr).expect("parse rejected JSON error");
            assert_eq!(error["error"]["kind"], "rejected");
            error["error"]["message"].as_str().unwrap().to_owned()
        } else {
            stderr.to_string()
        };
        assert!(
            message.contains("No Session token: run this inside an attached or owned Feld Session"),
            "feld {} said: {stderr}",
            arguments.join(" ")
        );
    }
}

#[test]
fn truncated_shutdown_response_succeeds_after_the_instance_disappears() {
    let fixture = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind controlled daemon server");
    let api = format!("http://{}", listener.local_addr().unwrap());
    let instance_id = "daemon_controlled";
    let token = "controlled-local-token";
    fs::write(fixture.home().join("admin/api-token"), token).expect("write controlled token");
    fs::write(
        fixture.home().join("daemon.json"),
        serde_json::to_vec(&serde_json::json!({
            "instanceId": instance_id,
            "pid": 4242,
            "api": api,
        }))
        .unwrap(),
    )
    .expect("write controlled daemon record");
    let daemon_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(fixture.home().join("daemon.lock"))
        .expect("open controlled daemon lock");
    daemon_lock
        .lock_exclusive()
        .expect("hold controlled daemon lock");

    let home = fs::canonicalize(fixture.home()).unwrap();
    let server = thread::spawn(move || {
        let (mut status_stream, _) = listener.accept().expect("accept status request");
        let status_request = read_http_request(&mut status_stream);
        assert!(status_request.starts_with("GET /v1/daemon/status "));
        let status_body = serde_json::to_vec(&serde_json::json!({
            "state": "running",
            "instanceId": instance_id,
            "pid": 4242,
            "home": home,
            "api": api,
            "startedAt": "2026-09-15T23:00:00Z",
            "serverTime": "2026-09-16T00:00:00Z",
            "historyStart": null,
            "timezone": "UTC",
            "providers": [],
            "nodes": [],
            "webUi": null,
            "otlp": null,
        }))
        .unwrap();
        write_http_response(&mut status_stream, &status_body);

        let (mut shutdown_stream, _) = listener.accept().expect("accept shutdown request");
        let shutdown_request = read_http_request(&mut shutdown_stream);
        assert!(shutdown_request.starts_with("POST /v1/daemon/shutdown "));
        shutdown_stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 40\r\nConnection: close\r\n\r\n{\"accepted\":",
            )
            .expect("write truncated shutdown response");
        shutdown_stream.flush().unwrap();
        drop(shutdown_stream);
        FileExt::unlock(&daemon_lock).expect("release controlled daemon lock");
    });

    let stopped = fixture.feld(&["daemon", "stop", "--json"]);
    server.join().expect("controlled daemon server completes");
    assert!(
        stopped.status.success(),
        "stop failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&stopped.stdout),
        String::from_utf8_lossy(&stopped.stderr)
    );
    let stopped_json: Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(stopped_json["stopped"], true);
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).expect("read HTTP request");
        assert_ne!(read, 0, "request ended before its headers");
        request.extend_from_slice(&buffer[..read]);
    }
    String::from_utf8(request).expect("HTTP request is UTF-8")
}

fn write_http_response(stream: &mut TcpStream, body: &[u8]) {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("write HTTP response headers");
    stream.write_all(body).expect("write HTTP response body");
    stream.flush().expect("flush HTTP response");
}
