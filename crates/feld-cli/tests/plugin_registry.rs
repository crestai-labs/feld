//! What `$FELD_HOME/plugins/` is worth: which directories become installed Plugins, what
//! core hands an operation, and what the answers do to a Session. The fixture Plugins under
//! `tests/fixtures/plugins/` are shell scripts, so these tests exercise the real process
//! protocol rather than a Rust double.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

mod support;

struct Fixture {
    temp: TempDir,
}

impl Fixture {
    /// A Feld home with the fixture Plugins installed, before any daemon reads them.
    fn new() -> Self {
        let fixture = Self {
            temp: tempfile::tempdir().expect("create temporary Feld home"),
        };
        copy_tree(&source_plugins(), &fixture.home().join("plugins"));
        support::install_plugins(fixture.home());
        fixture
    }

    fn home(&self) -> &Path {
        self.temp.path()
    }

    fn plugin_dir(&self, kind: &str, name: &str) -> PathBuf {
        self.home().join("plugins").join(kind).join(name)
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = support::feld();
        command
            .arg("--home")
            .arg(self.home())
            // The fixture Plugins record this, which is how the tests see that a Plugin
            // inherits the environment of its caller.
            .env("FELD_FIXTURE_INHERITED", "inherited")
            .env("HOME", self.home())
            .args(arguments);
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
        serde_json::from_slice(&output.stdout).expect("parse feld JSON output")
    }

    fn start_daemon(&self) {
        let started = self.json(&["--json", "daemon", "start"]);
        assert_eq!(started["daemon"]["state"], "running");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.feld(&["daemon", "stop"]);
    }
}

fn source_plugins() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugins")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create the Plugin directory");
    for entry in fs::read_dir(from).expect("read the fixture Plugins") {
        let entry = entry.expect("read a fixture Plugin entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy a fixture Plugin file");
        }
    }
}

fn names(plugins: &Value) -> Vec<String> {
    plugins
        .as_array()
        .expect("the daemon lists Plugins")
        .iter()
        .map(|plugin| plugin["name"].as_str().expect("a Plugin name").to_owned())
        .collect()
}

/// Waits for a Session read to satisfy a condition, which the periodic observation reaches
/// within a few ticks.
fn wait_for(fixture: &Fixture, actor: &str, condition: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let described =
            fixture.json(&["--json", "actor", "get", actor])["actor"]["latestSession"].clone();
        if condition(&described) {
            return described;
        }
        assert!(
            Instant::now() < deadline,
            "the Session never reached the expected state: {described}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn installed_plugins_are_listed_and_unusable_directories_are_left_out_with_a_log() {
    let fixture = Fixture::new();
    fixture.start_daemon();

    let status = fixture.json(&["--json", "daemon", "status"]);
    let runtimes = names(&status["runtimes"]);
    let providers = names(&status["providers"]);
    assert_eq!(runtimes, ["claude-code", "fixture-runtime"]);
    assert_eq!(providers, ["fixture-provider", "tmux"]);
    assert_eq!(
        status["runtimes"][1]["label"], "Fixture Runtime",
        "the label of a Plugin is its manifest's"
    );

    // A manifest whose kind is not its parent directory, and a directory without an
    // executable, are not installed and the daemon says why.
    assert!(!runtimes.contains(&"broken-manifest".to_owned()));
    assert!(!providers.contains(&"no-executable".to_owned()));
    let log = fs::read_to_string(fixture.home().join("daemon.log")).expect("read the daemon log");
    assert!(
        log.contains("broken-manifest is not installed")
            && log.contains("is not the parent directory"),
        "{log}"
    );
    assert!(
        log.contains("no-executable is not installed") && log.contains("no executable `plugin`"),
        "{log}"
    );

    let human = fixture.feld(&["daemon", "status"]);
    let printed = String::from_utf8_lossy(&human.stdout);
    assert!(
        printed.contains("runtimes:   claude-code, fixture-runtime"),
        "{printed}"
    );
    assert!(
        printed.contains("providers:  fixture-provider, tmux"),
        "{printed}"
    );

    // The Runtime and Provider listings name the same Plugins, with their labels.
    let listed = fixture.json(&["--json", "runtime", "list"]);
    assert_eq!(
        names(&listed["runtimes"]),
        ["claude-code", "fixture-runtime"]
    );
    assert_eq!(listed["runtimes"][1]["label"], "Fixture Runtime");
    assert_eq!(listed["runtimes"][1]["runningSessions"], 0);
    let listed = fixture.json(&["--json", "provider", "list"]);
    assert_eq!(names(&listed["providers"]), ["fixture-provider", "tmux"]);
    assert_eq!(
        listed["providers"][1]["nodes"],
        serde_json::json!(["local"])
    );
    assert_eq!(listed["providers"][1]["runningSessions"], 0);
    // Nothing has been recorded yet, so the history has no start.
    assert_eq!(
        fixture.json(&["--json", "daemon", "status"])["historyStart"],
        Value::Null
    );
    let human = fixture.feld(&["runtime", "list"]);
    let printed = String::from_utf8_lossy(&human.stdout);
    assert!(
        printed.contains("NAME") && printed.contains("Fixture Runtime"),
        "{printed}"
    );
    let human = fixture.feld(&["provider", "list"]);
    let printed = String::from_utf8_lossy(&human.stdout);
    assert!(
        printed.contains("NODES") && printed.contains("fixture-provider"),
        "{printed}"
    );

    // A Runtime without Sessions has Runtime metrics with nothing reported, and a name that
    // is not installed has none.
    let metrics = fixture.json(&["--json", "runtime", "metrics", "claude-code"]);
    assert_eq!(metrics["runtime"], "claude-code");
    assert_eq!(metrics["quota"]["status"], "unavailable");
    assert_eq!(metrics["metrics"][0]["sessions"]["reported"], 0);
    let everything = fixture.json(&["--json", "runtime", "metrics"]);
    assert_eq!(everything["runtimes"].as_array().unwrap().len(), 2);
    let human = fixture.feld(&["runtime", "metrics", "claude-code"]);
    let printed = String::from_utf8_lossy(&human.stdout);
    assert!(
        printed.starts_with("Range: last 5h (")
            && printed.contains("Runtime claude-code")
            && printed.contains("No rate limit report"),
        "{printed}"
    );

    // The range is the last 5 hours unless `--since` or `--start` sets it, and `--json` holds it.
    let span = |metrics: &Value| {
        let instant = |key: &str| {
            chrono::DateTime::parse_from_rfc3339(metrics[key].as_str().unwrap()).unwrap()
        };
        (instant("end") - instant("start")).num_minutes()
    };
    assert_eq!(span(&metrics), 300);
    let hour = fixture.json(&[
        "--json",
        "runtime",
        "metrics",
        "claude-code",
        "--since",
        "1h",
    ]);
    assert_eq!(span(&hour), 60);
    let printed = fixture
        .feld(&["runtime", "metrics", "claude-code", "--since", "30m"])
        .stdout;
    assert!(String::from_utf8_lossy(&printed).starts_with("Range: last 30m ("));
    let given = fixture.json(&[
        "--json",
        "runtime",
        "metrics",
        "claude-code",
        "--start",
        "2026-01-01T00:00:00Z",
    ]);
    assert_eq!(given["start"], "2026-01-01T00:00:00.000Z");
    for arguments in [
        &[
            "runtime",
            "metrics",
            "--since",
            "1h",
            "--start",
            "2026-01-01T00:00:00Z",
        ][..],
        &[
            "runtime",
            "metrics",
            "--since",
            "1h",
            "--end",
            "2026-01-01T00:00:00Z",
        ][..],
        &["runtime", "metrics", "--since", "0h"][..],
        &["runtime", "metrics", "--since", "5s"][..],
    ] {
        assert_eq!(
            fixture.feld(arguments).status.code(),
            Some(2),
            "feld {arguments:?}"
        );
    }
    assert!(
        !fixture
            .feld(&["runtime", "metrics", "ghost"])
            .status
            .success()
    );

    // A launchSpec may only choose from what is installed.
    let unknown_runtime = fixture.feld(&[
        "actor",
        "create",
        "ghost",
        "--kind",
        "agent",
        "--no-start",
        "--runtime",
        "ghost-runtime",
        "--provider",
        "fixture-provider",
        "--workdir",
        "/tmp",
    ]);
    assert_eq!(unknown_runtime.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&unknown_runtime.stderr).contains("is not installed"),
        "{}",
        String::from_utf8_lossy(&unknown_runtime.stderr)
    );
    let unknown_provider = fixture.feld(&[
        "actor",
        "create",
        "ghost",
        "--kind",
        "agent",
        "--no-start",
        "--runtime",
        "fixture-runtime",
        "--provider",
        "ghost-provider",
        "--workdir",
        "/tmp",
    ]);
    assert_eq!(unknown_provider.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&unknown_provider.stderr).contains("is not installed"),
        "{}",
        String::from_utf8_lossy(&unknown_provider.stderr)
    );
}

#[test]
fn operations_carry_the_request_and_the_environment_and_their_answers_drive_the_session() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    let workdir = fixture.home().join("work");
    fs::create_dir_all(&workdir).unwrap();
    // The CLI stores the canonical path, which on macOS resolves the temp dir's /var symlink.
    let workdir = fs::canonicalize(&workdir).unwrap();

    let started = fixture.json(&[
        "--json",
        "actor",
        "create",
        "fixture",
        "--kind",
        "agent",
        "--runtime",
        "fixture-runtime",
        "--provider",
        "fixture-provider",
        "--workdir",
        workdir.to_str().unwrap(),
    ]);
    let session = &started["session"];
    assert_eq!(session["execution"]["type"], "agent-runtime");
    assert_eq!(
        session["execution"]["label"], "Fixture Runtime",
        "the Session label is the Runtime Adapter manifest's"
    );
    assert_eq!(session["execution"]["runtime"], "fixture-runtime");
    assert_eq!(session["placement"]["provider"], "fixture-provider");
    assert_eq!(
        session["placement"]["placement"]["fixtureResource"],
        "running"
    );
    assert_eq!(session["observation"]["kind"], "running");

    let provider = fixture.plugin_dir("provider", "fixture-provider");
    let runtime = fixture.plugin_dir("runtime", "fixture-runtime");

    // The Provider is asked to place the execution before it is asked to launch it, and
    // both requests carry what plugin.md's table says.
    let place: Value = read_json(&provider.join("calls/place.request.json"));
    assert_eq!(place["node"], "local");
    assert_eq!(place["workdir"], workdir.to_str().unwrap());
    assert!(place["options"].is_object());

    let provider_launch: Value = read_json(&provider.join("calls/launch.request.json"));
    assert_eq!(
        provider_launch["placement"]["fixtureResource"], "pending",
        "launch receives what place answered"
    );
    assert_eq!(provider_launch["name"], "feld-default-fixture-1");
    assert_eq!(provider_launch["workdir"], workdir.to_str().unwrap());
    assert_eq!(provider_launch["command"][0], "/bin/sh");
    assert_eq!(provider_launch["env"]["FIXTURE_RUNTIME"], "1");

    let runtime_launch: Value = read_json(&runtime.join("calls/launch.request.json"));
    assert_eq!(runtime_launch["workspace"], "default");
    assert_eq!(runtime_launch["actor"], "fixture");
    assert_eq!(runtime_launch["session"], session["id"]);
    assert_eq!(runtime_launch["workdir"], workdir.to_str().unwrap());
    assert!(
        runtime_launch["token"]
            .as_str()
            .is_some_and(|token| !token.is_empty())
    );
    assert_eq!(
        runtime_launch["sessionDir"],
        fixture
            .home()
            .join("sessions")
            .join(session["id"].as_str().unwrap())
            .join("runtime")
            .to_str()
            .unwrap(),
        "the Runtime Adapter is given the Session's own directory"
    );

    // Core runs `<dir>/plugin <operation>` with the Plugin's own environment on top of the
    // one it was called in.
    let environment = fs::read_to_string(provider.join("calls/launch.env")).unwrap();
    assert!(environment.contains("argv=launch"), "{environment}");
    assert!(
        environment.contains(&format!("FELD_HOME={}", fixture.home().display())),
        "{environment}"
    );
    assert!(
        environment.contains("FELD_API=http://127.0.0.1:"),
        "{environment}"
    );
    assert!(
        environment.contains(&format!("FELD_PLUGIN_DIR={}", provider.display())),
        "{environment}"
    );
    assert!(
        environment.contains("FELD_FIXTURE_INHERITED=inherited"),
        "the Plugin inherits the environment of its caller: {environment}"
    );

    // An answer of `gone` closes the Session, which is the periodic observation working.
    fs::write(provider.join("state"), "gone\n").unwrap();
    let ended = wait_for(&fixture, "fixture", |session| !session["end"].is_null());
    assert_eq!(ended["observation"]["kind"], "gone");

    // `feld session stop` delivers the Runtime Adapter's `stopInput` through the Provider.
    fixture.json(&["--json", "actor", "start", "fixture"]);
    let stopped = fixture.json(&["--json", "session", "stop", "fixture"]);
    assert_eq!(
        stopped["session"]["lifecycle"]["stop"]["attempt"],
        "the fixture Provider delivered /fixture-exit"
    );
    assert_eq!(
        stopped["session"]["lifecycle"]["stop"]["observed"]["kind"],
        "gone"
    );
    let stop_request: Value = read_json(&provider.join("calls/stop.request.json"));
    assert_eq!(stop_request["input"], json!(["/fixture-exit"]));
    assert_eq!(stop_request["placement"]["fixtureResource"], "running");
    let described = fixture.json(&["--json", "actor", "get", "fixture"]);
    assert_eq!(described["actor"]["latestSession"]["endKind"], "stopped");

    // An operation that exits non-zero could not answer, and its stderr is the reason. The
    // Session stays open, and the same request tries again.
    let running = fixture.json(&["--json", "actor", "start", "fixture"]);
    assert_eq!(running["session"]["observation"]["kind"], "running");
    let killed = fixture.json(&["--json", "session", "kill", "fixture"]);
    assert!(killed["session"]["end"].is_null(), "{killed}");
    let kill = &killed["session"]["lifecycle"]["kill"];
    assert!(kill["attempt"].is_null(), "{kill}");
    assert_eq!(kill["observed"]["kind"], "unknown");
    assert!(
        kill["observed"]["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("refuses to kill")),
        "{kill}"
    );
    fs::write(provider.join("kill-answers"), "").unwrap();
    let killed = fixture.json(&["--json", "session", "kill", "fixture"]);
    let kill = &killed["session"]["lifecycle"]["kill"];
    assert_eq!(kill["attempt"], "the fixture Provider killed the execution");
    assert_eq!(kill["observed"]["kind"], "gone");
    assert_eq!(killed["session"]["endKind"], "stopped");
}

#[test]
fn a_failed_cleanup_is_logged_with_the_provider_and_the_session() {
    let fixture = Fixture::new();
    fixture.start_daemon();
    let workdir = fixture.home().join("work");
    fs::create_dir_all(&workdir).unwrap();
    let started = fixture.json(&[
        "--json",
        "actor",
        "create",
        "fixture",
        "--kind",
        "agent",
        "--runtime",
        "fixture-runtime",
        "--provider",
        "fixture-provider",
        "--workdir",
        workdir.to_str().unwrap(),
    ]);
    let session = started["session"]["id"].as_str().unwrap().to_owned();
    let provider = fixture.plugin_dir("provider", "fixture-provider");
    fs::write(provider.join("cleanup-fails"), "").unwrap();

    let stopped = fixture.json(&["--json", "session", "stop", "fixture"]);
    assert_eq!(stopped["session"]["endKind"], "stopped", "{stopped}");
    let log = fs::read_to_string(fixture.home().join("daemon.log")).expect("read the daemon log");
    let line = log
        .lines()
        .find(|line| line.contains("could not clean up"))
        .unwrap_or_else(|| panic!("the failed cleanup was not logged: {log}"));
    assert!(
        line.starts_with(&format!(
            "fixture-provider could not clean up after Session '{session}': "
        )),
        "{line}"
    );
    assert!(
        line.contains("the fixture Provider cannot clean up"),
        "{line}"
    );
}

#[test]
fn self_token_follows_the_environment_then_the_self_index() {
    let fixture = Fixture::new();

    // Nothing to resolve: nothing printed, and the exit code says so.
    let missing = fixture.feld(&["self", "token"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());

    // An owned Session carries its token in the environment.
    let from_environment = fixture
        .command(&["self", "token"])
        .env("FELD_SESSION_TOKEN", "owned-token")
        .output()
        .unwrap();
    assert!(from_environment.status.success());
    assert_eq!(
        String::from_utf8_lossy(&from_environment.stdout).trim(),
        "owned-token"
    );

    // An attached Session finds it through the self index of the Provider whose `self`
    // answers, under the key core derives from that Provider and its selector.
    let home = feld_core::home::FeldHome::resolve(Some(fixture.home().to_path_buf())).unwrap();
    home.create().unwrap();
    let key = feld_core::plugin::self_index_key("fixture-provider", &json!({"id": "here"}));
    let lock = feld_core::session_token::lock_self_index(&home, &key).unwrap();
    let process = feld_core::process::incarnation(std::process::id())
        .unwrap()
        .expect("this test process is running");
    feld_core::session_token::write_self_index(
        &home,
        "session_fixture",
        "fixture",
        &process,
        "indexed",
        &lock,
    )
    .unwrap();
    drop(lock);

    let from_index = fixture
        .command(&["self", "token"])
        .env("FIXTURE_SELF", "here")
        .output()
        .unwrap();
    assert!(
        from_index.status.success(),
        "{}",
        String::from_utf8_lossy(&from_index.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&from_index.stdout).trim(),
        "indexed"
    );

    // The same index says nothing where no Provider recognises the environment.
    let elsewhere = fixture.feld(&["self", "token"]);
    assert_eq!(elsewhere.status.code(), Some(1));
    assert!(elsewhere.stdout.is_empty());

    // A Provider whose `self` fails is passed over like one that answers nothing.
    let failing = fixture
        .command(&["self", "token"])
        .env("FIXTURE_SELF_FAILS", "fixture-provider")
        .env("FIXTURE_SELF", "here")
        .output()
        .unwrap();
    assert_eq!(
        failing.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&failing.stderr)
    );
    assert!(failing.stdout.is_empty());

    // The resolution goes on to the Provider after it (Providers are asked in name order),
    // which finds the token.
    let broken = fixture.plugin_dir("provider", "broken-self");
    copy_tree(&fixture.plugin_dir("provider", "fixture-provider"), &broken);
    fs::write(
        broken.join("plugin.json"),
        r#"{"name":"broken-self","kind":"provider","label":"Broken Self"}"#,
    )
    .unwrap();
    let past_failing = fixture
        .command(&["self", "token"])
        .env("FIXTURE_SELF_FAILS", "broken-self")
        .env("FIXTURE_SELF", "here")
        .output()
        .unwrap();
    assert!(
        past_failing.status.success(),
        "{}",
        String::from_utf8_lossy(&past_failing.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&past_failing.stdout).trim(),
        "indexed"
    );
}

#[test]
fn attach_sweeps_the_self_index_entries_of_ended_runtimes_and_superseded_places() {
    let fixture = Fixture::new();
    let home = feld_core::home::FeldHome::resolve(Some(fixture.home().to_path_buf())).unwrap();
    home.create().unwrap();
    let running = feld_core::process::incarnation(std::process::id())
        .unwrap()
        .expect("this test process is running");
    let ended = feld_core::process::ProcessIncarnation {
        pid: running.pid,
        start_time: "Thu Jan  1 00:00:00 1970".to_owned(),
    };
    let key = |id: &str| feld_core::plugin::self_index_key("fixture-provider", &json!({"id": id}));
    let write = |id: &str, session: &str, process: &feld_core::process::ProcessIncarnation| {
        let lock = feld_core::session_token::lock_self_index(&home, &key(id)).unwrap();
        feld_core::session_token::write_self_index(
            &home, session, "fixture", process, "token", &lock,
        )
        .unwrap();
    };
    write("alive", "session_alive", &running);
    write("ended", "session_ended", &ended);
    // An attach that never wrote its entry leaves only the lock file.
    drop(feld_core::session_token::lock_self_index(&home, &key("abandoned")).unwrap());
    // The same place attached again replaces the entry and the token of the Session before.
    write("here", "session_before", &ended);
    // The entry the Session after had at another place goes, and the token file it shares
    // with the new entry stays.
    write("elsewhere", "session_after", &running);
    let lock = feld_core::session_token::lock_self_index(&home, &key("here")).unwrap();
    feld_core::session_token::write_self_index(
        &home,
        "session_after",
        "fixture",
        &running,
        "token",
        &lock,
    )
    .unwrap();
    feld_core::session_token::sweep_self_index(&home, &lock, "session_after").unwrap();
    drop(lock);

    let names = |directory: PathBuf| {
        let mut names = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    assert_eq!(
        names(home.token_dir()),
        ["session_after.token", "session_alive.token"]
    );
    let mut index = [key("alive"), key("here")]
        .iter()
        .flat_map(|key| [format!("{key}.json"), format!("{key}.lock")])
        .collect::<Vec<_>>();
    index.sort();
    assert_eq!(names(home.self_index_dir()), index);
}

fn read_json(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}
