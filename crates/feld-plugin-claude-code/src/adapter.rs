//! The operations core calls: what starts Claude Code, what recognises one, what its own
//! environment tells about it, and what a Claude Code a person started needs to be attached.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

use feld_core::home::{ADMIN_DIR, FeldHome, TOKENS_DIR};
use feld_core::plugin::protocol::{
    IdentityRequest, IdentityResponse, MatchRequest, MatchResponse, PrepareAction, PrepareEntry,
    PrepareRequest, PrepareResponse, RestoreAction, RestoreEntry, RestoreRequest, RestoreResponse,
    RuntimeIdentity, RuntimeLaunchRequest, RuntimeLaunchResponse,
};
use feld_core::process::{ProcessSnapshot, quote};
use feld_core::spool::Directory;

use crate::{plugin_dir, transcript};
use feld_core::plugin::sdk::env::{feld_home, plugin_directory};
use feld_core::plugin::sdk::runtime::{self, ConfigRoot};

/// Claude Code's configuration root: `CLAUDE_CONFIG_DIR`, otherwise `$HOME/.claude`.
const CONFIG_ROOT: ConfigRoot = ConfigRoot {
    variable: "CLAUDE_CONFIG_DIR",
    home_dir: ".claude",
};

/// The settings keys this Adapter owns. A declaration that sets one of them is refused,
/// because the Session would then have no way to sign its telemetry.
const ADAPTER_OWNED_SETTINGS: [&str; 2] = ["otelHeadersHelper", "statusLine"];

/// Builds the Session's settings file and Claude Code plugin, and answers what starts
/// Claude Code with them.
pub fn launch(request: RuntimeLaunchRequest) -> Result<RuntimeLaunchResponse> {
    let mut settings = declared_settings(request.runtime_settings.clone())?;
    let home = feld_home()?;
    let source = plugin_directory()?;
    let api = runtime::feld_api()?;
    let root = request.session_dir.clone();
    home.create_owner_dir(&root)?;
    let plugin = root.join("plugin");
    plugin_dir::write(&home, &source, &plugin, Some(&request.session))?;

    settings.insert(
        "otelHeadersHelper".to_owned(),
        Value::String(quote(
            &plugin.join("bin/otel-headers").display().to_string(),
        )),
    );
    settings.insert(
        "statusLine".to_owned(),
        json!({
            "type": "command",
            "command": quote(&plugin.join("bin/status-line").display().to_string()),
        }),
    );
    let owned_environment = owned_environment(&home, &api, &request, request.collector.as_deref());
    let environment = settings
        .entry("env")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("runtimeSettings env must be an object")?;
    for (key, value) in &owned_environment {
        environment.insert(key.clone(), Value::String(value.clone()));
    }
    confine(&mut settings, &home, &request.session)?;
    let settings_path = root.join("settings.json");
    home.write_owner_file(&settings_path, &serde_json::to_vec_pretty(&settings)?)?;

    let env = launch_environment(
        owned_environment,
        request.collector.as_deref(),
        CONFIG_ROOT.variable_value(),
    );
    Ok(RuntimeLaunchResponse {
        command: vec![
            "claude".to_owned(),
            "--settings".to_owned(),
            settings_path.display().to_string(),
            "--plugin-dir".to_owned(),
            plugin.display().to_string(),
        ],
        // The declared environment lies beneath the Adapter's own, and the settings `env`
        // reaches Claude Code and its tools over both.
        env: request.env.clone().into_iter().chain(env).collect(),
        identity: None,
    })
}

/// Keeps the Session's tools to what of the Feld home is theirs, beside what the declaration
/// asks of the sandbox and the permissions: the sandbox and the Read tool are denied the
/// Admin's files, the spool and the tokens of the `attached` Sessions, and the sandbox reads
/// and writes the Session's own directory of the spool, through which `feld` reaches Feld
/// when the sandbox denies TCP.
fn confine(settings: &mut Map<String, Value>, home: &FeldHome, session: &str) -> Result<()> {
    let spool = feld_core::spool::sandbox_path(home);
    let own = feld_core::spool::sandbox_dir(home, &Directory::Session(session.to_owned()));
    let root = spool
        .parent()
        .context("the spool has a Feld home above it")?;
    let denied = [root.join(ADMIN_DIR), spool.clone(), root.join(TOKENS_DIR)];
    let path = |path: &Path| Value::String(path.display().to_string());
    extend(
        settings,
        "sandbox",
        "filesystem",
        "denyRead",
        denied.iter().map(|denied| path(denied)),
    )?;
    extend(settings, "sandbox", "filesystem", "allowRead", [path(&own)])?;
    extend(
        settings,
        "sandbox",
        "filesystem",
        "allowWrite",
        [path(&own)],
    )?;
    // A rule of an absolute path starts with `//`.
    extend(
        settings,
        "permissions",
        "",
        "deny",
        denied
            .iter()
            .map(|denied| Value::String(format!("Read(/{}/**)", denied.display()))),
    )?;
    Ok(())
}

/// Appends `values` to the list at `outer.inner.key` of the settings (`outer.key` when
/// `inner` is empty), keeping what the declaration listed there.
fn extend(
    settings: &mut Map<String, Value>,
    outer: &str,
    inner: &str,
    key: &str,
    values: impl IntoIterator<Item = Value>,
) -> Result<()> {
    let name = [outer, inner, key]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(".");
    let mut table = settings
        .entry(outer)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .with_context(|| format!("runtimeSettings {outer} must be an object"))?;
    if !inner.is_empty() {
        table = table
            .entry(inner)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .with_context(|| format!("runtimeSettings {outer}.{inner} must be an object"))?;
    }
    table
        .entry(key)
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .with_context(|| format!("runtimeSettings {name} must be an array"))?
        .extend(values);
    Ok(())
}

/// What the launch spec declared as Claude Code's settings, refused when it claims a key this
/// Adapter owns. Nothing is written before this answers.
fn declared_settings(runtime_settings: Option<Value>) -> Result<Map<String, Value>> {
    runtime::declared_settings(runtime_settings, &ADAPTER_OWNED_SETTINGS)
}

/// Whether a process is a Claude Code: both the executable and the argv's first word are
/// named `claude`, so that a `claude-malware` beside it does not pass. Its telemetry reaches
/// the Session when the environment it started in exports to this daemon's Collector.
pub fn match_process(request: MatchRequest) -> Result<MatchResponse> {
    let matched = is_claude(&request.process);
    let telemetry_prepared = match (matched, request.collector.as_deref()) {
        (true, Some(collector)) => feld_core::process::environment(request.process.pid)
            .map(|environment| exports_to(&environment, collector)),
        _ => None,
    };
    Ok(MatchResponse {
        matched,
        telemetry_prepared,
        telemetry_reason: (telemetry_prepared == Some(false)).then(|| {
            "Claude Code did not start with this daemon's telemetry environment; start it again from a shell that ran `eval \"$(feld env)\"`".to_owned()
        }),
    })
}

/// Whether a started environment sends OTLP to `collector`.
fn exports_to(environment: &BTreeMap<String, String>, collector: &str) -> bool {
    environment
        .get("OTEL_EXPORTER_OTLP_ENDPOINT")
        .is_some_and(|endpoint| endpoint.trim_end_matches('/') == collector.trim_end_matches('/'))
}

/// The Runtime identity a Claude Code exposes to the processes it runs, from the session id
/// in `CLAUDE_CODE_SESSION_ID` and the transcript file found by it, with the model of the
/// last response in that file (null before the first one). A Claude Code attached from
/// outside is identified by its own helpers, so a request that names its process gets null.
pub fn identity(request: IdentityRequest) -> Result<IdentityResponse> {
    if request.process.is_some() {
        return Ok(IdentityResponse::default());
    }
    let identity = (|| {
        let session_id = std::env::var("CLAUDE_CODE_SESSION_ID")
            .ok()
            .filter(|value| !value.is_empty())?;
        let transcript = locate_transcript(&CONFIG_ROOT.path()?, &session_id)?;
        Some(RuntimeIdentity {
            session_id,
            model: transcript::latest_model(&transcript).map_or(Value::Null, Value::from),
            transcript_path: transcript.display().to_string(),
        })
    })();
    Ok(IdentityResponse { identity })
}

/// The Claude Code a person starts themselves: the Feld plugin under the skills directory
/// and the user settings that point at its helpers, answered entry by entry and written
/// when `apply`. What is written is the same whichever daemon prepares it; the daemon a
/// Claude Code reaches is the one of the environment it starts in (`feld env`).
pub fn prepare(request: PrepareRequest) -> Result<PrepareResponse> {
    let home = feld_home()?;
    let source = plugin_directory()?;
    // Claude Code reads its plugins and user settings from its configuration root, so the
    // preparation writes where that Claude Code looks.
    let claude = CONFIG_ROOT.required()?;
    let plugin = claude.join("skills/feld");
    let settings_path = claude.join("settings.json");
    let mut settings = match fs::read(&settings_path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
            .context("decode Claude Code user settings")?
            .as_object()
            .cloned()
            .context("Claude Code user settings must be an object")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Map::new(),
        Err(error) => return Err(error).context("read Claude Code user settings"),
    };
    let record_path = home.root().join(PREPARE_RECORD);
    let mut records: PrepareRecords = runtime::read_prepare_record(&record_path)?;
    let record = records
        .roots
        .entry(claude.display().to_string())
        .or_default();

    // A directory holding files of the person's beside Feld's, or in place of Feld's, is
    // written into with their consent, and the files of other names stay.
    let plugin_files = plugin_dir::root_files(&source)?;
    let plugin_action = if runtime::holds_files(&plugin, plugin_files.clone()) {
        PrepareAction::Keep
    } else if runtime::foreign_files(&plugin, &plugin_files, &record.files).is_empty() {
        PrepareAction::Write
    } else {
        PrepareAction::Replace
    };
    let helper = plugin.join("bin/otel-headers").display().to_string();
    let status_line = plugin.join("bin/status-line").display().to_string();
    let helper_next = helper_entry(settings.get("otelHeadersHelper"), &helper);
    let status_next = status_line_entry(
        settings.get("statusLine"),
        &status_line,
        record.written.get("statusLine"),
        record.replaced.get("statusLine"),
    );
    let mut entries = vec![PrepareEntry {
        path: plugin.display().to_string(),
        key: "skills/feld/".to_owned(),
        current: None,
        next: None,
        action: plugin_action,
    }];
    for (key, (action, next)) in [
        ("otelHeadersHelper", &helper_next),
        ("statusLine", &status_next),
    ] {
        entries.push(PrepareEntry {
            path: settings_path.display().to_string(),
            key: key.to_owned(),
            current: settings.get(key).map(shown),
            next: next.as_ref().map(shown),
            action: *action,
        });
    }
    let helpers_kept = entries
        .iter()
        .all(|entry| entry.action == PrepareAction::Keep);
    let environment = feld_environment(&settings);
    if let FeldEnvironment::Whole(removed) = &environment {
        entries.push(PrepareEntry {
            path: settings_path.display().to_string(),
            key: "env".to_owned(),
            current: settings.get("env").map(shown),
            next: without_environment(&settings, removed).as_ref().map(shown),
            action: PrepareAction::Replace,
        });
    }
    let prepared = entries
        .iter()
        .all(|entry| entry.action == PrepareAction::Keep);
    let helper_kept = helper_next.0 == PrepareAction::Keep;

    let mut notes = Vec::new();
    if let FeldEnvironment::Part = environment {
        notes.push(format!(
            "env in {} has a loopback OTEL_EXPORTER_OTLP_ENDPOINT without the rest of the telemetry environment an earlier Feld wrote, and stays as it is",
            settings_path.display()
        ));
    }
    if request.apply {
        let plugin_written = plugin_action != PrepareAction::Keep;
        if plugin_written {
            record.files = runtime::file_digests(&plugin_files);
        }
        let mut changed = false;
        if let FeldEnvironment::Whole(removed) = environment {
            match without_environment(&settings, &removed) {
                Some(left) => settings.insert("env".to_owned(), left),
                None => settings.remove("env"),
            };
            let replaced = record
                .replaced
                .entry("env")
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(replaced) = replaced.as_object_mut() {
                replaced.extend(removed);
            }
            changed = true;
        }
        for (key, (action, next)) in [
            ("otelHeadersHelper", helper_next),
            ("statusLine", status_next),
        ] {
            let (Some(next), false) = (next, action == PrepareAction::Keep) else {
                continue;
            };
            if let Some(current) = settings.get(key).filter(|_| action.needs_consent()) {
                record.replaced.insert(key.to_owned(), current.clone());
            }
            record.written.insert(key.to_owned(), next.clone());
            settings.insert(key.to_owned(), next);
            changed = true;
        }
        if plugin_written || changed {
            // The record of what is replaced is kept before the settings change, so a
            // value of the person's never sits in neither, and the record of the plugin's
            // files before they are written, so they stay known as Feld's.
            home.create_owner_dir(
                record_path
                    .parent()
                    .expect("the record sits in a directory"),
            )?;
            home.write_owner_file_atomic(&record_path, &serde_json::to_vec_pretty(&records)?)?;
        }
        if plugin_written {
            plugin_dir::write(&home, &source, &plugin, None)?;
        }
        if changed {
            home.write_person_file(&settings_path, &serde_json::to_vec_pretty(&settings)?)?;
        }
        if !prepared {
            notes.push("run /reload-plugins in a Claude Code that is already open".to_owned());
            notes.push(
                "restart a Claude Code started before this init to send its metrics".to_owned(),
            );
        }
    } else if plugin_action == PrepareAction::Replace {
        notes.push(format!(
            "{} holds files Feld did not put there; run `feld init` to write the Feld plugin beside them",
            plugin.display()
        ));
    } else if plugin_action != PrepareAction::Keep {
        notes.push(if fs::symlink_metadata(&plugin).is_ok() {
            format!(
                "the Feld plugin under {} is from another feld version; run `feld init`",
                plugin.display()
            )
        } else {
            format!(
                "the Feld plugin is not under {}; run `feld init`",
                plugin.display()
            )
        });
    } else if !helpers_kept {
        notes.push(format!(
            "{} does not point at the Feld helpers; run `feld init`",
            settings_path.display()
        ));
    } else if !prepared {
        notes.push(format!(
            "env in {} has the telemetry environment an earlier Feld wrote; run `feld init`",
            settings_path.display()
        ));
    }
    Ok(PrepareResponse {
        entries,
        prepared,
        notes,
        daemon_notes: Vec::new(),
        // Claude Code reads `otelHeadersHelper` when it starts, so a Claude Code started
        // before Feld's helper was there signs no export with the Session token.
        telemetry_prepared: Some(helper_kept),
        telemetry_reason: (!helper_kept).then(|| {
            if request.apply {
                "Claude Code started before Feld set its otelHeadersHelper; start it again from a shell that ran `eval \"$(feld env)\"`".to_owned()
            } else {
                "this Claude Code has no Feld settings; run `feld init` and start it again from a shell that ran `eval \"$(feld env)\"`".to_owned()
            }
        }),
    })
}

/// Where the Feld home keeps what `prepare` wrote into a configuration root, relative to
/// the home. It sits outside `plugins/`, which the daemon writes again at every start.
const PREPARE_RECORD: &str = "prepared/claude-code.json";

/// What `prepare` wrote into each configuration root it prepared, and what it replaced
/// there.
#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
struct PrepareRecords {
    roots: std::collections::BTreeMap<String, PrepareRecord>,
}

/// What `prepare` wrote into one configuration root, and what it replaced there.
#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
struct PrepareRecord {
    #[serde(default)]
    written: Map<String, Value>,
    #[serde(default)]
    replaced: Map<String, Value>,
    /// The digests of the files of `skills/feld/` as `prepare` last wrote them.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    files: std::collections::BTreeMap<String, String>,
}

/// Takes back what `prepare` wrote into every configuration root the prepare record names,
/// and into the one this environment points at: the Feld plugin goes, and each setting
/// Feld's is removed, given back the value it replaced, or unchained. A setting a person
/// changed after `prepare` stays. A root the record does not name gives up only the values
/// that name Feld's helpers.
pub fn restore(request: RestoreRequest) -> Result<RestoreResponse> {
    let records: PrepareRecords = runtime::restore_record(request.record, PREPARE_RECORD)?;
    let mut roots: Vec<PathBuf> = records.roots.keys().map(PathBuf::from).collect();
    if let Some(current) = CONFIG_ROOT
        .path()
        .filter(|current| !roots.contains(current))
    {
        roots.push(current);
    }
    let plugin_files = plugin_dir::root_files(&plugin_directory()?)?;
    let mut response = RestoreResponse::default();
    for root in roots {
        restore_root(
            &root,
            records.roots.get(&root.display().to_string()),
            &plugin_files,
            request.apply,
            &mut response,
        )?;
    }
    Ok(response)
}

/// Takes back what `prepare` wrote into `root`, whose `skills/feld/` is Feld's while it
/// holds nothing but `plugin_files` and the files `record` knows.
fn restore_root(
    root: &Path,
    record: Option<&PrepareRecord>,
    plugin_files: &[(PathBuf, Vec<u8>)],
    apply: bool,
    response: &mut RestoreResponse,
) -> Result<()> {
    let plugin = root.join("skills/feld");
    let settings_path = root.join("settings.json");
    let mut settings = match fs::read(&settings_path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
            .context("decode Claude Code user settings")?
            .as_object()
            .cloned()
            .context("Claude Code user settings must be an object")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Map::new(),
        Err(error) => return Err(error).context("read Claude Code user settings"),
    };
    let plugin_present = fs::symlink_metadata(&plugin).is_ok()
        && runtime::restore_directory(
            &plugin,
            plugin_files,
            &record
                .map(|record| record.files.clone())
                .unwrap_or_default(),
            "skills/feld/",
            response,
        );
    let mut changed = false;
    for key in ADAPTER_OWNED_SETTINGS {
        let Some(current) = settings.get(key).cloned() else {
            continue;
        };
        let Some((action, next)) = restored_setting(key, &current, record) else {
            continue;
        };
        response.entries.push(RestoreEntry {
            path: settings_path.display().to_string(),
            key: key.to_owned(),
            current: Some(shown(&current)),
            next: next.as_ref().map(shown),
            action,
        });
        match (action, next) {
            (RestoreAction::Keep, _) => response.notes.push(format!(
                "{key} in {} changed after `feld init` and stays as it is",
                settings_path.display()
            )),
            (_, Some(next)) => {
                settings.insert(key.to_owned(), next);
                changed = true;
            }
            (_, None) => {
                settings.remove(key);
                changed = true;
            }
        }
    }
    if let Some(next) = record
        .and_then(|record| record.replaced.get("env"))
        .and_then(|replaced| restored_environment(settings.get("env"), replaced))
    {
        response.entries.push(RestoreEntry {
            path: settings_path.display().to_string(),
            key: "env".to_owned(),
            current: settings.get("env").map(shown),
            next: Some(shown(&next)),
            action: RestoreAction::Restore,
        });
        settings.insert("env".to_owned(), next);
        changed = true;
    }
    if record.is_none() && (plugin_present || changed) {
        response.notes.push(format!(
            "{} has no prepare record, so only the values that name Feld's helpers are taken out; a value they replaced is not known",
            root.display()
        ));
    }
    if apply {
        if plugin_present {
            fs::remove_dir_all(&plugin).with_context(|| format!("remove {}", plugin.display()))?;
        }
        if changed {
            feld_home()?
                .write_person_file(&settings_path, &serde_json::to_vec_pretty(&settings)?)?;
        }
    }
    Ok(())
}

/// What becomes of one setting `prepare` owns, and the value it takes back (None removes
/// it). None when the value is not Feld's.
fn restored_setting(
    key: &str,
    current: &Value,
    record: Option<&PrepareRecord>,
) -> Option<(RestoreAction, Option<Value>)> {
    let replaced = record.and_then(|record| record.replaced.get(key));
    match record.and_then(|record| record.written.get(key)) {
        Some(written) if written != current => Some((RestoreAction::Keep, None)),
        Some(_) => Some(
            unchained(current)
                .map(|next| (RestoreAction::Unchain, Some(next)))
                .or_else(|| replaced.map(|value| (RestoreAction::Restore, Some(value.clone()))))
                .unwrap_or((RestoreAction::Remove, None)),
        ),
        None => {
            let names_helper = match key {
                "statusLine" => current
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains(STATUS_LINE_HELPER)),
                _ => current
                    .as_str()
                    .is_some_and(|helper| helper.contains(OTEL_HEADERS_HELPER)),
            };
            names_helper.then(|| {
                unchained(current)
                    .map(|next| (RestoreAction::Unchain, Some(next)))
                    .unwrap_or((RestoreAction::Remove, None))
            })
        }
    }
}

/// The user settings `env` given back the entries `prepare` took out of it, beside the ones
/// there. An entry a person set again since stays. None when nothing comes back.
fn restored_environment(current: Option<&Value>, replaced: &Value) -> Option<Value> {
    let mut environment = match current {
        Some(current) => current.as_object()?.clone(),
        None => Map::new(),
    };
    let mut restored = false;
    for (key, value) in replaced.as_object()? {
        if !environment.contains_key(key) {
            environment.insert(key.clone(), value.clone());
            restored = true;
        }
    }
    restored.then_some(Value::Object(environment))
}

/// A status line whose command runs Feld's helper with `--chain`, given back the command
/// it carries and the rest of the value as it is.
fn unchained(current: &Value) -> Option<Value> {
    let command = current.get("command")?.as_str()?;
    let (helper, chained) = command.split_once(" --chain ")?;
    if !helper.contains(STATUS_LINE_HELPER) {
        return None;
    }
    let mut next = current.as_object()?.clone();
    next.insert("command".to_owned(), Value::String(shell_unquote(chained)?));
    Some(Value::Object(next))
}

/// The one word a shell reads from `quoted`, for the forms [`quote`] writes and
/// plain words. None when it is more than one word.
fn shell_unquote(quoted: &str) -> Option<String> {
    let mut word = String::new();
    let mut characters = quoted.trim().chars();
    while let Some(character) = characters.next() {
        match character {
            '\'' => loop {
                match characters.next()? {
                    '\'' => break,
                    inside => word.push(inside),
                }
            },
            '\\' => word.push(characters.next()?),
            space if space.is_whitespace() => return None,
            other => word.push(other),
        }
    }
    Some(word)
}

/// What becomes of `otelHeadersHelper`, and the value it takes when it changes. Claude
/// Code runs it through a shell, so `helper` is written quoted; the bare `helper` an
/// earlier Feld wrote is written again quoted.
fn helper_entry(current: Option<&Value>, helper: &str) -> (PrepareAction, Option<Value>) {
    let quoted = Value::String(quote(helper));
    match current {
        Some(current) if *current == quoted => (PrepareAction::Keep, None),
        Some(current) if current.as_str() != Some(helper) => (PrepareAction::Replace, Some(quoted)),
        _ => (PrepareAction::Write, Some(quoted)),
    }
}

/// What becomes of `statusLine`, and the value it takes when it changes. A command that
/// runs Feld's helper already, directly or from a person's script, and one a person
/// changed after Feld wrote it, stay; any other command is carried by the helper's
/// `--chain`, with the rest of `statusLine` kept. A value that is still the one Feld
/// recorded as replaced was never written over, and is prepared again. A command that
/// starts with the bare `helper`, as an earlier Feld wrote it, is written again with the
/// helper quoted.
fn status_line_entry(
    current: Option<&Value>,
    helper: &str,
    written: Option<&Value>,
    replaced: Option<&Value>,
) -> (PrepareAction, Option<Value>) {
    let quoted = quote(helper);
    let ours = || json!({"type": "command", "command": quoted});
    let Some(current) = current else {
        return (PrepareAction::Write, Some(ours()));
    };
    let command = current.get("command").and_then(Value::as_str);
    if let (Some(rest), Some(object)) = (
        command
            .and_then(|command| command.strip_prefix(helper))
            .filter(|rest| rest.is_empty() || rest.starts_with(" --chain ")),
        current.as_object(),
    ) {
        let mut next = object.clone();
        next.insert(
            "command".to_owned(),
            Value::String(format!("{quoted}{rest}")),
        );
        return (PrepareAction::Write, Some(Value::Object(next)));
    }
    if command.is_some_and(runs_status_line_helper)
        || (written.is_some_and(|written| written != current) && replaced != Some(current))
    {
        return (PrepareAction::Keep, None);
    }
    match (command, current.as_object()) {
        (Some(command), Some(object)) if !command.trim().is_empty() => {
            let mut next = object.clone();
            next.insert(
                "command".to_owned(),
                Value::String(format!("{quoted} --chain {}", quote(command))),
            );
            (PrepareAction::Chain, Some(Value::Object(next)))
        }
        _ => (PrepareAction::Replace, Some(ours())),
    }
}

/// Where the status line helper sits under any configuration root.
const STATUS_LINE_HELPER: &str = "skills/feld/bin/status-line";

/// Where the OTLP header helper sits under any configuration root.
const OTEL_HEADERS_HELPER: &str = "skills/feld/bin/otel-headers";

/// Whether a status line command runs Feld's helper: it names the helper itself, however
/// it spells the configuration root, or a script it runs does (`~/s.sh`, `bash ~/s.sh`).
fn runs_status_line_helper(command: &str) -> bool {
    if command.contains(STATUS_LINE_HELPER) {
        return true;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    command
        .split_whitespace()
        .map(|word| word.trim_matches(|character| character == '\'' || character == '"'))
        .map(|word| {
            ["~/", "$HOME/", "${HOME}/"]
                .into_iter()
                .find_map(|prefix| word.strip_prefix(prefix))
                .map(|rest| format!("{home}/{rest}"))
                .unwrap_or_else(|| word.to_owned())
        })
        .filter(|word| Path::new(word).is_file())
        .any(|script| {
            fs::read(&script)
                .is_ok_and(|script| String::from_utf8_lossy(&script).contains(STATUS_LINE_HELPER))
        })
}

/// How an entry's value reads on one line: a command as it is, anything else as JSON.
fn shown(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other
            .get("command")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| other.to_string()),
    }
}

/// What an earlier Feld wrote into the user settings `env`, the telemetry environment, which
/// now comes from the shell a Claude Code starts in. It is Feld's only as a whole: every key
/// of [`feld_core::telemetry::shell_exporter_environment`] with Feld's value and an endpoint
/// on loopback.
fn feld_environment(settings: &Map<String, Value>) -> FeldEnvironment {
    let Some(environment) = settings.get("env").and_then(Value::as_object) else {
        return FeldEnvironment::Absent;
    };
    let endpoint_is_feld = |value: Option<&str>| {
        value.is_some_and(|endpoint| endpoint.starts_with("http://127.0.0.1:"))
    };
    let mut found = Map::new();
    let mut whole = true;
    for (key, value) in feld_core::telemetry::shell_exporter_environment("") {
        let written = environment.get(&key).and_then(Value::as_str);
        let fields = match key.as_str() {
            "OTEL_EXPORTER_OTLP_ENDPOINT" => endpoint_is_feld(written),
            _ => written == Some(value.as_str()),
        };
        if fields {
            found.insert(key.clone(), environment[&key].clone());
        } else {
            whole = false;
        }
    }
    if whole {
        FeldEnvironment::Whole(found)
    } else if found.contains_key("OTEL_EXPORTER_OTLP_ENDPOINT") {
        FeldEnvironment::Part
    } else {
        FeldEnvironment::Absent
    }
}

/// How much of the telemetry environment an earlier Feld wrote the user settings `env` holds.
enum FeldEnvironment {
    Absent,
    /// The whole of it, the keys and values that go.
    Whole(Map<String, Value>),
    /// A loopback endpoint without the rest, which may be a person's and stays.
    Part,
}

/// The user settings `env` without `removed`, and None when nothing is left of it.
fn without_environment(
    settings: &Map<String, Value>,
    removed: &Map<String, Value>,
) -> Option<Value> {
    let mut environment = settings.get("env")?.as_object()?.clone();
    environment.retain(|key, _| !removed.contains_key(key));
    (!environment.is_empty()).then_some(Value::Object(environment))
}

fn owned_environment(
    home: &FeldHome,
    api: &str,
    request: &RuntimeLaunchRequest,
    collector: Option<&str>,
) -> Vec<(String, String)> {
    let mut environment = runtime::feld_environment(home, api, request);
    match collector {
        Some(collector) => {
            environment.extend(feld_core::telemetry::exporter_environment(collector))
        }
        None => environment.extend(
            [
                ("CLAUDE_CODE_ENABLE_TELEMETRY", "0"),
                ("OTEL_METRICS_EXPORTER", "none"),
                ("OTEL_LOGS_EXPORTER", "none"),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned())),
        ),
    }
    environment
}

/// The environment the Provider starts Claude Code in. Claude Code reads the telemetry
/// variables from the settings file as well, so this carries only what its child processes
/// need from the start, the telemetry the exporter needs before the settings are read, and
/// what the pane must not keep from the Claude Code the Provider's server was started in.
/// A pane takes its environment from the Provider's server rather than from the daemon, so
/// the daemon's configuration root is handed on as well: the Adapter looks for the transcript
/// there.
fn launch_environment(
    owned_environment: Vec<(String, String)>,
    collector: Option<&str>,
    config_root: Option<String>,
) -> Vec<(String, String)> {
    let mut environment = owned_environment
        .into_iter()
        .filter(|(key, _)| !key.starts_with("OTEL_") && key != "CLAUDE_CODE_ENABLE_TELEMETRY")
        .collect::<Vec<_>>();
    if let Some(collector) = collector {
        environment.extend(feld_core::telemetry::exporter_environment(collector));
    }
    environment.extend(parent_markers());
    environment.extend(config_root.map(|root| (CONFIG_ROOT.variable.to_owned(), root)));
    environment
}

/// What the launch environment tells Claude Code about its parent: Feld started it, and
/// its transcript file is the Console's own source. A Provider server started from
/// inside a Claude Code gives its panes that Claude Code's markers, and a Claude Code
/// that reads them as its own parent writes no transcript file. An empty value is absent to
/// Claude Code, and `CLAUDE_CODE_FORCE_SESSION_PERSISTENCE` keeps the transcript file whatever
/// else the pane inherited.
fn parent_markers() -> Vec<(String, String)> {
    feld_core::process::CLAUDE_CODE_PARENT_MARKERS
        .into_iter()
        .map(|key| (key, ""))
        .chain([("CLAUDE_CODE_FORCE_SESSION_PERSISTENCE", "1")])
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// The transcript file of one Claude Code session under the configuration root:
/// `projects/<project>/<session id>.jsonl`. Claude Code names the file after the session id
/// it hands its child processes as `CLAUDE_CODE_SESSION_ID`, so the file is found by name
/// rather than by guessing how the project directory encodes the working directory. None
/// while no such file exists.
fn locate_transcript(config_root: &Path, session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty()
        || session_id
            .chars()
            .any(|character| !(character.is_ascii_alphanumeric() || character == '-'))
    {
        return None;
    }
    let mut candidates = fs::read_dir(config_root.join("projects"))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(format!("{session_id}.jsonl")))
        .filter_map(|path| {
            let metadata = fs::symlink_metadata(&path).ok()?;
            metadata.is_file().then(|| (metadata.modified().ok(), path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    candidates.into_iter().next().map(|(_, path)| path)
}

fn is_claude(process: &ProcessSnapshot) -> bool {
    let named_claude = |value: &str| {
        Path::new(value)
            .file_name()
            .is_some_and(|name| name == "claude")
    };
    named_claude(&process.executable)
        && process
            .argv
            .first()
            .is_some_and(|argv0| named_claude(argv0))
}

#[cfg(test)]
mod tests;
