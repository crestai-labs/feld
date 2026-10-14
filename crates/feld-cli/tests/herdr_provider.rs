//! The herdr Execution Provider against a real herdr: a Session launched through the
//! daemon, observed while it runs, and closed by the exit of its own program.
//!
//! Every test drives a headless server of its own ([`support::HerdrServer`]), so nothing
//! here reaches the real `~/.config/herdr` or the session a person is using. A machine
//! without herdr has no server to drive, and the tests step aside with the reason.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

const FELD: &str = env!("CARGO_BIN_EXE_feld");

mod support;

use support::{ArgvLog, HerdrServer};

struct Fixture {
    temporary: TempDir,
    herdr: HerdrServer,
    argv: Option<ArgvLog>,
}

impl Fixture {
    /// A Feld home with the herdr Provider installed and a herdr server of its own, or
    /// nothing when this machine has no herdr.
    fn new(tag: &str) -> Option<Self> {
        Self::starting(tag, false)
    }

    /// [`Fixture::new`] whose daemon runs herdr through a stand-in that logs every argv.
    fn logging_argv(tag: &str) -> Option<Self> {
        Self::starting(tag, true)
    }

    fn starting(tag: &str, logging: bool) -> Option<Self> {
        let herdr = HerdrServer::start(tag)?;
        let temporary = tempfile::tempdir().expect("create a fixture Feld home");
        support::install_plugins_with_herdr(temporary.path());
        let argv = logging.then(|| ArgvLog::wrapping(&[&herdr.program()]));
        let fixture = Self {
            temporary,
            herdr,
            argv,
        };
        fixture.json(&["daemon", "start", "--json"]);
        Some(fixture)
    }

    fn home(&self) -> &Path {
        self.temporary.path()
    }

    /// A `feld` command that reaches this fixture's herdr and no other, and that answers
    /// `self` with nothing even when the test itself runs inside a herdr pane.
    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(self.home())
            .env("HOME", self.home())
            .env_remove("TMUX")
            .env_remove("TMUX_PANE");
        self.herdr.apply(&mut command);
        if let Some(argv) = &self.argv {
            argv.apply(&mut command);
        }
        command.args(arguments);
        command
    }

    fn feld(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().expect("run feld")
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
        serde_json::from_slice(&output.stdout).expect("decode feld JSON output")
    }

    /// Creates a job on this fixture's herdr and starts it.
    fn launch(&self, actor: &str, options: &[&str], command: &[&str]) -> Value {
        let home = self.home().to_string_lossy().into_owned();
        let mut arguments = vec![
            "actor",
            "create",
            actor,
            "--kind",
            "job",
            "--start",
            "--provider",
            "herdr",
            "--herdr-session",
            self.herdr.session(),
            "--workdir",
            home.as_str(),
            "--json",
        ];
        arguments.extend_from_slice(options);
        arguments.push("--");
        arguments.extend_from_slice(command);
        self.json(&arguments)
    }

    /// The Session of one Actor, read afresh.
    fn refreshed(&self, name: &str) -> Value {
        let listed = self.json(&["actor", "list", "-a", "--refresh", "--json"]);
        let actor = listed["actors"]
            .as_array()
            .expect("actor list lists Actors")
            .iter()
            .find(|actor| actor["name"] == name)
            .unwrap_or_else(|| panic!("actor list lists {name}: {listed}"));
        let session = match actor["currentSession"] {
            Value::Null => &actor["latestSession"],
            _ => &actor["currentSession"],
        };
        session.clone()
    }

    /// Waits until the Session of one Actor is observed as one of `kinds`.
    fn wait_for_kind(&self, actor: &str, kinds: &[&str]) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let session = self.refreshed(actor);
            if kinds.contains(&session["observation"]["kind"].as_str().unwrap_or_default()) {
                return session;
            }
            assert!(
                Instant::now() < deadline,
                "the Session of {actor} was never observed as one of {kinds:?}: {session}"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// A workspace of this fixture's own for a launch to borrow.
    fn workspace(&self, label: &str) -> Value {
        self.herdr.json(&[
            "workspace",
            "create",
            "--cwd",
            &self.home().to_string_lossy(),
            "--label",
            label,
            "--no-focus",
        ])
    }

    /// Whether this fixture's herdr still holds one resource, by ID.
    fn holds(&self, resource: &str, id: &str) -> bool {
        let listed = self.herdr.json(&[resource, "list"]);
        listed[format!("{resource}s")]
            .as_array()
            .unwrap_or_else(|| panic!("herdr lists {resource}s: {listed}"))
            .iter()
            .any(|held| held[format!("{resource}_id")] == id)
    }

    /// Stops the daemon, so that a test that runs the Plugin's `stop` itself is the only
    /// one acting on the Placement. The daemon closes a Session whose program it sees gone,
    /// and its cleanup closes the resource the test is still observing.
    fn stop_daemon(&self) {
        self.json(&["daemon", "stop", "--json"]);
    }

    /// One operation of the installed herdr Plugin, run the way core runs it.
    fn plugin(&self, operation: &str, request: &Value) -> Value {
        let mut command = Command::new(self.home().join("plugins/provider/herdr/plugin"));
        command
            .arg(operation)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        self.herdr.apply(&mut command);
        let mut child = command.spawn().expect("run the herdr Plugin");
        child
            .stdin
            .take()
            .expect("the Plugin takes its request on stdin")
            .write_all(request.to_string().as_bytes())
            .expect("write the Plugin request");
        let output = child.wait_with_output().expect("read the Plugin answer");
        assert!(
            output.status.success(),
            "the herdr Plugin failed at {operation}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("decode the Plugin answer")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
    }
}

/// The stored Placement of a Session, which is what the Provider reads back.
fn placement(session: &Value) -> Value {
    session["placement"]["placement"].clone()
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} in {value}"))
}

/// A program that ends on its own closes the Session with the exit code the launch script
/// left in the pane's terminal title, and the Placement says the workspace was created for
/// it.
#[test]
fn a_program_that_exits_closes_the_session_with_its_code() {
    let Some(fixture) = Fixture::new("exit") else {
        return;
    };
    assert!(
        fixture
            .herdr
            .reported_socket()
            .starts_with(fixture.herdr.root()),
        "the fixture server answers on a socket under its own root"
    );

    fixture.launch("exit-code", &[], &["sh", "-c", "exit 7"]);
    let listed = fixture.json(&["actor", "list", "-a", "--refresh", "--json"]);
    let session = listed["actors"]
        .as_array()
        .expect("actor list lists Actors")
        .iter()
        .find(|actor| actor["name"] == "exit-code")
        .map(|actor| actor["latestSession"].clone())
        .expect("actor list lists the Actor that was started");
    assert_eq!(session["observation"]["kind"], "gone");
    assert_eq!(session["exitCode"], 7);
    assert_eq!(session["outcome"], "failed");

    let placement = placement(&session);
    assert_eq!(placement["unit"], "workspace");
    assert_eq!(placement["workspaceOrigin"], "created");
    assert_eq!(placement["tabOrigin"], "created");
    assert_eq!(placement["paneOrigin"], "created");
    assert_eq!(placement["endpoint"]["value"], fixture.herdr.session());
    let summary = text(&placement, "summary");
    assert!(
        summary.starts_with("workspace w") && summary.ends_with(", created)"),
        "{summary}"
    );
}

/// A running program sits in a pane herdr lists, with the environment core asked the pane
/// to be created with and without the script that put it there.
#[test]
fn a_running_program_holds_the_pane_and_the_launch_environment() {
    let Some(fixture) = Fixture::logging_argv("env") else {
        return;
    };
    let environment = fixture.home().join("environment");
    let script = format!("printenv > {}; sleep 30", environment.display());
    fixture.launch("runner", &[], &["sh", "-c", &script]);
    let session = fixture.wait_for_kind("runner", &["running"]);
    assert!(
        session["lifecycle"]["startSettled"].is_string(),
        "a running Session settled its start: {session}"
    );

    let placement = placement(&session);
    let pane_id = text(&placement, "paneId").to_owned();
    let terminal_id = text(&placement, "terminalId").to_owned();
    let panes = fixture.herdr.json(&["pane", "list"]);
    let pane = panes["panes"]
        .as_array()
        .expect("herdr lists panes")
        .iter()
        .find(|pane| pane["pane_id"] == pane_id.as_str())
        .expect("herdr holds the pane the Placement recorded");
    assert_eq!(pane["terminal_id"], terminal_id.as_str());

    let written = support::wait_for_written(&environment);
    let variables: Vec<&str> = written.lines().collect();
    for expected in ["FELD_API=", "FELD_ACTOR=runner", "FELD_WORKSPACE=default"] {
        assert!(
            variables.iter().any(|line| line.starts_with(expected)),
            "the pane's program holds {expected}: {written}"
        );
    }
    let token = variables
        .iter()
        .find_map(|line| line.strip_prefix("FELD_SESSION_TOKEN="))
        .unwrap_or_else(|| panic!("the pane's program holds its Session token: {written}"));
    // The token travelled in an owner-only file, never on a command line `ps` shows every
    // user of the machine.
    let argv = fixture
        .argv
        .as_ref()
        .expect("the fixture logs herdr's argv");
    assert!(
        argv.holding("workspace create").len() == 1,
        "herdr was run through the stand-in"
    );
    let seen = argv.holding(token);
    assert!(seen.is_empty(), "the token was on a command line: {seen:?}");
    support::assert_no_env_file_holds(token);
    // herdr's own variables reach the program, which is what `feld actor attach --self` reads.
    assert!(
        variables
            .iter()
            .any(|line| line == &format!("HERDR_PANE_ID={pane_id}")),
        "the pane's program knows the pane it sits in: {written}"
    );
    // The script the launch travelled in takes itself out before the program starts.
    assert!(
        !variables
            .iter()
            .any(|line| line.starts_with("FELD_LAUNCH_SCRIPT=")),
        "the launch script left the program's environment: {written}"
    );
}

/// The units that borrow: a tab takes an existing workspace, a pane takes an existing pane
/// to split, and the Placement records which resource each launch created.
#[test]
fn the_borrowing_units_record_what_they_created_and_what_they_borrowed() {
    let Some(fixture) = Fixture::new("units") else {
        return;
    };
    let created = fixture.herdr.json(&[
        "workspace",
        "create",
        "--cwd",
        &fixture.home().to_string_lossy(),
        "--label",
        "borrowed",
        "--no-focus",
    ]);
    let workspace = text(&created["workspace"], "workspace_id").to_owned();
    let root_pane = text(&created["root_pane"], "pane_id").to_owned();
    let root_tab = text(&created["tab"], "tab_id").to_owned();

    let tab_session = fixture.launch(
        "in-a-tab",
        &["--herdr-unit", "tab", "--herdr-workspace", &workspace],
        &["sleep", "30"],
    );
    let tab = placement(&tab_session["session"]);
    assert_eq!(tab["unit"], "tab");
    assert_eq!(tab["workspaceId"], workspace.as_str());
    assert_eq!(tab["workspaceOrigin"], "existing");
    assert_eq!(tab["tabOrigin"], "created");
    assert_eq!(tab["paneOrigin"], "created");
    assert_eq!(tab["borrowed"]["selector"], workspace.as_str());
    assert_ne!(tab["tabId"], root_tab.as_str());
    let summary = text(&tab, "summary");
    assert!(
        summary.starts_with(&format!("tab {workspace}:t")) && summary.ends_with(", created)"),
        "{summary}"
    );

    let pane_session = fixture.launch(
        "in-a-pane",
        &[
            "--herdr-unit",
            "pane",
            "--herdr-pane",
            &root_pane,
            "--herdr-split",
            "down",
        ],
        &["sleep", "30"],
    );
    let pane = placement(&pane_session["session"]);
    assert_eq!(pane["unit"], "pane");
    assert_eq!(pane["split"], "down");
    assert_eq!(pane["workspaceId"], workspace.as_str());
    assert_eq!(pane["workspaceOrigin"], "existing");
    assert_eq!(pane["tabId"], root_tab.as_str());
    assert_eq!(pane["tabOrigin"], "existing");
    assert_eq!(pane["paneOrigin"], "created");
    assert_eq!(pane["borrowed"]["paneId"], root_pane.as_str());
    assert_ne!(pane["paneId"], root_pane.as_str());
    let summary = text(&pane, "summary");
    assert!(
        summary.starts_with(&format!("pane {workspace}:p")) && summary.ends_with(", created)"),
        "{summary}"
    );

    for actor in ["in-a-tab", "in-a-pane"] {
        assert_eq!(
            fixture.wait_for_kind(actor, &["running"])["observation"]["kind"],
            "running"
        );
    }
}

/// A pane herdr moves to another workspace is still the recorded one, observed as
/// `relocated` with where it sits now.
#[test]
fn a_moved_pane_is_observed_as_relocated_with_its_current_position() {
    let Some(fixture) = Fixture::new("moved") else {
        return;
    };
    fixture.launch("traveller", &[], &["sleep", "30"]);
    let session = fixture.wait_for_kind("traveller", &["running"]);
    let placed = placement(&session);
    let pane_id = text(&placed, "paneId").to_owned();
    let workspace = text(&placed, "workspaceId").to_owned();

    let moved = fixture.herdr.json(&[
        "pane",
        "move",
        &pane_id,
        "--new-workspace",
        "--label",
        "elsewhere",
        "--no-focus",
    ]);
    let target_workspace = text(&moved["move_result"]["pane"], "workspace_id").to_owned();
    assert_ne!(target_workspace, workspace);

    let relocated = fixture.wait_for_kind("traveller", &["relocated"]);
    let detail = relocated["observation"]["detail"]
        .as_str()
        .expect("an observation says what it saw");
    assert!(
        detail.contains(&format!("workspace {target_workspace}")),
        "{detail}"
    );
    // The Placement keeps the IDs it recorded; where the pane sits now is the observation.
    assert_eq!(placement(&relocated)["paneId"], pane_id.as_str());
    assert_eq!(placement(&relocated)["workspaceId"], workspace.as_str());
}

/// Stopping the server ends the pane's program, which closes the Session. The pane comes
/// back with the same ID and a new terminal when the server starts again, and the Provider
/// tells the two apart.
#[test]
fn a_server_restart_ends_the_session_and_leaves_a_mismatched_pane() {
    let Some(mut fixture) = Fixture::new("restart") else {
        return;
    };
    fixture.launch("outlives-nothing", &[], &["sleep", "30"]);
    let running = fixture.wait_for_kind("outlives-nothing", &["running"]);
    let placed = placement(&running);
    let pane_id = text(&placed, "paneId").to_owned();
    let terminal_id = text(&placed, "terminalId").to_owned();

    fixture.herdr.stop_server();
    let ended = fixture.wait_for_kind("outlives-nothing", &["gone", "lost"]);
    assert!(
        ended["end"].is_string() || ended["observation"]["kind"] == "lost",
        "a Session whose program is gone is closed: {ended}"
    );
    assert!(ended["exitCode"].is_null());

    // The restored pane keeps the ID and takes a new terminal, which is how the Provider
    // sees that the resource under the recorded ID is another one.
    fixture.herdr.start_server();
    let restored = fixture.herdr.json(&["pane", "get", &pane_id]);
    assert_ne!(restored["pane"]["terminal_id"], terminal_id.as_str());
    let observed = fixture.plugin("observe", &json!({ "placements": [placed] }));
    assert_eq!(observed["results"][0]["observation"]["kind"], "mismatched");
}

/// The pane the Runtime a person started sits in: the Placement takes it as `existing`,
/// the Session is `attached`, and it closes when that Runtime process ends while the pane
/// and its shell stay.
#[test]
fn attach_takes_the_pane_a_runtime_runs_in_and_closes_with_it() {
    let Some(fixture) = Fixture::new("attach") else {
        return;
    };
    let workdir = fixture.home().join("attachable");
    fs::create_dir(&workdir).expect("create the workdir of the attached Runtime");
    let claude = workdir.join("claude");
    if !support::compile_runtime(&claude, None) {
        return;
    }
    // `feld actor attach` reads Claude Code's configuration root and writes nothing there,
    // so this test holds the real one still and checks afterwards that nothing touched it.
    let real_settings = std::env::var_os("HOME")
        .map(|value| Path::new(&value).join(".claude/settings.json"))
        .expect("the test environment has a HOME");
    let before = fs::metadata(&real_settings)
        .and_then(|data| data.modified())
        .ok();

    let created = fixture.herdr.json(&[
        "workspace",
        "create",
        "--cwd",
        &workdir.to_string_lossy(),
        "--label",
        "attachable",
        "--no-focus",
    ]);
    let pane = text(&created["root_pane"], "pane_id").to_owned();
    let session_name = fixture.herdr.session().to_owned();
    let attach = |name: &str| {
        fixture.feld(&[
            "actor",
            "attach",
            name,
            "--herdr-pane",
            &pane,
            "--herdr-session",
            &session_name,
            "--runtime",
            "claude-code",
            "--json",
        ])
    };

    // A pane that is only running its shell has no Runtime in front of it to take.
    let refused = attach("too-early");
    assert!(
        !refused.status.success(),
        "attaching to a pane without a Runtime: {}",
        String::from_utf8_lossy(&refused.stdout)
    );
    let refusal = String::from_utf8_lossy(&refused.stderr).into_owned();
    assert!(refusal.contains("Claude Code"), "{refusal}");

    assert!(
        fixture
            .herdr
            .run(&["pane", "run", &pane, "./claude"])
            .status
            .success()
    );
    let runtime_pid = wait_for_foreground(&fixture, &pane);

    let attached: Value =
        serde_json::from_slice(&attach("attached").stdout).expect("decode feld actor attach JSON");
    assert_eq!(attached["session"]["ownership"], "attached");
    assert_eq!(attached["session"]["startKind"], "attach");
    assert_eq!(attached["session"]["observation"]["kind"], "running");
    let placed = placement(&attached["session"]);
    assert_eq!(placed["unit"], "pane");
    assert_eq!(placed["paneId"], pane.as_str());
    assert_eq!(placed["paneOrigin"], "existing");
    assert_eq!(placed["tabOrigin"], "existing");
    assert_eq!(placed["workspaceOrigin"], "existing");
    // The pane an attach takes is the resource it borrows, and it holds the Runtime the
    // observations that follow are judged against.
    assert_eq!(placed["borrowed"]["paneId"], pane.as_str());
    assert_eq!(placed["process"]["pid"], runtime_pid);
    assert!(placed["program"].is_null());
    assert!(
        text(&placed, "workdir").ends_with("attachable"),
        "the Placement takes the directory the Runtime runs in: {placed}"
    );
    let summary = text(&placed, "summary");
    assert!(
        summary.starts_with(&format!("pane {pane} (")) && summary.ends_with(", existing)"),
        "{summary}"
    );
    assert!(
        text(&placed, "attachCommand").contains(text(&placed, "terminalId")),
        "the attach command opens the pane's own terminal: {placed}"
    );

    // The same Runtime incarnation is the same Session, with a token issued afresh.
    let again: Value =
        serde_json::from_slice(&attach("attached").stdout).expect("decode the second attach");
    assert_eq!(again["session"]["id"], attached["session"]["id"]);
    assert_eq!(again["reattached"], true);
    assert_ne!(again["token"], attached["token"]);
    let listed = fixture.json(&["actor", "list", "--json"]);
    assert_eq!(
        listed["actors"]
            .as_array()
            .expect("actor list lists Actors")
            .iter()
            .filter(|actor| actor["name"] == "attached")
            .count(),
        1
    );

    // The Runtime is not this Provider's child, so its end has no exit code to report.
    assert!(
        Command::new("kill")
            .arg(runtime_pid.to_string())
            .status()
            .expect("end the attached Runtime")
            .success()
    );
    let gone = fixture.wait_for_kind("attached", &["gone"]);
    assert!(gone["exitCode"].is_null(), "{gone}");
    assert!(gone["end"].is_string(), "{gone}");

    // The pane and the shell the Runtime ran in are left as they were.
    let still = fixture.herdr.json(&["pane", "get", &pane]);
    assert_eq!(still["pane"]["terminal_id"], placed["terminalId"]);
    let info = fixture
        .herdr
        .json(&["pane", "process-info", "--pane", &pane]);
    assert_eq!(
        info["process_info"]["shell_pid"],
        placed["paneProcess"]["pid"]
    );

    assert_eq!(
        before,
        fs::metadata(&real_settings)
            .and_then(|data| data.modified())
            .ok(),
        "feld actor attach touched {}",
        real_settings.display()
    );
}

/// `feld actor attach --self` from inside a pane: the same Placement the selector from outside
/// makes, and a self index the processes of that pane resolve their token through.
#[test]
fn self_attach_from_inside_a_pane_serves_the_token_of_its_children() {
    let Some(fixture) = Fixture::new("self") else {
        return;
    };
    let workdir = fixture.home().join("inside");
    fs::create_dir(&workdir).expect("create the workdir of the attached Runtime");
    let attached = fixture.home().join("attached.json");
    let errors = fixture.home().join("attached.err");
    let token = fixture.home().join("token");
    let home = fixture.home().display().to_string();
    // What the Runtime runs inside its own pane: the attach, and then a child of it that
    // resolves the Session token through the self index the attach wrote.
    let inside = format!(
        "{prefix} HOME='{home}' {FELD} --home '{home}' actor attach --self --runtime claude-code --json > '{attached}' 2> '{errors}'; \
         {prefix} HOME='{home}' {FELD} --home '{home}' self token > '{token}' 2>&1",
        prefix = fixture.herdr.shell_prefix(),
        attached = attached.display(),
        errors = errors.display(),
        token = token.display(),
    );
    let claude = workdir.join("claude");
    if !support::compile_runtime(&claude, Some(&inside)) {
        return;
    }
    let created = fixture.herdr.json(&[
        "workspace",
        "create",
        "--cwd",
        &workdir.to_string_lossy(),
        "--label",
        "inside",
        "--no-focus",
    ]);
    let pane = text(&created["root_pane"], "pane_id").to_owned();
    assert!(
        fixture
            .herdr
            .run(&["pane", "run", &pane, "./claude"])
            .status
            .success()
    );

    let deadline = Instant::now() + Duration::from_secs(60);
    while fs::metadata(&token).is_ok_and(|data| data.len() == 0) || !token.is_file() {
        assert!(
            Instant::now() < deadline,
            "the attach inside the pane did not finish; stdout: {}; stderr: {}",
            fs::read_to_string(&attached).unwrap_or_default(),
            fs::read_to_string(&errors).unwrap_or_default(),
        );
        thread::sleep(Duration::from_millis(50));
    }
    let answer: Value = serde_json::from_slice(&fs::read(&attached).expect("the attach answered"))
        .unwrap_or_else(|error| {
            panic!(
                "decode the attach inside the pane: {error}; stderr: {}",
                fs::read_to_string(&errors).unwrap_or_default()
            )
        });
    assert_eq!(answer["session"]["ownership"], "attached");
    assert_eq!(answer["session"]["startKind"], "attach");
    // The name comes from the directory the Runtime runs in, since none was given.
    assert_eq!(answer["session"]["actor"], "inside-1");
    let placed = placement(&answer["session"]);
    assert_eq!(placed["paneId"], pane.as_str());
    assert_eq!(placed["paneOrigin"], "existing");
    // A pane answers `self` with the socket of its own server, so one pane keeps one key.
    assert_eq!(placed["endpoint"]["kind"], "path");
    assert_eq!(
        placed["endpoint"]["value"],
        fixture.herdr.reported_socket().display().to_string()
    );

    // The child of the Runtime resolves the Session token through the self index.
    let resolved = fs::read_to_string(&token).expect("the child answered");
    assert_eq!(
        resolved.trim(),
        answer["token"].as_str().expect("attach answers a token"),
        "the child of the attached Runtime resolves its Session token"
    );
}

/// An attach from outside that names the server by its session name writes the self index
/// entry `--self` writes, so the Runtime's children find the token.
#[test]
fn outside_attach_writes_the_self_index_entry_of_self_attach() {
    let Some(fixture) = Fixture::new("outside") else {
        return;
    };
    let workdir = fixture.home().join("outside");
    fs::create_dir(&workdir).expect("create the workdir of the attached Runtime");
    let token = fixture.home().join("token");
    let again = fixture.home().join("again.json");
    let again_error = fixture.home().join("again.err");
    let home = fixture.home().display().to_string();
    // The child of the Runtime waits until it resolves a token, then attaches with `--self`.
    let inside = format!(
        "until {prefix} HOME='{home}' {FELD} --home '{home}' self token > '{token}' 2> /dev/null; do sleep 0.1; done; \
         {prefix} HOME='{home}' {FELD} --home '{home}' actor attach --self --runtime claude-code --json > '{again}' 2> '{again_error}'",
        prefix = fixture.herdr.shell_prefix(),
        token = token.display(),
        again = again.display(),
        again_error = again_error.display(),
    );
    let claude = workdir.join("claude");
    if !support::compile_runtime(&claude, Some(&inside)) {
        return;
    }
    let created = fixture.herdr.json(&[
        "workspace",
        "create",
        "--cwd",
        &workdir.to_string_lossy(),
        "--label",
        "outside",
        "--no-focus",
    ]);
    let pane = text(&created["root_pane"], "pane_id").to_owned();
    assert!(
        fixture
            .herdr
            .run(&["pane", "run", &pane, "./claude"])
            .status
            .success()
    );
    wait_for_foreground(&fixture, &pane);
    let attached = fixture.json(&[
        "actor",
        "attach",
        "outside",
        "--herdr-pane",
        &pane,
        "--herdr-session",
        fixture.herdr.session(),
        "--runtime",
        "claude-code",
        "--json",
    ]);

    let deadline = Instant::now() + Duration::from_secs(60);
    while fs::metadata(&again).map_or(true, |data| data.len() == 0) {
        assert!(
            Instant::now() < deadline,
            "the child did not resolve its token and attach again; token: {:?}; stderr: {}",
            fs::read_to_string(&token).ok(),
            fs::read_to_string(&again_error).unwrap_or_default(),
        );
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(&token)
            .expect("the child answered")
            .trim(),
        attached["token"].as_str().expect("attach answers a token")
    );
    let again: Value = serde_json::from_slice(&fs::read(&again).expect("the attach answered"))
        .expect("decode the attach inside the pane");
    assert_eq!(again["session"]["actor"], "outside");
    // The token the child resolved shows the keys agree; the sweep of the other entries
    // of the Session leaves one either way.
    let entries: Vec<_> = fs::read_dir(fixture.home().join("self-index"))
        .expect("read the self index")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    assert_eq!(entries.len(), 1, "{entries:?}");
    let index: Value =
        serde_json::from_slice(&fs::read(&entries[0]).expect("read the self index entry"))
            .expect("decode the self index entry");
    assert_eq!(index["sessionId"], again["session"]["id"]);
}

/// The pid of the process in front of a pane, once it is something other than the shell
/// that pane started with.
fn wait_for_foreground(fixture: &Fixture, pane: &str) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let info = fixture
            .herdr
            .json(&["pane", "process-info", "--pane", pane]);
        let info = &info["process_info"];
        let leader = info["foreground_process_group_id"].as_u64();
        if let Some(leader) = leader
            && Some(leader) != info["shell_pid"].as_u64()
        {
            return leader;
        }
        assert!(
            Instant::now() < deadline,
            "nothing came to the front of pane {pane}: {info}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

/// `feld session stop` on a `program` Session: the Provider signals the pane's program, the
/// Session closes `gone` with what it tried and what it saw, and the workspace the launch
/// created is closed with it.
#[test]
fn a_stopped_program_closes_the_session_and_takes_the_created_workspace() {
    let Some(fixture) = Fixture::new("stop") else {
        return;
    };
    fixture.launch("stoppable", &[], &["sleep", "30"]);
    let running = fixture.wait_for_kind("stoppable", &["running"]);
    let workspace = text(&placement(&running), "workspaceId").to_owned();

    let stopped = fixture.json(&["session", "stop", "stoppable", "--json"])["session"].clone();
    assert_eq!(stopped["observation"]["kind"], "gone");
    assert_eq!(stopped["outcome"], "stopped");
    assert_eq!(stopped["endKind"], "stopped");
    assert_eq!(stopped["exitCode"], 143);
    let stop = &stopped["lifecycle"]["stop"];
    assert_eq!(stop["attempt"], "herdr sent SIGTERM to the pane's program");
    assert_eq!(stop["observed"]["kind"], "gone");

    assert!(
        !fixture.holds("workspace", &workspace),
        "the workspace the launch created is closed with the Session"
    );
}

/// The `stopInput` of a Runtime reaches the pane as a line: the program reads it from the
/// pane's terminal and ends itself, which is what `feld session stop` does for an
/// `agent-runtime`.
#[test]
fn a_stop_input_reaches_the_program_through_the_pane() {
    let Some(fixture) = Fixture::new("input") else {
        return;
    };
    fixture.launch(
        "reader",
        &[],
        &[
            "sh",
            "-c",
            "read line; case \"$line\" in /exit) exit 5 ;; *) exit 9 ;; esac",
        ],
    );
    let running = fixture.wait_for_kind("reader", &["running"]);
    fixture.stop_daemon();
    let stopped = fixture.plugin(
        "stop",
        &json!({ "placement": placement(&running), "input": ["/exit", ""] }),
    );
    assert_eq!(stopped["attempt"], "herdr sent /exit to the pane");
    assert_eq!(stopped["observation"]["kind"], "gone");
    assert_eq!(stopped["observation"]["exitCode"], 5);
}

/// A stop ends the program alone: the pane's process passes to the launch script's `sleep`
/// under the same pid, so every observation after the stop still matches it and reads
/// `gone`, never a pane process on its way out.
#[test]
fn every_stop_input_ends_the_program_under_the_pane_process_it_was_recorded_with() {
    let Some(fixture) = Fixture::new("repeat") else {
        return;
    };
    let running: Vec<Value> = (0..10)
        .map(|round| {
            let actor = format!("reader-{round}");
            fixture.launch(&actor, &[], &["sh", "-c", "read line; exit 0"]);
            fixture.wait_for_kind(&actor, &["running"])
        })
        .collect();
    fixture.stop_daemon();
    for running in running {
        let stopped = fixture.plugin(
            "stop",
            &json!({ "placement": placement(&running), "input": ["/exit"] }),
        );
        assert_eq!(stopped["observation"]["kind"], "gone", "{stopped}");
        assert_eq!(stopped["observation"]["exitCode"], 0);
    }
}

/// `feld session kill` on a `program` Session: SIGKILL reaches the pane's program, the exit code
/// the launch script wrote is the signal's, and each unit closes the resource it created
/// while everything it borrowed stays.
#[test]
fn a_killed_program_ends_with_the_signal_code_and_each_unit_closes_what_it_created() {
    let Some(fixture) = Fixture::new("kill") else {
        return;
    };
    let borrowed = fixture.workspace("borrowed");
    let root_pane = text(&borrowed["root_pane"], "pane_id").to_owned();
    let root_tab = text(&borrowed["tab"], "tab_id").to_owned();
    let workspace = text(&borrowed["workspace"], "workspace_id").to_owned();

    fixture.launch(
        "in-a-tab",
        &["--herdr-unit", "tab", "--herdr-workspace", &workspace],
        &["sleep", "30"],
    );
    let tab = text(
        &placement(&fixture.wait_for_kind("in-a-tab", &["running"])),
        "tabId",
    )
    .to_owned();
    let killed = fixture.json(&["session", "kill", "in-a-tab", "--json"])["session"].clone();
    assert_eq!(killed["outcome"], "stopped");
    assert_eq!(killed["exitCode"], 137);
    let kill = &killed["lifecycle"]["kill"];
    assert_eq!(kill["attempt"], "herdr killed the pane's program (SIGKILL)");
    assert_eq!(kill["observed"]["kind"], "gone");
    assert!(!fixture.holds("tab", &tab), "the created tab is closed");
    assert!(
        fixture.holds("workspace", &workspace),
        "the borrowed workspace stays"
    );

    fixture.launch(
        "in-a-pane",
        &["--herdr-unit", "pane", "--herdr-pane", &root_pane],
        &["sleep", "30"],
    );
    let pane = text(
        &placement(&fixture.wait_for_kind("in-a-pane", &["running"])),
        "paneId",
    )
    .to_owned();
    fixture.json(&["session", "kill", "in-a-pane", "--json"]);
    assert!(!fixture.holds("pane", &pane), "the created pane is closed");
    assert!(
        fixture.holds("pane", &root_pane) && fixture.holds("tab", &root_tab),
        "the borrowed pane and its tab stay"
    );
}

/// A created pane that is the last one of the tab it was split out of stays, so that the
/// borrowed tab stays with it.
#[test]
fn the_last_pane_of_a_borrowed_tab_stays_when_the_session_closes() {
    let Some(fixture) = Fixture::new("lastpane") else {
        return;
    };
    let borrowed = fixture.workspace("borrowed");
    let root_pane = text(&borrowed["root_pane"], "pane_id").to_owned();
    let root_tab = text(&borrowed["tab"], "tab_id").to_owned();

    fixture.launch(
        "alone",
        &["--herdr-unit", "pane", "--herdr-pane", &root_pane],
        &["sleep", "30"],
    );
    let pane = text(
        &placement(&fixture.wait_for_kind("alone", &["running"])),
        "paneId",
    )
    .to_owned();
    // The pane the launch split out of is closed, which leaves the created one holding
    // the borrowed tab on its own.
    fixture.herdr.json(&["pane", "close", &root_pane]);

    fixture.json(&["session", "kill", "alone", "--json"]);
    assert!(
        fixture.holds("pane", &pane),
        "the borrowed tab's last pane stays"
    );
    assert!(fixture.holds("tab", &root_tab), "the borrowed tab stays");
}
