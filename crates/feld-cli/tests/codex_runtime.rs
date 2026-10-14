//! The operations of the codex Runtime Adapter, run the way core runs them: what a Codex
//! answers about itself from its own environment, and what a Codex a person starts
//! themselves needs written before it can be attached. The Session these make is in
//! `codex_session.rs`.
//!
//! Everything here runs against fixture roots alone: `FELD_HOME`, `HOME`, and `CODEX_HOME`
//! are temporary directories, so nothing reaches the real `~/.codex` or `~/.feld`.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

mod support;

/// The Plugin executable running `name`, with none of the `FELD_*` variables of the test
/// process nor its `CODEX_HOME`, `CODEX_THREAD_ID`, and `CODEX_SESSION_ID`: a test run inside
/// a Codex would otherwise answer for that Codex's thread.
fn plugin(plugin_dir: &Path, name: &str) -> Command {
    let mut command = Command::new(support::plugin_executable("codex"));
    support::without_outside_variables(&mut command)
        .arg(name)
        .env("FELD_PLUGIN_DIR", plugin_dir)
        .env_remove("CODEX_HOME")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_SESSION_ID");
    command
}

/// One operation of the Plugin executable, run the way core runs it: the request on stdin,
/// the response on stdout, and the environment core puts around it.
fn operation(
    plugin_dir: &Path,
    name: &str,
    request: &Value,
    environment: &[(&str, &Path)],
) -> Value {
    let mut command = plugin(plugin_dir, name);
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (key, value) in environment {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(request.to_string().as_bytes()).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "feld-plugin-codex {name} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// A Codex thread id, which is the UUID the rollout file is named after.
const THREAD: &str = "01a0c16a-1e37-76b3-a04a-31f907002406";

/// The `identity` a Codex answers for itself: the thread id of its own environment and the
/// rollout file of the configuration root named after it.
#[test]
fn identity_comes_from_the_thread_id_and_the_rollout_file() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("feld");
    support::install_plugins_with_codex(&home);
    let plugin_dir = home.join("plugins/runtime/codex");
    let codex_home = temporary.path().join("codex");
    let day = codex_home.join("sessions/2026/09/21");
    fs::create_dir_all(&day).unwrap();
    let rollout = day.join(format!("rollout-2026-09-21T09-42-32-{THREAD}.jsonl"));
    fs::write(&rollout, b"{}\n").unwrap();

    let answer = plugin(&plugin_dir, "identity")
        .env("FELD_HOME", &home)
        .env("HOME", &codex_home)
        .env("CODEX_HOME", &codex_home)
        .env("CODEX_THREAD_ID", THREAD)
        .output()
        .unwrap();
    let identity: Value = serde_json::from_slice(&answer.stdout).unwrap();
    assert_eq!(identity["identity"]["sessionId"], THREAD);
    // Codex reports the rollout file by the path it resolved, and so does this, so that the
    // identity of one thread is the same one however its root is reached.
    assert_eq!(
        identity["identity"]["transcriptPath"],
        fs::canonicalize(&rollout).unwrap().display().to_string()
    );
    assert!(identity["identity"]["model"].is_null());

    // Without a thread id in the environment there is nothing to relate the Session to.
    let without = plugin(&plugin_dir, "identity")
        .env("FELD_HOME", &home)
        .env("HOME", &codex_home)
        .env("CODEX_HOME", &codex_home)
        .output()
        .unwrap();
    let answered: Value = serde_json::from_slice(&without.stdout).unwrap();
    assert!(answered["identity"].is_null());
}

/// Preparing a Codex twice leaves it as the first preparation made it: the hook groups this
/// Adapter declared are replaced rather than repeated, and the shims stay the ones that
/// reach the Plugin. The shims find that Plugin in the Feld home of their environment, so
/// they name no Feld home. What one preparation writes, and what it leaves alone, is in
/// `codex_session.rs` where an attach makes it.
#[test]
fn preparing_a_codex_again_keeps_what_the_first_preparation_wrote() {
    let temporary = tempfile::tempdir().unwrap();
    let feld_home = temporary.path().join("feld");
    support::install_plugins_with_codex(&feld_home);
    let plugin_dir = feld_home.join("plugins/runtime/codex");
    let home = temporary.path().join("home");
    let codex_home = temporary.path().join("codex");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&codex_home).unwrap();
    let roots = [
        ("FELD_HOME", feld_home.as_path()),
        ("HOME", home.as_path()),
        ("CODEX_HOME", codex_home.as_path()),
    ];

    let response = operation(
        &plugin_dir,
        "prepare",
        &serde_json::json!({"apply": true, "collector": "http://127.0.0.1:4318"}),
        &roots,
    );
    // Codex fixes its OTLP header at startup, so the Codex being prepared exports nothing
    // this Session can be charged with however the preparation goes.
    assert_eq!(response["telemetryPrepared"], false);
    assert!(
        response["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note.as_str().unwrap().contains("trust")),
        "{response}"
    );
    let skill = codex_home.join("skills/feld");
    for name in ["hook", "queue-helper"] {
        let shim = skill.join("bin").join(name);
        assert!(shim.is_file(), "{name}");
        let script = fs::read_to_string(&shim).unwrap();
        assert!(script.contains(&format!("helper {name}")), "{name}");
        assert!(
            script.contains("${FELD_HOME:-$HOME/.feld}/plugins/runtime/codex/plugin"),
            "{script}"
        );
        assert!(
            !script.contains(&feld_home.display().to_string()),
            "{script}"
        );
    }
    let hooks: Value =
        serde_json::from_slice(&fs::read(codex_home.join("hooks.json")).unwrap()).unwrap();

    assert_eq!(response["prepared"], false, "{response}");
    let actions = |response: &Value| {
        response["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["action"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(actions(&response), ["write", "write", "add"]);
    // The shared spool is named by the path it resolves to, as Codex refuses a symlink in a
    // root.
    let spool = fs::canonicalize(&feld_home).unwrap().join("spool/shared");
    let config = fs::read_to_string(codex_home.join("config.toml")).unwrap();
    assert_eq!(
        config,
        format!(
            "[sandbox_workspace_write]\nwritable_roots = [\"{}\"]\n",
            spool.display()
        )
    );

    // Preparing again keeps what is there, and has nothing to say.
    let again = operation(
        &plugin_dir,
        "prepare",
        &serde_json::json!({"apply": true, "collector": Value::Null}),
        &roots,
    );
    assert_eq!(again["prepared"], true, "{again}");
    assert_eq!(actions(&again), ["keep", "keep", "keep"]);
    assert_eq!(again["notes"].as_array().unwrap().len(), 0);
    let repeated: Value =
        serde_json::from_slice(&fs::read(codex_home.join("hooks.json")).unwrap()).unwrap();
    assert_eq!(repeated, hooks);
    assert_eq!(
        fs::read_to_string(codex_home.join("config.toml")).unwrap(),
        config
    );
}

/// A check of a configuration root names each part the preparation would write, and says
/// whether it is not there or another feld version wrote it. A part already in place is
/// left out of the note.
#[test]
fn the_check_tells_an_absent_part_from_one_another_version_wrote() {
    let temporary = tempfile::tempdir().unwrap();
    let feld_home = temporary.path().join("feld");
    support::install_plugins_with_codex(&feld_home);
    let plugin_dir = feld_home.join("plugins/runtime/codex");
    let home = temporary.path().join("home");
    let codex_home = temporary.path().join("codex");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&codex_home).unwrap();
    let roots = [
        ("FELD_HOME", feld_home.as_path()),
        ("HOME", home.as_path()),
        ("CODEX_HOME", codex_home.as_path()),
    ];
    let check = || {
        let response = operation(
            &plugin_dir,
            "prepare",
            &serde_json::json!({"apply": false, "collector": Value::Null}),
            &roots,
        );
        let actions: Vec<(String, String)> = response["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                (
                    entry["key"].as_str().unwrap().to_owned(),
                    entry["action"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        let notes: Vec<String> = response["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|note| note.as_str().unwrap().to_owned())
            .collect();
        (actions, notes)
    };
    let pair = |skill: &str, hooks: &str| {
        vec![
            ("skills/feld/".to_owned(), skill.to_owned()),
            ("hooks".to_owned(), hooks.to_owned()),
            ("writable_roots".to_owned(), "keep".to_owned()),
        ]
    };
    let skill = codex_home.join("skills/feld");
    let hooks_path = codex_home.join("hooks.json");
    // A root a person listed above the spool lets the sandbox write it already.
    let config_path = codex_home.join("config.toml");
    let resolved_home = fs::canonicalize(&feld_home).unwrap();
    fs::write(
        &config_path,
        format!(
            "[sandbox_workspace_write]\nwritable_roots = [\"{}\"]\n",
            resolved_home.display()
        ),
    )
    .unwrap();

    // Nothing of Feld's is there yet.
    assert_eq!(
        check(),
        (
            pair("write", "write"),
            vec![format!(
                "the Feld skill is not under {}; the Feld hooks are not in {}; run `feld init`",
                skill.display(),
                hooks_path.display()
            )]
        )
    );

    operation(
        &plugin_dir,
        "prepare",
        &serde_json::json!({"apply": true, "collector": Value::Null}),
        &roots,
    );
    assert_eq!(check(), (pair("keep", "keep"), vec![]));

    // A skill a person edited is theirs, written into only with their consent.
    let skill_md = skill.join("SKILL.md");
    let current_skill = fs::read(&skill_md).unwrap();
    fs::write(&skill_md, b"# feld\n").unwrap();
    assert_eq!(
        check(),
        (
            pair("replace", "keep"),
            vec![format!(
                "{} holds files Feld did not put there; run `feld init`",
                skill.display()
            )]
        )
    );

    // The skill another feld version wrote, which the prepare record knows, beside the
    // hooks of this one.
    let record_path = feld_home.join("prepared/codex.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    let digest = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(b"# feld\n"));
    record["skills"][codex_home.display().to_string()]["SKILL.md"] = Value::String(digest);
    fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(
        check(),
        (
            pair("write", "keep"),
            vec![format!(
                "the Feld skill under {} is from another feld version; run `feld init`",
                skill.display()
            )]
        )
    );
    fs::write(&skill_md, current_skill).unwrap();

    // Hooks another feld version declared: one event of this version's is missing.
    let current_hooks = fs::read(&hooks_path).unwrap();
    let mut hooks: Value = serde_json::from_slice(&current_hooks).unwrap();
    let declared = hooks["hooks"].as_object_mut().unwrap();
    let event = declared.keys().next().unwrap().clone();
    declared.remove(&event);
    fs::write(&hooks_path, hooks.to_string()).unwrap();
    assert_eq!(
        check(),
        (
            pair("keep", "write"),
            vec![format!(
                "the Feld hooks in {} are from another feld version; run `feld init`",
                hooks_path.display()
            )]
        )
    );
    fs::write(&hooks_path, current_hooks).unwrap();

    // Without that root, the spool is not among the roots.
    fs::remove_file(&config_path).unwrap();
    let mut absent = pair("keep", "keep");
    absent[2].1 = "add".to_owned();
    assert_eq!(
        check(),
        (
            absent,
            vec![format!(
                "the Feld spool {} is not in the writable_roots of {}; run `feld init`",
                resolved_home.join("spool/shared").display(),
                config_path.display()
            )]
        )
    );
}

/// A shared spool a person listed among the writable roots themselves stays theirs: `prepare`
/// keeps it, and `restore` takes out only what `prepare` added, which here is nothing.
#[test]
fn a_shared_spool_a_person_listed_stays_through_prepare_and_restore() {
    let temporary = tempfile::tempdir().unwrap();
    let feld_home = temporary.path().join("feld");
    support::install_plugins_with_codex(&feld_home);
    let plugin_dir = feld_home.join("plugins/runtime/codex");
    let home = temporary.path().join("home");
    let codex_home = temporary.path().join("codex");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&codex_home).unwrap();
    let roots = [
        ("FELD_HOME", feld_home.as_path()),
        ("HOME", home.as_path()),
        ("CODEX_HOME", codex_home.as_path()),
    ];
    let spool = fs::canonicalize(&feld_home).unwrap().join("spool/shared");
    let config_path = codex_home.join("config.toml");
    let theirs = format!(
        "[sandbox_workspace_write]\nwritable_roots = [\"{}\"]\n",
        spool.display()
    );
    fs::write(&config_path, &theirs).unwrap();

    let prepared = operation(
        &plugin_dir,
        "prepare",
        &serde_json::json!({"apply": true, "collector": Value::Null}),
        &roots,
    );
    assert_eq!(prepared["entries"][2]["action"], "keep", "{prepared}");
    let record: Value =
        serde_json::from_slice(&fs::read(feld_home.join("prepared/codex.json")).unwrap()).unwrap();
    let restored = operation(
        &plugin_dir,
        "restore",
        &serde_json::json!({"apply": true, "record": record}),
        &roots,
    );
    assert!(
        restored["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["key"] != "writable_roots"),
        "{restored}"
    );
    assert_eq!(fs::read_to_string(&config_path).unwrap(), theirs);
}

/// The whole spool an earlier `feld init` listed among the writable roots opens every
/// Session's spool, so `prepare` puts the shared spool in its place, beside the other roots,
/// and `restore` takes the shared spool out again.
#[test]
fn the_shared_spool_replaces_the_whole_spool_through_prepare_and_restore() {
    let temporary = tempfile::tempdir().unwrap();
    let feld_home = temporary.path().join("feld");
    support::install_plugins_with_codex(&feld_home);
    let plugin_dir = feld_home.join("plugins/runtime/codex");
    let home = temporary.path().join("home");
    let codex_home = temporary.path().join("codex");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&codex_home).unwrap();
    let roots = [
        ("FELD_HOME", feld_home.as_path()),
        ("HOME", home.as_path()),
        ("CODEX_HOME", codex_home.as_path()),
    ];
    let resolved_home = fs::canonicalize(&feld_home).unwrap();
    let legacy = resolved_home.join("spool");
    let shared = resolved_home.join("spool/shared");
    let config_path = codex_home.join("config.toml");
    let work = temporary.path().join("work");
    fs::write(
        &config_path,
        format!(
            "[sandbox_workspace_write]\nwritable_roots = [\"{}\", \"{}\"]\n",
            work.display(),
            legacy.display()
        ),
    )
    .unwrap();

    let checked = operation(
        &plugin_dir,
        "prepare",
        &serde_json::json!({"apply": false, "collector": Value::Null}),
        &roots,
    );
    assert_eq!(checked["entries"][2]["action"], "replace", "{checked}");
    assert_eq!(
        checked["entries"][2]["current"],
        legacy.display().to_string(),
        "{checked}"
    );
    assert_eq!(
        checked["entries"][2]["next"],
        shared.display().to_string(),
        "{checked}"
    );

    let prepared = operation(
        &plugin_dir,
        "prepare",
        &serde_json::json!({"apply": true, "collector": Value::Null}),
        &roots,
    );
    assert_eq!(prepared["entries"][2]["action"], "replace", "{prepared}");
    assert_eq!(
        fs::read_to_string(&config_path).unwrap(),
        format!(
            "[sandbox_workspace_write]\nwritable_roots = [\"{}\", \"{}\"]\n",
            work.display(),
            shared.display()
        )
    );

    let record: Value =
        serde_json::from_slice(&fs::read(feld_home.join("prepared/codex.json")).unwrap()).unwrap();
    let restored = operation(
        &plugin_dir,
        "restore",
        &serde_json::json!({"apply": true, "record": record}),
        &roots,
    );
    assert!(
        restored["entries"].as_array().unwrap().iter().any(|entry| {
            entry["key"] == "writable_roots"
                && entry["action"] == "remove"
                && entry["current"] == shared.display().to_string()
        }),
        "{restored}"
    );
    assert_eq!(
        fs::read_to_string(&config_path).unwrap(),
        format!(
            "[sandbox_workspace_write]\nwritable_roots = [\"{}\"]\n",
            work.display()
        )
    );
}

/// A record an earlier `feld init` kept names the whole spool it listed, so `restore` takes
/// that out and keeps the shared spool a person listed beside it.
#[test]
fn restore_after_an_earlier_version_keeps_the_shared_spool_a_person_listed() {
    let temporary = tempfile::tempdir().unwrap();
    let feld_home = temporary.path().join("feld");
    support::install_plugins_with_codex(&feld_home);
    let plugin_dir = feld_home.join("plugins/runtime/codex");
    let home = temporary.path().join("home");
    let codex_home = temporary.path().join("codex");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&codex_home).unwrap();
    let roots = [
        ("FELD_HOME", feld_home.as_path()),
        ("HOME", home.as_path()),
        ("CODEX_HOME", codex_home.as_path()),
    ];
    let resolved_home = fs::canonicalize(&feld_home).unwrap();
    let legacy = resolved_home.join("spool");
    let shared = resolved_home.join("spool/shared");
    let config_path = codex_home.join("config.toml");
    fs::write(
        &config_path,
        format!(
            "[sandbox_workspace_write]\nwritable_roots = [\"{}\", \"{}\"]\n",
            legacy.display(),
            shared.display()
        ),
    )
    .unwrap();
    let root = codex_home.display().to_string();
    let record = serde_json::json!({"roots": [root], "spool": {root.clone(): "element"}});

    let restored = operation(
        &plugin_dir,
        "restore",
        &serde_json::json!({"apply": true, "record": record}),
        &roots,
    );
    let removed: Vec<_> = restored["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["key"] == "writable_roots")
        .map(|entry| entry["current"].clone())
        .collect();
    assert_eq!(removed, [legacy.display().to_string()], "{restored}");
    assert_eq!(
        fs::read_to_string(&config_path).unwrap(),
        format!(
            "[sandbox_workspace_write]\nwritable_roots = [\"{}\"]\n",
            shared.display()
        )
    );
}

/// A running daemon that starts its Codex Sessions with the settings of another configuration
/// root than the one prepared is noted apart from the other notes, as only a daemon started
/// from another environment settles it.
#[test]
fn a_daemon_on_another_configuration_root_is_noted() {
    use fs2::FileExt;

    let temporary = tempfile::tempdir().unwrap();
    let feld_home = temporary.path().join("feld");
    support::install_plugins_with_codex(&feld_home);
    let plugin_dir = feld_home.join("plugins/runtime/codex");
    let home = temporary.path().join("home");
    let codex_home = temporary.path().join("codex");
    let daemons = temporary.path().join("daemons-codex");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&codex_home).unwrap();
    let roots = [
        ("FELD_HOME", feld_home.as_path()),
        ("HOME", home.as_path()),
        ("CODEX_HOME", codex_home.as_path()),
    ];
    let check = || {
        operation(
            &plugin_dir,
            "prepare",
            &serde_json::json!({"apply": false, "collector": Value::Null}),
            &roots,
        )
    };
    let record = |codex_home: &Path| {
        fs::write(
            feld_home.join("daemon.json"),
            serde_json::json!({
                "instanceId": "test",
                "pid": 1,
                "api": "http://127.0.0.1:1",
                "environment": {"HOME": home, "CODEX_HOME": codex_home},
            })
            .to_string(),
        )
        .unwrap();
    };
    record(&daemons);

    // A record no running daemon holds says nothing.
    assert!(check()["daemonNotes"].is_null());

    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(feld_home.join("daemon.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    assert_eq!(
        check()["daemonNotes"],
        serde_json::json!([format!(
            "the running Feld daemon starts Codex with the settings under {daemons}, not {codex}; run `CODEX_HOME={daemons} feld init --runtime codex` too, or restart the daemon from this shell",
            daemons = daemons.display(),
            codex = codex_home.display()
        )])
    );
    // The same root, however it is spelled, is no other root.
    record(&codex_home.join("."));
    assert!(check()["daemonNotes"].is_null());
    lock.unlock().unwrap();
}
