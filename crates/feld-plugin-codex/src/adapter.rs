//! The operations core calls: what starts Codex, what recognises one, what its own
//! environment tells about it, and what a Codex a person started needs to be attached.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use toml_edit::DocumentMut;

use feld_core::home::{ADMIN_DIR, FeldHome, TOKENS_DIR};
use feld_core::plugin::protocol::{
    IdentityRequest, IdentityResponse, MatchRequest, MatchResponse, PrepareAction, PrepareEntry,
    PrepareRequest, PrepareResponse, RestoreAction, RestoreEntry, RestoreRequest, RestoreResponse,
    RuntimeIdentity, RuntimeLaunchRequest, RuntimeLaunchResponse, SelfResponse,
};
use feld_core::process::ProcessSnapshot;
use feld_core::telemetry::telemetry_token;

use crate::skill_dir::{self, HELPERS};
use crate::toml_value;
use crate::writable_roots::{self, Added};
use feld_core::plugin::sdk::env::{feld_home, plugin_directory};
use feld_core::plugin::sdk::runtime::{self, ConfigRoot};

/// Codex's configuration root: `CODEX_HOME`, otherwise `$HOME/.codex`.
pub(crate) const CONFIG_ROOT: ConfigRoot = ConfigRoot {
    variable: "CODEX_HOME",
    home_dir: ".codex",
};

/// The settings keys this Adapter owns. A declaration that sets one of them is refused,
/// because the Session would then reach neither its hooks nor its Collector.
const ADAPTER_OWNED_SETTINGS: [&str; 2] = ["hooks", "otel"];

/// The one Adapter-owned setting a declaration may carry: the prompt Codex starts its
/// first turn with.
const INITIAL_PROMPT_SETTING: &str = "initialPrompt";

/// What Codex is told when the launch spec declares no prompt of its own. Codex writes its
/// rollout file and fires `SessionStart` on the first input, so the launch carries one.
const INITIAL_PROMPT: &str = "This Codex runs as a Feld Session, and the `feld` command reaches Feld. Messages addressed to you arrive as the input of a new turn once your current turn ends; a running tool call never receives them. Answer `ready` and end this turn now. Wait by ending your turn, not with a sleep, a poll, or any other tool call.";

/// The hook events the two parts beside Codex are declared for.
const HOOK_EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "PostToolUse",
    "Stop",
    "SubagentStart",
    "SubagentStop",
    "PostCompact",
    "SessionEnd",
];

/// Answers what starts Codex with this Session's settings. Codex takes every setting on
/// the command line, so nothing is written for a launch.
pub fn launch(request: RuntimeLaunchRequest) -> Result<RuntimeLaunchResponse> {
    let home = feld_home()?;
    let plugin = plugin_directory()?;
    let api = runtime::feld_api()?;
    build_launch(
        &request,
        &home,
        &plugin,
        &api,
        CONFIG_ROOT.variable_value(),
        CONFIG_ROOT.path().as_deref(),
    )
}

fn build_launch(
    request: &RuntimeLaunchRequest,
    home: &FeldHome,
    plugin: &Path,
    api: &str,
    config_root: Option<String>,
    prepared_root: Option<&Path>,
) -> Result<RuntimeLaunchResponse> {
    let declared = declared_settings(request.runtime_settings.clone())?;
    let sandbox = sandbox_roots(&declared.settings, home, &request.session, prepared_root)?;
    let profile = session_profile(
        &declared.settings,
        home,
        &request.session,
        prepared_root,
        &sandbox,
    );
    // The thread runs in the TUI, not in a shared app-server whose restart would drop the
    // permissions the Session started with.
    let mut command = vec!["codex".to_owned(), "--no-daemon".to_owned()];
    let workdir = request.trust_workdir.then(|| Path::new(&request.workdir));
    for (key, value) in declared
        .settings
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .chain(owned_settings(
            &plugin.join("bin"),
            prepared_root,
            request.collector.as_deref(),
            workdir,
        ))
        // After the declared settings, so the roots of this Session's sandbox are these.
        .chain(profile)
        .chain([(SANDBOX_ROOTS_SETTING.to_owned(), sandbox)])
    {
        command.push("-c".to_owned());
        command.push(toml_value::override_argument(&key, &value)?);
    }
    // A workdir the Admin trusts brings its own hooks, which no person is there to review.
    if request.trust_workdir {
        command.push("--dangerously-bypass-hook-trust".to_owned());
    }
    command.push(declared.initial_prompt);
    Ok(RuntimeLaunchResponse {
        command,
        // The declared environment lies beneath the Adapter's own, and
        // `shell_environment_policy` decides what of it the shells Codex runs see.
        env: request
            .env
            .clone()
            .into_iter()
            .chain(launch_environment(home, api, request, config_root))
            .collect(),
        // Codex names the thread when it starts, so the hook delivers the identity.
        identity: None,
    })
}

/// What the launch spec declared as Codex's settings, refused when it claims a key this
/// Adapter owns, with the initial prompt taken out of it.
struct Declared {
    settings: Map<String, Value>,
    initial_prompt: String,
}

fn declared_settings(runtime_settings: Option<Value>) -> Result<Declared> {
    let mut settings = runtime::declared_settings(runtime_settings, &ADAPTER_OWNED_SETTINGS)?;
    let initial_prompt = match settings.remove(INITIAL_PROMPT_SETTING) {
        None => INITIAL_PROMPT.to_owned(),
        Some(Value::String(prompt)) if !prompt.is_empty() => prompt,
        Some(_) => bail!(
            "runtimeSettings '{INITIAL_PROMPT_SETTING}' must be the text Codex starts its first turn with"
        ),
    };
    Ok(Declared {
        settings,
        initial_prompt,
    })
}

/// The settings this Adapter owns: the hooks the two parts beside Codex run from and the
/// trust Codex keeps for them, the exporters that carry this Session's telemetry, the
/// workspace this Session works in when the Admin trusts it, and the update check a Session
/// has no person to answer.
fn owned_settings(
    bin: &Path,
    prepared_root: Option<&Path>,
    collector: Option<&str>,
    trusted_workdir: Option<&Path>,
) -> Vec<(String, Value)> {
    let hooks = hook_declarations(bin);
    let trust = hook_trust(&hooks)
        .into_iter()
        .chain(prepared_root.into_iter().flat_map(prepared_hook_trust))
        .collect();
    let mut settings = vec![("hooks.state".to_owned(), Value::Object(trust))];
    settings.extend(
        hooks
            .into_iter()
            .map(|(event, group)| (format!("hooks.{event}"), json!([group]))),
    );
    settings.push(("otel.exporter".to_owned(), exporter(collector, "/v1/logs")));
    settings.push((
        "otel.metrics_exporter".to_owned(),
        exporter(collector, "/v1/metrics"),
    ));
    if let Some(workdir) = trusted_workdir {
        settings.push(("projects".to_owned(), trusted_projects(workdir)));
    }
    settings.push(("check_for_update_on_startup".to_owned(), json!(false)));
    settings
}

/// The setting that lists the writable roots of a `workspace-write` sandbox.
const SANDBOX_ROOTS_SETTING: &str = "sandbox_workspace_write.writable_roots";

/// The writable roots of this Session's sandbox: the ones the launch spec declares,
/// otherwise the ones of the prepared `config.toml`, without any part of the spool, and
/// with the spool of this Session, the one part of the spool its sandbox writes.
fn sandbox_roots(
    declared: &Map<String, Value>,
    home: &FeldHome,
    session: &str,
    prepared_root: Option<&Path>,
) -> Result<Value> {
    let declared = declared.get(SANDBOX_ROOTS_SETTING).or_else(|| {
        declared
            .get("sandbox_workspace_write")
            .and_then(|table| table.get("writable_roots"))
    });
    let roots: Vec<String> = match declared {
        Some(roots) => roots
            .as_array()
            .and_then(|roots| {
                roots
                    .iter()
                    .map(|root| root.as_str().map(str::to_owned))
                    .collect()
            })
            .with_context(|| {
                format!("runtimeSettings '{SANDBOX_ROOTS_SETTING}' must be an array of paths")
            })?,
        None => prepared_root
            .and_then(|root| writable_roots::read(&root.join("config.toml")).ok())
            .and_then(|config| writable_roots::roots(&config).ok())
            .unwrap_or_default(),
    };
    let spool = feld_core::spool::sandbox_path(home);
    let own = feld_core::spool::sandbox_dir(
        home,
        &feld_core::spool::Directory::Session(session.to_owned()),
    );
    Ok(roots
        .into_iter()
        .filter(|root| !feld_core::spool::covered_by(Path::new(root), &spool))
        .chain([own.display().to_string()])
        .map(Value::String)
        .collect())
}

/// The permission profile a `workspace-write` sandbox of this Session runs under: Codex's
/// `:workspace` with the writable roots, the network the sandbox allows, and the Feld home
/// denied but for the Session's own spool. Codex takes `default_permissions` over
/// `sandbox_mode`, so a Session whose sandbox is another mode, or whose launch spec names its
/// own profile, runs without one.
fn session_profile(
    declared: &Map<String, Value>,
    home: &FeldHome,
    session: &str,
    prepared_root: Option<&Path>,
    roots: &Value,
) -> Vec<(String, Value)> {
    if declared.contains_key(PROFILE_SETTING)
        || declared
            .keys()
            .any(|key| key == "permissions" || key.starts_with("permissions."))
    {
        return Vec::new();
    }
    let config = prepared_root
        .and_then(|root| writable_roots::read(&root.join("config.toml")).ok())
        .unwrap_or_default();
    let mode = declared
        .get("sandbox_mode")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| config.get("sandbox_mode")?.as_str().map(str::to_owned));
    if mode.as_deref() != Some("workspace-write") {
        return Vec::new();
    }
    let network = declared
        .get("sandbox_workspace_write.network_access")
        .or_else(|| {
            declared
                .get("sandbox_workspace_write")?
                .get("network_access")
        })
        .and_then(Value::as_bool)
        .or_else(|| {
            config
                .get("sandbox_workspace_write")?
                .get("network_access")?
                .as_bool()
        })
        .unwrap_or(false);
    let spool = feld_core::spool::sandbox_path(home);
    let mut filesystem = Map::new();
    for root in roots
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        filesystem.insert(root.to_owned(), json!("write"));
    }
    if let Some(root) = spool.parent() {
        for denied in [ADMIN_DIR, TOKENS_DIR] {
            filesystem.insert(root.join(denied).display().to_string(), json!("deny"));
        }
    }
    filesystem.insert(spool.display().to_string(), json!("deny"));
    let own = feld_core::spool::sandbox_dir(
        home,
        &feld_core::spool::Directory::Session(session.to_owned()),
    );
    filesystem.insert(own.display().to_string(), json!("write"));
    vec![
        (
            format!("permissions.{PROFILE}"),
            json!({
                "extends": ":workspace",
                "network": {"enabled": network},
                "filesystem": filesystem,
            }),
        ),
        (PROFILE_SETTING.to_owned(), json!(PROFILE)),
    ]
}

/// The setting that names the permission profile Codex runs its sandbox under.
const PROFILE_SETTING: &str = "default_permissions";
/// The permission profile the launch declares for the sandbox of a Session.
const PROFILE: &str = "feld-session";

/// The workdir of this Session, declared as one Codex trusts. Codex asks about a directory
/// it has not been told to trust before it takes any input, and a Session has no person to
/// answer that. Codex resolves the directory it starts in before it looks the entry up, so
/// the resolved path names it, and the path as it was given is declared beside it.
fn trusted_projects(workdir: &Path) -> Value {
    let trusted = json!({"trust_level": "trusted"});
    let mut projects = Map::new();
    if let Ok(resolved) = fs::canonicalize(workdir) {
        projects.insert(resolved.display().to_string(), trusted.clone());
    }
    projects.insert(workdir.display().to_string(), trusted);
    Value::Object(projects)
}

/// Where Codex keeps the hooks declared on its command line, in the key of their trust.
const SESSION_FLAGS_SOURCE: &str = "/<session-flags>/config.toml";

/// The trust Codex keeps for each hook this Adapter declares, so that Codex runs them
/// without asking a person to review them. Codex keys the trust of a hook by its source,
/// event, group and position, and records the hash of the hook as Codex normalizes it.
fn hook_trust(hooks: &[(&'static str, Value)]) -> Vec<(String, Value)> {
    hooks
        .iter()
        .map(|(event, group)| trust_entry(SESSION_FLAGS_SOURCE, event, 0, group))
        .collect()
}

/// The trust of the hook groups `feld init` put in the `hooks.json` of the configuration
/// root, which Codex reads beside the ones a launch declares. Only a group exactly as
/// `feld init` writes it is trusted; a group someone changed is left to Codex to ask about.
fn prepared_hook_trust(root: &Path) -> Vec<(String, Value)> {
    let path = root.join("hooks.json");
    let Ok(document) = read_hooks(&path) else {
        return Vec::new();
    };
    let bin = root.join("skills/feld/bin");
    let prepared: Map<String, Value> = hook_declarations(&bin)
        .into_iter()
        .map(|(event, group)| (event.to_owned(), group))
        .collect();
    let source = path.display().to_string();
    document
        .get("hooks")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .flat_map(|(event, groups)| {
            groups
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .filter(|(_, group)| prepared.get(event) == Some(*group))
                .map(|(index, group)| trust_entry(&source, event, index, group))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The trust of the one hook of a group, as `hooks.state` holds it.
fn trust_entry(source: &str, event: &str, group_index: usize, group: &Value) -> (String, Value) {
    let event = event_key(event);
    let hash = hook_hash(&event, group);
    (
        format!("{source}:{event}:{group_index}:0"),
        json!({"trusted_hash": hash}),
    )
}

/// The name Codex gives an event in the trust key and the hash: `SessionStart` is
/// `session_start`.
fn event_key(event: &str) -> String {
    let mut key = String::with_capacity(event.len() + 4);
    for (index, character) in event.chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            key.push('_');
        }
        key.push(character.to_ascii_lowercase());
    }
    key
}

/// The hash Codex trusts one hook group by: SHA-256 of the compact JSON of the group with
/// its keys sorted, the event beside it, and each command carrying the timeout Codex gives
/// it. Codex reads no matcher for `UserPromptSubmit` and `Stop`, and ends `SessionEnd` at
/// one second.
fn hook_hash(event: &str, group: &Value) -> String {
    let timeout = if event == "session_end" { 1 } else { 600 };
    let hooks: Vec<Value> = group["hooks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|hook| {
            // Written in the sorted order of the keys.
            json!({"async": false, "command": hook["command"], "timeout": timeout, "type": "command"})
        })
        .collect();
    let mut identity = Map::new();
    identity.insert("event_name".to_owned(), json!(event));
    identity.insert("hooks".to_owned(), json!(hooks));
    if !matches!(event, "user_prompt_submit" | "stop") {
        identity.insert("matcher".to_owned(), group["matcher"].clone());
    }
    let digest = Sha256::digest(Value::Object(identity).to_string());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

/// One hook group per event, running the hook reached at `bin`. Codex ends every hook it
/// runs at the hook's timeout, so the queue helper, which lives as long as the Session, is
/// no hook of its own: the hook starts it outside Codex's reach.
fn hook_declarations(bin: &Path) -> Vec<(&'static str, Value)> {
    let hook = bin.join("hook").display().to_string();
    HOOK_EVENTS
        .into_iter()
        .map(|event| {
            let hooks = json!([{"type": "command", "command": hook}]);
            (event, json!({"matcher": "", "hooks": hooks}))
        })
        .collect()
}

/// Where one signal goes. Codex posts to the endpoint as it is given, so the path the
/// Collector receives that signal on is part of it.
fn exporter(collector: Option<&str>, path: &str) -> Value {
    match collector {
        Some(collector) => json!({
            "otlp-http": {
                "endpoint": format!("{}{path}", collector.trim_end_matches('/')),
                "protocol": "binary",
            }
        }),
        None => json!("none"),
    }
}

/// The environment the Provider starts Codex in. Codex and everything below it inherits
/// this, so the two parts beside it and the `feld` the model runs from a shell all know
/// the Session, and every export of this process carries its telemetry token. The hooks this
/// Adapter declares are the ones of its own directory, which every Session of this machine
/// shares, so the Session they address is in the environment rather than in a shim. A pane
/// takes its environment from the Provider's server rather than from the daemon, so the
/// daemon's configuration root is handed on as well: the Adapter looks for the rollout there.
fn launch_environment(
    home: &FeldHome,
    api: &str,
    request: &RuntimeLaunchRequest,
    config_root: Option<String>,
) -> Vec<(String, String)> {
    runtime::feld_environment(home, api, request)
        .into_iter()
        .chain(
            [
                (
                    "OTEL_EXPORTER_OTLP_HEADERS",
                    format!("Authorization=Bearer {}", telemetry_token(&request.token)),
                ),
                ("OTEL_METRIC_EXPORT_INTERVAL", "10000".to_owned()),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value)),
        )
        .chain(config_root.map(|root| (CONFIG_ROOT.variable.to_owned(), root)))
        .collect()
}

/// Whether a process is a Codex: the executable or argv[0] is named `codex`, or it is the
/// `node` that runs the `codex` of an npm installation.
pub fn match_process(request: MatchRequest) -> Result<MatchResponse> {
    Ok(MatchResponse {
        matched: is_codex(&request.process),
        // Codex builds its OTLP header from the environment it started in, so a Codex a
        // person started carries no token and its telemetry reaches no Session.
        telemetry_prepared: Some(false),
        telemetry_reason: Some(ATTACHED_TELEMETRY.to_owned()),
    })
}

/// What an attached Codex's metrics come from, as the `telemetry` line of an attach says.
const ATTACHED_TELEMETRY: &str = "Codex attributes its telemetry only in Sessions Feld starts; this Session reports the rollout gauges";

fn is_codex(process: &ProcessSnapshot) -> bool {
    let name = |value: &str| {
        Path::new(value)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    };
    // Node 24 names its main thread `MainThread`, which Linux reports as the command name,
    // so the name the process was started under (argv[0]) counts as well.
    let names = [
        Some(process.executable.as_str()),
        process.argv.first().map(String::as_str),
    ]
    .into_iter()
    .flatten()
    .filter_map(name)
    .collect::<Vec<_>>();
    let runs_codex = || {
        process
            .argv
            .get(1)
            .and_then(|argument| name(argument))
            .is_some_and(|name| name == "codex" || name == "codex.js")
    };
    names.iter().any(|name| name == "codex")
        || (names.iter().any(|name| name == "node") && runs_codex())
}

/// The Runtime identity a Codex exposes to the processes it runs, from the thread its
/// environment names (the parent's for a Sub Agent) and the rollout file named after it.
/// An attach from outside names the Codex process instead, and the identity is the thread
/// whose rollout file that Codex holds open. `model` is left null; the hook delivers it.
pub fn identity(request: IdentityRequest) -> Result<IdentityResponse> {
    Ok(IdentityResponse {
        identity: match request.process {
            Some(process) => held_identity(process.pid)?,
            None => identity_of(own_thread().as_deref(), CONFIG_ROOT.path().as_deref()),
        },
    })
}

/// The identity of the thread a Codex process writes: the rollout file it, or the native
/// Codex an npm installation runs under it, holds open. A Sub Agent's thread has a rollout
/// file of its own in the same process, and the Session is the thread that started it.
fn held_identity(pid: u32) -> Result<Option<RuntimeIdentity>> {
    let open = feld_core::process::open_files(&feld_core::process::descendants(pid)?)?;
    Ok(identity_among(open))
}

/// The identity of the Session's thread among the files a Codex holds open.
fn identity_among(open: Vec<PathBuf>) -> Option<RuntimeIdentity> {
    let mut threads: Vec<(Option<std::time::SystemTime>, String, PathBuf)> = open
        .into_iter()
        .filter_map(|path| {
            let thread = rollout_thread(&path)?;
            (!is_sub_agent_rollout(&path)).then(|| {
                let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
                (modified, thread, path)
            })
        })
        .collect();
    // A thread the Codex left for a newer one may still be open; the newer one is written last.
    threads.sort_by_key(|(modified, ..)| std::cmp::Reverse(*modified));
    threads.into_iter().next().map(|(_, thread, path)| {
        let resolved = fs::canonicalize(&path).unwrap_or(path);
        RuntimeIdentity {
            session_id: thread,
            transcript_path: resolved.display().to_string(),
            model: Value::Null,
        }
    })
}

/// The thread a rollout file is named after, when the path is one:
/// `sessions/<YYYY>/<MM>/<DD>/rollout-<start>-<thread id>.jsonl`.
fn rollout_thread(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    let thread = stem.get(stem.len().checked_sub(36)?..)?;
    let sessions = path.ancestors().nth(DAY_DEPTH + 1)?;
    (is_thread_id(thread) && sessions.file_name()? == "sessions").then(|| thread.to_owned())
}

/// The configuration root a rollout file sits under, which is the one the Codex that
/// writes it reads.
pub(crate) fn rollout_config_root(rollout: &Path) -> Option<PathBuf> {
    rollout_thread(rollout)?;
    rollout
        .ancestors()
        .nth(DAY_DEPTH + 2)
        .map(Path::to_path_buf)
}

/// Whether a rollout file's first record says a Sub Agent's thread writes it.
fn is_sub_agent_rollout(path: &Path) -> bool {
    use std::io::BufRead;
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut first = String::new();
    if std::io::BufReader::new(file).read_line(&mut first).is_err() {
        return false;
    }
    serde_json::from_str::<Value>(&first).is_ok_and(|record| {
        record
            .pointer("/payload/thread_source")
            .and_then(Value::as_str)
            == Some("subagent")
            || record.pointer("/payload/source/subagent").is_some()
    })
}

/// Where Codex names the thread a tool runs in. A Sub Agent's tool runs in its own thread.
const THREAD_ENV: &str = "CODEX_THREAD_ID";

/// Where Codex names the thread that started the tree a tool's thread belongs to: the thread
/// itself, or for a Sub Agent the thread that started it. The hook sets it from the thread
/// its event names, which for a Sub Agent's event is the parent's too.
pub(crate) const SESSION_ENV: &str = "CODEX_SESSION_ID";

/// `self`: the thread the caller belongs to, from the environment Codex gives its tools. A
/// Sub Agent belongs to the thread that started it. Core checks that a Codex that runs
/// threads is in the caller's parent chain: the TUI that runs its thread in-process, or the
/// app-server a TUI connects to.
pub fn current() -> Result<SelfResponse> {
    Ok(match own_thread() {
        Some(thread) => SelfResponse {
            selector: Some(json!({ "thread": thread })),
            notes: vec![NO_DAEMON.to_owned()],
        },
        None => SelfResponse::default(),
    })
}

/// The thread the caller belongs to, as Codex names it in the environment of its tools.
fn own_thread() -> Option<String> {
    [SESSION_ENV, THREAD_ENV]
        .into_iter()
        .filter_map(|variable| std::env::var(variable).ok())
        .find(|thread| is_thread_id(thread))
}

/// How a person keeps a Codex thread in the process in front of the pane.
const NO_DAEMON: &str = "A Codex started as `codex --no-daemon` runs its thread in the pane, where `--self` finds the pane by itself.";

fn identity_of(thread: Option<&str>, config_root: Option<&Path>) -> Option<RuntimeIdentity> {
    let thread = thread.filter(|thread| is_thread_id(thread))?;
    let rollout = locate_rollout(config_root?, thread)?;
    // Codex resolves the path of the rollout file it writes, and its hooks report that
    // one. The identity of a Session is the same file however it is reached, so the
    // resolved path is what this answers too.
    let resolved = fs::canonicalize(&rollout).unwrap_or(rollout);
    Some(RuntimeIdentity {
        session_id: thread.to_owned(),
        transcript_path: resolved.display().to_string(),
        model: Value::Null,
    })
}

/// The Codex a person starts themselves: the `feld` skill and the hook declarations that
/// reach the two parts beside it, and the shared spool among the writable roots of its
/// sandbox, answered entry by entry and written when `apply`. Each is added beside what is
/// there, so an entry is written, added, replaced or kept, and a skill directory holding
/// files of the person's beside Feld's is written into with their consent, those files
/// staying.
pub fn prepare(request: PrepareRequest) -> Result<PrepareResponse> {
    let home = feld_home()?;
    let source = plugin_directory()?;
    // Codex reads its skills and its hook declarations from its configuration root, so the
    // preparation writes where that Codex looks.
    let root = CONFIG_ROOT.required()?;
    let skill = root.join("skills/feld");
    let hooks_path = root.join("hooks.json");
    let record_path = home.root().join(PREPARE_RECORD);
    let mut records: PrepareRecords = runtime::read_prepare_record(&record_path)?;
    let skill_files = skill_dir::files(&source)?;
    let skill_action = if runtime::holds_files(&skill, skill_files.clone()) {
        PrepareAction::Keep
    } else if runtime::foreign_files(
        &skill,
        &skill_files,
        &records.files(&root.display().to_string()),
    )
    .is_empty()
    {
        PrepareAction::Write
    } else {
        PrepareAction::Replace
    };
    let current_hooks = read_hooks(&hooks_path)?;
    let next_hooks = declared_hooks(current_hooks.clone(), &skill)?;
    let hooks_action = action(next_hooks == current_hooks);
    let config_path = root.join("config.toml");
    let legacy = feld_core::spool::sandbox_path(&home);
    let spool = feld_core::spool::sandbox_dir(&home, &feld_core::spool::Directory::Shared);
    let mut config = writable_roots::read(&config_path)?;
    let spool_action = spool_action(&config, &legacy, &spool)?;
    let entries = vec![
        PrepareEntry {
            path: skill.display().to_string(),
            key: "skills/feld/".to_owned(),
            current: None,
            next: None,
            action: skill_action,
        },
        PrepareEntry {
            path: hooks_path.display().to_string(),
            key: "hooks".to_owned(),
            current: None,
            next: None,
            action: hooks_action,
        },
        PrepareEntry {
            path: config_path.display().to_string(),
            key: "writable_roots".to_owned(),
            current: (spool_action == PrepareAction::Replace).then(|| legacy.display().to_string()),
            next: Some(spool.display().to_string()),
            action: spool_action,
        },
    ];
    let prepared = skill_action == PrepareAction::Keep
        && hooks_action == PrepareAction::Keep
        && spool_action == PrepareAction::Keep;

    let mut notes = Vec::new();
    if request.apply {
        let mut recorded = records.roots.insert(root.display().to_string());
        if skill_action != PrepareAction::Keep {
            records.skills.insert(
                root.display().to_string(),
                runtime::file_digests(&skill_files),
            );
            recorded = true;
        }
        if spool_action != PrepareAction::Keep {
            let key = root.display().to_string();
            match prepare_spool(&mut config, spool_action, &legacy, &spool)? {
                // An earlier addition a person took out by hand, or the one the shared spool
                // replaces, keeps what it brought; the element Feld lists is the shared spool.
                Some(added) => {
                    let added = records.spool.get(&key).map_or(added, SpoolRecord::added);
                    records.spool.insert(
                        key,
                        SpoolRecord::Listed {
                            path: spool.display().to_string(),
                            added,
                        },
                    );
                }
                // The shared spool a person listed stays theirs.
                None => {
                    records.spool.remove(&key);
                }
            }
            recorded = true;
        }
        // The record of what is added is kept before `config.toml` changes, and the record
        // of the skill's files before they are written, so they stay known as Feld's.
        if recorded {
            home.create_owner_dir(
                record_path
                    .parent()
                    .expect("the record sits in a directory"),
            )?;
            home.write_owner_file_atomic(&record_path, &serde_json::to_vec_pretty(&records)?)?;
        }
        if spool_action != PrepareAction::Keep {
            writable_roots::write(&home, &config_path, &config)?;
        }
        if skill_action != PrepareAction::Keep {
            skill_dir::write(&home, &source, &skill)?;
        }
        if hooks_action != PrepareAction::Keep {
            home.write_person_file(&hooks_path, &serde_json::to_vec_pretty(&next_hooks)?)?;
            notes.push("the next Codex you start asks to trust the Feld hooks".to_owned());
        }
    } else if !prepared {
        let skill_state = match skill_action {
            PrepareAction::Keep => PartState::Current,
            PrepareAction::Replace => PartState::Foreign,
            _ if fs::symlink_metadata(&skill).is_ok() => PartState::Other,
            _ => PartState::Absent,
        };
        let hooks_state = match hooks_action {
            PrepareAction::Keep => PartState::Current,
            _ if undeclared_hooks(current_hooks, &skill).1 > 0 => PartState::Other,
            _ => PartState::Absent,
        };
        notes.push(unprepared_note(
            &skill,
            skill_state,
            &hooks_path,
            hooks_state,
            match spool_action {
                PrepareAction::Keep => None,
                PrepareAction::Replace => Some((PartState::Other, &spool, &config_path)),
                _ => Some((PartState::Absent, &spool, &config_path)),
            },
        ));
    }
    Ok(PrepareResponse {
        entries,
        prepared,
        notes,
        daemon_notes: daemon_root_note(&home, &root).into_iter().collect(),
        // Codex fixes its OTLP header at startup, so a Codex that is already running
        // exports nothing this Session can be charged with.
        telemetry_prepared: Some(false),
        telemetry_reason: Some(ATTACHED_TELEMETRY.to_owned()),
    })
}

/// What preparing the shared `spool` does to the writable roots of `config`. The whole spool
/// an earlier `feld init` listed as `legacy` opens every Session's spool, so the shared spool
/// takes its place.
fn spool_action(config: &DocumentMut, legacy: &Path, spool: &Path) -> Result<PrepareAction> {
    Ok(if writable_roots::lists(config, legacy)? {
        PrepareAction::Replace
    } else if writable_roots::covers(config, spool)? {
        PrepareAction::Keep
    } else {
        PrepareAction::Add
    })
}

/// Does `action` to the writable roots of `config`, and answers what it brought there:
/// nothing when the shared spool that takes the place of `legacy` is listed already.
fn prepare_spool(
    config: &mut DocumentMut,
    action: PrepareAction,
    legacy: &Path,
    spool: &Path,
) -> Result<Option<Added>> {
    if action != PrepareAction::Replace {
        return writable_roots::add(config, spool).map(Some);
    }
    let listed = writable_roots::lists(config, spool)?;
    writable_roots::replace(config, legacy, spool)?;
    Ok((!listed).then_some(Added::Element))
}

/// The note of a running daemon that starts its Codex Sessions with the settings of another
/// configuration root than the one prepared here, whose sandbox the preparation therefore
/// does not reach. The daemon leaves the variables that name the root in its record.
fn daemon_root_note(home: &FeldHome, root: &Path) -> Option<String> {
    if !daemon_running(home) {
        return None;
    }
    let environment = feld_core::home::read_daemon_record(home).ok()?.environment;
    let daemon_root = match environment.get(CONFIG_ROOT.variable) {
        Some(root) => PathBuf::from(root),
        None => PathBuf::from(environment.get("HOME")?).join(CONFIG_ROOT.home_dir),
    };
    let resolved = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    (resolved(&daemon_root) != resolved(root)).then(|| {
        format!(
            "the running Feld daemon starts Codex with the settings under {}, not {}; run `CODEX_HOME={} feld init --runtime codex` too, or restart the daemon from this shell",
            daemon_root.display(),
            root.display(),
            daemon_root.display()
        )
    })
}

/// Whether a daemon holds the lock of the Feld home.
fn daemon_running(home: &FeldHome) -> bool {
    use std::os::fd::AsRawFd;
    let Ok(lock) = fs::File::open(home.lock_path()) else {
        return false;
    };
    // SAFETY: flock on a descriptor this function owns.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        // SAFETY: as above.
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) };
        return false;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK)
}

/// Where one part of the preparation stands in a configuration root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PartState {
    /// What this feld version writes.
    Current,
    /// There, as another feld version wrote it.
    Other,
    /// There, holding files Feld did not put there.
    Foreign,
    /// Not there.
    Absent,
}

/// The note of a configuration root `prepare` would write into, naming each part that is not
/// there and each part another feld version wrote. `spool` is where the shared spool stands
/// in a `config.toml` whose writable roots lack it, with the two paths.
fn unprepared_note(
    skill: &Path,
    skill_state: PartState,
    hooks: &Path,
    hooks_state: PartState,
    spool: Option<(PartState, &Path, &Path)>,
) -> String {
    let mut parts = Vec::new();
    match skill_state {
        PartState::Current => {}
        PartState::Other => parts.push(format!(
            "the Feld skill under {} is from another feld version",
            skill.display()
        )),
        PartState::Foreign => parts.push(format!(
            "{} holds files Feld did not put there",
            skill.display()
        )),
        PartState::Absent => parts.push(format!("the Feld skill is not under {}", skill.display())),
    }
    match hooks_state {
        PartState::Current => {}
        PartState::Other | PartState::Foreign => parts.push(format!(
            "the Feld hooks in {} are from another feld version",
            hooks.display()
        )),
        PartState::Absent => parts.push(format!("the Feld hooks are not in {}", hooks.display())),
    }
    match spool {
        None | Some((PartState::Current, ..)) => {}
        Some((PartState::Other | PartState::Foreign, spool, config)) => parts.push(format!(
            "the writable_roots of {} hold the whole Feld spool another feld version listed, in place of {}",
            config.display(),
            spool.display()
        )),
        Some((PartState::Absent, spool, config)) => parts.push(format!(
            "the Feld spool {} is not in the writable_roots of {}",
            spool.display(),
            config.display()
        )),
    }
    parts.push("run `feld init`".to_owned());
    parts.join("; ")
}

/// Where the Feld home keeps the configuration roots `prepare` wrote into, relative to the
/// home. It sits outside `plugins/`, which the daemon writes again at every start.
const PREPARE_RECORD: &str = "prepared/codex.json";

/// The configuration roots `prepare` wrote into. `prepare` only adds beside what is there,
/// so the root is all a restore needs to find what to take back, along with what adding
/// the spool of this Feld home brought into the `config.toml` of each root it was added to.
#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
struct PrepareRecords {
    roots: std::collections::BTreeSet<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    spool: std::collections::BTreeMap<String, SpoolRecord>,
    /// The digests of the files of each root's `skills/feld/` as `prepare` last wrote them.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    skills: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}

/// What adding the spool of this Feld home brought into the `config.toml` of a root: the
/// element `path`, or, in a record an earlier version kept, the whole spool.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
enum SpoolRecord {
    Listed { path: String, added: Added },
    Whole(Added),
}

impl SpoolRecord {
    fn added(&self) -> Added {
        match self {
            Self::Listed { added, .. } | Self::Whole(added) => *added,
        }
    }

    /// The element `prepare` listed among the writable roots.
    fn path(&self, home: &FeldHome) -> PathBuf {
        match self {
            Self::Listed { path, .. } => PathBuf::from(path),
            Self::Whole(_) => feld_core::spool::sandbox_path(home),
        }
    }
}

impl PrepareRecords {
    /// The digests of the skill files `prepare` last wrote into `root`.
    fn files(&self, root: &str) -> std::collections::BTreeMap<String, String> {
        self.skills.get(root).cloned().unwrap_or_default()
    }
}

/// Takes back what `prepare` wrote into every configuration root the prepare record names,
/// and into the one this environment points at: the `feld` skill and the hook groups that
/// run its shims, found by where they sit, and the spool of this Feld home the record says
/// `prepare` listed among the writable roots.
pub fn restore(request: RestoreRequest) -> Result<RestoreResponse> {
    let records: PrepareRecords = runtime::restore_record(request.record, PREPARE_RECORD)?;
    let mut roots: Vec<PathBuf> = records.roots.iter().map(PathBuf::from).collect();
    if let Some(current) = CONFIG_ROOT
        .path()
        .filter(|current| !roots.contains(current))
    {
        roots.push(current);
    }
    let home = feld_home()?;
    let skill_files = skill_dir::files(&plugin_directory()?)?;
    let mut response = RestoreResponse::default();
    for root in roots {
        let written = records.files(&root.display().to_string());
        restore_root(&root, &skill_files, &written, request.apply, &mut response)?;
        // Only the spool `prepare` added goes; one a person listed stays theirs.
        if let Some(spool) = records.spool.get(&root.display().to_string()) {
            let path = spool.path(&home);
            restore_spool(
                &home,
                &root,
                &path,
                spool.added(),
                request.apply,
                &mut response,
            )?;
        }
    }
    Ok(response)
}

/// Takes `spool` out of the writable roots of a configuration root's `config.toml`, with the
/// key or the table adding it brought once nothing else is left in it.
fn restore_spool(
    home: &FeldHome,
    root: &Path,
    spool: &Path,
    added: Added,
    apply: bool,
    response: &mut RestoreResponse,
) -> Result<()> {
    let path = root.join("config.toml");
    // A document Feld cannot read is the person's to fix; the rest of the restore goes on.
    let mut config = match writable_roots::read(&path) {
        Ok(config) => config,
        Err(error) => {
            response.notes.push(format!(
                "{error:#}; take {} out of its writable_roots yourself",
                spool.display()
            ));
            return Ok(());
        }
    };
    if writable_roots::remove(&mut config, spool, added) {
        response.entries.push(RestoreEntry {
            path: path.display().to_string(),
            key: "writable_roots".to_owned(),
            current: Some(spool.display().to_string()),
            next: None,
            action: RestoreAction::Remove,
        });
        if apply {
            writable_roots::write(home, &path, &config)?;
        }
    }
    Ok(())
}

/// Takes back what `prepare` wrote into `root`, whose `skills/feld/` is Feld's while it
/// holds nothing but `skill_files` and the files `written` knows.
fn restore_root(
    root: &Path,
    skill_files: &[(PathBuf, Vec<u8>)],
    written: &std::collections::BTreeMap<String, String>,
    apply: bool,
    response: &mut RestoreResponse,
) -> Result<()> {
    let skill = root.join("skills/feld");
    let hooks_path = root.join("hooks.json");
    let skill_present = fs::symlink_metadata(&skill).is_ok()
        && runtime::restore_directory(&skill, skill_files, written, "skills/feld/", response);
    let current_hooks = read_hooks(&hooks_path)?;
    let (next_hooks, removed) = undeclared_hooks(current_hooks, &skill);
    if removed > 0 {
        response.entries.push(RestoreEntry {
            path: hooks_path.display().to_string(),
            key: "hooks".to_owned(),
            current: Some(format!("{removed} Feld hook entries")),
            next: None,
            action: RestoreAction::Remove,
        });
        response.notes.push(format!(
            "Codex keeps the trust of the Feld hooks under [hooks.state] of {}; Codex manages it",
            root.join("config.toml").display()
        ));
    }
    if apply {
        if skill_present {
            fs::remove_dir_all(&skill).with_context(|| format!("remove {}", skill.display()))?;
        }
        if removed > 0 {
            // A file of Feld's hooks alone goes; one a person linked keeps its link.
            let linked = fs::symlink_metadata(&hooks_path)
                .is_ok_and(|metadata| metadata.file_type().is_symlink());
            if next_hooks.is_empty() && !linked {
                fs::remove_file(&hooks_path)
                    .with_context(|| format!("remove {}", hooks_path.display()))?;
            } else {
                feld_home()?
                    .write_person_file(&hooks_path, &serde_json::to_vec_pretty(&next_hooks)?)?;
            }
        }
    }
    Ok(())
}

/// `hooks.json` without the hooks that run the shims of `skill`, and how many there were.
/// A group, an event and a document left empty by that go too.
fn undeclared_hooks(mut document: Map<String, Value>, skill: &Path) -> (Map<String, Value>, usize) {
    let ours = our_commands(skill);
    let mut removed = 0;
    if let Some(hooks) = document.get_mut("hooks").and_then(Value::as_object_mut) {
        for groups in hooks.values_mut().filter_map(Value::as_array_mut) {
            removed += remove_ours(groups, &ours);
        }
        if removed > 0 {
            hooks.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
        }
    }
    if removed > 0
        && document
            .get("hooks")
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty)
    {
        document.remove("hooks");
    }
    (document, removed)
}

fn action(current: bool) -> PrepareAction {
    if current {
        PrepareAction::Keep
    } else {
        PrepareAction::Write
    }
}

/// Codex's `hooks.json` as it is, empty when there is none.
fn read_hooks(path: &Path) -> Result<Map<String, Value>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
            .context("decode Codex hooks.json")?
            .as_object()
            .cloned()
            .context("Codex hooks.json must be an object"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(error).context("read Codex hooks.json"),
    }
}

/// `hooks.json` with this Adapter's hook groups merged in, leaving every other event and
/// every hook a person put there as it is. An event already holding Feld's hook as this
/// version declares it, in a group of its matcher, stays as it is.
fn declared_hooks(mut document: Map<String, Value>, skill: &Path) -> Result<Map<String, Value>> {
    let ours = our_commands(skill);
    let hooks = document
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("the hooks of Codex hooks.json must be an object")?;
    for (event, group) in hook_declarations(&skill.join("bin")) {
        let declared = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .with_context(|| format!("the {event} of Codex hooks.json must be an array"))?;
        if holds_declared(declared, &group, &ours) {
            continue;
        }
        remove_ours(declared, &ours);
        declared.push(group);
    }
    Ok(document)
}

/// The commands of the hooks that run the shims of `skill`.
fn our_commands(skill: &Path) -> Vec<String> {
    HELPERS
        .iter()
        .map(|name| skill_dir::helper(skill, name).display().to_string())
        .collect()
}

/// Whether a hook runs one of `ours`.
fn is_ours(hook: &Value, ours: &[String]) -> bool {
    ours.iter().any(|our| hook["command"] == **our)
}

/// Takes the hooks that run one of `ours` out of `groups`, and a group left without hooks
/// by that, and answers how many hooks went. A person's hooks in the same group stay.
fn remove_ours(groups: &mut Vec<Value>, ours: &[String]) -> usize {
    let mut removed = 0;
    groups.retain_mut(|group| {
        let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            return true;
        };
        let before = hooks.len();
        hooks.retain(|hook| !is_ours(hook, ours));
        removed += before - hooks.len();
        before == hooks.len() || !hooks.is_empty()
    });
    removed
}

/// Whether an event's groups hold Feld's hook once, as `declared` has it, in a group of
/// `declared`'s matcher.
fn holds_declared(groups: &[Value], declared: &Value, ours: &[String]) -> bool {
    let hooks = || {
        groups
            .iter()
            .filter_map(|group| group["hooks"].as_array())
            .flatten()
    };
    hooks().filter(|hook| is_ours(hook, ours)).count() == 1
        && groups.iter().any(|group| {
            group["matcher"] == declared["matcher"]
                && group["hooks"]
                    .as_array()
                    .zip(declared["hooks"].as_array())
                    .is_some_and(|(hooks, wanted)| wanted.iter().all(|hook| hooks.contains(hook)))
        })
}

/// The thread id grammar, which is a UUID. Codex names the rollout file after the id it
/// hands its child processes, so only an id that can name one is looked for.
pub fn is_thread_id(value: &str) -> bool {
    let groups: Vec<&str> = value.split('-').collect();
    groups.len() == 5
        && [8, 4, 4, 4, 12] == groups.iter().map(|group| group.len()).collect::<Vec<_>>()[..]
        && groups
            .iter()
            .all(|group| group.chars().all(|character| character.is_ascii_hexdigit()))
}

/// The rollout file of one Codex thread under the configuration root:
/// `sessions/<YYYY>/<MM>/<DD>/rollout-<start>-<thread id>.jsonl`. The file is found by the
/// thread id it is named after rather than by the day it was started on. None while no
/// such file exists, which is until that Codex takes its first input.
pub(crate) fn locate_rollout(config_root: &Path, thread: &str) -> Option<PathBuf> {
    let suffix = format!("-{thread}.jsonl");
    let mut candidates = Vec::new();
    collect_rollouts(
        &config_root.join("sessions"),
        DAY_DEPTH,
        &suffix,
        &mut candidates,
    );
    candidates.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    candidates.into_iter().next().map(|(_, path)| path)
}

/// How deep the rollout files sit under `sessions/`: year, month, day.
const DAY_DEPTH: usize = 3;

fn collect_rollouts(
    directory: &Path,
    depth: usize,
    suffix: &str,
    into: &mut Vec<(Option<std::time::SystemTime>, PathBuf)>,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() && depth > 0 {
            collect_rollouts(&path, depth - 1, suffix, into);
        } else if metadata.is_file() && name.starts_with("rollout-") && name.ends_with(suffix) {
            into.push((metadata.modified().ok(), path));
        }
    }
}

#[cfg(test)]
mod tests;
