//! The spool: the files a CLI inside a Runtime's sandbox hands the daemon its requests
//! through, in place of TCP (`docs/spec/design/spool.md`). This module holds what both
//! sides read the same way: the names of the files and the shape of what they carry.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The `version` of the request and the response this build writes and reads.
pub const VERSION: u32 = 1;

/// The directory of the spool the `attached` Sessions share. The other directories of the
/// spool are named by the `owned` Session each belongs to.
pub const SHARED_DIR: &str = "shared";

/// Which Sessions may hand requests through one directory of the spool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Directory {
    /// The `attached` Sessions.
    Shared,
    /// The one `owned` Session of this id.
    Session(String),
}

impl Directory {
    /// The directory of the spool a directory entry named `name` is, or `None` for a name no
    /// Session's directory has.
    pub fn of_name(name: &str) -> Option<Self> {
        if name == SHARED_DIR {
            return Some(Self::Shared);
        }
        let valid = !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
        valid.then(|| Self::Session(name.to_owned()))
    }

    /// Where this directory is in the spool of `home`.
    pub fn path(&self, home: &crate::home::FeldHome) -> PathBuf {
        match self {
            Self::Shared => home.shared_spool_dir(),
            Self::Session(session) => home.session_spool_dir(session),
        }
    }
}

/// Which state of a request a spool file holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    /// A request waiting to be taken, which the CLI writes.
    Request,
    /// A request the daemon took and is handling.
    Taken,
    /// The daemon's response, which the CLI reads and removes.
    Response,
    /// A request the CLI withdrew before the daemon took it.
    Withdrawn,
}

impl State {
    fn suffix(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Taken => "taken",
            Self::Response => "response",
            Self::Withdrawn => "withdrawn",
        }
    }
}

/// The name of the file of request `id` in `state`.
pub fn file_name(id: &str, state: State) -> String {
    format!("{id}.{}", state.suffix())
}

/// The name a file of request `id` in `state` is written under before it is renamed into
/// place, so that the other side never reads it half written.
pub fn partial_name(id: &str, state: State) -> String {
    format!("{id}.{}.tmp", state.suffix())
}

/// The id and the state a spool file name holds, or `None` for a name that is neither a
/// request file nor one written in place (a partial file, or a stranger).
pub fn parse_name(name: &str) -> Option<(&str, State)> {
    let (id, suffix) = name.split_once('.')?;
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let state = [
        State::Request,
        State::Taken,
        State::Response,
        State::Withdrawn,
    ]
    .into_iter()
    .find(|state| state.suffix() == suffix)?;
    Some((id, state))
}

/// The spool as a Runtime's sandbox settings name it: an absolute path with no symlink in
/// it, which Codex refuses in a writable root. A home that does not exist yet is named as
/// it would resolve once created.
pub fn sandbox_path(home: &crate::home::FeldHome) -> PathBuf {
    let root = home.root();
    let root = root
        .canonicalize()
        .or_else(|_| std::path::absolute(root))
        .unwrap_or_else(|_| root.to_path_buf());
    root.join("spool")
}

/// One directory of the spool as a Runtime's sandbox settings name it, as
/// [`sandbox_path`] names the spool.
pub fn sandbox_dir(home: &crate::home::FeldHome, directory: &Directory) -> PathBuf {
    let spool = sandbox_path(home);
    match directory {
        Directory::Shared => spool.join(SHARED_DIR),
        Directory::Session(session) => spool.join(session),
    }
}

/// Whether a writable root of a sandbox lets the processes in it write into `spool`: the
/// root is the spool or a directory above it.
pub fn covered_by(spool: &Path, root: &Path) -> bool {
    let resolved = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    spool.starts_with(root) || resolved(spool).starts_with(resolved(root))
}

/// A fresh request id: 128 random bits as 32 hex digits.
pub fn fresh_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut bytes))
        .context("read random bytes for a spool request id")?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// An HTTP request of the daemon API, as one spool file carries it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub version: u32,
    pub method: String,
    /// The path of the API, with its query.
    pub path: String,
    pub content_type: Option<String>,
    pub body: Option<String>,
    /// The Session tokens the CLI found, in the order `auth.md` resolves them.
    pub credentials: Vec<Credential>,
    pub caller: Caller,
}

/// A Session token the CLI found, and where it found it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub source: Source,
    /// The Plugin whose key of the self index answered, for a token of the self index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    pub token: String,
}

/// Where the CLI found a Session token.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The environment variable `FELD_SESSION_TOKEN` of an `owned` Session.
    Environment,
    /// The self index under a Provider's key.
    Provider,
    /// The self index under a Runtime's key.
    Runtime,
}

/// The process that wrote a request, as it states itself.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Caller {
    pub pid: u32,
    /// The link of `/proc/self/ns/pid` on Linux; null elsewhere.
    pub pid_namespace: Option<String>,
}

impl Caller {
    /// The calling process.
    pub fn current() -> Self {
        Self {
            pid: std::process::id(),
            pid_namespace: std::fs::read_link("/proc/self/ns/pid")
                .ok()
                .map(|link| link.display().to_string()),
        }
    }
}

/// The daemon's HTTP response, as one spool file carries it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub version: u32,
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        let id = fresh_id().unwrap();
        assert_eq!(id.len(), 32);
        for state in [
            State::Request,
            State::Taken,
            State::Response,
            State::Withdrawn,
        ] {
            assert_eq!(
                parse_name(&file_name(&id, state)),
                Some((id.as_str(), state))
            );
            assert_eq!(parse_name(&partial_name(&id, state)), None);
        }
        assert_eq!(parse_name("notes.request"), None);
        assert_eq!(parse_name(&format!("{id}.other")), None);
    }

    #[test]
    fn request_reads_as_the_spec_writes_it() {
        let request: Request = serde_json::from_str(
            r#"{"version":1,"method":"POST","path":"/v1/self/messages/read",
                "contentType":"application/json","body":"[]",
                "credentials":[{"source":"provider","plugin":"tmux","token":"t"}],
                "caller":{"pid":48213,"pidNamespace":null}}"#,
        )
        .unwrap();
        assert_eq!(request.credentials[0].source, Source::Provider);
        assert_eq!(request.credentials[0].plugin.as_deref(), Some("tmux"));
        assert_eq!(request.caller.pid, 48213);
    }
}
