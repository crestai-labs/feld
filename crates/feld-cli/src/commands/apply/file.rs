//! The declaration file: the Actors it declares, with `${VAR}` expanded and paths made
//! absolute.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, bail};
use feld_core::id::valid_key;
use feld_core::model::{ActorKind, Grant, LaunchSpec};
use feld_core::plugin::sdk::provider::valid_env_name;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::{Declaration, DeclaredActor, DeclaredSchedule};

/// The one version of the file format.
const VERSION: u32 = 1;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct File {
    version: u32,
    name: String,
    workspace: Option<String>,
    #[serde(default)]
    defaults: Launch,
    #[serde(default)]
    actors: Vec<Entry>,
}

/// The launchSpec fields an Actor and `defaults` declare.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Launch {
    runtime: Option<String>,
    runtime_settings: Option<Value>,
    trust_workdir: Option<bool>,
    command: Option<Vec<String>>,
    workdir: Option<String>,
    #[serde(default, deserialize_with = "env")]
    env: BTreeMap<String, String>,
    env_file: Option<String>,
    provider: Option<String>,
    node: Option<String>,
    provider_options: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    name: String,
    kind: ActorKind,
    runtime: Option<String>,
    runtime_settings: Option<Value>,
    trust_workdir: Option<bool>,
    command: Option<Vec<String>>,
    workdir: Option<String>,
    #[serde(default, deserialize_with = "env")]
    env: BTreeMap<String, String>,
    env_file: Option<String>,
    provider: Option<String>,
    node: Option<String>,
    provider_options: Option<Value>,
    schedule: Option<ScheduleEntry>,
    grants: Option<BTreeSet<Grant>>,
    #[serde(default, deserialize_with = "present")]
    accept_from: Option<Option<BTreeSet<String>>>,
}

/// A field that is written, as `null` or a value, as against one left out.
fn present<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<BTreeSet<String>>>, D::Error> {
    Option::<BTreeSet<String>>::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScheduleEntry {
    cron: String,
    timezone: Option<String>,
    enabled: Option<bool>,
}

/// An `env` value written as a number or a boolean reads as its text, as a shell gives it.
fn env<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error> {
    let values = BTreeMap::<String, Value>::deserialize(deserializer)?;
    values
        .into_iter()
        .map(|(name, value)| {
            let text = match value {
                Value::String(text) => text,
                Value::Number(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                _ => {
                    return Err(serde::de::Error::custom(format!(
                        "env {name} must be a string, a number or a boolean"
                    )));
                }
            };
            Ok((name, text))
        })
        .collect()
}

/// Reads the declaration file at `path`. `${VAR}` takes its value from `lookup`, and a
/// relative path resolves from the file's directory.
pub fn read(path: &Path, lookup: &dyn Fn(&str) -> Option<String>) -> Result<Declaration> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let base = std::path::absolute(path)
        .with_context(|| format!("resolve {}", path.display()))?
        .parent()
        .map(Path::to_owned)
        .unwrap_or_default();
    parse(&text, &base, lookup).with_context(|| format!("read {}", path.display()))
}

pub fn parse(
    text: &str,
    base: &Path,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Declaration> {
    // The YAML is read as plain values first, so that `${VAR}` expands in every string
    // before the shape is checked.
    let mut values: Value = serde_saphyr::from_str(text)?;
    expand_values(&mut values, lookup)?;
    let file: File = serde_json::from_value(values)?;
    if file.version != VERSION {
        bail!(
            "version {} is unknown; this Feld reads version {VERSION}",
            file.version
        );
    }
    if !valid_key(&file.name) {
        bail!("name must match ^[a-z0-9][a-z0-9-]*$; got '{}'", file.name);
    }
    if let Some(workspace) = file.workspace.as_ref().filter(|key| !valid_key(key)) {
        bail!("workspace must match ^[a-z0-9][a-z0-9-]*$; got '{workspace}'");
    }
    let home = lookup("HOME").map(PathBuf::from);
    let path = |value: String| absolute(&value, base, home.as_deref());
    let mut names = BTreeSet::new();
    let mut actors = Vec::new();
    for entry in file.actors {
        if !valid_key(&entry.name) {
            bail!(
                "Actor names must match ^[a-z0-9][a-z0-9-]*$; got '{}'",
                entry.name
            );
        }
        if !names.insert(entry.name.clone()) {
            bail!("Actor '{}' is declared twice", entry.name);
        }
        if entry.schedule.is_some() && entry.kind != ActorKind::Job {
            bail!("Actor '{}': only a job has a schedule", entry.name);
        }
        if let Some(name) = entry.env.keys().find(|name| !valid_env_name(name)) {
            bail!(
                "Actor '{}': env name '{name}' must be letters, digits, and '_', not starting with a digit",
                entry.name
            );
        }
        let defaults = &file.defaults;
        // An Actor's own `runtime` or `command` decides what it runs, so the defaults lend
        // only the fields of the same side.
        let runs_command =
            entry.command.is_some() || (entry.runtime.is_none() && entry.kind == ActorKind::Job);
        let mut env = defaults.env.clone();
        env.extend(entry.env);
        let runtime = entry
            .runtime
            .or_else(|| (!runs_command).then(|| defaults.runtime.clone()).flatten());
        let launch = LaunchSpec {
            runtime_settings: entry.runtime_settings.or_else(|| {
                runtime
                    .is_some()
                    .then(|| defaults.runtime_settings.clone())
                    .flatten()
            }),
            trust_workdir: entry
                .trust_workdir
                .or(runtime
                    .is_some()
                    .then_some(defaults.trust_workdir)
                    .flatten())
                .filter(|trust| *trust),
            runtime,
            command: entry
                .command
                .or_else(|| runs_command.then(|| defaults.command.clone()).flatten()),
            workdir: Some(
                entry
                    .workdir
                    .or_else(|| defaults.workdir.clone())
                    .map_or_else(|| base.to_owned(), path)
                    .display()
                    .to_string(),
            ),
            env: (!env.is_empty()).then_some(env),
            env_file: entry
                .env_file
                .or_else(|| defaults.env_file.clone())
                .map(|value| path(value).display().to_string()),
            provider: entry.provider.or_else(|| defaults.provider.clone()),
            node: entry.node.or_else(|| defaults.node.clone()),
            provider_options: entry
                .provider_options
                .or_else(|| defaults.provider_options.clone()),
        };
        actors.push(DeclaredActor {
            name: entry.name,
            kind: entry.kind,
            launch,
            schedule: entry.schedule.map(|schedule| DeclaredSchedule {
                cron: schedule.cron,
                timezone: schedule.timezone,
                enabled: schedule.enabled,
            }),
            grants: entry.grants,
            accept_from: entry.accept_from,
        });
    }
    Ok(Declaration {
        name: file.name,
        workspace: file.workspace,
        actors,
    })
}

/// `~/` reads from the home directory, and a relative path from `base`.
fn absolute(value: &str, base: &Path, home: Option<&Path>) -> PathBuf {
    match (value.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if value == "~" && home.is_some() => home.expect("checked").to_owned(),
        _ => base.join(value),
    }
}

/// Expands `${VAR}` in every string value, leaving the keys of mappings as written.
fn expand_values(value: &mut Value, lookup: &dyn Fn(&str) -> Option<String>) -> Result<()> {
    match value {
        Value::String(text) => *text = expand(text, lookup)?,
        Value::Array(items) => {
            for item in items {
                expand_values(item, lookup)?;
            }
        }
        Value::Object(mapping) => {
            for (_, value) in mapping.iter_mut() {
                expand_values(value, lookup)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

/// `${VAR}` becomes the value of VAR, which must be set, and `$$` becomes `$`. Any other `$`
/// stays as written.
fn expand(text: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<String> {
    let mut expanded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        expanded.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        if let Some(after) = after.strip_prefix('$') {
            expanded.push('$');
            rest = after;
        } else if let Some(after) = after.strip_prefix('{') {
            let end = after
                .find('}')
                .ok_or_else(|| anyhow!("'${{' without a closing '}}' in \"{text}\""))?;
            let name = &after[..end];
            if !valid_env_name(name) {
                bail!("${{{name}}} does not name an environment variable");
            }
            let value = lookup(name).ok_or_else(|| anyhow!("${{{name}}} is not set"))?;
            expanded.push_str(&value);
            rest = &after[end + 1..];
        } else {
            expanded.push('$');
            rest = after;
        }
    }
    expanded.push_str(rest);
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(name: &str) -> Option<String> {
        match name {
            "EPIC" => Some("96".to_owned()),
            "HOME" => Some("/home/me".to_owned()),
            _ => None,
        }
    }

    fn parse_file(text: &str) -> Result<Declaration> {
        parse(text, Path::new("/repo"), &lookup)
    }

    #[test]
    fn expands_variables_and_keeps_escaped_and_bare_dollars() {
        assert_eq!(
            expand("a ${EPIC} $$ $x", &lookup).expect("expand"),
            "a 96 $ $x"
        );
        assert!(
            expand("${MISSING}", &lookup)
                .unwrap_err()
                .to_string()
                .contains("${MISSING} is not set")
        );
        assert!(expand("${EPIC", &lookup).is_err());
    }

    #[test]
    fn actors_take_the_defaults_and_resolve_paths_from_the_file() {
        let declaration = parse_file(
            r#"
version: 1
name: harness
workspace: dev
defaults:
  workdir: .
  envFile: .env
  env:
    A: "1"
    EPIC: "0"
actors:
  - name: dispatcher
    kind: job
    command: [python3, dispatch.py]
    env:
      EPIC: ${EPIC}
      N: 3
    schedule:
      cron: "*/15 * * * *"
    grants: [read, control]
  - name: reviewer
    kind: agent
    runtime: codex
    workdir: ~/work
    acceptFrom: [dispatcher]
"#,
        )
        .expect("parse");
        assert_eq!(declaration.name, "harness");
        assert_eq!(declaration.workspace.as_deref(), Some("dev"));
        let [dispatcher, reviewer] = &declaration.actors[..] else {
            panic!("two Actors");
        };
        assert_eq!(dispatcher.launch.workdir.as_deref(), Some("/repo/."));
        assert_eq!(dispatcher.launch.env_file.as_deref(), Some("/repo/.env"));
        let env = dispatcher.launch.env.as_ref().expect("env");
        assert_eq!(env["EPIC"], "96");
        assert_eq!(env["A"], "1");
        assert_eq!(env["N"], "3");
        assert_eq!(dispatcher.grants.as_ref().map(BTreeSet::len), Some(2));
        assert_eq!(
            dispatcher
                .schedule
                .as_ref()
                .map(|schedule| schedule.enabled),
            Some(None)
        );
        assert!(dispatcher.accept_from.is_none(), "left out keeps it");
        assert_eq!(reviewer.launch.workdir.as_deref(), Some("/home/me/work"));
        assert_eq!(reviewer.launch.runtime.as_deref(), Some("codex"));
        assert!(reviewer.launch.command.is_none());
        assert!(reviewer.grants.is_none(), "left out keeps them");
        assert_eq!(
            reviewer.accept_from.as_ref().and_then(Option::as_ref),
            Some(&BTreeSet::from(["dispatcher".to_owned()]))
        );
    }

    #[test]
    fn a_written_null_accept_from_and_empty_grants_are_declared() {
        let declaration = parse_file(
            "version: 1\nname: harness\nactors:\n  - {name: a, kind: job, command: [x], grants: [], acceptFrom: null}\n",
        )
        .expect("parse");
        let actor = &declaration.actors[0];
        assert_eq!(actor.grants.as_ref().map(BTreeSet::len), Some(0));
        assert_eq!(actor.accept_from, Some(None));
    }

    #[test]
    fn refuses_unknown_fields_versions_and_duplicates() {
        let base = "version: 1\nname: harness\n";
        assert!(
            parse_file(&format!(
                "{base}actors:\n  - {{name: a, kind: job, comand: [x]}}\n"
            ))
            .is_err()
        );
        assert!(parse_file("version: 2\nname: harness\n").is_err());
        assert!(
            parse_file(&format!(
                "{base}actors:\n  - {{name: a, kind: job, command: [x]}}\n  - {{name: a, kind: job, command: [y]}}\n"
            ))
            .is_err()
        );
        assert!(
            parse_file(&format!(
                "{base}actors:\n  - {{name: a, kind: agent, runtime: codex, schedule: {{cron: '* * * * *'}}}}\n"
            ))
            .is_err()
        );
    }
}
