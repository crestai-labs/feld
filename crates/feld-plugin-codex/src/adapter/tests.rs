use super::*;

/// The skill files of this version, from the Plugin's directory in the repository.
fn skill_files() -> Vec<(PathBuf, Vec<u8>)> {
    skill_dir::files(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/runtime/codex"))
        .unwrap()
}

fn restore_root_now(root: &Path, apply: bool, response: &mut RestoreResponse) {
    restore_root(root, &skill_files(), &Default::default(), apply, response).unwrap();
}

#[test]
fn the_note_names_each_part_that_is_absent_or_from_another_version() {
    let skill = Path::new("/c/skills/feld");
    let hooks = Path::new("/c/hooks.json");
    assert_eq!(
        unprepared_note(skill, PartState::Other, hooks, PartState::Current, None),
        "the Feld skill under /c/skills/feld is from another feld version; run `feld init`"
    );
    assert_eq!(
        unprepared_note(skill, PartState::Absent, hooks, PartState::Absent, None),
        "the Feld skill is not under /c/skills/feld; the Feld hooks are not in /c/hooks.json; run `feld init`"
    );
    assert_eq!(
        unprepared_note(skill, PartState::Current, hooks, PartState::Other, None),
        "the Feld hooks in /c/hooks.json are from another feld version; run `feld init`"
    );
    assert_eq!(
        unprepared_note(
            skill,
            PartState::Current,
            hooks,
            PartState::Current,
            Some((
                PartState::Absent,
                Path::new("/f/spool/shared"),
                Path::new("/c/config.toml")
            ))
        ),
        "the Feld spool /f/spool/shared is not in the writable_roots of /c/config.toml; run `feld init`"
    );
    assert_eq!(
        unprepared_note(
            skill,
            PartState::Current,
            hooks,
            PartState::Current,
            Some((
                PartState::Other,
                Path::new("/f/spool/shared"),
                Path::new("/c/config.toml")
            ))
        ),
        "the writable_roots of /c/config.toml hold the whole Feld spool another feld version listed, in place of /f/spool/shared; run `feld init`"
    );
}

#[test]
fn prepare_adds_the_shared_spool_and_replaces_the_whole_spool_an_earlier_version_listed() {
    let legacy = Path::new("/f/spool");
    let shared = Path::new("/f/spool/shared");
    let config = |text: &str| -> DocumentMut { text.parse().unwrap() };

    let mut absent = config("model = \"o3\"\n");
    assert_eq!(
        spool_action(&absent, legacy, shared).unwrap(),
        PrepareAction::Add
    );
    let added = prepare_spool(&mut absent, PrepareAction::Add, legacy, shared).unwrap();
    assert_eq!(added, Some(Added::Table));
    assert_eq!(writable_roots::roots(&absent).unwrap(), ["/f/spool/shared"]);
    assert_eq!(
        spool_action(&absent, legacy, shared).unwrap(),
        PrepareAction::Keep
    );

    // A root above the spool that a person listed covers the shared spool.
    let above = config("[sandbox_workspace_write]\nwritable_roots = [\"/f\"]\n");
    assert_eq!(
        spool_action(&above, legacy, shared).unwrap(),
        PrepareAction::Keep
    );

    let mut earlier =
        config("[sandbox_workspace_write]\nwritable_roots = [\"/work\", \"/f/spool\"]\n");
    assert_eq!(
        spool_action(&earlier, legacy, shared).unwrap(),
        PrepareAction::Replace
    );
    let added = prepare_spool(&mut earlier, PrepareAction::Replace, legacy, shared).unwrap();
    assert_eq!(added, Some(Added::Element));
    assert_eq!(
        writable_roots::roots(&earlier).unwrap(),
        ["/work", "/f/spool/shared"]
    );
    assert_eq!(
        spool_action(&earlier, legacy, shared).unwrap(),
        PrepareAction::Keep
    );

    // A shared spool a person listed beside the whole spool stays, and Feld adds nothing.
    let mut both = config(
        "[sandbox_workspace_write]\nwritable_roots = [\"/f/spool/shared\", \"/work\", \"/f/spool\"]\n",
    );
    let added = prepare_spool(&mut both, PrepareAction::Replace, legacy, shared).unwrap();
    assert_eq!(added, None);
    assert_eq!(
        writable_roots::roots(&both).unwrap(),
        ["/f/spool/shared", "/work"]
    );
}

#[test]
fn the_spool_record_names_the_element_prepare_listed() {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
    let earlier: SpoolRecord = serde_json::from_value(serde_json::json!("key")).unwrap();
    assert_eq!(earlier.added(), Added::Key);
    assert_eq!(earlier.path(&home), home.root().join("spool"));
    let listed: SpoolRecord =
        serde_json::from_value(serde_json::json!({"path": "/f/spool/shared", "added": "element"}))
            .unwrap();
    assert_eq!(listed.added(), Added::Element);
    assert_eq!(listed.path(&home), PathBuf::from("/f/spool/shared"));
}

#[test]
fn only_the_hook_groups_that_run_the_feld_shims_are_undeclared() {
    let skill = Path::new("/c/skills/feld");
    let theirs = json!({"matcher": "", "hooks": [{"type": "command", "command": "/mine.sh"}]});
    let mut document = Map::new();
    document.insert("hooks".to_owned(), json!({"Stop": [theirs.clone()]}));
    let declared = declared_hooks(document.clone(), skill).unwrap();
    let (undeclared, removed) = undeclared_hooks(declared, skill);
    assert_eq!(removed, HOOK_EVENTS.len());
    assert_eq!(undeclared, document);

    let (empty, removed) = undeclared_hooks(declared_hooks(Map::new(), skill).unwrap(), skill);
    assert_eq!(removed, HOOK_EVENTS.len());
    assert!(empty.is_empty(), "{empty:?}");
}

#[test]
fn restore_takes_back_the_skill_and_the_feld_hooks_under_a_root() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let skill = root.join("skills/feld");
    fs::create_dir_all(skill.join("bin")).unwrap();
    let theirs = json!({"matcher": "", "hooks": [{"type": "command", "command": "/mine.sh"}]});
    let mut document = Map::new();
    document.insert("hooks".to_owned(), json!({"Stop": [theirs]}));
    let hooks_path = root.join("hooks.json");
    let declared = declared_hooks(document.clone(), &skill).unwrap();
    fs::write(&hooks_path, serde_json::to_vec(&declared).unwrap()).unwrap();

    let mut dry = RestoreResponse::default();
    restore_root_now(root, false, &mut dry);
    assert_eq!(dry.entries.len(), 2);
    assert!(skill.exists());

    restore_root_now(root, true, &mut RestoreResponse::default());
    assert!(!skill.exists());
    assert_eq!(read_hooks(&hooks_path).unwrap(), document);
}

#[test]
fn restore_empties_a_linked_hooks_file_of_feld_hooks_alone_and_keeps_the_link() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("codex");
    let skill = root.join("skills/feld");
    fs::create_dir_all(skill.join("bin")).unwrap();
    let real = temporary.path().join("hooks.json");
    let declared = declared_hooks(Map::new(), &skill).unwrap();
    fs::write(&real, serde_json::to_vec(&declared).unwrap()).unwrap();
    let hooks_path = root.join("hooks.json");
    std::os::unix::fs::symlink(&real, &hooks_path).unwrap();

    restore_root_now(&root, true, &mut RestoreResponse::default());
    assert!(
        fs::symlink_metadata(&hooks_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(read_hooks(&real).unwrap(), Map::new());
}

#[test]
fn restore_takes_only_the_spool_prepare_listed() {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
    let root = temporary.path().join("codex");
    fs::create_dir_all(&root).unwrap();
    let both = "model = \"o3\"\n\n[sandbox_workspace_write]\nwritable_roots = [\"/f/spool\", \"/f/spool/shared\"]\n";

    // Either spool a person listed beside the one Feld listed stays theirs.
    for (ours, theirs) in [
        ("/f/spool", "/f/spool/shared"),
        ("/f/spool/shared", "/f/spool"),
    ] {
        fs::write(root.join("config.toml"), both).unwrap();
        let mut response = RestoreResponse::default();
        restore_spool(
            &home,
            &root,
            Path::new(ours),
            Added::Table,
            true,
            &mut response,
        )
        .unwrap();
        let removed: Vec<_> = response
            .entries
            .iter()
            .map(|entry| entry.current.as_deref().unwrap())
            .collect();
        assert_eq!(removed, [ours]);
        let config = writable_roots::read(&root.join("config.toml")).unwrap();
        assert_eq!(writable_roots::roots(&config).unwrap(), [theirs]);
    }

    let mut last = RestoreResponse::default();
    let shared = Path::new("/f/spool/shared");
    fs::write(
        root.join("config.toml"),
        "model = \"o3\"\n\n[sandbox_workspace_write]\nwritable_roots = [\"/f/spool/shared\"]\n",
    )
    .unwrap();
    restore_spool(&home, &root, shared, Added::Table, true, &mut last).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("config.toml")).unwrap(),
        "model = \"o3\"\n"
    );
    let mut again = RestoreResponse::default();
    restore_spool(&home, &root, shared, Added::Table, true, &mut again).unwrap();
    assert!(again.entries.is_empty(), "{:?}", again.entries);
}

#[test]
fn restore_takes_the_spool_out_of_a_linked_config_and_keeps_the_link() {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
    let root = temporary.path().join("codex");
    fs::create_dir_all(&root).unwrap();
    let target = temporary.path().join("dotfiles.toml");
    let person = "# mine\n[sandbox_workspace_write]\nwritable_roots = [\"/work\"]\n";
    fs::write(&target, person).unwrap();
    std::os::unix::fs::symlink(&target, root.join("config.toml")).unwrap();
    let spool = Path::new("/f/spool");
    let mut config = writable_roots::read(&root.join("config.toml")).unwrap();
    writable_roots::add(&mut config, spool).unwrap();
    writable_roots::write(&home, &root.join("config.toml"), &config).unwrap();
    assert!(fs::read_to_string(&target).unwrap().contains("/f/spool"));

    let mut dry = RestoreResponse::default();
    restore_spool(&home, &root, spool, Added::Element, false, &mut dry).unwrap();
    assert_eq!(dry.entries.len(), 1);
    assert_eq!(dry.entries[0].current.as_deref(), Some("/f/spool"));

    restore_spool(
        &home,
        &root,
        spool,
        Added::Element,
        true,
        &mut RestoreResponse::default(),
    )
    .unwrap();
    assert!(
        fs::symlink_metadata(root.join("config.toml"))
            .unwrap()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), person);
}

const THREAD: &str = "01a0c16a-1e37-76b3-a04a-31f907002406";

fn snapshot(executable: &str, argv: &[&str]) -> ProcessSnapshot {
    ProcessSnapshot {
        pid: 1,
        started_at: "now".to_owned(),
        executable: executable.to_owned(),
        argv: argv.iter().map(|argument| (*argument).to_owned()).collect(),
    }
}

fn request(
    collector: Option<&str>,
    runtime_settings: Option<Value>,
    workdir: &Path,
) -> RuntimeLaunchRequest {
    RuntimeLaunchRequest {
        session: "session_0aa4d9".to_owned(),
        actor: "writer".to_owned(),
        workspace: "default".to_owned(),
        workdir: workdir.display().to_string(),
        runtime_settings,
        trust_workdir: false,
        env: [("GH_CONFIG_DIR", "/gh"), ("FELD_ACTOR", "spoof")]
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect(),
        session_dir: workdir.join("sessions/session_0aa4d9/runtime"),
        token: "token".to_owned(),
        collector: collector.map(str::to_owned),
    }
}

fn launched(
    collector: Option<&str>,
    runtime_settings: Option<Value>,
) -> Result<RuntimeLaunchResponse> {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().to_owned())).unwrap();
    build_launch(
        &request(collector, runtime_settings, temporary.path()),
        &home,
        Path::new("/plugins/runtime/codex"),
        "http://127.0.0.1:8080",
        Some("/configs/codex".to_owned()),
        None,
    )
}

/// The settings of one launch, as the `-c` arguments spell them.
fn overrides(response: &RuntimeLaunchResponse) -> Vec<String> {
    response
        .command
        .windows(2)
        .filter(|pair| pair[0] == "-c")
        .map(|pair| pair[1].clone())
        .collect()
}

#[test]
fn a_launch_carries_the_hooks_the_exporters_and_a_first_prompt() {
    let response = launched(
        Some("http://127.0.0.1:4318"),
        Some(json!({"model": "gpt-5.6"})),
    )
    .unwrap();

    assert_eq!(response.command[..2], ["codex", "--no-daemon"]);
    assert_eq!(
        response.command[response.command.len() - 2..],
        ["--", INITIAL_PROMPT]
    );
    assert!(response.identity.is_none());

    let overrides = overrides(&response);
    // What the launch spec declared comes first, as the TOML of its JSON.
    assert_eq!(overrides[0], r#"model="gpt-5.6""#);
    for event in HOOK_EVENTS {
        assert!(
            overrides.contains(&format!(
                r#"hooks.{event}=[{{matcher="",hooks=[{{type="command",command="/plugins/runtime/codex/bin/hook"}}]}}]"#
            )),
            "{event} is not declared in {overrides:?}"
        );
    }
    // Codex posts to the endpoint as it is given, so each signal carries its own path.
    assert!(overrides.contains(
        &r#"otel.exporter={otlp-http={endpoint="http://127.0.0.1:4318/v1/logs",protocol="binary"}}"#.to_owned()
    ), "{overrides:?}");
    assert!(overrides.contains(
        &r#"otel.metrics_exporter={otlp-http={endpoint="http://127.0.0.1:4318/v1/metrics",protocol="binary"}}"#.to_owned()
    ), "{overrides:?}");
    assert!(overrides.contains(&"check_for_update_on_startup=false".to_owned()));

    let environment = response.env;
    assert_eq!(environment["FELD_API"], "http://127.0.0.1:8080");
    assert_eq!(environment["FELD_ACTOR"], "writer");
    assert_eq!(environment["FELD_WORKSPACE"], "default");
    assert_eq!(environment["FELD_SESSION_TOKEN"], "token");
    assert_eq!(environment["FELD_SESSION_ID"], "session_0aa4d9");
    assert_eq!(
        environment["OTEL_EXPORTER_OTLP_HEADERS"],
        format!("Authorization=Bearer {}", telemetry_token("token"))
    );
    assert_eq!(environment["OTEL_METRIC_EXPORT_INTERVAL"], "10000");
    // The pane need not have the daemon's configuration root, so the launch names it.
    assert_eq!(environment["CODEX_HOME"], "/configs/codex");
    // The declared environment reaches Codex beneath the Adapter's own variables.
    assert_eq!(environment["GH_CONFIG_DIR"], "/gh");
}

/// A workdir the Admin trusts is declared trusted under the path Codex resolves it to,
/// as well as under the path it was given, and the hooks it brings run unreviewed.
#[test]
fn a_launch_trusts_the_workdir_the_admin_trusts() {
    let temporary = tempfile::tempdir().unwrap();
    let workdir = temporary.path();
    let home = FeldHome::resolve(Some(workdir.join("feld"))).unwrap();
    let response = build_launch(
        &RuntimeLaunchRequest {
            trust_workdir: true,
            ..request(None, None, workdir)
        },
        &home,
        Path::new("/plugins/runtime/codex"),
        "http://127.0.0.1:8080",
        None,
        None,
    )
    .unwrap();

    let declared: Vec<String> = overrides(&response)
        .into_iter()
        .filter(|argument| argument.starts_with("projects="))
        .collect();
    assert_eq!(declared.len(), 1, "{declared:?}");
    for path in [workdir.to_owned(), fs::canonicalize(workdir).unwrap()] {
        assert!(
            declared[0].contains(&format!(
                r#""{}"={{trust_level="trusted"}}"#,
                path.display()
            )),
            "{} is not trusted in {declared:?}",
            path.display()
        );
    }
    assert!(
        response
            .command
            .contains(&"--dangerously-bypass-hook-trust".to_owned()),
        "{:?}",
        response.command
    );
}

/// A workdir the Admin has not declared trusted is left to Codex's own configuration,
/// so the settings and hooks of its `.codex/` take no effect unasked.
#[test]
fn a_launch_leaves_an_undeclared_workdir_untrusted() {
    let response = launched(None, None).unwrap();

    assert!(
        !overrides(&response)
            .iter()
            .any(|argument| argument.starts_with("projects=")),
        "{:?}",
        response.command
    );
    assert!(
        !response
            .command
            .contains(&"--dangerously-bypass-hook-trust".to_owned()),
        "{:?}",
        response.command
    );
}

/// The trust a launch declares for its hooks is the one Codex 0.160.0 records when a
/// person trusts the same hooks declared on the command line.
#[test]
fn a_launch_trusts_its_own_hooks_as_codex_records_them() {
    let recorded = [
        (
            "session_start",
            "8be29a2613740202fbb35d8fb77eaf9ff89a6920abb119fa4acf80ba2f806aba",
        ),
        (
            "user_prompt_submit",
            "5a55a150cb8270cb73efc32698e95dd5120faae7925379bcccc5cfbbe2917c0e",
        ),
        (
            "post_tool_use",
            "33ccb884e6bf776f83a0af38e8f0ef89d1dc5ee64af1b7f34c5d6fadd5603f80",
        ),
        (
            "stop",
            "b8b5687505512f6175ca0e88717d1320e10f5a2a7bc8b7c467275f66ea9a94cc",
        ),
        (
            "subagent_start",
            "0c3cf5cb354eaffd8e22ded975cde5d0cbf166556037ed71dea1ef10be43dd62",
        ),
        (
            "subagent_stop",
            "51d7569fcec809d17e46c888a8007a01499ea18a33b24062faacd55b5b09cf12",
        ),
        (
            "post_compact",
            "f1532c23c3e289414ae307b4ccbfe4403fbfbf814ef4f420f0bf06ff7c650676",
        ),
        (
            "session_end",
            "f237380f674a520bfc3c75f02bf73b5d201b33365c36c55cce41d2ae6a2809dc",
        ),
    ];
    let expected = recorded
        .into_iter()
        .map(|(event, hash)| {
            format!(
                r#""/<session-flags>/config.toml:{event}:0:0"={{trusted_hash="sha256:{hash}"}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    let overrides = overrides(&launched(None, None).unwrap());
    assert!(
        overrides.contains(&format!("hooks.state={{{expected}}}")),
        "{overrides:?}"
    );
}

/// The hook groups `feld init` put in `hooks.json` are trusted where they sit, and a
/// group a person put there is left to Codex to ask about.
#[test]
fn a_launch_trusts_the_hooks_feld_init_prepared() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let skill = root.join("skills/feld");
    let person = json!({"matcher": "", "hooks": [{"type": "command", "command": "/usr/bin/true"}]});
    let document = declared_hooks(
        json!({"hooks": {"Stop": [person]}})
            .as_object()
            .unwrap()
            .clone(),
        &skill,
    )
    .unwrap();
    fs::write(
        root.join("hooks.json"),
        Value::Object(document.clone()).to_string(),
    )
    .unwrap();

    let trust: Map<String, Value> = prepared_hook_trust(root).into_iter().collect();
    let source = root.join("hooks.json").display().to_string();
    assert_eq!(trust.len(), HOOK_EVENTS.len(), "{trust:?}");
    let ours = &document["hooks"]["Stop"][1];
    assert_eq!(
        trust[&format!("{source}:stop:1:0")],
        json!({"trusted_hash": hook_hash("stop", ours)})
    );
    assert!(
        !trust.contains_key(&format!("{source}:stop:0:0")),
        "{trust:?}"
    );
}

/// A Feld hook group someone added a hook to or changed is no longer the one `feld
/// init` wrote, so the launch trusts none of it and Codex asks about it.
#[test]
fn a_launch_leaves_a_changed_feld_hook_group_to_codex() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let shim = root.join("skills/feld/bin/hook").display().to_string();
    let other = json!({"type": "command", "command": "/usr/bin/true"});
    let ours = json!({"type": "command", "command": shim});
    let document = json!({"hooks": {
        "SessionStart": [{"matcher": "", "hooks": [other, ours]}],
        "Stop": [{"matcher": "", "hooks": [{"type": "command", "command": shim, "timeout": 5}]}],
    }});
    fs::write(root.join("hooks.json"), document.to_string()).unwrap();

    assert_eq!(prepared_hook_trust(root), Vec::new());
}

/// The roots of a launch's sandbox: the declared ones or the prepared ones, without any
/// part of the spool, and with this Session's own spool last.
fn launched_roots(runtime_settings: Option<Value>, config: Option<&str>) -> (String, PathBuf) {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
    let spool = feld_core::spool::sandbox_path(&home);
    let root = temporary.path().join("codex");
    fs::create_dir_all(&root).unwrap();
    if let Some(config) = config {
        let config = config.replace("$SPOOL", &spool.display().to_string());
        fs::write(root.join("config.toml"), config).unwrap();
    }
    let response = build_launch(
        &request(None, runtime_settings, temporary.path()),
        &home,
        Path::new("/plugins/runtime/codex"),
        "http://127.0.0.1:8080",
        None,
        Some(&root),
    )
    .unwrap();
    let overrides = overrides(&response);
    let last = overrides.last().unwrap().clone();
    let declared: Vec<_> = overrides
        .iter()
        .filter(|argument| argument.starts_with("sandbox_workspace_write.writable_roots="))
        .collect();
    assert_eq!(declared.last().copied(), Some(&last), "{overrides:?}");
    (last, spool)
}

#[test]
fn a_launch_sandbox_writes_its_own_spool_and_no_other() {
    let own = |spool: &Path| spool.join("session_0aa4d9").display().to_string();

    let (roots, spool) = launched_roots(None, None);
    assert_eq!(
        roots,
        format!(
            r#"sandbox_workspace_write.writable_roots=["{}"]"#,
            own(&spool)
        )
    );

    let (roots, spool) = launched_roots(
        None,
        Some(
            "[sandbox_workspace_write]\nwritable_roots = [\"/work\", \"$SPOOL\", \"$SPOOL/shared\", \"/tmp\"]\n",
        ),
    );
    assert_eq!(
        roots,
        format!(
            r#"sandbox_workspace_write.writable_roots=["/work","/tmp","{}"]"#,
            own(&spool)
        )
    );

    // A declaration takes the place of the prepared roots, under the dotted key or in the table.
    let config = Some("[sandbox_workspace_write]\nwritable_roots = [\"/prepared\"]\n");
    for declared in [
        json!({"sandbox_workspace_write.writable_roots": ["/declared", "$SPOOL/other"]}),
        json!({"sandbox_workspace_write": {"writable_roots": ["/declared", "$SPOOL/other"], "network_access": true}}),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
        let spool = feld_core::spool::sandbox_path(&home);
        let declared: Value = serde_json::from_str(
            &declared
                .to_string()
                .replace("$SPOOL", &spool.display().to_string()),
        )
        .unwrap();
        let roots =
            sandbox_roots(declared.as_object().unwrap(), &home, "session_0aa4d9", None).unwrap();
        assert_eq!(roots, json!(["/declared", own(&spool)]), "{declared}");
        let (roots, _) = launched_roots(Some(declared.clone()), config);
        assert!(roots.contains(r#"["/declared","#), "{roots}");
    }

    let error = launched(
        None,
        Some(json!({"sandbox_workspace_write.writable_roots": "/work"})),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("must be an array of paths"), "{error}");
}

#[test]
fn a_launch_without_a_collector_exports_nowhere() {
    let overrides = overrides(&launched(None, None).unwrap());
    assert!(
        overrides.contains(&r#"otel.exporter="none""#.to_owned()),
        "{overrides:?}"
    );
    assert!(
        overrides.contains(&r#"otel.metrics_exporter="none""#.to_owned()),
        "{overrides:?}"
    );
}

#[test]
fn adapter_owned_settings_are_refused_and_the_first_prompt_is_the_one_declared() {
    for owned in ADAPTER_OWNED_SETTINGS {
        let error = launched(None, Some(json!({owned: {}})))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("adapter-owned key '{owned}'")),
            "{error}"
        );
    }
    assert!(launched(None, Some(json!("text"))).is_err());

    let declared = launched(None, Some(json!({INITIAL_PROMPT_SETTING: "start here"}))).unwrap();
    assert_eq!(declared.command.last().unwrap(), "start here");
    // A prompt that starts with `-` follows `--`, so Codex reads it as the prompt.
    let dashed = launched(None, Some(json!({INITIAL_PROMPT_SETTING: "--yolo"}))).unwrap();
    assert_eq!(dashed.command[dashed.command.len() - 2..], ["--", "--yolo"]);
    // The prompt is this Adapter's own, so it reaches Codex as the prompt and not as a
    // setting of its own.
    assert!(
        !overrides(&declared)
            .iter()
            .any(|argument| argument.starts_with(INITIAL_PROMPT_SETTING)),
        "{:?}",
        declared.command
    );
    assert!(launched(None, Some(json!({INITIAL_PROMPT_SETTING: ""}))).is_err());
    assert!(launched(None, Some(json!({INITIAL_PROMPT_SETTING: 7}))).is_err());
}

#[test]
fn a_codex_is_its_own_executable_or_the_node_that_runs_one() {
    assert!(is_codex(&snapshot("/opt/homebrew/bin/codex", &["codex"])));
    assert!(is_codex(&snapshot(
        "/usr/local/bin/node",
        &["node", "/usr/local/bin/codex"]
    )));
    assert!(is_codex(&snapshot(
        "/usr/local/bin/node",
        &["node", "/opt/@openai/codex/bin/codex.js", "--help"]
    )));
    assert!(!is_codex(&snapshot(
        "/tmp/codex-malware",
        &["codex-malware"]
    )));
    assert!(!is_codex(&snapshot(
        "/usr/local/bin/node",
        &["node", "server.js"]
    )));
    assert!(!is_codex(&snapshot("/usr/local/bin/node", &["node"])));
    assert!(!is_codex(&snapshot("claude", &["claude"])));
    // Node 24 on Linux: the command name is its main thread's, `MainThread`.
    assert!(is_codex(&snapshot(
        "MainThread",
        &["node", "/usr/local/bin/codex", "--no-daemon"]
    )));
    assert!(!is_codex(&snapshot("MainThread", &["node", "server.js"])));
    assert!(!is_codex(&snapshot("MainThread", &["MainThread"])));
}

#[test]
fn an_identity_is_the_thread_id_and_the_rollout_file_named_after_it() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let day = root.join("sessions/2026/09/21");
    fs::create_dir_all(&day).unwrap();
    let rollout = day.join(format!("rollout-2026-09-21T09-42-32-{THREAD}.jsonl"));
    fs::write(&rollout, b"{}\n").unwrap();

    let identity = identity_of(Some(THREAD), Some(root)).unwrap();
    assert_eq!(identity.session_id, THREAD);
    assert_eq!(
        identity.transcript_path,
        fs::canonicalize(&rollout).unwrap().display().to_string()
    );
    assert_eq!(identity.model, Value::Null);

    // A configuration root reached through a link answers the same file as the hooks
    // of that Codex, which report the path Codex resolved.
    let linked = temporary.path().join("linked");
    std::os::unix::fs::symlink(root, &linked).unwrap();
    assert_eq!(
        identity_of(Some(THREAD), Some(&linked))
            .unwrap()
            .transcript_path,
        identity.transcript_path
    );

    // A thread whose rollout file is not there yet has no identity to deliver.
    assert!(identity_of(Some("01a0c16b-56c2-7192-8f21-9811d2b18391"), Some(root)).is_none());
    assert!(identity_of(None, Some(root)).is_none());
    assert!(identity_of(Some(THREAD), None).is_none());
}

/// An attach from outside reads the thread from the rollout files the Codex holds open:
/// the one its own thread writes, not a Sub Agent's, nor any other file it holds.
#[test]
fn an_identity_from_outside_is_the_rollout_file_the_codex_holds_open() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("codex");
    let day = root.join("sessions/2026/10/06");
    fs::create_dir_all(&day).unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../feld-cli/tests/fixtures");
    let parent = "01a0c15d-8b37-7da3-82ba-6291e17a77ac";
    let child = "01a0c15d-9ee7-7823-b1da-519047940755";
    let own = day.join(format!("rollout-2026-10-06T14-16-58-{parent}.jsonl"));
    let sub_agent = day.join(format!("rollout-2026-10-06T14-17-30-{child}.jsonl"));
    fs::copy(fixtures.join("codex-0.150.0-parent.jsonl"), &own).unwrap();
    fs::copy(fixtures.join("codex-0.150.0-subagent.jsonl"), &sub_agent).unwrap();
    let lock = root.join(format!("thread-writer-locks/{parent}.lock"));

    let identity = identity_among(vec![lock.clone(), sub_agent.clone(), own.clone()]).unwrap();
    assert_eq!(identity.session_id, parent);
    assert_eq!(
        identity.transcript_path,
        own.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(identity.model, Value::Null);
    assert!(identity_among(vec![lock, sub_agent]).is_none());
    // The queue helper reaches the thread through the root the rollout file sits under.
    assert_eq!(rollout_config_root(&own), Some(root));
    assert_eq!(
        rollout_config_root(&temporary.path().join("rollout.jsonl")),
        None
    );
}

#[test]
fn only_a_uuid_is_looked_for_as_a_thread_id() {
    assert!(is_thread_id(THREAD));
    for value in [
        "",
        "01a0c16a",
        "../../etc/passwd",
        "01a0c16a-1e37-76b3-a04a-31f90700240",
        "01a0c16a-1e37-76b3-a04a-31f90700240z",
        "01a0c16a_1e37_76b3_a04a_31f907002406",
    ] {
        assert!(!is_thread_id(value), "'{value}' is no thread id");
    }
}

#[test]
fn the_hook_declaration_joins_what_the_person_declared() {
    let skill = Path::new("/codex/skills/feld");
    let mine = json!({"matcher": "", "hooks": [{"type": "command", "command": "/usr/bin/mine"}]});
    let theirs = json!({"hooks": {"Stop": [mine], "Notification": [mine]}});

    let written =
        Value::Object(declared_hooks(theirs.as_object().unwrap().clone(), skill).unwrap());
    let hooks = &written["hooks"];
    // An event this Adapter says nothing about, and a group a person declared, stay.
    assert_eq!(hooks["Notification"], json!([mine]));
    assert_eq!(hooks["Stop"][0], mine);
    assert_eq!(
        hooks["Stop"][1]["hooks"][0]["command"],
        skill.join("bin/hook").display().to_string()
    );
    // The queue helper is started by the hook, so Codex runs nothing else.
    assert_eq!(
        hooks["SessionStart"][0]["hooks"].as_array().unwrap().len(),
        1
    );
    for event in HOOK_EVENTS {
        assert!(hooks[event].is_array(), "{event} is not declared");
    }

    // Preparing again replaces what this Adapter declared rather than repeating it.
    let again = Value::Object(declared_hooks(written.as_object().unwrap().clone(), skill).unwrap());
    assert_eq!(again, written);
}

/// A `hooks.json` an earlier version wrote runs the queue helper as a hook of its own,
/// which Codex ends at the hook's timeout. Preparing again leaves only the hook.
#[test]
fn the_hook_declaration_replaces_the_queue_helper_an_earlier_version_declared() {
    let skill = Path::new("/codex/skills/feld");
    let hook = skill.join("bin/hook").display().to_string();
    let queue = skill.join("bin/queue-helper").display().to_string();
    let earlier = json!({"hooks": {"SessionStart": [{"matcher": "", "hooks": [
        {"type": "command", "command": hook},
        {"type": "command", "command": queue, "async": true},
    ]}]}});

    let written =
        Value::Object(declared_hooks(earlier.as_object().unwrap().clone(), skill).unwrap());
    assert_eq!(
        written["hooks"]["SessionStart"],
        json!([{"matcher": "", "hooks": [{"type": "command", "command": hook}]}])
    );
}

#[test]
fn a_hook_a_person_put_in_the_feld_group_stays() {
    let skill = Path::new("/c/skills/feld");
    let mine = json!({"type": "command", "command": "/mine.sh"});
    let mut declared = declared_hooks(Map::new(), skill).unwrap();
    declared["hooks"]["Stop"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(mine.clone());

    // Feld's hook is there as this version declares it, so the preparation keeps it.
    assert_eq!(declared_hooks(declared.clone(), skill).unwrap(), declared);

    let (undeclared, removed) = undeclared_hooks(declared, skill);
    assert_eq!(removed, HOOK_EVENTS.len());
    assert_eq!(
        Value::Object(undeclared),
        json!({"hooks": {"Stop": [{"matcher": "", "hooks": [mine]}]}})
    );
}

#[test]
fn a_feld_hook_of_another_version_is_replaced_beside_a_person_s_hook() {
    let skill = Path::new("/c/skills/feld");
    let mine = json!({"type": "command", "command": "/mine.sh"});
    let hook = skill.join("bin/hook").display().to_string();
    let earlier = json!({"type": "command", "command": hook, "timeout": 5});
    let mut document = Map::new();
    document.insert(
        "hooks".to_owned(),
        json!({"Stop": [{"matcher": "", "hooks": [earlier, mine.clone()]}]}),
    );

    let declared = declared_hooks(document, skill).unwrap();
    assert_eq!(
        declared["hooks"]["Stop"],
        json!([
            {"matcher": "", "hooks": [mine]},
            {"matcher": "", "hooks": [{"type": "command", "command": hook}]},
        ])
    );
}

#[test]
fn restore_keeps_a_skill_directory_holding_files_feld_did_not_put_there() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let skill = root.join("skills/feld");
    fs::create_dir_all(skill.join("bin")).unwrap();
    fs::write(skill.join("SKILL.md"), "mine").unwrap();
    fs::write(skill.join("notes.md"), "mine").unwrap();

    let mut response = RestoreResponse::default();
    restore_root_now(root, true, &mut response);
    assert_eq!(response.entries[0].action, RestoreAction::Keep);
    assert_eq!(
        response.entries[0].current.as_deref(),
        Some("SKILL.md, notes.md")
    );
    assert!(
        response.notes[0].contains("SKILL.md, notes.md"),
        "{:?}",
        response.notes
    );
    assert!(skill.join("SKILL.md").is_file());
    assert!(skill.join("notes.md").is_file());
}

#[test]
fn restore_keeps_a_skill_a_person_edited_and_takes_one_an_earlier_feld_wrote() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let skill = root.join("skills/feld");
    runtime::write_files(
        &FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap(),
        &skill,
        skill_files(),
    )
    .unwrap();
    fs::write(skill.join("SKILL.md"), "edited").unwrap();

    let mut response = RestoreResponse::default();
    restore_root_now(root, true, &mut response);
    assert_eq!(response.entries[0].action, RestoreAction::Keep);
    assert_eq!(response.entries[0].current.as_deref(), Some("SKILL.md"));
    assert!(skill.join("SKILL.md").is_file());

    // The SKILL.md an earlier version wrote, which the record knows, is Feld's.
    let written = runtime::file_digests(&[(PathBuf::from("SKILL.md"), b"edited".to_vec())]);
    let mut response = RestoreResponse::default();
    restore_root(root, &skill_files(), &written, true, &mut response).unwrap();
    assert_eq!(response.entries[0].action, RestoreAction::Remove);
    assert!(!skill.exists());
}

#[test]
fn a_workspace_write_launch_denies_the_feld_home_but_its_own_spool() {
    let temporary = tempfile::tempdir().unwrap();
    let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
    let spool = feld_core::spool::sandbox_path(&home);
    let root = spool.parent().unwrap();
    let own = spool.join("session_0aa4d9");
    let roots = json!(["/work", own.display().to_string()]);
    let profile = |declared: Value| {
        session_profile(
            declared.as_object().unwrap(),
            &home,
            "session_0aa4d9",
            None,
            &roots,
        )
    };

    let declared = profile(json!({
        "sandbox_mode": "workspace-write",
        "sandbox_workspace_write": {"network_access": true},
    }));
    let mut filesystem = Map::new();
    for (path, access) in [
        ("/work".to_owned(), "write"),
        (own.display().to_string(), "write"),
        (root.join("admin").display().to_string(), "deny"),
        (root.join("tokens").display().to_string(), "deny"),
        (spool.display().to_string(), "deny"),
    ] {
        filesystem.insert(path, json!(access));
    }
    assert_eq!(
        declared,
        vec![
            (
                "permissions.feld-session".to_owned(),
                json!({
                    "extends": ":workspace",
                    "network": {"enabled": true},
                    "filesystem": filesystem,
                }),
            ),
            ("default_permissions".to_owned(), json!("feld-session")),
        ]
    );

    // Another mode, or a profile the launch spec names, is the declaration's to keep.
    for declared in [
        json!({}),
        json!({"sandbox_mode": "read-only"}),
        json!({"sandbox_mode": "workspace-write", "default_permissions": "mine"}),
        json!({"sandbox_mode": "workspace-write", "permissions.mine.extends": ":workspace"}),
    ] {
        assert!(profile(declared.clone()).is_empty(), "{declared}");
    }

    // The mode and the network of the prepared config.toml stand without a declaration.
    let config = temporary.path().join("codex");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("config.toml"),
        "sandbox_mode = \"workspace-write\"\n",
    )
    .unwrap();
    let prepared = session_profile(&Map::new(), &home, "session_0aa4d9", Some(&config), &roots);
    assert_eq!(prepared[0].1["network"], json!({"enabled": false}));
}
