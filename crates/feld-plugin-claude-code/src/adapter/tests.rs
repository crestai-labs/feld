use super::*;

/// The plugin files of this version, from the Plugin's directory in the repository.
fn plugin_files() -> Vec<(PathBuf, Vec<u8>)> {
    plugin_dir::root_files(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/runtime/claude-code"),
    )
    .unwrap()
}

fn snapshot(executable: &str, argv0: &str) -> ProcessSnapshot {
    ProcessSnapshot {
        pid: 1,
        started_at: "now".to_owned(),
        executable: executable.to_owned(),
        argv: vec![argv0.to_owned()],
    }
}

#[test]
fn only_a_claude_executable_run_as_claude_matches() {
    assert!(is_claude(&snapshot("claude", "/usr/local/bin/claude")));
    assert!(!is_claude(&snapshot(
        "claude-malware",
        "/tmp/claude-malware"
    )));
    assert!(!is_claude(&snapshot("claude", "/tmp/claude-malware")));
    assert!(!is_claude(&ProcessSnapshot {
        pid: 1,
        started_at: "now".to_owned(),
        executable: "claude".to_owned(),
        argv: Vec::new(),
    }));
}

#[test]
fn transcript_is_located_by_session_id_under_any_project() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let session = "0f1e2d3c-4b5a-6978-8a9b-0c1d2e3f4a5b";
    let project = root.join("projects/-Users-me-work");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join(format!("{session}.jsonl")), b"{}\n").unwrap();
    assert_eq!(
        locate_transcript(root, session),
        Some(project.join(format!("{session}.jsonl")))
    );
    assert_eq!(locate_transcript(root, "other"), None);
    // A session id that could escape the projects directory never names a file.
    assert_eq!(locate_transcript(root, "../../etc/passwd"), None);
}

#[test]
fn the_launch_environment_answers_feld_telemetry_and_a_cancelled_parent_marker() {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().to_owned())).unwrap();
    let collector = "http://127.0.0.1:4318";
    let request = RuntimeLaunchRequest {
        session: "session_0aa4d9".to_owned(),
        actor: "writer".to_owned(),
        workspace: "default".to_owned(),
        workdir: temporary.path().display().to_string(),
        runtime_settings: None,
        trust_workdir: false,
        env: Default::default(),
        session_dir: temporary.path().join("sessions/session_0aa4d9/runtime"),
        token: "token".to_owned(),
        collector: Some(collector.to_owned()),
    };
    let api = "http://127.0.0.1:8080";
    let environment = launch_environment(
        owned_environment(&home, api, &request, request.collector.as_deref()),
        request.collector.as_deref(),
        Some("/configs/claude".to_owned()),
    )
    .into_iter()
    .collect::<std::collections::BTreeMap<_, _>>();

    assert_eq!(environment["FELD_API"], api);
    assert_eq!(environment["FELD_ACTOR"], "writer");
    assert_eq!(environment["FELD_SESSION_TOKEN"], "token");
    assert_eq!(environment["OTEL_EXPORTER_OTLP_ENDPOINT"], collector);
    // A Provider server started from inside a Claude Code hands the pane these, and a
    // Claude Code that keeps them saves no transcript.
    for marker in feld_core::process::CLAUDE_CODE_PARENT_MARKERS {
        assert_eq!(environment[marker], "", "{marker}");
    }
    assert_eq!(environment["CLAUDE_CODE_FORCE_SESSION_PERSISTENCE"], "1");
    // The pane need not have the daemon's configuration root, so the launch names it.
    assert_eq!(environment["CLAUDE_CONFIG_DIR"], "/configs/claude");
}

#[test]
fn the_otel_headers_helper_is_written_kept_or_replaced() {
    let helper = "/c/My Config/skills/feld/bin/otel-headers";
    let quoted = json!("'/c/My Config/skills/feld/bin/otel-headers'");
    assert_eq!(
        helper_entry(None, helper),
        (PrepareAction::Write, Some(quoted.clone()))
    );
    assert_eq!(
        helper_entry(Some(&quoted), helper),
        (PrepareAction::Keep, None)
    );
    // The bare path an earlier Feld wrote is Feld's, and is written again quoted.
    assert_eq!(
        helper_entry(Some(&json!(helper)), helper),
        (PrepareAction::Write, Some(quoted.clone()))
    );
    // A person's value is replaced with consent, even one that runs Feld's helper.
    for theirs in [
        "~/mine.sh",
        "env CUSTOM_HEADER=1 '/c/My Config/skills/feld/bin/otel-headers'",
        "/elsewhere/skills/feld/bin/otel-headers",
    ] {
        assert_eq!(
            helper_entry(Some(&json!(theirs)), helper),
            (PrepareAction::Replace, Some(quoted.clone())),
            "{theirs}"
        );
    }
}

#[test]
fn a_status_line_that_runs_the_helper_or_that_a_person_changed_is_kept() {
    let helper = "/c/skills/feld/bin/status-line";
    let keeps = |current: Value, written: Option<Value>| {
        assert_eq!(
            status_line_entry(Some(&current), helper, written.as_ref(), None),
            (PrepareAction::Keep, None),
            "{current}"
        );
    };
    keeps(json!({"type": "command", "command": quote(helper)}), None);
    keeps(
        json!({"type": "command", "command": format!("{} --chain 'mine'", quote(helper))}),
        None,
    );
    // However the configuration root is spelled.
    keeps(
        json!({"type": "command", "command": "~/.claude/skills/feld/bin/status-line"}),
        None,
    );
    // A script of the person's that calls the helper itself.
    let temporary = tempfile::tempdir().unwrap();
    let script = temporary.path().join("statusline.sh");
    fs::write(
        &script,
        "#!/bin/sh\nprintf '%s' \"$input\" | ~/.claude/skills/feld/bin/status-line\n",
    )
    .unwrap();
    keeps(json!({"type": "command", "command": script}), None);
    keeps(
        json!({"type": "command", "command": format!("bash {}", script.display())}),
        None,
    );
    // A value Feld wrote and the person changed afterwards.
    keeps(
        json!({"type": "command", "command": "~/other.sh"}),
        Some(json!({"type": "command", "command": quote(helper)})),
    );
}

#[test]
fn a_status_line_an_earlier_feld_wrote_bare_is_written_again_quoted() {
    let helper = "/c/My Config/skills/feld/bin/status-line";
    let quoted = "'/c/My Config/skills/feld/bin/status-line'";
    assert_eq!(
        status_line_entry(
            Some(&json!({"type": "command", "command": helper})),
            helper,
            None,
            None
        ),
        (
            PrepareAction::Write,
            Some(json!({"type": "command", "command": quoted}))
        )
    );
    let chained =
        json!({"type": "command", "command": format!("{helper} --chain 'mine'"), "padding": 0});
    assert_eq!(
        status_line_entry(Some(&chained), helper, Some(&chained), None),
        (
            PrepareAction::Write,
            Some(
                json!({"type": "command", "command": format!("{quoted} --chain 'mine'"), "padding": 0})
            )
        )
    );
}

#[test]
fn another_status_line_is_chained_and_one_without_a_command_replaced() {
    let helper = "/c/skills/feld/bin/status-line";
    let quoted = quote(helper);
    assert_eq!(
        status_line_entry(None, helper, None, None),
        (
            PrepareAction::Write,
            Some(json!({"type": "command", "command": quoted}))
        )
    );
    let current = json!({"type": "command", "command": "~/it's.sh", "padding": 0});
    assert_eq!(
        status_line_entry(Some(&current), helper, None, None),
        (
            PrepareAction::Chain,
            Some(json!({
                "type": "command",
                "command": format!("{quoted} --chain '~/it'\\''s.sh'"),
                "padding": 0
            }))
        )
    );
    // A prepare whose settings were never written leaves the person's value as the
    // one it replaced.
    let written = json!({"type": "command", "command": format!("{quoted} --chain 'mine'")});
    let mine = json!({"type": "command", "command": "mine"});
    assert_eq!(
        status_line_entry(Some(&mine), helper, Some(&written), Some(&mine)),
        (PrepareAction::Chain, Some(written.clone()))
    );
    assert_eq!(
        status_line_entry(Some(&json!({"type": "static"})), helper, None, None),
        (
            PrepareAction::Replace,
            Some(json!({"type": "command", "command": quoted}))
        )
    );
}

#[test]
fn a_setting_feld_wrote_is_removed_restored_or_unchained_and_one_changed_since_kept() {
    let helper = "'/c/My Config/skills/feld/bin/status-line'";
    let headers = json!("'/c/My Config/skills/feld/bin/otel-headers'");
    let chained = json!({"type": "command", "command": format!("{helper} --chain '~/it'\\''s.sh'"), "padding": 0});
    let record = PrepareRecord {
        written: [
            ("otelHeadersHelper".to_owned(), headers.clone()),
            ("statusLine".to_owned(), chained.clone()),
        ]
        .into_iter()
        .collect(),
        replaced: [("otelHeadersHelper".to_owned(), json!("~/mine.sh"))]
            .into_iter()
            .collect(),
        files: Default::default(),
    };
    assert_eq!(
        restored_setting("otelHeadersHelper", &headers, Some(&record)),
        Some((RestoreAction::Restore, Some(json!("~/mine.sh"))))
    );
    assert_eq!(
        restored_setting("statusLine", &chained, Some(&record)),
        Some((
            RestoreAction::Unchain,
            Some(json!({"type": "command", "command": "~/it's.sh", "padding": 0}))
        ))
    );
    assert_eq!(
        restored_setting("otelHeadersHelper", &json!("~/other.sh"), Some(&record)),
        Some((RestoreAction::Keep, None))
    );
    let written_alone = PrepareRecord {
        written: [("otelHeadersHelper".to_owned(), headers.clone())]
            .into_iter()
            .collect(),
        replaced: Map::new(),
        files: Default::default(),
    };
    assert_eq!(
        restored_setting("otelHeadersHelper", &headers, Some(&written_alone)),
        Some((RestoreAction::Remove, None))
    );
}

#[test]
fn without_a_record_only_the_values_that_name_the_helpers_go() {
    // Quoted as Feld writes it, and bare as an earlier Feld wrote it.
    for (helper, headers) in [
        (
            "'/c/My Config/skills/feld/bin/status-line'",
            "'/c/My Config/skills/feld/bin/otel-headers'",
        ),
        (
            "/c/My Config/skills/feld/bin/status-line",
            "/c/My Config/skills/feld/bin/otel-headers",
        ),
    ] {
        assert_without_a_record(helper, headers);
    }
    assert_eq!(
        restored_setting("otelHeadersHelper", &json!("~/mine.sh"), None),
        None
    );
}

fn assert_without_a_record(helper: &str, headers: &str) {
    assert_eq!(
        restored_setting(
            "statusLine",
            &json!({"type": "command", "command": helper}),
            None
        ),
        Some((RestoreAction::Remove, None))
    );
    assert_eq!(
        restored_setting(
            "statusLine",
            &json!({"type": "command", "command": format!("{helper} --chain mine")}),
            None
        ),
        Some((
            RestoreAction::Unchain,
            Some(json!({"type": "command", "command": "mine"}))
        ))
    );
    assert_eq!(
        restored_setting("otelHeadersHelper", &json!(headers), None),
        Some((RestoreAction::Remove, None))
    );
}

#[test]
fn restore_takes_back_what_prepare_wrote_under_the_recorded_root() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("My Config");
    fs::create_dir_all(root.join("skills/feld/bin")).unwrap();
    // The bare path an earlier Feld wrote.
    let helper = root
        .join("skills/feld/bin/otel-headers")
        .display()
        .to_string();
    let settings = root.join("settings.json");
    fs::write(
        &settings,
        serde_json::to_vec(&json!({"model": "opus", "otelHeadersHelper": helper})).unwrap(),
    )
    .unwrap();
    let record = PrepareRecord {
        written: [("otelHeadersHelper".to_owned(), json!(helper))]
            .into_iter()
            .collect(),
        replaced: Map::new(),
        files: Default::default(),
    };

    let mut dry = RestoreResponse::default();
    restore_root(&root, Some(&record), &plugin_files(), false, &mut dry).unwrap();
    let actions: Vec<_> = dry
        .entries
        .iter()
        .map(|entry| (entry.key.as_str(), entry.action))
        .collect();
    assert_eq!(
        actions,
        [
            ("skills/feld/", RestoreAction::Remove),
            ("otelHeadersHelper", RestoreAction::Remove)
        ]
    );
    assert!(root.join("skills/feld").exists());

    restore_root(
        &root,
        Some(&record),
        &plugin_files(),
        true,
        &mut RestoreResponse::default(),
    )
    .unwrap();
    assert!(!root.join("skills/feld").exists());
    let left: Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    assert_eq!(left, json!({"model": "opus"}));
}

#[test]
fn only_the_whole_telemetry_environment_an_earlier_feld_wrote_is_feld_s() {
    let earlier: Map<String, Value> =
        feld_core::telemetry::shell_exporter_environment("http://127.0.0.1:4318")
            .into_iter()
            .map(|(key, value)| (key, Value::String(value)))
            .collect();
    let settings = |environment: &Map<String, Value>| {
        [("env".to_owned(), Value::Object(environment.clone()))]
            .into_iter()
            .collect::<Map<String, Value>>()
    };
    let mut with_mine = earlier.clone();
    with_mine.insert("MINE".to_owned(), json!("1"));
    match feld_environment(&settings(&with_mine)) {
        FeldEnvironment::Whole(removed) => {
            assert_eq!(removed, earlier);
            assert_eq!(
                without_environment(&settings(&with_mine), &removed),
                Some(json!({"MINE": "1"}))
            );
            assert_eq!(without_environment(&settings(&earlier), &removed), None);
        }
        _ => panic!("the whole environment is Feld's"),
    }
    let mut own = earlier.clone();
    own.insert(
        "OTEL_EXPORTER_OTLP_ENDPOINT".to_owned(),
        json!("https://collector.example.com"),
    );
    assert!(matches!(
        feld_environment(&settings(&own)),
        FeldEnvironment::Absent
    ));
    let mut part = earlier.clone();
    part.remove("OTEL_METRIC_EXPORT_INTERVAL");
    assert!(matches!(
        feld_environment(&settings(&part)),
        FeldEnvironment::Part
    ));
    assert!(matches!(
        feld_environment(&Map::new()),
        FeldEnvironment::Absent
    ));
}

#[test]
fn the_environment_prepare_took_out_comes_back_beside_a_persons() {
    let replaced = json!({"OTEL_LOGS_EXPORTER": "otlp", "OTEL_METRICS_EXPORTER": "otlp"});
    assert_eq!(
        restored_environment(None, &replaced),
        Some(replaced.clone())
    );
    assert_eq!(
        restored_environment(Some(&json!({"OTEL_LOGS_EXPORTER": "none"})), &replaced),
        Some(json!({"OTEL_LOGS_EXPORTER": "none", "OTEL_METRICS_EXPORTER": "otlp"}))
    );
    assert_eq!(restored_environment(Some(&replaced), &replaced), None);
}

#[test]
fn adapter_owned_settings_are_refused_before_anything_is_written() {
    for owned in ADAPTER_OWNED_SETTINGS {
        let error = declared_settings(Some(json!({owned: "mine"})))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("adapter-owned key '{owned}'")),
            "{error}"
        );
    }
    assert!(declared_settings(Some(json!("text"))).is_err());
    assert_eq!(declared_settings(None).unwrap(), Map::new());
    assert_eq!(
        declared_settings(Some(json!({"model": "opus"}))).unwrap()["model"],
        "opus"
    );
}

#[test]
fn restore_keeps_a_plugin_directory_holding_files_feld_did_not_put_there() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("config");
    let plugin = root.join("skills/feld");
    let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
    runtime::write_files(&home, &plugin, plugin_files()).unwrap();
    fs::write(plugin.join("bin/mine"), "").unwrap();
    fs::write(plugin.join("skills/feld/SKILL.md"), "edited").unwrap();

    let mut response = RestoreResponse::default();
    restore_root(&root, None, &plugin_files(), true, &mut response).unwrap();
    assert_eq!(response.entries[0].action, RestoreAction::Keep);
    assert_eq!(
        response.entries[0].current.as_deref(),
        Some("bin/mine, skills/feld/SKILL.md")
    );
    assert!(plugin.join("bin/otel-headers").is_file());
    assert!(plugin.join("bin/mine").is_file());
    // Nothing went, so there is nothing a missing record leaves unknown.
    assert_eq!(response.notes.len(), 1, "{:?}", response.notes);
}

#[test]
fn the_sandbox_reads_and_writes_its_own_spool_and_none_of_the_rest() {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().to_owned())).unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let mut settings = json!({
        "sandbox": {"enabled": true, "filesystem": {"denyRead": ["~/.ssh"]}},
        "permissions": {"deny": ["Bash(rm:*)"]},
    })
    .as_object()
    .unwrap()
    .clone();
    confine(&mut settings, &home, "session_1").unwrap();
    let path = |relative: &str| root.join(relative).display().to_string();
    let filesystem = &settings["sandbox"]["filesystem"];
    assert_eq!(
        filesystem["denyRead"],
        json!(["~/.ssh", path("admin"), path("spool"), path("tokens")])
    );
    assert_eq!(filesystem["allowRead"], json!([path("spool/session_1")]));
    assert_eq!(filesystem["allowWrite"], json!([path("spool/session_1")]));
    assert_eq!(settings["sandbox"]["enabled"], true);
    assert_eq!(
        settings["permissions"]["deny"],
        json!([
            "Bash(rm:*)",
            format!("Read(/{}/**)", path("admin")),
            format!("Read(/{}/**)", path("spool")),
            format!("Read(/{}/**)", path("tokens")),
        ])
    );

    let mut refused = json!({"sandbox": {"filesystem": {"denyRead": "~/.ssh"}}})
        .as_object()
        .unwrap()
        .clone();
    let error = confine(&mut refused, &home, "session_1").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("runtimeSettings sandbox.filesystem.denyRead must be an array"),
        "{error:#}"
    );
}
