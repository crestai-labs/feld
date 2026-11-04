//! The spool (`docs/spec/design/spool.md`): the daemon takes the requests written as files,
//! and a CLI inside a sandbox that denies TCP goes through it.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;

mod support;

struct Fixture {
    temporary: TempDir,
    socket: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let socket = temporary.path().join("spool.sock");
        support::install_plugins(temporary.path());
        Self { temporary, socket }
    }

    /// A fixture with the codex Runtime Adapter installed beside the defaults.
    fn with_codex() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let socket = temporary.path().join("spool.sock");
        support::install_plugins_with_codex(temporary.path());
        Self { temporary, socket }
    }

    fn home(&self) -> &Path {
        self.temporary.path()
    }

    fn spool(&self) -> PathBuf {
        self.home().join("spool")
    }

    /// The directory of the spool of the Session of `token`, or the shared one of the
    /// `attached` Sessions for a token of no `owned` Session.
    fn directory(&self, token: &str) -> PathBuf {
        match self.owned_session(token) {
            Some(session) => self.spool().join(session),
            None => self.spool().join("shared"),
        }
    }

    /// The `owned` Session whose token `token` is.
    fn owned_session(&self, token: &str) -> Option<String> {
        Connection::open(self.home().join("admin/feld.db"))
            .unwrap()
            .query_row(
                "SELECT id FROM sessions WHERE token = ?1 AND ownership = 'owned'",
                [token],
                |row| row.get(0),
            )
            .ok()
    }

    /// The directory a CLI writes `request` into: its Session's own for the token of the
    /// environment, the shared one otherwise.
    fn directory_of(&self, request: &Value) -> PathBuf {
        let first = &request["credentials"][0];
        match (first["source"].as_str(), first["token"].as_str()) {
            (Some("environment"), Some(token)) => self.directory(token),
            _ => self.spool().join("shared"),
        }
    }

    fn command(&self) -> Command {
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(self.home())
            .env("HOME", self.home())
            .env_remove("TMUX")
            .env_remove("TMUX_PANE");
        command
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        self.command().args(arguments).output().unwrap()
    }

    fn succeed(&self, arguments: &[&str]) {
        let output = self.feld(arguments);
        assert!(output.status.success(), "feld {arguments:?}: {output:?}");
    }

    fn start_job(&self, actor: &str) -> String {
        let socket = self.socket.to_str().unwrap();
        self.succeed(&[
            "actor",
            "create",
            actor,
            "--kind",
            "job",
            "--tmux-socket",
            socket,
            "--start",
            "--",
            "sleep",
            "120",
        ]);
        Connection::open(self.home().join("admin/feld.db"))
            .unwrap()
            .query_row(
                "SELECT s.token FROM sessions s JOIN actors a ON a.id = s.actor_id
                 WHERE a.name = ?1 AND s.end IS NULL",
                [actor],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// The names of the files in the directories of the spool, sorted.
    fn listing(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.spool())
            .unwrap()
            .flat_map(|directory| fs::read_dir(directory.unwrap().path()).unwrap())
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// Writes `request` the way a CLI does, holding its lock while it waits for the response.
    fn exchange(&self, request: &Value) -> Value {
        self.exchange_in(&self.directory_of(request), request)
    }

    /// Writes `request` into `directory` of the spool, as [`Self::exchange`] does.
    fn exchange_in(&self, directory: &Path, request: &Value) -> Value {
        let (id, _lock) = self.write_request_in(directory, request, true);
        let response = directory.join(format!("{id}.response"));
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(bytes) = fs::read(&response) {
                fs::remove_file(&response).unwrap();
                return serde_json::from_slice(&bytes).unwrap();
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("no response to {request}; spool: {:?}", self.listing());
    }

    fn write_request(&self, request: &Value, lock: bool) -> (String, File) {
        self.write_request_in(&self.directory_of(request), request, lock)
    }

    fn write_request_in(&self, directory: &Path, request: &Value, lock: bool) -> (String, File) {
        let id = format!("{:032x}", rand_u128());
        let partial = directory.join(format!("{id}.request.tmp"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&partial)
            .unwrap();
        if lock {
            file.lock_exclusive().unwrap();
        }
        file.write_all(request.to_string().as_bytes()).unwrap();
        fs::rename(&partial, directory.join(format!("{id}.request"))).unwrap();
        (id, file)
    }

    fn api_token(&self) -> String {
        fs::read_to_string(self.home().join("admin/api-token"))
            .unwrap()
            .trim()
            .to_owned()
    }

    fn tmux(&self, arguments: &[&str]) {
        let mut command = Command::new("tmux");
        let _ = support::without_outside_variables(&mut command)
            .arg("-S")
            .arg(&self.socket)
            .args(arguments)
            .output();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
        self.tmux(&["kill-server"]);
    }
}

fn rand_u128() -> u128 {
    let mut bytes = [0u8; 16];
    use std::io::Read as _;
    File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut bytes)
        .unwrap();
    u128::from_le_bytes(bytes)
}

fn request(path: &str, credentials: Value) -> Value {
    request_of(path, credentials, std::process::id())
}

/// A request a process `caller` states it wrote.
fn request_of(path: &str, credentials: Value, caller: u32) -> Value {
    json!({
        "version": 1,
        "method": "GET",
        "path": path,
        "contentType": null,
        "body": null,
        "credentials": credentials,
        "caller": { "pid": caller, "pidNamespace": null },
    })
}

fn environment(token: &str) -> Value {
    json!([{ "source": "environment", "token": token }])
}

fn message(response: &Value) -> Value {
    serde_json::from_str::<Value>(response["body"].as_str().unwrap()).unwrap()["message"].clone()
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

/// The spool carries the requests of a Session token as TCP does, refuses the other tokens,
/// the paths it does not carry and the files it cannot read, and answers each of many
/// requests written at once.
#[test]
fn spool_carries_session_requests_and_refuses_the_rest() {
    let fixture = Fixture::new();
    fixture.succeed(&["daemon", "start"]);
    let token = fixture.start_job("worker");
    let local = fixture.api_token();

    let own = fixture.exchange(&request("/v1/self", environment(&token)));
    assert_eq!(own["status"], 200, "{own}");
    let body: Value = serde_json::from_str(own["body"].as_str().unwrap()).unwrap();
    assert_eq!(body["session"]["actor"], "worker");

    let sent = fixture.exchange(&json!({
        "version": 1,
        "method": "POST",
        "path": "/v1/workspaces/default/actors/worker/messages",
        "contentType": "application/json",
        "body": json!({ "body": "through the spool" }).to_string(),
        "credentials": environment(&token),
        "caller": { "pid": std::process::id(), "pidNamespace": null },
    }));
    assert_eq!(sent["status"], 200, "{sent}");

    for credentials in [
        environment(&local),
        json!([
            { "source": "environment", "token": token },
            { "source": "environment", "token": local },
        ]),
    ] {
        let refused = fixture.exchange(&request("/v1/workspaces", credentials));
        assert_eq!(refused["status"], 403, "{refused}");
        assert_eq!(message(&refused), "the spool accepts Session tokens only");
    }

    let events = fixture.exchange(&request("/v1/events", environment(&token)));
    assert_eq!(events["status"], 400);
    assert_eq!(
        message(&events),
        "this API path is not available over the spool"
    );
    let absolute = fixture.exchange(&request("http://x/v1/events", environment(&token)));
    assert_eq!(absolute["status"], 400);
    assert_eq!(
        message(&absolute),
        "the spool request path is not an absolute path"
    );
    let mut future = request("/v1/self", environment(&token));
    future["version"] = json!(2);
    assert_eq!(fixture.exchange(&future)["status"], 400);
    assert_eq!(fixture.exchange(&json!("not a request"))["status"], 400);

    for credentials in [
        json!([]),
        json!([{ "source": "provider", "plugin": "tmux", "token": token }]),
    ] {
        let untokened = fixture.exchange(&request("/v1/self", credentials));
        assert_eq!(untokened["status"], 401, "{untokened}");
    }

    let answers: Vec<Value> = thread::scope(|scope| {
        let workers: Vec<_> = (0..16)
            .map(|_| scope.spawn(|| fixture.exchange(&request("/v1/self", environment(&token)))))
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect()
    });
    assert!(answers.iter().all(|answer| answer["status"] == 200));
    assert_eq!(fixture.listing(), Vec::<String>::new());
}

/// Each `owned` Session has its own directory of the spool, which takes that Session's token
/// alone and goes when the Session ends; the shared directory takes no token of an `owned`
/// Session.
#[test]
fn a_directory_of_the_spool_takes_its_own_sessions_requests_only() {
    let fixture = Fixture::new();
    fixture.succeed(&["daemon", "start"]);
    let worker = fixture.start_job("worker");
    let peer = fixture.start_job("peer");
    let worker_directory = fixture.directory(&worker);
    let peer_directory = fixture.directory(&peer);
    assert_ne!(worker_directory, peer_directory);
    for directory in [
        &worker_directory,
        &peer_directory,
        &fixture.spool().join("shared"),
    ] {
        let mode = fs::metadata(directory).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{}", directory.display());
    }

    let own = fixture.exchange_in(
        &worker_directory,
        &request("/v1/self", environment(&worker)),
    );
    assert_eq!(own["status"], 200, "{own}");
    for directory in [&peer_directory, &fixture.spool().join("shared")] {
        let other = fixture.exchange_in(directory, &request("/v1/self", environment(&worker)));
        assert_eq!(other["status"], 403, "{other}");
        assert_eq!(
            message(&other),
            "the Session token does not belong to this directory of the spool"
        );
    }

    fixture.succeed(&["session", "stop", "peer"]);
    wait_until("the directory of the ended Session goes", || {
        !peer_directory.exists()
    });
    assert!(worker_directory.exists());
}

/// A request whose CLI ended before it was taken is dropped unanswered, a request the
/// daemon before was handling is answered as maybe applied, a file the daemon does not read
/// is left unread, and the scan removes what grows old.
#[test]
fn spool_settles_abandoned_left_over_and_old_files() {
    let fixture = Fixture::new();
    fixture.succeed(&["daemon", "start"]);
    let token = fixture.start_job("worker");
    let directory = fixture.directory(&token);

    let (abandoned, file) = fixture.write_request(&request("/v1/self", environment(&token)), false);
    drop(file);
    wait_until("the abandoned request is taken and dropped", || {
        fixture.listing().is_empty()
    });
    thread::sleep(Duration::from_millis(500));
    assert!(!directory.join(format!("{abandoned}.response")).exists());

    let elsewhere = fixture.home().join("elsewhere.json");
    fs::write(
        &elsewhere,
        request("/v1/self", environment(&token)).to_string(),
    )
    .unwrap();
    let linked = format!("{:032x}.request", rand_u128());
    std::os::unix::fs::symlink(&elsewhere, directory.join(&linked)).unwrap();

    fixture.succeed(&["daemon", "stop"]);
    let left = format!("{:032x}", rand_u128());
    fs::write(directory.join(format!("{left}.taken")), b"{}").unwrap();
    // The daemon before stopped while it wrote the response.
    fs::write(directory.join(format!("{left}.response.tmp")), b"{\"ver").unwrap();
    let old = [
        format!("{:032x}.response", rand_u128()),
        format!("{:032x}.withdrawn", rand_u128()),
        format!("{:032x}.request.tmp", rand_u128()),
    ];
    for name in &old {
        let path = directory.join(name);
        fs::write(&path, b"{}").unwrap();
        let touched = Command::new("touch")
            .args(["-t", "202001010000"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(touched.success());
    }
    fixture.succeed(&["daemon", "start"]);

    let response = directory.join(format!("{left}.response"));
    wait_until("the left-over request is answered", || response.exists());
    let answer: Value = serde_json::from_slice(&fs::read(&response).unwrap()).unwrap();
    assert_eq!(answer["status"], 503);
    assert!(!directory.join(format!("{left}.taken")).exists());
    assert!(!directory.join(format!("{left}.response.tmp")).exists());
    assert_eq!(
        message(&answer),
        "the Feld daemon restarted while handling the request; it may have been applied"
    );
    wait_until("the old files are removed", || {
        old.iter().all(|name| !directory.join(name).exists())
    });
    thread::sleep(Duration::from_millis(1500));
    assert!(response.exists(), "a fresh response stays for its CLI");
    assert!(
        fs::symlink_metadata(directory.join(&linked)).is_ok(),
        "the symlink is left unread for the scan"
    );
    assert!(elsewhere.exists());
}

/// The profile of a sandbox like Codex's `workspace-write`: TCP to loopback and starting
/// `ps` are denied, and of the Feld home only `writable`, one directory of the spool, is
/// writable. `admin/` is unreadable, and with `narrowed`, as under the permission profile
/// of an `owned` Session, so are `tokens/` and the spool but `writable`.
#[cfg(target_os = "macos")]
fn sandbox_profile(home: &Path, writable: &Path, narrowed: bool) -> String {
    format!(
        "{}(deny network-outbound (remote ip \"localhost:*\"))",
        network_sandbox_profile(home, writable, narrowed)
    )
}

/// [`sandbox_profile`] with TCP to loopback allowed, like `network_access: true`.
#[cfg(target_os = "macos")]
fn network_sandbox_profile(home: &Path, writable: &Path, narrowed: bool) -> String {
    let mut profile = format!(
        "(version 1)(allow default)\
         (deny process-exec (literal \"/bin/ps\"))\
         (deny file-write* (subpath \"{}\"))\
         (allow file-write* (subpath \"{}\"))\
         (deny file-read* (subpath \"{}\"))",
        home.display(),
        writable.display(),
        home.join("admin").display()
    );
    if narrowed {
        profile.push_str(&format!(
            "(deny file-read* (subpath \"{}\") (subpath \"{}\"))\
             (allow file-read* (subpath \"{}\"))",
            home.join("tokens").display(),
            home.join("spool").display(),
            writable.display()
        ));
    }
    profile
}

/// `feld` in the sandbox of the `owned` Session of `token`, which writes the Session's own
/// directory of the spool, or, without a token, in one that writes the shared directory.
#[cfg(target_os = "macos")]
fn sandboxed(fixture: &Fixture, arguments: &[&str], token: Option<&str>) -> Command {
    let home = fixture.home().canonicalize().unwrap();
    let session = token.and_then(|token| fixture.owned_session(token));
    let writable = home
        .join("spool")
        .join(session.as_deref().unwrap_or("shared"));
    let profile = sandbox_profile(&home, &writable, session.is_some());
    let mut command = Command::new("sandbox-exec");
    support::without_outside_variables(&mut command)
        .args(["-p", &profile])
        .arg(env!("CARGO_BIN_EXE_feld"))
        .arg("--home")
        .arg(&home)
        .args(arguments)
        .env("HOME", &home)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE");
    if let Some(token) = token {
        command.env("FELD_SESSION_TOKEN", token);
    }
    if let Some(session) = session {
        command.env("FELD_SESSION_ID", session);
    }
    command
}

/// Inside a sandbox that denies TCP, the CLI of an `owned` Session reads itself and the
/// daemon status, sends and waits for Messages through the spool, and a call without a Session token is sent outside
/// the sandbox. Outside the sandbox the CLI keeps to TCP.
#[cfg(target_os = "macos")]
#[test]
fn cli_inside_a_sandbox_goes_through_the_spool() {
    let fixture = Fixture::new();
    fixture.succeed(&["daemon", "start"]);
    let worker = fixture.start_job("worker");
    let peer = fixture.start_job("peer");

    let own = sandboxed(&fixture, &["self", "get", "--json"], Some(&worker))
        .output()
        .unwrap();
    assert!(own.status.success(), "{own:?}");
    let own: Value = serde_json::from_slice(&own.stdout).unwrap();
    assert_eq!(own["session"]["actor"], "worker");

    let waiting = sandboxed(
        &fixture,
        &["self", "message", "recv", "--wait", "--json"],
        Some(&peer),
    )
    .stdout(std::process::Stdio::piped())
    .spawn()
    .unwrap();
    wait_until("the waiting read is taken", || {
        fixture
            .listing()
            .iter()
            .any(|name| name.ends_with(".taken"))
    });
    let sent = sandboxed(
        &fixture,
        &["message", "send", "peer", "from the sandbox"],
        Some(&worker),
    )
    .output()
    .unwrap();
    assert!(sent.status.success(), "{sent:?}");
    let received = waiting.wait_with_output().unwrap();
    assert!(received.status.success(), "{received:?}");
    let received: Value = serde_json::from_slice(&received.stdout).unwrap();
    assert_eq!(received["messages"][0]["body"], "from the sandbox");
    assert_eq!(received["messages"][0]["from"]["name"], "worker");

    let status = sandboxed(&fixture, &["daemon", "status", "--json"], Some(&worker))
        .output()
        .unwrap();
    assert!(status.status.success(), "{status:?}");
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["state"], "running");

    for arguments in [&["actor", "ls"][..], &["daemon", "status"]] {
        let admin = sandboxed(&fixture, arguments, None).output().unwrap();
        assert_eq!(admin.status.code(), Some(1), "{admin:?}");
        assert!(
            String::from_utf8_lossy(&admin.stderr).contains("run it outside the sandbox"),
            "{admin:?}"
        );
    }

    // `--self` is refused with the attach of the caller's pane, to run outside the sandbox.
    let attach = sandboxed(
        &fixture,
        &["actor", "attach", "--self", "--runtime", "claude-code"],
        None,
    )
    .env("TMUX", "/private/tmp/tmux-501/default,123,0")
    .env("TMUX_PANE", "%7")
    .output()
    .unwrap();
    assert_eq!(attach.status.code(), Some(1), "{attach:?}");
    assert!(
        String::from_utf8_lossy(&attach.stderr).contains(
            "run it outside the sandbox:\n  feld actor attach <name> --workspace default \
             --runtime claude-code --tmux-pane %7 --tmux-socket /private/tmp/tmux-501/default"
        ),
        "{attach:?}"
    );

    assert!(fixture.feld(&["actor", "ls"]).status.success());
    assert_eq!(fixture.listing(), Vec::<String>::new());
}

/// Inside a sandbox that allows TCP but denies `admin/`, the CLI of an `owned` Session asks
/// the daemon over TCP with its Session token, without the spool it may not write.
#[cfg(target_os = "macos")]
#[test]
fn cli_denied_the_local_token_reaches_the_daemon_over_tcp() {
    let fixture = Fixture::new();
    fixture.succeed(&["daemon", "start"]);
    let worker = fixture.start_job("worker");
    let peer = fixture.start_job("peer");
    let home = fixture.home().canonicalize().unwrap();
    let profile = network_sandbox_profile(&home, &home.join("nowhere"), true);
    let feld = |arguments: &[&str]| {
        let mut command = Command::new("sandbox-exec");
        support::without_outside_variables(&mut command)
            .args(["-p", &profile])
            .arg(env!("CARGO_BIN_EXE_feld"))
            .arg("--home")
            .arg(&home)
            .args(arguments)
            .env("HOME", &home)
            .env("FELD_SESSION_TOKEN", &worker)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE");
        if let Some(session) = fixture.owned_session(&worker) {
            command.env("FELD_SESSION_ID", session);
        }
        command.output().unwrap()
    };

    let status = feld(&["daemon", "status", "--json"]);
    assert!(status.status.success(), "{status:?}");
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["state"], "running");

    let sent = feld(&["message", "send", "peer", "over TCP"]);
    assert!(sent.status.success(), "{sent:?}");
    let received = messages_of(&fixture, &peer);
    assert_eq!(received["messages"][0]["from"]["name"], "worker");
}

/// A waiting CLI cancels its request when interrupted, and tells whether a request reached
/// the daemon when the daemon stops under it or is not running.
#[cfg(target_os = "macos")]
#[test]
fn a_waiting_cli_cancels_on_a_signal_and_reports_a_stopped_daemon() {
    let fixture = Fixture::new();
    fixture.succeed(&["daemon", "start"]);
    let token = fixture.start_job("worker");
    let wait = ["self", "message", "recv", "--wait"];
    // The CLI asks the daemon status through the spool before it waits, so only the taken
    // read of the Messages is the request the daemon waits on.
    let taken = || {
        fs::read_dir(fixture.directory(&token))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "taken")
            })
            .filter_map(|path| fs::read(path).ok())
            .filter_map(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .any(|request| {
                request["path"]
                    .as_str()
                    .is_some_and(|path| path.starts_with("/v1/self/messages"))
            })
    };

    let interrupted = sandboxed(&fixture, &wait, Some(&token)).spawn().unwrap();
    wait_until("the waiting read is taken", taken);
    unsafe { libc::kill(interrupted.id() as i32, libc::SIGINT) };
    let interrupted = interrupted.wait_with_output().unwrap();
    assert_eq!(interrupted.status.code(), Some(130));
    wait_until("the daemon cancels the read", || {
        fixture.listing().is_empty()
    });

    let stopped = sandboxed(&fixture, &wait, Some(&token))
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    wait_until("the waiting read is taken", taken);
    fixture.succeed(&["daemon", "stop"]);
    let stopped = stopped.wait_with_output().unwrap();
    assert_eq!(stopped.status.code(), Some(2), "{stopped:?}");
    assert!(
        String::from_utf8_lossy(&stopped.stderr).contains(
            "the Feld daemon stopped while handling the request; it may have been applied"
        ),
        "{stopped:?}"
    );

    let absent = sandboxed(&fixture, &["self", "get"], Some(&token))
        .output()
        .unwrap();
    assert_eq!(absent.status.code(), Some(2), "{absent:?}");
    assert!(String::from_utf8_lossy(&absent.stderr).contains("no Feld daemon is running"));
}

/// The token of the open Session of `actor`.
fn session_token_of(fixture: &Fixture, actor: &str) -> String {
    Connection::open(fixture.home().join("admin/feld.db"))
        .unwrap()
        .query_row(
            "SELECT s.token FROM sessions s JOIN actors a ON a.id = s.actor_id
             WHERE a.name = ?1 AND s.end IS NULL",
            [actor],
            |row| row.get(0),
        )
        .unwrap()
}

/// Starts a stand-in Runtime named `name` in a new tmux session `session`, which runs
/// `script` with `sh` before it waits; answers false when this machine builds no stand-in.
fn start_runtime(fixture: &Fixture, name: &str, session: &str, script: Option<&str>) -> bool {
    let child = script.map(|script| {
        let path = fixture.home().join(format!("{session}.sh"));
        fs::write(&path, script).unwrap();
        format!("sh '{}'", path.display())
    });
    if !support::compile_runtime(&fixture.home().join(name), child.as_deref()) {
        return false;
    }
    let mut command = Command::new("tmux");
    let started = support::without_outside_variables(&mut command)
        .arg("-S")
        .arg(&fixture.socket)
        .args(["new-session", "-d", "-s", session, "-c"])
        .arg(fixture.home())
        .args(["--", &format!("exec ./{name}")])
        .status()
        .unwrap();
    assert!(started.success());
    true
}

fn attach(fixture: &Fixture, actor: &str, session: &str) {
    fixture.succeed(&[
        "actor",
        "attach",
        actor,
        "--tmux-pane",
        session,
        "--tmux-socket",
        fixture.socket.to_str().unwrap(),
        "--runtime",
        "claude-code",
    ]);
}

/// Inside a sandbox that denies TCP and the process table, the CLI under an attached
/// Runtime hands its self index token to the daemon, which checks the caller descends from
/// the Runtime; `feld self token` there answers only the environment's token.
#[cfg(target_os = "macos")]
#[test]
fn cli_under_an_attached_runtime_is_checked_by_the_daemon() {
    let fixture = Fixture::new();
    fixture.succeed(&["daemon", "start"]);
    let home = fixture.home().canonicalize().unwrap();
    let feld = env!("CARGO_BIN_EXE_feld");
    let profile = sandbox_profile(&home, &home.join("spool/shared"), false);
    let out = home.join("self.json");
    let token_error = home.join("token.err");
    let script = format!(
        "until ls '{home}'/self-index/*.json > /dev/null 2>&1; do sleep 0.1; done\n\
         sandbox-exec -p '{profile}' {feld} --home '{home}' self token > /dev/null 2> '{token_error}'\n\
         echo $? >> '{token_error}'\n\
         sandbox-exec -p '{profile}' {feld} --home '{home}' self get --json > '{out}.tmp' 2>&1\n\
         mv '{out}.tmp' '{out}'\n",
        home = home.display(),
        out = out.display(),
        token_error = token_error.display(),
    );
    if !start_runtime(&fixture, "claude", "attached", Some(&script)) {
        return;
    }
    attach(&fixture, "attached", "attached");
    wait_until("the sandboxed child reads itself", || out.exists());
    let output = fs::read_to_string(&out).unwrap();
    let own: Value = serde_json::from_str(&output).unwrap_or_else(|_| panic!("{output}"));
    assert_eq!(own["session"]["actor"], "attached");
    let token_error = fs::read_to_string(&token_error).unwrap();
    assert!(
        token_error.contains("cannot verify the self index from inside a sandbox")
            && token_error.ends_with("1\n"),
        "{token_error}"
    );

    // A process outside the Runtime that names the same pane finds the token, and the
    // daemon refuses it as a caller without one.
    let mut command = Command::new("tmux");
    let server = support::without_outside_variables(&mut command)
        .arg("-S")
        .arg(&fixture.socket)
        .args([
            "display-message",
            "-p",
            "-t",
            "attached",
            "#{pid} #{pane_id}",
        ])
        .output()
        .unwrap();
    let server = String::from_utf8(server.stdout).unwrap();
    let (pid, pane) = server.trim().split_once(' ').unwrap();
    let outside = sandboxed(&fixture, &["self", "get"], None)
        .env("TMUX", format!("{},{pid},0", fixture.socket.display()))
        .env("TMUX_PANE", pane)
        .output()
        .unwrap();
    assert_eq!(outside.status.code(), Some(1), "{outside:?}");
    assert!(
        String::from_utf8_lossy(&outside.stderr).contains("No Session token"),
        "{outside:?}"
    );
    let token = session_token_of(&fixture, "attached");
    let outside = fixture.exchange(&request(
        "/v1/self",
        json!([{ "source": "provider", "plugin": "tmux", "token": token }]),
    ));
    assert_eq!(outside["status"], 401, "{outside}");
}

/// Whether process `ancestor` is among the processes `pid` descends from.
fn descends_from(mut pid: u32, ancestor: u32) -> bool {
    for _ in 0..64 {
        let output = Command::new("ps")
            .args(["-o", "ppid=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let Ok(parent) = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u32>()
        else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        if parent <= 1 {
            return false;
        }
        pid = parent;
    }
    false
}

/// The pid of the program in front of the fixture's tmux session `session`.
fn pane_pid(fixture: &Fixture, session: &str) -> u32 {
    let mut command = Command::new("tmux");
    let output = support::without_outside_variables(&mut command)
        .arg("-S")
        .arg(&fixture.socket)
        .args(["display-message", "-p", "-t", session, "#{pane_pid}"])
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// A token of a Runtime's key passes for a caller with a process the Runtime Adapter's
/// `match` recognises in its parent chain, while the Runtime process the attach recorded
/// runs, and for no other caller.
#[test]
fn a_runtime_key_token_needs_the_runtime_in_the_callers_parent_chain() {
    let fixture = Fixture::with_codex();
    fixture.succeed(&["daemon", "start"]);
    if !start_runtime(&fixture, "claude", "attached", None)
        || !start_runtime(&fixture, "codex", "codex", Some("sleep 120"))
    {
        return;
    }
    attach(&fixture, "attached", "attached");
    let token = session_token_of(&fixture, "attached");
    let credentials = json!([{ "source": "runtime", "plugin": "codex", "token": token }]);

    // The `sleep` the stand-in codex runs through `sh`.
    let codex = pane_pid(&fixture, "codex");
    let mut sleeper = None;
    wait_until("the stand-in codex runs its child", || {
        let listing = Command::new("pgrep")
            .args(["-f", "^sleep 120$"])
            .output()
            .unwrap();
        sleeper = String::from_utf8_lossy(&listing.stdout)
            .lines()
            .filter_map(|line| line.trim().parse::<u32>().ok())
            .find(|pid| descends_from(*pid, codex));
        sleeper.is_some()
    });
    let sleeper = sleeper.unwrap();

    let under = fixture.exchange(&request_of("/v1/self", credentials.clone(), sleeper));
    assert_eq!(under["status"], 200, "{under}");
    // The stand-in in the attached pane runs under the tmux server, outside any codex,
    // whatever this test itself runs under.
    let outsider = pane_pid(&fixture, "attached");
    let outside = fixture.exchange(&request_of("/v1/self", credentials.clone(), outsider));
    assert_eq!(outside["status"], 401, "{outside}");
    let other = json!([{ "source": "runtime", "plugin": "claude-code", "token": token }]);
    let unmatched = fixture.exchange(&request_of("/v1/self", other, sleeper));
    assert_eq!(unmatched["status"], 401, "{unmatched}");

    // Once the attached Runtime ends, its token passes for nobody.
    fixture.tmux(&["kill-session", "-t", "attached"]);
    wait_until("the attached Session is refused", || {
        fixture.exchange(&request_of("/v1/self", credentials.clone(), sleeper))["status"] == 401
    });
}

/// Inside a sandbox with its own PID namespace (bubblewrap, as Codex uses on Linux) where
/// reading the process table is denied, the CLI under an attached Runtime states the pid it
/// has there, and the daemon finds that process in its own namespace to check it.
#[cfg(target_os = "linux")]
#[test]
fn cli_in_another_pid_namespace_is_found_by_the_daemon() {
    // A machine without bubblewrap, or one that denies it a PID namespace, cannot run this.
    let namespaced = Command::new("bwrap")
        .args(["--unshare-pid", "--dev-bind", "/", "/", "true"])
        .status()
        .is_ok_and(|status| status.success());
    if !namespaced {
        return;
    }
    let fixture = Fixture::new();
    fixture.succeed(&["daemon", "start"]);
    let home = fixture.home().canonicalize().unwrap();
    let feld = env!("CARGO_BIN_EXE_feld");
    let out = home.join("self.json");
    let sandbox =
        "bwrap --unshare-pid --dev-bind / / --proc /proc --ro-bind /dev/null \"$(command -v ps)\"";
    let script = format!(
        "until ls '{home}'/self-index/*.json > /dev/null 2>&1; do sleep 0.1; done\n\
         {sandbox} {feld} --home '{home}' self get --json > '{out}.tmp' 2>&1\n\
         mv '{out}.tmp' '{out}'\n",
        home = home.display(),
        out = out.display(),
    );
    if !start_runtime(&fixture, "claude", "attached", Some(&script)) {
        return;
    }
    attach(&fixture, "attached", "attached");
    wait_until("the namespaced child reads itself", || out.exists());
    let output = fs::read_to_string(&out).unwrap();
    let own: Value = serde_json::from_str(&output).unwrap_or_else(|_| panic!("{output}"));
    assert_eq!(own["session"]["actor"], "attached");
}

/// A seccomp filter for `bwrap --seccomp` that denies `connect` with `EPERM`, as Codex's
/// Linux sandbox denies TCP.
#[cfg(target_os = "linux")]
fn deny_connect(path: &Path) {
    // (code, jt, jf, k): load the syscall number; return EPERM for `connect`, else allow.
    let program: [(u16, u8, u8, u32); 4] = [
        (0x20, 0, 0, 0),
        (0x15, 0, 1, libc::SYS_connect as u32),
        (0x06, 0, 0, 0x0005_0000 | libc::EPERM as u32),
        (0x06, 0, 0, 0x7fff_0000),
    ];
    let mut bytes = Vec::new();
    for (code, jt, jf, k) in program {
        bytes.extend_from_slice(&code.to_ne_bytes());
        bytes.extend_from_slice(&[jt, jf]);
        bytes.extend_from_slice(&k.to_ne_bytes());
    }
    fs::write(path, bytes).unwrap();
}

/// Inside a sandbox with its own PID namespace that denies TCP but lets the CLI read the
/// process table of the namespace, as Codex's on Linux, no self index token resolves here;
/// the `feld self` commands and the Messages of the CLI under an attached Runtime go
/// through the spool for the daemon to check, and a caller under no Runtime has no token.
#[cfg(target_os = "linux")]
#[test]
fn cli_resolving_no_token_in_another_pid_namespace_goes_through_the_spool() {
    // A machine without bubblewrap, or one that denies it a PID namespace, cannot run this.
    let namespaced = Command::new("bwrap")
        .args(["--unshare-pid", "--dev-bind", "/", "/", "true"])
        .status()
        .is_ok_and(|status| status.success());
    if !namespaced {
        return;
    }
    let fixture = Fixture::new();
    fixture.succeed(&["daemon", "start"]);
    let peer = fixture.start_job("peer");
    let home = fixture.home().canonicalize().unwrap();
    let feld = env!("CARGO_BIN_EXE_feld");
    let filter = home.join("deny-connect.bpf");
    deny_connect(&filter);
    let sandbox = format!(
        "bwrap --unshare-pid --dev-bind / / --proc /proc --seccomp 3 3< '{}'",
        filter.display()
    );
    let go = home.join("go");
    let out = home.join("out");
    let script = format!(
        "until [ -e '{go}' ]; do sleep 0.1; done\n\
         {sandbox} {feld} --home '{home}' self get --json > '{out}.get' 2>&1\n\
         {sandbox} {feld} --home '{home}' self message recv --json > '{out}.recv' 2>&1\n\
         {sandbox} {feld} --home '{home}' message send peer 'from the namespace' > '{out}.send' 2>&1\n\
         touch '{out}'\n",
        home = home.display(),
        go = go.display(),
        out = out.display(),
    );
    if !start_runtime(&fixture, "claude", "attached", Some(&script)) {
        return;
    }
    attach(&fixture, "attached", "attached");
    fixture.succeed(&["message", "send", "attached", "to the namespace"]);
    fs::write(&go, "").unwrap();
    wait_until("the namespaced child runs its commands", || out.exists());
    let read = |suffix: &str| fs::read_to_string(home.join(format!("out.{suffix}"))).unwrap();
    let get = read("get");
    let own: Value = serde_json::from_str(&get).unwrap_or_else(|_| panic!("{get}"));
    assert_eq!(own["session"]["actor"], "attached");
    let recv = read("recv");
    let received: Value = serde_json::from_str(&recv).unwrap_or_else(|_| panic!("{recv}"));
    assert_eq!(
        received["messages"][0]["body"], "to the namespace",
        "{received}"
    );
    let send = read("send");
    let to_peer = &messages_of(&fixture, &peer)["messages"][0];
    assert_eq!(to_peer["body"], "from the namespace", "{send}");
    assert_eq!(to_peer["from"]["name"], "attached", "{to_peer}");

    let mut command = Command::new("sh");
    let outside = support::without_outside_variables(&mut command)
        .arg("-c")
        .arg(format!(
            "{sandbox} {feld} --home '{home}' self get",
            home = home.display()
        ))
        .env("HOME", &home)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .output()
        .unwrap();
    assert_eq!(outside.status.code(), Some(1), "{outside:?}");
    assert!(
        String::from_utf8_lossy(&outside.stderr).contains("No Session token"),
        "{outside:?}"
    );
}

/// The Messages waiting for the Session of `token`, read over TCP.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn messages_of(fixture: &Fixture, token: &str) -> Value {
    let output = fixture
        .command()
        .args(["self", "message", "recv", "--json"])
        .env("FELD_SESSION_TOKEN", token)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

/// The open Session of `actor`.
fn current_session(fixture: &Fixture, actor: &str) -> String {
    Connection::open(fixture.home().join("admin/feld.db"))
        .unwrap()
        .query_row(
            "SELECT s.id FROM sessions s JOIN actors a ON a.id = s.actor_id
             WHERE a.name = ?1 AND s.end IS NULL",
            [actor],
            |row| row.get(0),
        )
        .unwrap()
}

/// A request with a JSON body.
fn request_with(method: &str, path: &str, body: &Value, token: &str) -> Value {
    json!({
        "version": 1,
        "method": method,
        "path": path,
        "contentType": "application/json",
        "body": body.to_string(),
        "credentials": environment(token),
        "caller": { "pid": std::process::id(), "pidNamespace": null },
    })
}

/// A Session token reaches its own Session and what the grants of its Actor open, and the
/// spool answers as TCP does.
#[test]
fn a_session_token_reaches_what_its_grants_open_over_tcp_and_the_spool() {
    let fixture = Fixture::new();
    fixture.succeed(&["daemon", "start"]);
    let worker = fixture.start_job("worker");
    let lead = fixture.start_job("lead");
    let other = fixture.start_job("other");
    fixture.succeed(&["actor", "update", "lead", "--grant", "manage-agent"]);
    fixture.succeed(&[
        "actor",
        "create",
        "granted",
        "--kind",
        "agent",
        "--runtime",
        "claude-code",
        "--grant",
        "read",
        "--no-start",
    ]);
    let other_session = current_session(&fixture, "other");
    let as_session = |token: &str, arguments: &[&str]| {
        fixture
            .command()
            .env("FELD_SESSION_TOKEN", token)
            .args(arguments)
            .output()
            .unwrap()
    };
    let refused = |output: Output| {
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("403"),
            "{output:?}"
        );
    };
    let create = |name: &str| {
        json!({
            "name": name, "kind": "agent",
            "launchSpec": {"runtime": "claude-code", "workdir": "/tmp", "provider": "tmux", "node": "local"},
        })
    };
    let update = json!({"launchSpec": {"workdir": "/tmp"}});

    // Without grants: no create, no log of another Session.
    refused(as_session(
        &worker,
        &[
            "actor",
            "create",
            "rogue",
            "--kind",
            "agent",
            "--runtime",
            "claude-code",
            "--no-start",
        ],
    ));
    refused(as_session(&worker, &["session", "log", "other"]));
    let spooled = fixture.exchange(&request_with(
        "POST",
        "/v1/workspaces/default/actors",
        &create("rogue"),
        &worker,
    ));
    assert_eq!(spooled["status"], 403, "{spooled}");
    let spooled = fixture.exchange(&request(
        &format!("/v1/sessions/{other_session}/output"),
        environment(&worker),
    ));
    assert_eq!(spooled["status"], 403, "{spooled}");

    // `manage-agent`: an Agent without grants, never one with them.
    let created = as_session(
        &lead,
        &[
            "actor",
            "create",
            "helper",
            "--kind",
            "agent",
            "--runtime",
            "claude-code",
            "--no-start",
        ],
    );
    assert!(created.status.success(), "{created:?}");
    let updated = as_session(&lead, &["actor", "update", "helper", "--workdir", "/tmp"]);
    assert!(updated.status.success(), "{updated:?}");
    refused(as_session(
        &lead,
        &["actor", "update", "granted", "--workdir", "/tmp"],
    ));
    let spooled = fixture.exchange(&request_with(
        "POST",
        "/v1/workspaces/default/actors",
        &create("spooled-helper"),
        &lead,
    ));
    assert_eq!(spooled["status"], 200, "{spooled}");
    let spooled = fixture.exchange(&request_with(
        "PATCH",
        "/v1/workspaces/default/actors/spooled-helper",
        &update,
        &lead,
    ));
    assert_eq!(spooled["status"], 200, "{spooled}");
    let spooled = fixture.exchange(&request_with(
        "PATCH",
        "/v1/workspaces/default/actors/granted",
        &update,
        &lead,
    ));
    assert_eq!(spooled["status"], 403, "{spooled}");

    // Its own Session stops without grants.
    let stopped = as_session(&worker, &["session", "stop", "worker"]);
    assert!(stopped.status.success(), "{stopped:?}");
    let spooled = fixture.exchange(&request_with(
        "POST",
        &format!("/v1/sessions/{other_session}/stop"),
        &json!({}),
        &other,
    ));
    assert_eq!(spooled["status"], 200, "{spooled}");
}
