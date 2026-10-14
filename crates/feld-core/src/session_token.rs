//! How a process inside a Session finds its Session token.
//!
//! An `owned` Session carries the token in its environment. An `attached` Session
//! keeps it in the self index instead: the CLI that attached wrote the token and the
//! Runtime process beside it, under a key core derives from the Execution Provider's name
//! and the selector its `self` answered. Both are the one order of
//! `docs/spec/design/auth.md`, and `feld self token` prints its answer.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::home::{
    FELD_ACTOR_ENV, FELD_SESSION_ID_ENV, FELD_SESSION_TOKEN_ENV, FeldHome, absent_as_none,
    open_lock_file, read_json_if_present, read_token_file,
};
use crate::plugin::protocol::MatchRequest;
use crate::plugin::{Plugins, Registry, RuntimeAdapter, runtime_self_index_key, self_index_key};
use crate::process::ProcessIncarnation;
use crate::spool::{Credential, Source};

/// Where an attached Session keeps its token, and which Runtime process may read it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfIndex {
    pub session_id: String,
    /// The name of the Session's Actor. An entry an earlier version wrote has none.
    #[serde(default)]
    pub actor: Option<String>,
    pub token_file_location: PathBuf,
    pub runtime_process: ProcessIncarnation,
}

/// The exclusive hold on one self index entry, so that two attaches to the same place do
/// not interleave.
pub struct SelfIndexLock {
    _file: File,
    key: String,
}

impl SelfIndexLock {
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// The token of the calling process, with the Session it belongs to and the name of that
/// Session's Actor when they are known.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionToken {
    pub token: String,
    pub session_id: Option<String>,
    pub actor: Option<String>,
}

/// The caller's Session token, with the Plugins the Feld home has installed. The parts of
/// an attached Session find it only after the attach.
pub fn session_token(home: &FeldHome) -> Result<Option<SessionToken>> {
    resolve(home, &Registry::load(home, None))
}

/// Resolves the caller's Session token in the order of `auth.md`: the environment
/// first, then the self index entries of [`found`] that [`admits`] the caller in.
/// Fails with a [`crate::process::TableDenied`] when a sandbox denies reading the process table.
pub fn resolve(home: &FeldHome, registry: &Registry) -> Result<Option<SessionToken>> {
    if let Some(token) = environment_token() {
        return Ok(Some(token));
    }
    let caller = std::process::id();
    // The Runtimes' keys are read only when no Provider's key holds the caller's token.
    let groups: [CandidatesOf; 2] = [provider_candidates, runtime_candidates];
    for group in groups {
        for candidate in group(home, registry)? {
            if admits(&candidate.index, &candidate.credential, caller, registry)? {
                return Ok(Some(SessionToken {
                    token: candidate.credential.token,
                    session_id: Some(candidate.index.session_id),
                    actor: candidate.index.actor,
                }));
            }
        }
    }
    Ok(None)
}

/// The candidates of one kind of self index key.
type CandidatesOf = fn(&FeldHome, &Registry) -> Result<Vec<Candidate>>;

/// A Session token of the self index, before it is checked against the caller.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub credential: Credential,
    pub index: SelfIndex,
}

/// What the caller's environment and the Feld home name as its Session token.
pub enum Found {
    /// The token of an `owned` Session, which needs no check.
    Environment(SessionToken),
    /// The self index entries the keys of the caller's Providers and Runtimes name, in the
    /// order of `auth.md`.
    Index(Vec<Candidate>),
}

/// The caller's Session token candidates, read from the environment and the Feld home
/// only, so that a process inside a sandbox that denies reading the process table finds
/// them too.
pub fn found(home: &FeldHome, registry: &Registry) -> Result<Found> {
    if let Some(token) = environment_token() {
        return Ok(Found::Environment(token));
    }
    let mut candidates = provider_candidates(home, registry)?;
    candidates.extend(runtime_candidates(home, registry)?);
    Ok(Found::Index(candidates))
}

fn environment_token() -> Option<SessionToken> {
    let token = std::env::var(FELD_SESSION_TOKEN_ENV)
        .ok()
        .filter(|token| !token.is_empty())?;
    Some(SessionToken {
        token,
        session_id: std::env::var(FELD_SESSION_ID_ENV)
            .ok()
            .filter(|session| !session.is_empty()),
        actor: std::env::var(FELD_ACTOR_ENV)
            .ok()
            .filter(|actor| !actor.is_empty()),
    })
}

/// The self index entries the keys of the caller's Providers name.
fn provider_candidates(home: &FeldHome, registry: &Registry) -> Result<Vec<Candidate>> {
    let mut candidates = Vec::new();
    for provider in registry.providers() {
        let Some(implementation) = registry.provider(&provider.name) else {
            continue;
        };
        // A Provider that cannot answer `self` leaves the other candidates to resolve.
        let Some(selector) = implementation
            .current()
            .ok()
            .and_then(|answer| answer.selector)
        else {
            continue;
        };
        let key = self_index_key(&provider.name, &selector);
        if let Some(candidate) = candidate(home, &key, Source::Provider, &provider.name)? {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

/// The self index entries the keys of the caller's conversations of the Runtimes name.
fn runtime_candidates(home: &FeldHome, registry: &Registry) -> Result<Vec<Candidate>> {
    let mut candidates = Vec::new();
    for runtime in registry.runtimes() {
        let Some(implementation) = registry.runtime(&runtime.name) else {
            continue;
        };
        // An Adapter that answers no `self` has its tools found through the Provider.
        let Some(selector) = implementation
            .current()
            .ok()
            .and_then(|answer| answer.selector)
        else {
            continue;
        };
        let key = runtime_self_index_key(&runtime.name, &selector);
        if let Some(candidate) = candidate(home, &key, Source::Runtime, &runtime.name)? {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

fn candidate(
    home: &FeldHome,
    key: &str,
    source: Source,
    plugin: &str,
) -> Result<Option<Candidate>> {
    let Some(index) = read_self_index(home, key)? else {
        return Ok(None);
    };
    Ok(Some(Candidate {
        credential: Credential {
            source,
            plugin: Some(plugin.to_owned()),
            token: read_index_token(&index)?,
        },
        index,
    }))
}

/// Whether a token found at `credential` belongs to process `caller`, given the self index
/// entry that holds it: the Runtime process the entry recorded still runs, and for a
/// Provider's key the caller descends from it, for a Runtime's key a process the Runtime
/// Adapter's `match` recognises is in the caller's parent chain. A Runtime that runs its
/// conversations outside the Runtime process the attach took is checked by that process
/// alone, not by its incarnation (Codex's app-server restarts as it updates itself). The
/// environment's token needs no check.
pub fn admits(
    index: &SelfIndex,
    credential: &Credential,
    caller: u32,
    plugins: &dyn Plugins,
) -> Result<bool> {
    match credential.source {
        Source::Environment => Ok(true),
        Source::Provider => Ok(crate::process::matches(&index.runtime_process)?
            && crate::process::is_descendant(caller, index.runtime_process.pid)?),
        Source::Runtime => {
            let Some(adapter) = credential
                .plugin
                .as_deref()
                .and_then(|plugin| plugins.runtime(plugin))
            else {
                return Ok(false);
            };
            Ok(crate::process::matches(&index.runtime_process)?
                && runs_under(adapter.as_ref(), caller)?)
        }
    }
}

/// Whether a process the Runtime Adapter's `match` recognises is in the parent chain of
/// process `caller`.
pub fn runs_under(adapter: &dyn RuntimeAdapter, caller: u32) -> Result<bool> {
    let mut failed = None;
    let found = crate::process::find_ancestor(caller, |ancestor| {
        let Some(process) = crate::process::snapshot(ancestor)? else {
            return Ok(false);
        };
        match adapter.match_process(&MatchRequest {
            process,
            collector: None,
        }) {
            Ok(answer) => Ok(answer.matched),
            Err(error) => {
                failed = Some(error);
                Ok(true)
            }
        }
    })?;
    match failed {
        Some(error) => Err(error),
        None => Ok(found.is_some()),
    }
}

/// The self index entry that holds `token` for Session `session`, which the daemon checks a
/// caller of the spool against.
pub fn index_of(home: &FeldHome, session: &str, token: &str) -> Result<Option<SelfIndex>> {
    let directory = home.self_index_dir();
    let Some(entries) = absent_as_none(fs::read_dir(&directory)).context("read self index")? else {
        return Ok(None);
    };
    for entry in entries {
        let path = entry.context("read self index")?.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Some(key) = path.file_stem().and_then(|key| key.to_str()) else {
            continue;
        };
        // An entry another attach is replacing meanwhile is read on the next request.
        let Ok(Some(index)) = read_self_index(home, key) else {
            continue;
        };
        if index.session_id == session && read_index_token(&index).is_ok_and(|held| held == token) {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

pub fn lock_self_index(home: &FeldHome, key: &str) -> Result<SelfIndexLock> {
    let directory = home.self_index_dir();
    home.create_owner_dir(&directory)?;
    loop {
        let file = open_self_index_lock(home, key)?;
        let path = directory.join(format!("{key}.lock"));
        file.lock_exclusive()
            .with_context(|| format!("lock self index {}", path.display()))?;
        // A sweep removes the lock file of an entry it holds, so the hold counts only
        // while the file is still the one at the path.
        if holds_lock_path(&file, &path)? {
            return Ok(SelfIndexLock {
                _file: file,
                key: key.to_owned(),
            });
        }
    }
}

fn open_self_index_lock(home: &FeldHome, key: &str) -> Result<File> {
    open_lock_file(
        &home.self_index_dir().join(format!("{key}.lock")),
        "self index lock",
    )
}

#[cfg(unix)]
fn holds_lock_path(file: &File, path: &Path) -> Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let held = file.metadata().context("read self index lock")?;
    Ok(absent_as_none(fs::metadata(path))
        .context("read self index lock")?
        .is_some_and(|current| current.dev() == held.dev() && current.ino() == held.ino()))
}

#[cfg(not(unix))]
fn holds_lock_path(_file: &File, _path: &Path) -> Result<bool> {
    Ok(true)
}

/// Saves the token of an attached Session and the self index that points at it.
pub fn write_self_index(
    home: &FeldHome,
    session: &str,
    actor: &str,
    runtime_process: &ProcessIncarnation,
    token: &str,
    lock: &SelfIndexLock,
) -> Result<PathBuf> {
    let directory = home.token_dir();
    home.create_owner_dir(&directory)?;
    let token_path = directory.join(format!("{session}.token"));
    home.write_owner_file_atomic(&token_path, token.as_bytes())?;
    let index_dir = home.self_index_dir();
    home.create_owner_dir(&index_dir)?;
    let index_path = index_dir.join(format!("{}.json", lock.key));
    let replaced = read_self_index(home, &lock.key).ok().flatten();
    let index = SelfIndex {
        session_id: session.to_owned(),
        actor: Some(actor.to_owned()),
        token_file_location: token_path,
        runtime_process: runtime_process.clone(),
    };
    home.write_owner_file_atomic(&index_path, &serde_json::to_vec_pretty(&index)?)?;
    // The token of the Session this entry named before goes with it.
    if let Some(replaced) = replaced
        && replaced.token_file_location != index.token_file_location
    {
        remove_if_present(&replaced.token_file_location);
    }
    Ok(index_path)
}

/// Removes the self index entries, other than the one `lock` holds, whose Runtime process
/// has ended, together with the token files they point at. The Session of such an entry
/// is closed, so its token no longer passes. The other entries of `session` go too, so that
/// the token is found only where its latest attach put it. An entry another attach holds is
/// left to it.
pub fn sweep_self_index(home: &FeldHome, lock: &SelfIndexLock, session: &str) -> Result<()> {
    let directory = home.self_index_dir();
    let mut keys = BTreeSet::new();
    for entry in fs::read_dir(&directory).context("read self index")? {
        let path = entry.context("read self index")?.path();
        if let (Some(key), Some(extension)) = (path.file_stem(), path.extension())
            && (extension == "json" || extension == "lock")
            && let Some(key) = key.to_str()
            && key != lock.key
        {
            keys.insert(key.to_owned());
        }
    }
    for key in keys {
        let file = open_self_index_lock(home, &key)?;
        let lock_path = directory.join(format!("{key}.lock"));
        if file.try_lock_exclusive().is_err() || !holds_lock_path(&file, &lock_path)? {
            continue;
        }
        let index = match read_self_index(home, &key) {
            Ok(index) => index,
            // An entry that cannot be read is left as it is.
            Err(_) => continue,
        };
        if let Some(index) = &index {
            let superseded = index.session_id == session;
            if !superseded && crate::process::matches(&index.runtime_process)? {
                continue;
            }
            // A superseded entry shares the token file of the entry that replaced it.
            if !superseded {
                remove_if_present(&index.token_file_location);
            }
            remove_if_present(&directory.join(format!("{key}.json")));
        }
        remove_if_present(&lock_path);
    }
    Ok(())
}

fn remove_if_present(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn read_self_index(home: &FeldHome, key: &str) -> Result<Option<SelfIndex>> {
    read_json_if_present(
        &home.self_index_dir().join(format!("{key}.json")),
        "self index",
    )
}

pub fn read_index_token(index: &SelfIndex) -> Result<String> {
    read_token_file(&index.token_file_location, "Session token")
}
