use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;

use feld_core::home::FeldHome;
use feld_core::self_api::mailbox_lock_is_held;

const FELD: &str = env!("CARGO_BIN_EXE_feld");

mod support;

struct Fixture {
    temporary: TempDir,
    socket: PathBuf,
    bin: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::with(support::install_plugins)
    }

    /// A fixture whose home has the codex Runtime Adapter beside the Plugins Feld ships.
    fn with_codex() -> Self {
        Self::with(support::install_plugins_with_codex)
    }

    fn with(install: impl Fn(&Path)) -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let socket = temporary.path().join("message.sock");
        let bin = temporary.path().join("bin");
        fs::create_dir(&bin).unwrap();
        install(temporary.path());
        Self {
            temporary,
            socket,
            bin,
        }
    }

    /// The `PATH` a part beside a Runtime is started with, where the fixture's own
    /// executables come before the ones of this machine.
    fn path(&self) -> String {
        format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    fn home(&self) -> &Path {
        self.temporary.path()
    }

    fn command(&self) -> Command {
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(self.home())
            .env("HOME", self.home())
            .env("PATH", self.path())
            .env_remove("TMUX")
            .env_remove("TMUX_PANE");
        command
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        self.command().args(arguments).output().unwrap()
    }

    fn json(&self, arguments: &[&str]) -> Value {
        successful_json(arguments, self.feld(arguments))
    }

    fn session_json(&self, token: &str, arguments: &[&str]) -> Value {
        let output = self
            .command()
            .env("FELD_SESSION_TOKEN", token)
            .args(arguments)
            .output()
            .unwrap();
        successful_json(arguments, output)
    }

    fn start(&self) -> Value {
        self.json(&["daemon", "start", "--json"])
    }

    fn create_job(&self, actor: &str, start: bool) -> Value {
        let socket = self.socket.to_str().unwrap();
        let mut arguments = vec![
            "actor",
            "create",
            actor,
            "--kind",
            "job",
            "--tmux-socket",
            socket,
            "--json",
        ];
        if start {
            arguments.push("--start");
        }
        arguments.extend(["--", "sleep", "60"]);
        self.json(&arguments)
    }

    fn current_token(&self, actor: &str) -> (String, String) {
        let connection = Connection::open(self.home().join("admin/feld.db")).unwrap();
        connection
            .query_row(
                "SELECT s.id, s.token FROM sessions s JOIN actors a ON a.id = s.actor_id
                 WHERE a.workspace_key = 'default' AND a.name = ?1 AND a.deleted_at IS NULL
                   AND s.end IS NULL",
                [actor],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    fn api(&self) -> (String, String) {
        let status = self.json(&["daemon", "status", "--json"]);
        (
            status["api"].as_str().unwrap().to_owned(),
            fs::read_to_string(self.home().join("admin/api-token")).unwrap(),
        )
    }

    /// Runs tmux on the fixture socket. The server inherits this environment, so the Feld
    /// variables of a Session the tests may run inside stay out of the panes it opens.
    fn tmux(&self, arguments: &[&str]) -> Output {
        let mut command = Command::new("tmux");
        support::without_outside_variables(&mut command)
            .arg("-S")
            .arg(&self.socket)
            .args(arguments);
        for variable in [
            "CODEX_HOME",
            "CODEX_THREAD_ID",
            "CODEX_SESSION_ID",
            "TMUX",
            "TMUX_PANE",
        ] {
            command.env_remove(variable);
        }
        command.output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
        let _ = self.tmux(&["kill-server"]);
        support::stop_runtime_helpers(self.home());
    }
}

fn successful_json(arguments: &[&str], output: Output) -> Value {
    assert!(
        output.status.success(),
        "feld {} failed:\nstdout: {}\nstderr: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// The Admin gives an Actor its grants, which a replaced declaration keeps, and a Session token
/// that names `grants` is refused before anything is written.
#[test]
fn grants_are_given_by_the_admin_alone() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("worker", true);
    let (_, token) = fixture.current_token("worker");
    let grants = |actor: &Value| actor["grants"].clone();

    let created = fixture.json(&[
        "actor",
        "create",
        "lead",
        "--kind",
        "agent",
        "--runtime",
        "claude-code",
        "--grant",
        "read",
        "--grant",
        "control",
        "--grant",
        "read",
        "--no-start",
        "--json",
    ]);
    assert_eq!(grants(&created["actor"]), json!(["read", "control"]));
    let shown = fixture.feld(&["actor", "get", "lead"]);
    assert!(shown.status.success());
    let shown = String::from_utf8(shown.stdout).unwrap();
    assert!(
        shown
            .lines()
            .any(|line| line.split_whitespace().eq(["grants:", "read,control"])),
        "{shown}"
    );
    let replaced = fixture.json(&[
        "actor",
        "create",
        "lead",
        "--kind",
        "agent",
        "--runtime",
        "claude-code",
        "--no-start",
        "--json",
    ]);
    assert_eq!(grants(&replaced["actor"]), json!(["read", "control"]));

    let refused = |arguments: &[&str]| {
        let output = fixture
            .command()
            .env("FELD_SESSION_TOKEN", &token)
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{arguments:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "rejected");
        let message = error["error"]["message"].as_str().unwrap().to_owned();
        assert!(message.contains("grants"), "{message}");
    };
    refused(&[
        "actor",
        "create",
        "rogue",
        "--kind",
        "agent",
        "--runtime",
        "claude-code",
        "--grant",
        "manage-agent",
        "--no-start",
        "--json",
    ]);
    refused(&["actor", "update", "lead", "--grant", "manage-job", "--json"]);
    refused(&["actor", "update", "lead", "--no-grants", "--json"]);
    assert!(fixture.feld(&["actor", "get", "rogue"]).status.code() == Some(1));
    let (api, _) = fixture.api();
    let patched = Client::new()
        .patch(format!("{api}/v1/workspaces/default/actors/lead"))
        .bearer_auth(&token)
        .json(&json!({"grants": []}))
        .send()
        .unwrap();
    assert_eq!(patched.status(), reqwest::StatusCode::FORBIDDEN);
    // `null` is no way to leave `grants` out, so it never passes for an absent field.
    for (method, path, body) in [
        (
            reqwest::Method::PATCH,
            "/v1/workspaces/default/actors/lead",
            json!({"grants": null, "launchSpec": {"workdir": "/tmp"}}),
        ),
        (
            reqwest::Method::POST,
            "/v1/workspaces/default/actors",
            json!({
                "name": "rogue", "kind": "agent", "grants": null,
                "launchSpec": {"runtime": "claude-code", "workdir": "/tmp", "provider": "tmux", "node": "local"},
            }),
        ),
    ] {
        let refused = Client::new()
            .request(method, format!("{api}{path}"))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .unwrap();
        assert!(refused.status().is_client_error(), "{}", refused.status());
    }
    assert!(fixture.feld(&["actor", "get", "rogue"]).status.code() == Some(1));
    assert_ne!(
        fixture.json(&["actor", "get", "lead", "--json"])["actor"]["launchSpec"]["workdir"],
        "/tmp"
    );
    // Replacing the declaration of an Actor is managing it, which needs `manage-agent`.
    let replaced = fixture
        .command()
        .env("FELD_SESSION_TOKEN", &token)
        .args([
            "actor",
            "create",
            "lead",
            "--kind",
            "agent",
            "--runtime",
            "claude-code",
            "--no-start",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(replaced.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&replaced.stderr).contains("manage-agent"),
        "{}",
        String::from_utf8_lossy(&replaced.stderr)
    );

    let updated = fixture.json(&["actor", "update", "lead", "--grant", "manage-job", "--json"]);
    assert_eq!(grants(&updated["actor"]), json!(["manage-job"]));
    let listed = fixture.json(&["actor", "list", "-a", "--json"]);
    let lead = listed["actors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|actor| actor["name"] == "lead")
        .unwrap();
    assert_eq!(grants(lead), json!(["manage-job"]));
    let cleared = fixture.json(&["actor", "update", "lead", "--no-grants", "--json"]);
    assert_eq!(grants(&cleared["actor"]), json!([]));
    assert_eq!(
        grants(&fixture.json(&["actor", "get", "worker", "--json"])["actor"]),
        json!([])
    );
}

/// `trustWorkdir` is the Admin's to give: a Session token with `manage-agent` that turns it
/// on is refused, and an Actor that trusts its workdir is changed by the Admin alone.
#[test]
fn trust_workdir_is_given_by_the_admin_alone() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("worker", true);
    fixture.json(&[
        "actor",
        "update",
        "worker",
        "--grant",
        "manage-agent",
        "--json",
    ]);
    let (_, token) = fixture.current_token("worker");
    let as_session = |arguments: &[&str]| {
        fixture
            .command()
            .env("FELD_SESSION_TOKEN", &token)
            .args(arguments)
            .output()
            .unwrap()
    };
    let refused = |arguments: &[&str]| {
        let output = as_session(arguments);
        assert_eq!(output.status.code(), Some(1), "{arguments:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "rejected");
        let message = error["error"]["message"].as_str().unwrap().to_owned();
        assert!(message.contains("trustWorkdir"), "{message}");
    };
    let create = |name: &'static str, trust: bool| {
        let mut arguments = vec![
            "actor",
            "create",
            name,
            "--kind",
            "agent",
            "--runtime",
            "claude-code",
            "--no-start",
            "--json",
        ];
        if trust {
            arguments.push("--trust-workdir");
        }
        arguments
    };
    let trusts = |actor: &str| {
        fixture.json(&["actor", "get", actor, "--json"])["actor"]["launchSpec"]["trustWorkdir"]
            .clone()
    };

    refused(&create("rogue", true));
    assert!(fixture.feld(&["actor", "get", "rogue"]).status.code() == Some(1));
    successful_json(&create("plain", false), as_session(&create("plain", false)));
    refused(&["actor", "update", "plain", "--trust-workdir", "--json"]);
    assert_eq!(trusts("plain"), Value::Null);
    let update = ["actor", "update", "plain", "--workdir", "/tmp", "--json"];
    successful_json(&update, as_session(&update));

    let trusted = fixture.json(&["actor", "update", "plain", "--trust-workdir", "--json"]);
    assert_eq!(trusted["actor"]["launchSpec"]["trustWorkdir"], true);
    refused(&["actor", "update", "plain", "--workdir", "/var", "--json"]);
    refused(&["actor", "delete", "plain", "--json"]);
    fixture.json(&create("trusted", true));
    assert_eq!(trusts("trusted"), true);
}

/// Errors answer as `Error`: a Session token on a local-token operation is the wrong kind of
/// token, the caller is told apart before the request is read, and a boolean query parameter
/// without a value is true.
#[test]
fn errors_answer_as_error_after_the_caller_is_told_apart() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("worker", true);
    let (session, token) = fixture.current_token("worker");
    let (api, local_token) = fixture.api();
    let local_token = local_token.trim();
    let client = Client::new();
    let refused = |response: reqwest::blocking::Response, status: reqwest::StatusCode| {
        assert_eq!(response.status(), status);
        let body: Value = response.json().expect("the error is JSON");
        body["message"]
            .as_str()
            .expect("Error has a message")
            .to_owned()
    };

    let shutdown = client
        .post(format!("{api}/v1/daemon/shutdown"))
        .bearer_auth(&token)
        .send()
        .unwrap();
    let message = refused(shutdown, reqwest::StatusCode::FORBIDDEN);
    assert!(message.contains("requires the local token"), "{message}");
    let restart = client
        .post(format!("{api}/v1/daemon/restart"))
        .bearer_auth(&token)
        .send()
        .unwrap();
    let message = refused(restart, reqwest::StatusCode::FORBIDDEN);
    assert!(message.contains("requires the local token"), "{message}");
    let attach = client
        .post(format!("{api}/v1/workspaces/default/actors/worker/attach"))
        .bearer_auth(&token)
        .json(&json!({}))
        .send()
        .unwrap();
    let message = refused(attach, reqwest::StatusCode::FORBIDDEN);
    assert!(message.contains("requires the local token"), "{message}");

    for (path, body) in [
        ("/v1/workspaces", "notjson"),
        ("/v1/self/messages/read", "{}"),
    ] {
        let anonymous = client
            .post(format!("{api}{path}"))
            .header("content-type", "application/json")
            .body(body)
            .send()
            .unwrap();
        refused(anonymous, reqwest::StatusCode::UNAUTHORIZED);
    }
    let malformed = client
        .post(format!("{api}/v1/workspaces"))
        .bearer_auth(local_token)
        .header("content-type", "application/json")
        .body("notjson")
        .send()
        .unwrap();
    refused(malformed, reqwest::StatusCode::BAD_REQUEST);
    let mistyped = client
        .post(format!("{api}/v1/self/messages/read"))
        .bearer_auth(&token)
        .json(&json!({}))
        .send()
        .unwrap();
    refused(mistyped, reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    for path in [
        format!("/v1/sessions/{session}/output?after=-1"),
        "/v1/workspaces?includeDeleted=maybe".to_owned(),
    ] {
        let invalid = client
            .get(format!("{api}{path}"))
            .bearer_auth(local_token)
            .send()
            .unwrap();
        refused(invalid, reqwest::StatusCode::BAD_REQUEST);
    }
    let waiting = client
        .get(format!("{api}/v1/self/messages?wait=maybe"))
        .bearer_auth(&token)
        .send()
        .unwrap();
    refused(waiting, reqwest::StatusCode::BAD_REQUEST);

    for path in [
        "/v1/workspaces?includeDeleted",
        "/v1/sessions?current",
        "/v1/messages?waiting",
    ] {
        let flagged = client
            .get(format!("{api}{path}"))
            .bearer_auth(local_token)
            .send()
            .unwrap();
        assert_eq!(flagged.status(), reqwest::StatusCode::OK, "{path}");
    }
    let current: Value = client
        .get(format!("{api}/v1/sessions?current"))
        .bearer_auth(local_token)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert!(
        current["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|listed| listed["end"].is_null()),
        "?current lists only current Sessions: {current}"
    );

    fixture.json(&["session", "stop", "worker", "--json"]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.json(&["session", "get", &session, "--json"])["session"]["end"].is_null() {
        assert!(Instant::now() < deadline, "the stopped Session closes");
        thread::sleep(Duration::from_millis(50));
    }
    let closed = client
        .post(format!("{api}/v1/workspaces/default/actors"))
        .bearer_auth(&token)
        .header("content-type", "application/json")
        .body("notjson")
        .send()
        .unwrap();
    refused(closed, reqwest::StatusCode::UNAUTHORIZED);
    let closed = client
        .post(format!(
            "{api}/v1/workspaces/default/actors/worker/messages"
        ))
        .bearer_auth(&token)
        .header("content-type", "application/json")
        .body("notjson")
        .send()
        .unwrap();
    let message = refused(closed, reqwest::StatusCode::CONFLICT);
    assert!(message.contains("session closed"), "{message}");
}

/// The Runtime API names the caller's own Session: `GET /v1/self` answers it while it is
/// open and refuses it as session closed once it ends, and the local token names none.
#[test]
fn self_is_the_session_of_the_token() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("worker", true);
    let (session, token) = fixture.current_token("worker");
    let (api, local_token) = fixture.api();
    let client = Client::new();

    let own: Value = client
        .get(format!("{api}/v1/self"))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(own["session"]["id"], session);
    assert_eq!(own["session"]["actor"], "worker");
    assert_eq!(
        client
            .get(format!("{api}/v1/self"))
            .bearer_auth(local_token.trim())
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .get(format!("{api}/v1/self"))
            .bearer_auth("unknown")
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(format!("{api}/v1/self"))
            .send()
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );

    let shown = fixture.session_json(&token, &["self", "get", "--json"]);
    assert_eq!(shown["session"]["id"], session);
    let human = fixture
        .command()
        .env("FELD_SESSION_TOKEN", &token)
        .args(["self", "get"])
        .output()
        .unwrap();
    assert!(human.status.success());
    assert!(
        String::from_utf8_lossy(&human.stdout)
            .lines()
            .any(|line| line.split_whitespace().eq(["workspace:", "default"]))
    );
    let printed = fixture
        .command()
        .env("FELD_SESSION_TOKEN", &token)
        .args(["self", "token"])
        .output()
        .unwrap();
    assert!(printed.status.success());
    assert_eq!(String::from_utf8(printed.stdout).unwrap().trim(), token);

    fixture.json(&["session", "stop", "worker", "--json"]);
    let deadline = Instant::now() + Duration::from_secs(10);
    let closed = loop {
        let response = client
            .get(format!("{api}/v1/self"))
            .bearer_auth(&token)
            .send()
            .unwrap();
        if response.status() != reqwest::StatusCode::OK || Instant::now() > deadline {
            break response;
        }
        thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(closed.status(), reqwest::StatusCode::CONFLICT);
    assert!(
        closed.json::<Value>().unwrap()["message"]
            .as_str()
            .unwrap()
            .starts_with("session closed")
    );
    let closed_cli = fixture
        .command()
        .env("FELD_SESSION_TOKEN", &token)
        .args(["self", "get"])
        .output()
        .unwrap();
    assert_eq!(closed_cli.status.code(), Some(1));
    assert!(closed_cli.stdout.is_empty());
    assert!(String::from_utf8_lossy(&closed_cli.stderr).contains("session closed"));

    // The token of a closed Session names no spawner and no requester.
    for (method, path) in [
        (
            reqwest::Method::POST,
            "/v1/workspaces/default/actors/worker/start".to_owned(),
        ),
        (
            reqwest::Method::POST,
            "/v1/workspaces/default/actors/worker/restart".to_owned(),
        ),
        (
            reqwest::Method::POST,
            format!("/v1/sessions/{session}/stop"),
        ),
        (
            reqwest::Method::POST,
            format!("/v1/sessions/{session}/kill"),
        ),
    ] {
        let refused = client
            .request(method, format!("{api}{path}"))
            .bearer_auth(&token)
            .send()
            .unwrap();
        assert_eq!(refused.status(), reqwest::StatusCode::CONFLICT, "{path}");
        assert!(
            refused.json::<Value>().unwrap()["message"]
                .as_str()
                .unwrap()
                .starts_with("session closed")
        );
    }

    // The Actor still points at its closed current Session.
    let ended = fixture.json(&["session", "get", "worker", "--json"]);
    assert_eq!(ended["session"]["id"], session);
    assert!(ended["session"]["end"].is_string());
}

#[test]
fn message_delivery_is_atomic_persistent_and_generation_independent() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("sender", true);
    fixture.create_job("receiver", true);
    let (sender_session, sender_token) = fixture.current_token("sender");
    let (receiver_session, receiver_token) = fixture.current_token("receiver");
    let (api, local_token) = fixture.api();
    let client = Client::new();
    let local_recv = fixture.feld(&["self", "message", "recv", "--json"]);
    assert_eq!(local_recv.status.code(), Some(1));
    assert!(local_recv.stdout.is_empty());
    let local_error: Value = serde_json::from_slice(&local_recv.stderr).unwrap();
    assert_eq!(local_error["error"]["kind"], "rejected");

    let spoof = client
        .post(format!(
            "{api}/v1/workspaces/default/actors/receiver/messages"
        ))
        .bearer_auth(&sender_token)
        .json(&json!({"body":"spoof", "from":{"type":"admin"}}))
        .send()
        .unwrap();
    assert_eq!(spoof.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);

    let waiting_api = api.clone();
    let waiting_token = receiver_token.clone();
    let waiter = thread::spawn(move || {
        Client::new()
            .get(format!("{waiting_api}/v1/self/messages?limit=1&wait=true"))
            .bearer_auth(waiting_token)
            .send()
            .unwrap()
            .json::<Value>()
            .unwrap()
    });
    thread::sleep(Duration::from_millis(100));
    let sent: Value = client
        .post(format!(
            "{api}/v1/workspaces/default/actors/receiver/messages"
        ))
        .bearer_auth(&sender_token)
        .json(&json!({"body":"review this"}))
        .send()
        .unwrap()
        .json()
        .unwrap();
    let message_id = sent["message"]["id"].as_str().unwrap().to_owned();
    assert_eq!(sent["message"]["from"]["name"], "sender");
    assert_eq!(sent["message"]["fromSessionId"], sender_session);
    assert_eq!(sent["message"]["toSessionId"], receiver_session);
    assert_eq!(sent["message"]["waiting"], true);
    assert_eq!(sent["message"]["anchor"], format!("feld:{message_id}"));
    assert_eq!(waiter.join().unwrap()["messages"][0]["id"], message_id);
    let accepted =
        chrono::DateTime::parse_from_rfc3339(sent["message"]["accepted"].as_str().unwrap())
            .unwrap();
    let start = accepted
        .with_timezone(&chrono::FixedOffset::east_opt(9 * 3600).unwrap())
        .to_rfc3339();
    let end = (accepted + chrono::Duration::seconds(1)).to_rfc3339();
    let ranged: Value = client
        .get(format!("{api}/v1/messages"))
        .bearer_auth(local_token.trim())
        .query(&[("workspace", "default"), ("start", &start), ("end", &end)])
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(ranged["messages"][0]["id"], message_id);
    let empty: Value = client
        .get(format!("{api}/v1/messages"))
        .bearer_auth(local_token.trim())
        .query(&[("start", &start), ("end", &start)])
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(empty["messages"], json!([]));

    let deliveries = (0..8)
        .map(|_| {
            let api = api.clone();
            let token = receiver_token.clone();
            thread::spawn(move || {
                Client::new()
                    .get(format!("{api}/v1/self/messages?limit=1"))
                    .bearer_auth(token)
                    .send()
                    .unwrap()
                    .json::<Value>()
                    .unwrap()["messages"][0]["id"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
        })
        .collect::<Vec<_>>();
    assert!(
        deliveries
            .into_iter()
            .all(|join| join.join().unwrap() == message_id)
    );

    let reads = (0..8)
        .map(|_| {
            let api = api.clone();
            let token = receiver_token.clone();
            let message_id = message_id.clone();
            thread::spawn(move || {
                Client::new()
                    .post(format!("{api}/v1/self/messages/read"))
                    .bearer_auth(token)
                    .json(&vec![message_id])
                    .send()
                    .unwrap()
                    .json::<Value>()
                    .unwrap()["reads"][0]["readId"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
        })
        .collect::<Vec<_>>();
    let read_ids = reads
        .into_iter()
        .map(|join| join.join().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(read_ids.len(), 1);

    let parallel = (0..12)
        .map(|index| {
            let api = api.clone();
            let token = sender_token.clone();
            thread::spawn(move || {
                Client::new()
                    .post(format!(
                        "{api}/v1/workspaces/default/actors/receiver/messages"
                    ))
                    .bearer_auth(token)
                    .json(&json!({"body": format!("parallel {index}")}))
                    .send()
                    .unwrap()
                    .json::<Value>()
                    .unwrap()["message"]["fifo"]
                    .as_u64()
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let positions = parallel
        .into_iter()
        .map(|join| join.join().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(positions.len(), 12);
    let waiting = fixture.json(&["message", "list", "--to", "receiver", "--waiting", "--json"]);
    assert_eq!(waiting["messages"].as_array().unwrap().len(), 12);
    let from_sender = fixture.json(&["message", "list", "--from", "sender", "--json"]);
    assert_eq!(from_sender["messages"].as_array().unwrap().len(), 13);
    let from_receiver = fixture.json(&["message", "list", "--from", "receiver", "--json"]);
    assert_eq!(from_receiver["messages"], json!([]));
    let received = fixture.session_json(&receiver_token, &["self", "message", "recv", "--json"]);
    assert_eq!(received["messages"].as_array().unwrap().len(), 12);
    assert_eq!(received["reads"].as_array().unwrap().len(), 12);
    assert!(
        received["reads"]
            .as_array()
            .unwrap()
            .iter()
            .all(|read| { read["sessionId"] == receiver_session && read["readId"].is_string() })
    );
    let empty = fixture.session_json(&receiver_token, &["self", "message", "recv", "--json"]);
    assert_eq!(empty, json!({"messages": [], "reads": []}));

    let atomic: Value = client
        .post(format!(
            "{api}/v1/workspaces/default/actors/receiver/messages"
        ))
        .bearer_auth(local_token.trim())
        .json(&json!({"body":"atomic"}))
        .send()
        .unwrap()
        .json()
        .unwrap();
    let foreign: Value = client
        .post(format!(
            "{api}/v1/workspaces/default/actors/sender/messages"
        ))
        .bearer_auth(local_token.trim())
        .json(&json!({"body":"foreign"}))
        .send()
        .unwrap()
        .json()
        .unwrap();
    let rejected_telemetry = client
        .post(format!("{api}/v1/self/messages/read"))
        .bearer_auth(&receiver_token)
        .json(&vec![
            atomic["message"]["id"].as_str().unwrap(),
            foreign["message"]["id"].as_str().unwrap(),
        ])
        .send()
        .unwrap();
    assert_eq!(rejected_telemetry.status(), reqwest::StatusCode::FORBIDDEN);
    let still_unread: Value = client
        .get(format!("{api}/v1/self/messages"))
        .bearer_auth(&receiver_token)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(still_unread["messages"][0]["id"], atomic["message"]["id"]);
    let _: Value = client
        .post(format!("{api}/v1/self/messages/read"))
        .bearer_auth(&receiver_token)
        .json(&vec![atomic["message"]["id"].as_str().unwrap()])
        .send()
        .unwrap()
        .json()
        .unwrap();

    let sender_records = fixture.session_json(
        &sender_token,
        &["record", "list", "--actor", "sender", "--json"],
    );
    assert!(
        sender_records["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["kind"] == "message.sent"
                && record["data"]["messageId"] == message_id)
    );
    let receiver_records = fixture.json(&["record", "list", "--actor", "receiver", "--json"]);
    assert!(
        receiver_records["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["kind"] == "message.accepted"
                    && record["data"]["messageId"] == message_id
                    && record["data"]["waiting"] == false
            })
    );
    assert!(
        receiver_records["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["kind"] == "message.read"
                && record["data"]["messageId"] == message_id)
    );

    fixture.json(&["session", "stop", "receiver", "--json"]);
    let closed = client
        .get(format!("{api}/v1/self/messages"))
        .bearer_auth(&receiver_token)
        .send()
        .unwrap();
    assert_eq!(closed.status(), reqwest::StatusCode::CONFLICT);
    let waiting = fixture.json(&["message", "send", "receiver", "survive restart", "--json"]);
    assert_eq!(waiting["message"]["toSessionId"], Value::Null);
    // Without a current Session the accepted record belongs to the Actor alone.
    let sessionless = fixture.json(&[
        "record",
        "list",
        "--actor",
        "receiver",
        "--kind",
        "message.accepted",
        "--json",
    ]);
    assert!(sessionless["records"].as_array().unwrap().iter().any(
        |record| record["data"]["messageId"] == waiting["message"]["id"]
            && record["sessionId"] == Value::Null
            && record["data"]["waiting"] == true
    ));
    fixture.json(&["daemon", "restart", "--json"]);
    let persisted = fixture.json(&["message", "list", "--to", "receiver", "--json"]);
    assert!(
        persisted["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["id"] == waiting["message"]["id"] && message["waiting"] == true)
    );
    let listed = String::from_utf8(
        fixture
            .feld(&["message", "list", "--to", "receiver"])
            .stdout,
    )
    .unwrap();
    let help = String::from_utf8(fixture.feld(&["message", "list", "--help"]).stdout).unwrap();
    for text in [&listed, &help] {
        assert!(
            text.contains(
                "A read means the Message was handed to a Session, not that it was acted on."
            ),
            "the listing and its help say what a read means: {text}"
        );
    }

    fixture.json(&["actor", "start", "receiver", "--json"]);
    let (replacement_session, replacement_token) = fixture.current_token("receiver");
    let received = fixture.session_json(&replacement_token, &["self", "message", "recv", "--json"]);
    assert_eq!(received["messages"][0]["body"], "survive restart");
    assert_eq!(received["reads"][0]["sessionId"], replacement_session);

    let mut monitor = support::runtime_helper(fixture.home(), "monitor")
        .env("FELD_SESSION_TOKEN", &replacement_token)
        .env("FELD_SESSION_ID", &replacement_session)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let feld_home = FeldHome::resolve(Some(fixture.home().to_owned())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if mailbox_lock_is_held(&feld_home, &replacement_session) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "monitor did not acquire its lock"
        );
        thread::sleep(Duration::from_millis(25));
    }
    fixture.json(&["session", "stop", "receiver", "--json"]);
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if monitor.try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "monitor did not exit after Session token invalidation"
        );
        thread::sleep(Duration::from_millis(50));
    }

    fixture.json(&["session", "stop", "sender", "--json"]);
    let rejected = client
        .post(format!(
            "{api}/v1/workspaces/default/actors/receiver/messages"
        ))
        .bearer_auth(&sender_token)
        .json(&json!({"body":"closed"}))
        .send()
        .unwrap();
    assert_eq!(rejected.status(), reqwest::StatusCode::CONFLICT);
    assert!(
        rejected.json::<Value>().unwrap()["message"]
            .as_str()
            .unwrap()
            .starts_with("session closed")
    );
    assert!(!local_token.trim().is_empty());
}

#[test]
fn generated_claude_plugin_monitor_delivers_after_daemon_restart() {
    if !feld_core::test_require::tmux() {
        return;
    }
    let fixture = Fixture::new();
    let claude = fixture.bin.join("claude");
    fs::write(
        &claude,
        "#!/bin/sh\nplugin=\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = --plugin-dir ]; then plugin=$2; shift 2; else shift; fi\ndone\nexec python3 \"$(dirname \"$0\")/monitor-runner.py\" \"$plugin\"\n",
    )
    .unwrap();
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        fixture.bin.join("monitor-runner.py"),
        r#"import json
import os
import pathlib
import sys

plugin = pathlib.Path(sys.argv[1])
monitors = json.loads((plugin / "monitors/monitors.json").read_text())
assert isinstance(monitors, list) and len(monitors) == 1
entry = monitors[0]
assert entry == {
    "name": "feld",
    "command": "${CLAUDE_PLUGIN_ROOT}/bin/monitor",
    "description": "Feld mailbox: a Message from the Admin or another Actor; its fromKind says whose word it is",
}
os.environ["CLAUDE_PLUGIN_ROOT"] = str(plugin)
os.execv("/bin/sh", ["/bin/sh", "-c", entry["command"]])
"#,
    )
    .unwrap();
    fixture.start();
    let socket = fixture.socket.to_str().unwrap();
    let started = fixture.json(&[
        "actor",
        "create",
        "monitored",
        "--kind",
        "agent",
        "--runtime",
        "claude-code",
        "--tmux-socket",
        socket,
        "--json",
    ]);
    assert_eq!(started["session"]["outcome"], "running");
    let tmux_session = "feld-default-monitored-1";
    assert!(
        fixture
            .tmux(&["resize-window", "-t", tmux_session, "-x", "1000"])
            .status
            .success()
    );
    let feld_home = FeldHome::resolve(Some(fixture.home().to_owned())).unwrap();
    let feld_session = started["session"]["id"].as_str().unwrap();
    // The stand-in claude and the shims the Session's plugin holds are new files, which
    // macOS inspects the first time each runs. That takes seconds while the rest of this
    // file's tmux tests run beside this one.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if mailbox_lock_is_held(&feld_home, feld_session) {
            break;
        }
        if Instant::now() >= deadline {
            let capture = fixture.tmux(&["capture-pane", "-p", "-t", tmux_session]);
            panic!(
                "generated monitor did not acquire its Session lock: {}",
                String::from_utf8_lossy(&capture.stdout)
            );
        }
        thread::sleep(Duration::from_millis(25));
    }

    fixture.json(&["daemon", "restart", "--json"]);
    let sent = fixture.json(&["message", "send", "monitored", "plugin delivery", "--json"]);
    let message_id = sent["message"]["id"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let capture = fixture.tmux(&["capture-pane", "-p", "-S", "-100", "-t", tmux_session]);
        let text = String::from_utf8_lossy(&capture.stdout);
        if let Some(line) = text.lines().find(|line| line.contains("feld_message")) {
            let notification: Value = serde_json::from_str(line).unwrap();
            assert_eq!(notification["type"], "feld_message");
            assert_eq!(notification["message"]["id"], message_id);
            assert_eq!(notification["message"]["body"], "plugin delivery");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "monitor output was not delivered: {text}"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let listed = fixture.json(&["message", "list", "--to", "monitored", "--json"]);
        if listed["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["id"] == message_id && message["waiting"] == false)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "monitor did not commit Message read"
        );
        thread::sleep(Duration::from_millis(50));
    }

    let long_body = format!(
        "{}the instruction at the end",
        "This is the body of a long Message.\n".repeat(90)
    );
    assert!(long_body.len() > 3_000);
    let sent = fixture.json(&["message", "send", "monitored", &long_body, "--json"]);
    let long_id = sent["message"]["id"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let capture = fixture.tmux(&["capture-pane", "-p", "-J", "-S", "-100", "-t", tmux_session]);
        let text = String::from_utf8_lossy(&capture.stdout);
        if let Some(line) = text.lines().find(|line| line.contains(long_id)) {
            let line = line.trim_end();
            assert!(line.encode_utf16().count() <= 500, "{line}");
            let notification: Value = serde_json::from_str(line).unwrap();
            let entry = &notification["message"];
            assert!(entry.get("body").is_none());
            let excerpt = entry["excerpt"].as_str().unwrap();
            assert!(long_body.starts_with(excerpt.trim_end_matches('…')));
            assert_eq!(entry["show"], format!("feld message get {long_id}"));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "long Message was not delivered: {text}"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let (_, token) = fixture.current_token("monitored");
    let shown = fixture
        .command()
        .env("FELD_SESSION_TOKEN", &token)
        .args(["message", "get", long_id])
        .output()
        .unwrap();
    assert!(shown.status.success());
    let indented = long_body
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    assert!(
        String::from_utf8(shown.stdout)
            .unwrap()
            .ends_with(&format!("  body:\n{indented}"))
    );
    let shown = fixture.session_json(&token, &["message", "get", long_id, "--json"]);
    assert_eq!(shown["message"]["body"], long_body);
}

/// Only a Message without a read can be deleted, and a deleted one leaves the mailbox, the
/// listing, and the waiting count while `message get` still finds it.
#[test]
fn message_delete_accepts_only_messages_without_a_read() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("receiver", true);
    let (_, token) = fixture.current_token("receiver");
    let (api, _) = fixture.api();

    let read = fixture.json(&["message", "send", "receiver", "handed over", "--json"]);
    let read_id = read["message"]["id"].as_str().unwrap();
    let mut piped = fixture
        .command()
        .args(["message", "send", "receiver", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut piped.stdin.take().unwrap(), "from stdin\n".as_bytes()).unwrap();
    let waiting = successful_json(&["message", "send"], piped.wait_with_output().unwrap());
    assert_eq!(waiting["message"]["body"], "from stdin\n");
    let waiting_id = waiting["message"]["id"].as_str().unwrap();
    let reads: Value = Client::new()
        .post(format!("{api}/v1/self/messages/read"))
        .bearer_auth(&token)
        .json(&vec![read_id])
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(reads["reads"].as_array().unwrap().len(), 1);

    let rejected = fixture.feld(&["message", "delete", read_id, "--json"]);
    assert_eq!(rejected.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&rejected.stderr).unwrap();
    assert_eq!(error["error"]["kind"], "rejected");
    assert!(error["error"]["message"].as_str().unwrap().contains("409"));

    let deleted = fixture.json(&["message", "delete", waiting_id, "--json"]);
    assert_eq!(deleted["message"]["id"], waiting_id);
    assert!(deleted["message"]["deletedAt"].is_string());
    assert_eq!(deleted["message"]["waiting"], false);
    let text = String::from_utf8(fixture.feld(&["message", "get", waiting_id]).stdout).unwrap();
    assert!(text.contains("deleted_at:"), "{text}");
    assert_eq!(
        fixture.json(&["message", "get", read_id, "--json"])["message"]["deletedAt"],
        Value::Null
    );

    let listed = fixture.json(&["message", "list", "--to", "receiver", "--json"]);
    let ids = listed["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, [read_id]);
    let mailbox: Value = Client::new()
        .get(format!("{api}/v1/self/messages"))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(mailbox["messages"], json!([]));
    let actor = fixture.json(&["actor", "get", "receiver", "--json"]);
    assert_eq!(actor["actor"]["waitingCount"], 0);

    let again = fixture.feld(&["message", "delete", waiting_id, "--json"]);
    assert_eq!(again.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&again.stderr).unwrap();
    assert!(error["error"]["message"].as_str().unwrap().contains("404"));
}

/// A Message body that is empty after trimming is rejected and leaves no record, whether it
/// comes as an argument, as empty standard input, or through the daemon API.
#[test]
fn message_send_rejects_an_empty_body() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("receiver", true);
    let (api, local) = fixture.api();

    for body in ["", "   \n\t"] {
        let rejected = fixture.feld(&["message", "send", "receiver", body, "--json"]);
        assert_eq!(rejected.status.code(), Some(1));
        let error: Value = serde_json::from_slice(&rejected.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "rejected");
        let message = error["error"]["message"].as_str().unwrap();
        assert!(message.contains("400") && message.contains("Message body is empty"));
    }
    for stdin in ["", " \n"] {
        let mut piped = fixture
            .command()
            .args(["message", "send", "receiver"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut piped.stdin.take().unwrap(), stdin.as_bytes()).unwrap();
        let rejected = piped.wait_with_output().unwrap();
        assert_eq!(rejected.status.code(), Some(1));
        let stderr = String::from_utf8(rejected.stderr).unwrap();
        assert!(stderr.contains("Message body is empty"), "{stderr}");
    }
    let response = Client::new()
        .post(format!(
            "{api}/v1/workspaces/default/actors/receiver/messages"
        ))
        .bearer_auth(&local)
        .json(&json!({"body": " "}))
        .send()
        .unwrap();
    assert_eq!(response.status(), 400);

    let listed = fixture.json(&["message", "list", "--to", "receiver", "--json"]);
    assert_eq!(listed["messages"], json!([]));
}

/// A label a sender gives its Messages selects them in a list, which shows the label; a
/// Message without one has a null label. A label outside the grammar of Actor names is
/// rejected.
#[test]
fn message_labels_select_the_messages_a_sender_gave_them() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("sender", true);
    fixture.create_job("receiver", true);
    let (_, sender_token) = fixture.current_token("sender");
    let (api, local) = fixture.api();

    for body in ["first nudge", "second nudge"] {
        let sent = fixture.session_json(
            &sender_token,
            &[
                "message", "send", "receiver", "--label", "nudge", body, "--json",
            ],
        );
        assert_eq!(sent["message"]["label"], "nudge");
    }
    fixture.json(&[
        "message",
        "send",
        "receiver",
        "--label",
        "nudge",
        "from the Admin",
        "--json",
    ]);
    let plain = fixture.session_json(
        &sender_token,
        &["message", "send", "receiver", "unlabeled", "--json"],
    );
    assert_eq!(plain["message"]["label"], Value::Null);

    let nudges = fixture.json(&[
        "message", "list", "--from", "sender", "--label", "nudge", "--json",
    ]);
    let bodies = nudges["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| {
            assert_eq!(message["label"], "nudge");
            message["preview"].as_str().unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(bodies, ["first nudge", "second nudge"]);
    let all = fixture.json(&["message", "list", "--to", "receiver", "--json"]);
    assert_eq!(all["messages"].as_array().unwrap().len(), 4);

    let listed = String::from_utf8(
        fixture
            .feld(&["message", "list", "--to", "receiver"])
            .stdout,
    )
    .unwrap();
    assert!(listed.contains("LABEL"), "{listed}");
    let id = nudges["messages"][0]["id"].as_str().unwrap();
    let text = String::from_utf8(fixture.feld(&["message", "get", id]).stdout).unwrap();
    assert!(text.contains("label:        nudge"), "{text}");

    let rejected = fixture.feld(&["message", "send", "receiver", "--label", "Nudge", "x"]);
    assert_eq!(rejected.status.code(), Some(2));
    let response = Client::new()
        .post(format!(
            "{api}/v1/workspaces/default/actors/receiver/messages"
        ))
        .bearer_auth(&local)
        .json(&json!({"label": "-nudge", "body": "x"}))
        .send()
        .unwrap();
    assert_eq!(response.status(), 400);
    let all = fixture.json(&["message", "list", "--to", "receiver", "--json"]);
    assert_eq!(all["messages"].as_array().unwrap().len(), 4);
}

#[test]
fn recv_broken_pipe_returns_operational_error_without_committing_read() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("large-mailbox", true);
    let (_, token) = fixture.current_token("large-mailbox");
    let (api, local_token) = fixture.api();
    let body = "x".repeat(512 * 1024);
    let sent: Value = Client::new()
        .post(format!(
            "{api}/v1/workspaces/default/actors/large-mailbox/messages"
        ))
        .bearer_auth(local_token.trim())
        .json(&json!({"body": body}))
        .send()
        .unwrap()
        .json()
        .unwrap();
    let message_id = sent["message"]["id"].as_str().unwrap();

    for json_output in [true, false] {
        let mut command = fixture.command();
        command
            .env("FELD_SESSION_TOKEN", &token)
            .args(["self", "message", "recv"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if json_output {
            command.arg("--json");
        }
        let mut child = command.spawn().unwrap();
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "broken-pipe recv unexpectedly exited with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        if json_output {
            let error: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(error["error"]["kind"], "operational");
        } else {
            assert!(String::from_utf8_lossy(&output.stderr).starts_with("Error:"));
        }
        let mailbox: Value = Client::new()
            .get(format!("{api}/v1/self/messages"))
            .bearer_auth(&token)
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(mailbox["messages"][0]["id"], message_id);
        assert_eq!(mailbox["messages"][0]["read"], Value::Null);
        assert_eq!(mailbox["messages"][0]["waiting"], true);
    }

    // Every other read ends quietly with 0 once the reader of stdout has gone.
    for arguments in [
        vec!["message", "list", "--to", "large-mailbox", "--json"],
        vec!["message", "get", message_id, "--json"],
    ] {
        let mut child = fixture
            .command()
            .args(&arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "feld {} unexpectedly exited with {:?}: {}",
            arguments.join(" "),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
    }
}

/// A reader that takes the Messages and closes before the reads are written leaves them
/// committed: `recv` ends with 2 and names how to read them again.
#[test]
fn recv_reader_closed_after_commit_names_committed_messages() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("closing-reader", true);
    let (_, token) = fixture.current_token("closing-reader");
    let (api, local_token) = fixture.api();

    for json_output in [false, true] {
        // The reads may reach the pipe before the reader closes, so a run can end with 0.
        let (message_id, output) = (0..10)
            .find_map(|_| {
                let sent: Value = Client::new()
                    .post(format!(
                        "{api}/v1/workspaces/default/actors/closing-reader/messages"
                    ))
                    .bearer_auth(local_token.trim())
                    .json(&json!({"body": "third"}))
                    .send()
                    .unwrap()
                    .json()
                    .unwrap();
                let message_id = sent["message"]["id"].as_str().unwrap().to_owned();
                let mut command = fixture.command();
                command
                    .env("FELD_SESSION_TOKEN", &token)
                    .args(["self", "message", "recv"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                if json_output {
                    command.arg("--json");
                }
                let mut child = command.spawn().unwrap();
                let mut stdout = child.stdout.take().unwrap();
                let mut read = Vec::new();
                let mut buffer = [0; 4096];
                while !String::from_utf8_lossy(&read).contains("third") {
                    let count = std::io::Read::read(&mut stdout, &mut buffer).unwrap();
                    assert_ne!(count, 0, "recv ended before printing the Message");
                    read.extend_from_slice(&buffer[..count]);
                }
                drop(stdout);
                let output = child.wait_with_output().unwrap();
                (output.status.code() != Some(0)).then_some((message_id, output))
            })
            .expect("the reader never closed before the reads were written");
        assert_eq!(output.status.code(), Some(2));
        let message = if json_output {
            let error: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(error["error"]["kind"], "operational");
            error["error"]["message"].as_str().unwrap().to_owned()
        } else {
            String::from_utf8(output.stderr).unwrap()
        };
        assert!(
            message.contains(&format!("mailbox read committed for {message_id}"))
                && message.contains("feld message get <message-id>"),
            "{message}"
        );

        let mailbox: Value = Client::new()
            .get(format!("{api}/v1/self/messages"))
            .bearer_auth(&token)
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(mailbox["messages"], json!([]));
        let got = fixture.json(&["message", "get", &message_id, "--json"]);
        assert_eq!(got["message"]["body"], "third");
    }
}

/// A Codex thread id, which is the UUID its rollout file is named after.
const CODEX_THREAD: &str = "01a0c16a-1e37-76b3-a04a-31f907002406";

/// The `codex` the queue helper reaches on `PATH`: it writes down the arguments of every
/// call, takes each Message into a turn of the thread whose rollout file is `rollout` and,
/// while the marker of a thread without a rollout file is there, refuses the Message the
/// way Codex refuses one it cannot queue yet. Answers the two paths, or nothing when this
/// machine has no C compiler to build the stand-in with.
fn fake_codex(fixture: &Fixture, rollout: &Path) -> Option<(PathBuf, PathBuf)> {
    let calls = fixture.home().join("codex-queue.log");
    let without_rollout = fixture.home().join("codex-without-rollout");
    fs::write(rollout, b"").unwrap();
    support::FakeCodex::new()
        .queueing(&calls)
        .refusing_while(&without_rollout)
        .taking_into(rollout)
        .compile(&fixture.bin.join("codex"))
        .then_some((calls, without_rollout))
}

/// The calls that carried one Message.
fn calls_carrying(calls: &Path, message: &str) -> Vec<Vec<String>> {
    support::codex_queue_calls(calls)
        .into_iter()
        .filter(|call| call.last().is_some_and(|line| line.contains(message)))
        .collect()
}

/// Runs the hook the way Codex runs it, with one event on its stdin. A Session named here
/// is the one an owned Codex carries in its environment; without one, the hook resolves
/// what a Codex a person started resolves.
fn run_hook(
    fixture: &Fixture,
    event: &str,
    rollout: &Path,
    session: Option<(&str, &str)>,
) -> Output {
    use std::io::Write;

    let mut command = support::codex_runtime_helper(fixture.home(), "hook");
    if let Some((token, session)) = session {
        command
            .env("FELD_SESSION_TOKEN", token)
            .env("FELD_SESSION_ID", session);
    }
    let mut hook = command.stdin(Stdio::piped()).spawn().unwrap();
    let input = json!({
        "session_id": CODEX_THREAD,
        "transcript_path": rollout,
        "hook_event_name": event,
        "model": "gpt-5.6",
    });
    hook.stdin
        .as_mut()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    hook.wait_with_output().unwrap()
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "{what}");
        thread::sleep(Duration::from_millis(50));
    }
}

/// Whether the mailbox holds one Message as read.
fn is_read(fixture: &Fixture, actor: &str, message: &str) -> bool {
    fixture.json(&["message", "list", "--to", actor, "--json"])["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|listed| listed["id"] == message && listed["waiting"] == false)
}

/// How many times one Message's read was recorded, which is once however often the helper
/// offered it to Codex.
fn reads_of(fixture: &Fixture, actor: &str, message: &str) -> usize {
    fixture.json(&["record", "list", "--actor", actor, "--json"])["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|record| record["kind"] == "message.read" && record["data"]["messageId"] == message)
        .count()
}

/// A queue helper whose Feld home is gone ends: the Session it would resolve again is lost
/// with that home, so it waits for no daemon there.
#[test]
fn codex_queue_helper_ends_when_its_feld_home_is_gone() {
    let fixture = Fixture::with_codex();
    let home = FeldHome::resolve(Some(fixture.home().to_owned())).unwrap();
    let session = "session_0f3a9c";
    let mut helper = support::codex_runtime_helper(fixture.home(), "queue-helper")
        .env("FELD_SESSION_TOKEN", "fst_gone")
        .env("FELD_SESSION_ID", session)
        .env("CODEX_THREAD_ID", CODEX_THREAD)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    wait_until(
        "the queue helper did not take the Session's mailbox lock",
        || mailbox_lock_is_held(&home, session),
    );
    // No daemon runs in the home, and the helper waits for one to come back.
    thread::sleep(Duration::from_secs(1));
    assert!(helper.try_wait().unwrap().is_none());

    fs::remove_dir_all(fixture.home()).unwrap();
    wait_until("the queue helper outlived its Feld home", || {
        helper.try_wait().unwrap().is_some()
    });
    assert!(!fixture.home().exists());
}

/// The Codex queue helper: it puts each Message into the thread with `codex queue`, records
/// the read only once the thread took it, and leaves the delivery to the one helper that
/// holds the Session's mailbox lock.
#[test]
fn codex_queue_helper_delivers_each_message_once_through_codex_queue() {
    if !feld_core::test_require::tmux() {
        return;
    }
    let fixture = Fixture::with_codex();
    let rollout = fixture.home().join("rollout.jsonl");
    let Some((calls, without_rollout)) = fake_codex(&fixture, &rollout) else {
        return;
    };
    fixture.start();
    fixture.create_job("codex-receiver", true);
    let (session, token) = fixture.current_token("codex-receiver");
    let home = FeldHome::resolve(Some(fixture.home().to_owned())).unwrap();
    let helper = || {
        let mut command = support::codex_runtime_helper(fixture.home(), "queue-helper");
        command
            .env("FELD_SESSION_TOKEN", &token)
            .env("FELD_SESSION_ID", &session)
            .env("CODEX_THREAD_ID", CODEX_THREAD)
            .env("FELD_CODEX_ROLLOUT", &rollout)
            .env("PATH", fixture.path())
            .stdin(Stdio::null());
        command
    };

    // The thread has no rollout file yet, so Codex takes nothing into its queue.
    fs::write(&without_rollout, b"").unwrap();
    let mut delivering = helper().spawn().unwrap();
    wait_until(
        "the queue helper did not take the Session's mailbox lock",
        || mailbox_lock_is_held(&home, &session),
    );
    // A second helper of the same Session leaves the delivery to the first and ends.
    let second = helper().output().unwrap();
    assert!(second.status.success());

    let waiting = fixture.json(&["message", "send", "codex-receiver", "queue this", "--json"]);
    let waiting_id = waiting["message"]["id"].as_str().unwrap().to_owned();
    wait_until("the queue helper did not offer the Message again", || {
        calls_carrying(&calls, &waiting_id).len() >= 2
    });
    // Nothing was handed over, so the Message keeps its place in the mailbox.
    assert!(!is_read(&fixture, "codex-receiver", &waiting_id));

    // The thread has a rollout file from here on, and takes what it is offered.
    fs::remove_file(&without_rollout).unwrap();
    wait_until("the read of the queued Message was not recorded", || {
        is_read(&fixture, "codex-receiver", &waiting_id)
    });
    let call = calls_carrying(&calls, &waiting_id).pop().unwrap();
    assert_eq!(call[..4], ["queue", "--thread", CODEX_THREAD, "--message"]);
    let line: Value = serde_json::from_str(&call[4]).unwrap();
    assert_eq!(line["type"], "feld_message");
    assert_eq!(line["message"]["id"], waiting_id);
    assert_eq!(line["message"]["from"], "Admin");
    assert_eq!(line["message"]["body"], "queue this");

    // Codex takes a Message of any length whole, so a long body reaches the model as it is.
    let long_body = format!(
        "{}the instruction at the end",
        "This is the body of a long Message.\n".repeat(90)
    );
    assert!(long_body.len() > 3_000);
    let long = fixture.json(&["message", "send", "codex-receiver", &long_body, "--json"]);
    let long_id = long["message"]["id"].as_str().unwrap().to_owned();
    wait_until("the long Message was not delivered", || {
        is_read(&fixture, "codex-receiver", &long_id)
    });
    let call = calls_carrying(&calls, &long_id).pop().unwrap();
    let line: Value = serde_json::from_str(&call[4]).unwrap();
    assert_eq!(line["message"]["body"], long_body);
    assert!(line["message"].get("excerpt").is_none());

    // Each Message was handed over once, and its read was recorded once.
    assert_eq!(calls_carrying(&calls, &long_id).len(), 1);
    assert_eq!(reads_of(&fixture, "codex-receiver", &waiting_id), 1);
    assert_eq!(reads_of(&fixture, "codex-receiver", &long_id), 1);

    // The SessionEnd hook asks the helper to stop, and it does.
    let ended = run_hook(
        &fixture,
        "SessionEnd",
        &fixture.home().join("rollout.jsonl"),
        Some((&token, &session)),
    );
    assert!(ended.status.success());
    wait_until(
        "the queue helper did not stop when the Session ended",
        || delivering.try_wait().unwrap().is_some(),
    );
}

/// A Message in the Codex queue that no turn has taken yet, as while a long tool call holds
/// the turn: it is `queued` and still waiting, and its delete is refused, because Codex
/// would hand it to the model all the same. Its read comes once a turn takes it.
#[test]
fn codex_queued_message_waits_for_a_turn_and_refuses_its_delete() {
    use std::io::Write;

    if !feld_core::test_require::tmux() {
        return;
    }
    let fixture = Fixture::with_codex();
    let rollout = fixture.home().join("rollout.jsonl");
    let calls = fixture.home().join("codex-queue.log");
    fs::write(&rollout, b"").unwrap();
    // A Codex whose turn is busy: it queues, and takes nothing in yet.
    if !support::FakeCodex::new()
        .queueing(&calls)
        .compile(&fixture.bin.join("codex"))
    {
        return;
    }
    fixture.start();
    fixture.create_job("codex-receiver", true);
    let (session, token) = fixture.current_token("codex-receiver");
    let mut helper = support::codex_runtime_helper(fixture.home(), "queue-helper")
        .env("FELD_SESSION_TOKEN", &token)
        .env("FELD_SESSION_ID", &session)
        .env("CODEX_THREAD_ID", CODEX_THREAD)
        .env("FELD_CODEX_ROLLOUT", &rollout)
        .env("PATH", fixture.path())
        .stdin(Stdio::null())
        .spawn()
        .unwrap();

    let sent = fixture.json(&["message", "send", "codex-receiver", "busy turn", "--json"]);
    let message = sent["message"]["id"].as_str().unwrap().to_owned();
    wait_until("the Message was not queued into the thread", || {
        !calls_carrying(&calls, &message).is_empty()
    });
    let got = fixture.json(&["message", "get", &message, "--json"])["message"].clone();
    assert_eq!(got["waiting"], true);
    assert_eq!(got["queued"]["sessionId"], session.as_str());
    assert!(got["read"].is_null());
    let listed = String::from_utf8(
        fixture
            .feld(&["message", "list", "--to", "codex-receiver"])
            .stdout,
    )
    .unwrap();
    assert!(listed.contains(" queued "), "{listed}");
    let actor = fixture.json(&["actor", "get", "codex-receiver", "--json"]);
    assert_eq!(actor["actor"]["waitingCount"], 1, "{actor}");

    let refused = fixture.feld(&["message", "delete", &message, "--json"]);
    assert_eq!(refused.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&refused.stderr).unwrap();
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("queued"),
        "{error}"
    );

    // The next turn takes it in.
    let line = calls_carrying(&calls, &message)
        .pop()
        .unwrap()
        .pop()
        .unwrap();
    let taken = json!({
        "type": "response_item",
        "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": line}]},
    });
    let mut file = fs::OpenOptions::new().append(true).open(&rollout).unwrap();
    writeln!(file, "{taken}").unwrap();
    wait_until("the read of the taken Message was not recorded", || {
        is_read(&fixture, "codex-receiver", &message)
    });
    assert_eq!(calls_carrying(&calls, &message).len(), 1);

    run_hook(&fixture, "SessionEnd", &rollout, Some((&token, &session)));
    wait_until(
        "the queue helper did not stop when the Session ended",
        || helper.try_wait().unwrap().is_some(),
    );
}

/// A Message Codex refuses to queue is in no queue, so the helper takes its `queued` back:
/// it stays deletable, and once deleted the helper offers it no more.
#[test]
fn codex_message_codex_refused_to_queue_can_be_deleted() {
    if !feld_core::test_require::tmux() {
        return;
    }
    let fixture = Fixture::with_codex();
    let rollout = fixture.home().join("rollout.jsonl");
    let Some((calls, without_rollout)) = fake_codex(&fixture, &rollout) else {
        return;
    };
    fs::write(&without_rollout, b"").unwrap();
    fixture.start();
    fixture.create_job("codex-receiver", true);
    let (session, token) = fixture.current_token("codex-receiver");
    let mut helper = support::codex_runtime_helper(fixture.home(), "queue-helper")
        .env("FELD_SESSION_TOKEN", &token)
        .env("FELD_SESSION_ID", &session)
        .env("CODEX_THREAD_ID", CODEX_THREAD)
        .env("FELD_CODEX_ROLLOUT", &rollout)
        .env("PATH", fixture.path())
        .stdin(Stdio::null())
        .spawn()
        .unwrap();

    let sent = fixture.json(&["message", "send", "codex-receiver", "refused", "--json"]);
    let message = sent["message"]["id"].as_str().unwrap().to_owned();
    wait_until("Codex was not asked to queue the Message twice", || {
        calls_carrying(&calls, &message).len() >= 2
    });
    // Between two offers the Message is in no queue, and Feld deletes it.
    wait_until("the refused Message could not be deleted", || {
        fixture
            .feld(&["message", "delete", &message, "--json"])
            .status
            .success()
    });
    let deleted = fixture.json(&["message", "get", &message, "--json"])["message"].clone();
    assert!(deleted["deletedAt"].is_string());
    assert!(deleted["read"].is_null());

    // Codex takes Messages from here on, and the deleted one is not among them.
    fs::remove_file(&without_rollout).unwrap();
    let next = fixture.json(&["message", "send", "codex-receiver", "next", "--json"]);
    let next = next["message"]["id"].as_str().unwrap().to_owned();
    wait_until("the next Message was not delivered", || {
        is_read(&fixture, "codex-receiver", &next)
    });
    let offered = calls_carrying(&calls, &message).len();
    thread::sleep(Duration::from_millis(500));
    assert_eq!(calls_carrying(&calls, &message).len(), offered);
    assert!(
        !fs::read_to_string(&rollout).unwrap().contains(&message),
        "the deleted Message reached a turn"
    );

    run_hook(&fixture, "SessionEnd", &rollout, Some((&token, &session)));
    wait_until(
        "the queue helper did not stop when the Session ended",
        || helper.try_wait().unwrap().is_some(),
    );
}

/// Codex ends a hook, together with the process group it runs the hook in, at the hook's
/// timeout. The queue helper the SessionStart hook starts is outside that group, so a Codex
/// whose turn is over goes on taking Messages after the time of its hooks is up.
#[test]
fn codex_queue_helper_outlives_the_timeout_of_the_hook_that_started_it() {
    use std::io::Write;
    use std::os::unix::process::CommandExt;

    if !feld_core::test_require::tmux() {
        return;
    }
    let fixture = Fixture::with_codex();
    let Some((calls, _)) = fake_codex(&fixture, &fixture.home().join("rollout.jsonl")) else {
        return;
    };
    fixture.start();
    fixture.create_job("codex-receiver", true);
    let (session, token) = fixture.current_token("codex-receiver");
    let home = FeldHome::resolve(Some(fixture.home().to_owned())).unwrap();

    // Codex runs the hook of its first event as the leader of a process group of its own.
    let mut hook = support::codex_runtime_helper(fixture.home(), "hook")
        .env("FELD_SESSION_TOKEN", &token)
        .env("FELD_SESSION_ID", &session)
        .env("PATH", fixture.path())
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let group = hook.id();
    let event = json!({
        "session_id": CODEX_THREAD,
        "transcript_path": fixture.home().join("rollout.jsonl"),
        "hook_event_name": "SessionStart",
        "model": "gpt-5.6",
    });
    hook.stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    assert!(hook.wait().unwrap().success());
    wait_until("the hook did not start the queue helper", || {
        mailbox_lock_is_held(&home, &session)
    });

    // The timeout: Codex kills whatever is left of the hook's process group.
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{group}")])
        .stderr(Stdio::null())
        .status();
    thread::sleep(Duration::from_millis(500));
    assert!(mailbox_lock_is_held(&home, &session));

    let sent = fixture.json(&[
        "message",
        "send",
        "codex-receiver",
        "after the timeout",
        "--json",
    ]);
    let message = sent["message"]["id"].as_str().unwrap().to_owned();
    wait_until("the Message was not delivered after the timeout", || {
        is_read(&fixture, "codex-receiver", &message)
    });
    let call = calls_carrying(&calls, &message).pop().unwrap();
    assert_eq!(call[..4], ["queue", "--thread", CODEX_THREAD, "--message"]);

    run_hook(
        &fixture,
        "SessionEnd",
        &fixture.home().join("rollout.jsonl"),
        Some((&token, &session)),
    );
    wait_until(
        "the queue helper did not stop when the Session ended",
        || !mailbox_lock_is_held(&home, &session),
    );
}

/// The parts of a Codex a person starts themselves: the hook before the attach exits 0 with
/// no Session to address, and the one after it starts the queue helper, which delivers from
/// then on.
#[test]
fn codex_parts_started_before_an_attach_deliver_after_it() {
    if !feld_core::test_require::tmux() {
        return;
    }
    let fixture = Fixture::with_codex();
    let codex_home = fixture.home().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    let rollout = codex_home.join(format!("rollout-{CODEX_THREAD}.jsonl"));
    let Some((calls, _)) = fake_codex(&fixture, &rollout) else {
        return;
    };
    fs::write(&rollout, b"{}\n").unwrap();
    fixture.start();
    let home = fixture.home().display().to_string();
    let workdir = fixture.home().join("attached");
    fs::create_dir_all(&workdir).unwrap();
    let shim = |name: &str| {
        fixture
            .home()
            .join("plugins/runtime/codex/bin")
            .join(name)
            .display()
            .to_string()
    };
    let event = |name: &str| {
        json!({
            "session_id": CODEX_THREAD,
            "transcript_path": rollout,
            "cwd": workdir,
            "hook_event_name": name,
            "model": "gpt-5.6",
        })
        .to_string()
    };
    // What Codex itself does around the parts: the hook of its first event, the attach the
    // model makes from the `feld` skill, and the hook of the next event. The wait at the
    // end is what Codex is while it runs, which is what the hook resolves its token through.
    let child = format!(
        "export HOME='{home}' FELD_HOME='{home}' CODEX_HOME='{codex_home}' CODEX_THREAD_ID={CODEX_THREAD} PATH='{bin}':\"$PATH\"; \
         printf '%s' '{start}' | '{hook}' > '{home}/hook.before.log' 2>&1; echo $? > '{home}/hook.before'; \
         '{feld}' --home '{home}' actor attach --self --runtime codex --json > '{home}/attach.json' 2> '{home}/attach.err'; \
         printf '%s' '{prompt}' | '{hook}' > '{home}/hook.after.log' 2>&1; echo $? > '{home}/hook.after'; \
         echo done > '{home}/done'; wait",
        codex_home = codex_home.display(),
        bin = fixture.bin.display(),
        start = event("SessionStart"),
        prompt = event("UserPromptSubmit"),
        hook = shim("hook"),
        feld = FELD,
    );
    if !support::FakeCodex::new()
        .beside(&child)
        .compile(&workdir.join("codex"))
    {
        return;
    }
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "codex",
                "-c",
                workdir.to_str().unwrap(),
                "--",
                "exec ./codex",
            ])
            .status
            .success()
    );

    let attach = fixture.home().join("attach.json");
    wait_until("the attach did not run", || {
        fixture.home().join("done").is_file()
    });
    let attached: Value = serde_json::from_str(&fs::read_to_string(&attach).unwrap_or_else(|_| {
        panic!(
            "no attach: {}",
            fs::read_to_string(fixture.home().join("attach.err")).unwrap_or_default()
        )
    }))
    .unwrap();
    let session = attached["session"]["id"].as_str().unwrap().to_owned();
    let actor = attached["session"]["actor"].as_str().unwrap().to_owned();

    // The hook before the attach found no Session to address and ended the way it ends
    // when the daemon answers: without failing Codex.
    assert_eq!(
        fs::read_to_string(fixture.home().join("hook.before")).unwrap(),
        "0\n"
    );
    assert!(
        fs::read_to_string(fixture.home().join("hook.before.log"))
            .unwrap()
            .contains("token")
    );
    // The one after it delivers the identity of the thread the attach took.
    assert_eq!(
        fs::read_to_string(fixture.home().join("hook.after")).unwrap(),
        "0\n"
    );
    let described = fixture.json(&["session", "get", &session, "--json"]);
    assert_eq!(described["session"]["model"], json!(["Codex", "gpt-5.6"]));

    // The hook after the attach started the queue helper, which delivers with its token.
    let sent = fixture.json(&["message", "send", &actor, "after the attach", "--json"]);
    let message = sent["message"]["id"].as_str().unwrap().to_owned();
    wait_until(
        &format!(
            "the Message was not queued into the thread; lock held: {}; processes:\n{}",
            mailbox_lock_is_held(
                &FeldHome::resolve(Some(fixture.home().to_owned())).unwrap(),
                &session
            ),
            String::from_utf8_lossy(
                &Command::new("sh")
                    .arg("-c")
                    .arg("ps -eo pid,ppid,command | grep -e plugin -e codex | grep -v grep")
                    .output()
                    .unwrap()
                    .stdout
            )
        ),
        || !calls_carrying(&calls, &message).is_empty(),
    );
    let call = calls_carrying(&calls, &message).pop().unwrap();
    assert_eq!(call[..4], ["queue", "--thread", CODEX_THREAD, "--message"]);
    wait_until("the read of the queued Message was not recorded", || {
        is_read(&fixture, &actor, &message)
    });

    // The end of the Codex ends the part that reads its mailbox.
    let home = FeldHome::resolve(Some(fixture.home().to_owned())).unwrap();
    let (_, token) = fixture.current_token(&actor);
    run_hook(&fixture, "SessionEnd", &rollout, Some((&token, &session)));
    wait_until(
        "the queue helper did not stop when the Session ended",
        || !mailbox_lock_is_held(&home, &session),
    );
}

/// A Codex a person started and left waiting for its next input fires no hook. Attached from
/// outside its pane, it is told by the rollout file it holds open, and the queue helper the
/// attach starts delivers without a hook. An attach again to the same Codex replaces the
/// token, and the helper it starts takes over from the one whose token it replaced.
#[test]
fn a_waiting_codex_attached_from_outside_receives_messages_after_each_attach() {
    if !feld_core::test_require::tmux() {
        return;
    }
    let fixture = Fixture::with_codex();
    let codex_home = fs::canonicalize(fixture.home()).unwrap().join("codex-home");
    let rollout = support::codex_rollout_path(&codex_home, CODEX_THREAD);
    fs::create_dir_all(rollout.parent().unwrap()).unwrap();
    let Some((calls, _)) = fake_codex(&fixture, &rollout) else {
        return;
    };
    let workdir = fixture.home().join("waiting");
    fs::create_dir_all(&workdir).unwrap();
    if !support::FakeCodex::new()
        .holding_open(&rollout)
        .compile(&workdir.join("codex"))
    {
        return;
    }
    fixture.start();
    assert!(
        fixture
            .tmux(&[
                "new-session",
                "-d",
                "-s",
                "waiting",
                "-c",
                workdir.to_str().unwrap(),
                "--",
                "exec ./codex",
            ])
            .status
            .success()
    );
    let pane = String::from_utf8(
        fixture
            .tmux(&["display-message", "-p", "-t", "waiting", "#{pane_id}"])
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    let (api, local) = fixture.api();
    let home = FeldHome::resolve(Some(fixture.home().to_owned())).unwrap();
    let socket = fixture.socket.to_str().unwrap();
    let attach = || {
        fixture.json(&[
            "actor",
            "attach",
            "codex-waiting",
            "--runtime",
            "codex",
            "--tmux-pane",
            &pane,
            "--tmux-socket",
            socket,
            "--json",
        ])
    };
    let delivered = |what: &str| {
        let sent = fixture.json(&["message", "send", "codex-waiting", what, "--json"]);
        let message = sent["message"]["id"].as_str().unwrap().to_owned();
        let deadline = Instant::now() + Duration::from_secs(40);
        while !is_read(&fixture, "codex-waiting", &message) {
            assert!(
                Instant::now() < deadline,
                "'{what}' was not read; queued: {:?}",
                calls_carrying(&calls, &message)
            );
            thread::sleep(Duration::from_millis(100));
        }
        let call = calls_carrying(&calls, &message).pop().unwrap();
        assert_eq!(call[..4], ["queue", "--thread", CODEX_THREAD, "--message"]);
    };

    let attached = attach();
    let session = attached["session"]["id"].as_str().unwrap().to_owned();
    let transcript: Value = Client::new()
        .get(format!("{api}/v1/sessions/{session}/transcript"))
        .bearer_auth(&local)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(transcript["state"], "readable", "{transcript}");
    delivered("after the attach");

    let again = attach();
    assert_eq!(again["session"]["id"], session.as_str(), "{again}");
    assert_eq!(again["reattached"], true, "{again}");
    delivered("after the attach again");
    assert!(mailbox_lock_is_held(&home, &session));
}

/// A Session token reads how its own Actor starts and never how another does, and the event
/// stream it opened closes when its Session does.
#[test]
fn a_session_token_reads_no_other_launch_spec_and_its_stream_closes_with_its_session() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("worker", true);
    fixture.create_job("other", false);
    fixture.json(&[
        "actor",
        "update",
        "other",
        "--env",
        "API_KEY=kept-secret",
        "--json",
    ]);
    let (_, token) = fixture.current_token("worker");

    let other = fixture.session_json(&token, &["actor", "get", "other", "--json"]);
    assert!(other["actor"]["launchSpec"].is_null(), "{other}");
    let listed = fixture.session_json(&token, &["actor", "list", "-a", "--json"]);
    assert!(!listed.to_string().contains("kept-secret"), "{listed}");
    let own = fixture.session_json(&token, &["actor", "get", "worker", "--json"]);
    assert!(own["actor"]["launchSpec"]["command"].is_array(), "{own}");
    assert_eq!(
        fixture.json(&["actor", "get", "other", "--json"])["actor"]["launchSpec"]["env"]["API_KEY"],
        "kept-secret"
    );

    let (api, _) = fixture.api();
    let stream = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap()
        .get(format!("{api}/v1/events"))
        .bearer_auth(&token)
        .send()
        .unwrap();
    assert_eq!(stream.status(), reqwest::StatusCode::OK);
    let reader = thread::spawn(move || stream.text());
    fixture.json(&["session", "stop", "worker", "--json"]);
    // The body ends only when the daemon closes the stream.
    let events = reader.join().unwrap().expect("the stream closes");
    assert!(events.contains("resource"), "{events}");
}

/// A Session token sends to the Actors of its Workspace alone, and an Actor with `acceptFrom`
/// takes Messages from the Actors it names and from the Admin. A Session that manages the
/// Actor sets `acceptFrom` too.
#[test]
fn a_session_sends_within_its_workspace_to_an_actor_that_accepts_it() {
    let fixture = Fixture::new();
    fixture.start();
    fixture.create_job("lead", true);
    fixture.create_job("stranger", true);
    fixture.create_job("worker", false);
    let (_, lead) = fixture.current_token("lead");
    let (_, stranger) = fixture.current_token("stranger");
    let socket = fixture.socket.to_str().unwrap();
    fixture.json(&["workspace", "create", "other", "--json"]);
    fixture.json(&[
        "--workspace",
        "other",
        "actor",
        "create",
        "remote",
        "--kind",
        "job",
        "--tmux-socket",
        socket,
        "--json",
        "--",
        "sleep",
        "60",
    ]);
    let refused = |token: &str, arguments: &[&str], expected: &str| {
        let output = fixture
            .command()
            .env("FELD_SESSION_TOKEN", token)
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{arguments:?}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "rejected");
        let message = error["error"]["message"].as_str().unwrap();
        assert!(message.contains(expected), "{message}");
    };

    let remote = [
        "--workspace",
        "other",
        "message",
        "send",
        "remote",
        "hello",
        "--json",
    ];
    refused(&lead, &remote, "within its Workspace");
    refused(
        &lead,
        &[
            "--workspace",
            "other",
            "message",
            "send",
            "nobody",
            "hello",
            "--json",
        ],
        "within its Workspace",
    );
    assert_eq!(fixture.json(&remote)["message"]["to"]["workspace"], "other");

    let updated = fixture.json(&[
        "actor",
        "update",
        "worker",
        "--accept-from",
        "lead",
        "--json",
    ]);
    assert_eq!(updated["actor"]["acceptFrom"], json!(["lead"]));
    let shown = String::from_utf8(fixture.feld(&["actor", "get", "worker"]).stdout).unwrap();
    assert!(
        shown
            .lines()
            .any(|line| line.split_whitespace().eq(["accept_from:", "lead"])),
        "{shown}"
    );
    let send = ["message", "send", "worker", "hello", "--json"];
    let sent = fixture.session_json(&lead, &send);
    assert_eq!(sent["message"]["from"]["name"], "lead");
    refused(
        &stranger,
        &send,
        "does not accept Messages from Actor 'stranger'",
    );
    assert_eq!(fixture.json(&send)["message"]["from"]["type"], "admin");

    // Setting `acceptFrom` is managing the Actor.
    let accept_stranger = [
        "actor",
        "update",
        "worker",
        "--accept-from",
        "stranger",
        "--json",
    ];
    refused(&lead, &accept_stranger, "manage-job");
    fixture.json(&["actor", "update", "lead", "--grant", "manage-job", "--json"]);
    let updated = fixture.session_json(&lead, &accept_stranger);
    assert_eq!(updated["actor"]["acceptFrom"], json!(["stranger"]));
    refused(&lead, &send, "does not accept Messages from Actor 'lead'");
    fixture.session_json(&stranger, &send);

    // An empty `acceptFrom` takes Messages from the Admin alone.
    let (api, admin) = fixture.api();
    let patched = Client::new()
        .patch(format!("{api}/v1/workspaces/default/actors/worker"))
        .bearer_auth(admin.trim())
        .json(&json!({"acceptFrom": []}))
        .send()
        .unwrap();
    assert_eq!(patched.status(), reqwest::StatusCode::OK);
    assert_eq!(
        patched.json::<Value>().unwrap()["actor"]["acceptFrom"],
        json!([])
    );
    refused(&lead, &send, "does not accept Messages from Actor 'lead'");
    refused(
        &stranger,
        &send,
        "does not accept Messages from Actor 'stranger'",
    );
    assert_eq!(fixture.json(&send)["message"]["from"]["type"], "admin");

    let cleared = fixture.json(&["actor", "update", "worker", "--no-accept-from", "--json"]);
    assert!(cleared["actor"]["acceptFrom"].is_null(), "{cleared}");
    fixture.session_json(&lead, &send);
}
