//! What every Runtime Adapter does the same way: where its Runtime keeps its configuration,
//! the daemon a launched Runtime calls, the settings a declaration may carry, the Feld
//! environment a launched Runtime runs in, and the files it places beside a Runtime.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::home::{
    FELD_ACTOR_ENV, FELD_API_ENV, FELD_HOME_ENV, FELD_SESSION_ID_ENV, FELD_SESSION_TOKEN_ENV,
    FELD_WORKSPACE_ENV, FeldHome,
};
use crate::plugin::protocol::{RestoreAction, RestoreEntry, RestoreResponse, RuntimeLaunchRequest};

/// Where a Runtime reads its configuration: the directory `variable` names, otherwise
/// `home_dir` under `$HOME`.
#[derive(Clone, Copy, Debug)]
pub struct ConfigRoot {
    pub variable: &'static str,
    pub home_dir: &'static str,
}

impl ConfigRoot {
    /// The configuration root of this environment.
    pub fn path(&self) -> Option<PathBuf> {
        match std::env::var_os(self.variable) {
            Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
            _ => std::env::var_os("HOME").map(|home| PathBuf::from(home).join(self.home_dir)),
        }
    }

    /// The configuration root of this environment, refused when neither variable is set.
    pub fn required(&self) -> Result<PathBuf> {
        self.path()
            .with_context(|| format!("neither {} nor HOME is set", self.variable))
    }

    /// The variable as this process has it, which the daemon runs with its own environment.
    pub fn variable_value(&self) -> Option<String> {
        std::env::var(self.variable)
            .ok()
            .filter(|value| !value.is_empty())
    }
}

/// The daemon API core put in the environment, which a launched Runtime calls.
pub fn feld_api() -> Result<String> {
    std::env::var(FELD_API_ENV)
        .ok()
        .filter(|api| !api.is_empty())
        .context("FELD_API is not set; the Runtime would have no daemon to call")
}

/// What the launch spec declared as the Runtime's settings, refused when it claims a key of
/// `owned`, which the Adapter sets itself. Nothing is written before this answers.
pub fn declared_settings(
    runtime_settings: Option<Value>,
    owned: &[&str],
) -> Result<Map<String, Value>> {
    let settings = match runtime_settings {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(settings)) => settings,
        Some(_) => bail!("runtimeSettings must be a JSON object"),
    };
    for owned in owned {
        if settings.contains_key(*owned) {
            bail!("runtimeSettings must not set adapter-owned key '{owned}'");
        }
    }
    Ok(settings)
}

/// The Feld environment a launched Runtime and everything below it runs in: the daemon it
/// calls, the Session token it calls with, the Actor and Workspace of the Session, and the
/// Feld home.
pub fn feld_environment(
    home: &FeldHome,
    api: &str,
    request: &RuntimeLaunchRequest,
) -> Vec<(String, String)> {
    [
        (FELD_API_ENV, api.to_owned()),
        (FELD_SESSION_TOKEN_ENV, request.token.clone()),
        (FELD_SESSION_ID_ENV, request.session.clone()),
        (FELD_ACTOR_ENV, request.actor.clone()),
        (FELD_WORKSPACE_ENV, request.workspace.clone()),
        (FELD_HOME_ENV, home.root().display().to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect()
}

/// A shim that runs one part through the Plugin of the Feld home it finds in its
/// environment, `FELD_HOME` or else `~/.feld`, where the home keeps it at `installed`.
/// Without that Plugin the shim runs `absent`.
pub fn installed_shim(installed: &str, name: &str, absent: &str) -> String {
    format!(
        "#!/bin/sh\nplugin=\"${{FELD_HOME:-$HOME/.feld}}/{installed}\"\n[ -x \"$plugin\" ] || {absent}\nexec \"$plugin\" helper {name} \"$@\"\n"
    )
}

/// Writes `files`, relative to `directory`, as the owner's alone. What sits under `bin/`
/// is made runnable.
pub fn write_files(
    home: &FeldHome,
    directory: &Path,
    files: Vec<(PathBuf, Vec<u8>)>,
) -> Result<()> {
    for (relative, contents) in files {
        let target = directory.join(&relative);
        home.create_owner_dir(target.parent().expect("a written file sits in a directory"))?;
        home.write_owner_file(&target, &contents)?;
        #[cfg(unix)]
        if relative.starts_with("bin") {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("make {} runnable", target.display()))?;
        }
    }
    Ok(())
}

/// Whether `directory` already holds `files` as they are.
pub fn holds_files(directory: &Path, files: Vec<(PathBuf, Vec<u8>)>) -> bool {
    files
        .into_iter()
        .all(|(relative, contents)| fs::read(directory.join(relative)).ok() == Some(contents))
}

/// The digests of `files`, by their names relative to the directory they are written
/// into: what a prepare record keeps of the files `prepare` wrote, so that a later version
/// still knows them as Feld's.
pub fn file_digests(files: &[(PathBuf, Vec<u8>)]) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(relative, contents)| (relative.display().to_string(), digest(contents)))
        .collect()
}

fn digest(contents: &[u8]) -> String {
    format!("{:x}", Sha256::digest(contents))
}

/// What `directory` holds that Feld did not put there, relative to it and in order: a file
/// beyond those named by `files`, and one of theirs whose contents are neither as `files`
/// has them nor as `written` (the digests [`file_digests`] gave of what an earlier `prepare`
/// wrote) has them. A directory none of `files` sit in is listed whole, and a link is
/// listed as it is. Empty when `directory` is not there.
pub fn foreign_files(
    directory: &Path,
    files: &[(PathBuf, Vec<u8>)],
    written: &BTreeMap<String, String>,
) -> Vec<PathBuf> {
    let mut foreign = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory.join(&relative)) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = relative.join(entry.file_name());
            let kind = entry.file_type().ok();
            if kind.is_some_and(|kind| kind.is_dir()) {
                if files.iter().any(|(file, _)| file.starts_with(&path)) {
                    pending.push(path);
                } else {
                    foreign.push(path);
                }
                continue;
            }
            let Some((_, contents)) = files.iter().find(|(file, _)| *file == path) else {
                foreign.push(path);
                continue;
            };
            let is_feld_s = kind.is_some_and(|kind| kind.is_file())
                && fs::read(directory.join(&path)).is_ok_and(|current| {
                    current == *contents
                        || written.get(&path.display().to_string()) == Some(&digest(&current))
                });
            if !is_feld_s {
                foreign.push(path);
            }
        }
    }
    foreign.sort();
    foreign
}

/// Answers what `restore` does with a directory `prepare` wrote `files` into, which is
/// there: it goes when it holds nothing but Feld's files ([`foreign_files`]), and otherwise
/// stays with a note naming what a person put there. Returns whether it goes.
pub fn restore_directory(
    directory: &Path,
    files: &[(PathBuf, Vec<u8>)],
    written: &BTreeMap<String, String>,
    key: &str,
    response: &mut RestoreResponse,
) -> bool {
    let foreign = foreign_files(directory, files, written);
    let shown: Vec<String> = foreign
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    response.entries.push(RestoreEntry {
        path: directory.display().to_string(),
        key: key.to_owned(),
        current: (!foreign.is_empty()).then(|| shown.join(", ")),
        next: None,
        action: if foreign.is_empty() {
            RestoreAction::Remove
        } else {
            RestoreAction::Keep
        },
    });
    if !foreign.is_empty() {
        response.notes.push(format!(
            "{} holds files Feld did not put there ({}) and stays as it is",
            directory.display(),
            shown.join(", ")
        ));
    }
    foreign.is_empty()
}

/// What the Feld home keeps of what `prepare` wrote, empty when there is none yet, and an
/// error naming `path` when it does not read, as what it keeps would be written over.
pub fn read_prepare_record<Record: DeserializeOwned + Default>(path: &Path) -> Result<Record> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            anyhow!(
                "the prepare record {} does not read: {error}; fix it or move it aside, then run `feld init` again",
                path.display()
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Record::default()),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

/// The prepare record `restore` is handed, in the Runtime Adapter's shape: empty when there
/// is none, and an error naming where the Feld home keeps it, `relative` to the home, when
/// it does not read.
pub fn restore_record<Record: DeserializeOwned + Default>(
    record: Option<Value>,
    relative: &str,
) -> Result<Record> {
    let Some(record) = record else {
        return Ok(Record::default());
    };
    serde_json::from_value(record).map_err(|error| {
        let path = super::env::feld_home()
            .map(|home| home.root().join(relative))
            .unwrap_or_else(|_| PathBuf::from(relative));
        anyhow!(
            "the prepare record {} does not read: {error}; fix it or move it aside, then run again",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_files_lists_what_feld_did_not_put_there() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("feld");
        let files = [
            (PathBuf::from("SKILL.md"), b"skill".to_vec()),
            (PathBuf::from("bin/hook"), b"hook".to_vec()),
        ];
        let none = BTreeMap::new();
        assert!(foreign_files(&root, &files, &none).is_empty());

        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("SKILL.md"), "skill").unwrap();
        assert!(foreign_files(&root, &files, &none).is_empty());

        fs::write(root.join("bin/mine"), "").unwrap();
        fs::create_dir_all(root.join("notes/deep")).unwrap();
        fs::write(root.join("notes/deep/a.md"), "").unwrap();
        assert_eq!(
            foreign_files(&root, &files, &none),
            [PathBuf::from("bin/mine"), PathBuf::from("notes")]
        );
    }

    #[test]
    fn a_file_of_feld_s_name_is_feld_s_only_as_feld_wrote_it() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let files = [(PathBuf::from("SKILL.md"), b"skill".to_vec())];
        fs::write(root.join("SKILL.md"), "mine").unwrap();
        assert_eq!(
            foreign_files(root, &files, &BTreeMap::new()),
            [PathBuf::from("SKILL.md")]
        );

        // What an earlier version wrote is Feld's while its contents stay as written.
        let earlier = file_digests(&[(PathBuf::from("SKILL.md"), b"mine".to_vec())]);
        assert!(foreign_files(root, &files, &earlier).is_empty());
        fs::write(root.join("SKILL.md"), "edited").unwrap();
        assert_eq!(
            foreign_files(root, &files, &earlier),
            [PathBuf::from("SKILL.md")]
        );
    }
}
