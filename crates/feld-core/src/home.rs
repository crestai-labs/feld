use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

pub const FELD_API_ENV: &str = "FELD_API";
pub const FELD_ACTOR_ENV: &str = "FELD_ACTOR";
pub const FELD_HOME_ENV: &str = "FELD_HOME";
pub const FELD_PLUGIN_DIR_ENV: &str = "FELD_PLUGIN_DIR";
pub const FELD_SESSION_ID_ENV: &str = "FELD_SESSION_ID";
pub const FELD_SESSION_TOKEN_ENV: &str = "FELD_SESSION_TOKEN";
pub const FELD_WORKSPACE_ENV: &str = "FELD_WORKSPACE";

#[derive(Clone, Debug)]
pub struct FeldHome {
    root: PathBuf,
}

impl FeldHome {
    pub fn resolve(explicit: Option<PathBuf>) -> Result<Self> {
        let root = match explicit {
            Some(path) => path,
            None => {
                let user_home = std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .context("HOME is not set; pass --home or set FELD_HOME")?;
                PathBuf::from(user_home).join(".feld")
            }
        };
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether this is the Feld home a person uses by default, `~/.feld`, rather than one
    /// named for a test or an isolated daemon.
    pub fn is_default(&self) -> bool {
        let Some(user_home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) else {
            return false;
        };
        let resolved = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        resolved(&self.root) == resolved(&PathBuf::from(user_home).join(".feld"))
    }

    pub fn create(&self) -> Result<()> {
        let mut builder = DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        builder.mode(0o700);
        builder
            .create(&self.root)
            .with_context(|| format!("create Feld home {}", self.root.display()))?;
        #[cfg(unix)]
        fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("secure Feld home {}", self.root.display()))?;
        self.create_owner_dir(&self.admin_dir())
    }

    pub fn canonical_root(&self) -> Result<PathBuf> {
        fs::canonicalize(&self.root)
            .with_context(|| format!("resolve Feld home {}", self.root.display()))
    }

    /// The owner-only directory of what only the Admin reads: the local token, the
    /// Collector configuration, the store, and the pairing pages. A Runtime sandbox that can
    /// narrow its reads denies it and reads the rest of the home.
    pub fn admin_dir(&self) -> PathBuf {
        self.root.join(ADMIN_DIR)
    }

    pub fn store_path(&self) -> PathBuf {
        self.admin_dir().join(STORE_FILE)
    }

    /// The store as a reader beside the daemon finds it: at the top of the home while no
    /// daemon of this version has started there to move it into `admin/`.
    pub fn existing_store_path(&self) -> PathBuf {
        let legacy = self.root.join(STORE_FILE);
        if !self.store_path().exists() && legacy.exists() {
            return legacy;
        }
        self.store_path()
    }

    pub fn lock_path(&self) -> PathBuf {
        self.root.join("daemon.lock")
    }

    pub fn lifecycle_lock_path(&self) -> PathBuf {
        self.root.join("daemon.lifecycle.lock")
    }

    pub fn record_path(&self) -> PathBuf {
        self.root.join("daemon.json")
    }

    pub fn token_path(&self) -> PathBuf {
        self.admin_dir().join(TOKEN_FILE)
    }

    /// The configuration the daemon hands its Collector, with the internal token.
    pub fn collector_config_path(&self) -> PathBuf {
        self.admin_dir().join(COLLECTOR_CONFIG_FILE)
    }

    /// Where the daemon serves its API to the parts beside a Runtime, which send their
    /// Session token only here: the owner-only Feld home keeps any other user from
    /// answering in the daemon's place. A home whose path is too long for a socket has it
    /// elsewhere, as its [`DaemonRecord`] says.
    pub fn socket_path(&self) -> PathBuf {
        self.root.join("daemon.sock")
    }

    /// The owner-only place of the socket of a Feld home whose path is too long for one:
    /// a directory of this user's own in the temporary directory, and a name made from the
    /// home's path.
    pub fn short_socket_path(&self) -> PathBuf {
        let mut hash = Sha256::new();
        hash.update(self.root.as_os_str().as_encoded_bytes());
        let name = format!("{:x}", hash.finalize());
        // SAFETY: geteuid has no preconditions and cannot fail.
        let user = unsafe { libc::geteuid() };
        std::env::temp_dir()
            .join(format!("feld-{user}"))
            .join(format!("{}.sock", &name[..16]))
    }

    /// Where a CLI inside a Runtime's sandbox hands the daemon its requests as files.
    pub fn spool_dir(&self) -> PathBuf {
        self.root.join("spool")
    }

    /// The spool of one `owned` Session, the only part of the spool its Runtime's sandbox
    /// writes.
    pub fn session_spool_dir(&self, session: &str) -> PathBuf {
        self.spool_dir().join(session)
    }

    /// The spool the `attached` Sessions share: a Runtime a person started has its sandbox
    /// before Feld knows its Session.
    pub fn shared_spool_dir(&self) -> PathBuf {
        self.spool_dir().join(crate::spool::SHARED_DIR)
    }

    pub fn log_path(&self) -> PathBuf {
        self.root.join("daemon.log")
    }

    /// Where the Plugin directories live. `feld init` fills it, and the daemon reads it
    /// when it starts.
    pub fn plugins_dir(&self) -> PathBuf {
        self.root.join("plugins")
    }

    /// The owner-only directory a Runtime Adapter fills with the files of one Session.
    pub fn session_runtime_dir(&self, session: &str) -> PathBuf {
        self.root.join("sessions").join(session).join("runtime")
    }

    pub fn session_console_dir(&self, session: &str) -> PathBuf {
        self.root.join("sessions").join(session).join("console")
    }

    pub fn session_output_path(&self, session: &str) -> PathBuf {
        self.session_console_dir(session).join("output.jsonl")
    }

    pub fn session_stdout_fifo(&self, session: &str) -> PathBuf {
        self.session_console_dir(session).join("stdout.fifo")
    }

    pub fn session_stderr_fifo(&self, session: &str) -> PathBuf {
        self.session_console_dir(session).join("stderr.fifo")
    }

    pub fn session_output_done(&self, session: &str) -> PathBuf {
        self.session_console_dir(session).join("capture.done")
    }

    /// Where the Runtime's Monitor helper takes its exclusive hold on one Session, so that
    /// however many helpers a Runtime runs, one of them delivers.
    pub fn monitor_lock_path(&self, session: &str) -> PathBuf {
        let mut hash = Sha256::new();
        hash.update(session.as_bytes());
        self.root
            .join("monitor-locks")
            .join(format!("{:x}.lock", hash.finalize()))
    }

    pub fn token_dir(&self) -> PathBuf {
        self.root.join(TOKENS_DIR)
    }

    /// The owner-only pages `feld ui` opens the browser with.
    pub fn ui_pair_dir(&self) -> PathBuf {
        self.admin_dir().join(UI_PAIR_DIR)
    }

    pub fn self_index_dir(&self) -> PathBuf {
        self.root.join("self-index")
    }

    pub fn create_owner_dir(&self, path: &Path) -> Result<()> {
        let mut builder = DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        builder.mode(0o700);
        builder
            .create(path)
            .with_context(|| format!("create {}", path.display()))?;
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("secure {}", path.display()))?;
        Ok(())
    }

    pub fn open_owner_file(&self, path: &Path, append: bool) -> Result<File> {
        let mut options = OpenOptions::new();
        options.create(true).write(true);
        if append {
            options.append(true);
        } else {
            options.truncate(true);
        }
        #[cfg(unix)]
        options.mode(0o600);
        let file = options
            .open(path)
            .with_context(|| format!("open {}", path.display()))?;
        #[cfg(unix)]
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("secure {}", path.display()))?;
        Ok(file)
    }

    pub fn write_owner_file(&self, path: &Path, contents: &[u8]) -> Result<()> {
        let mut file = self.open_owner_file(path, false)?;
        file.write_all(contents)
            .with_context(|| format!("write {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync {}", path.display()))
    }

    pub fn write_owner_file_atomic(&self, path: &Path, contents: &[u8]) -> Result<()> {
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        self.write_owner_file(&temporary, contents)?;
        fs::rename(&temporary, path).with_context(|| format!("replace {}", path.display()))
    }

    /// Creates a person's directory that is missing, owner-only, and leaves one that is
    /// there as it is.
    pub fn create_person_dir(&self, path: &Path) -> Result<()> {
        if path.is_dir() {
            return Ok(());
        }
        self.create_owner_dir(path)
    }

    /// Replaces a person's file whole: the contents go to a file beside the one its symlinks
    /// name, which then takes that file's place with its mode. A new file is the owner's
    /// alone.
    pub fn write_person_file(&self, path: &Path, contents: &[u8]) -> Result<()> {
        let target = symlink_target(path)?;
        let permissions = absent_as_none(fs::metadata(&target))
            .with_context(|| format!("inspect {}", target.display()))?
            .map(|metadata| metadata.permissions());
        let directory = target.parent().context("a file sits in a directory")?;
        self.create_person_dir(directory)?;
        let name = target.file_name().context("a file has a name")?;
        let temporary = directory.join(format!(
            ".{}.feld-{}.tmp",
            name.to_string_lossy(),
            std::process::id()
        ));
        let written = (|| {
            let mut file = self.open_owner_file(&temporary, false)?;
            if let Some(permissions) = permissions {
                file.set_permissions(permissions)
                    .with_context(|| format!("keep the mode of {}", target.display()))?;
            }
            file.write_all(contents)
                .with_context(|| format!("write {}", temporary.display()))?;
            file.sync_all()
                .with_context(|| format!("sync {}", temporary.display()))?;
            fs::rename(&temporary, &target).with_context(|| format!("replace {}", target.display()))
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        written?;
        // The rename reaches the disk with the directory.
        if let Ok(directory) = File::open(directory) {
            let _ = directory.sync_all();
        }
        Ok(())
    }

    /// The local token. A daemon of an earlier version keeps it at the top of the home
    /// until a daemon of this version starts there.
    pub fn read_token(&self) -> Result<String> {
        let path = self.token_path();
        if !path.exists() && self.root.join(TOKEN_FILE).exists() {
            return read_token_file(&self.root.join(TOKEN_FILE), "daemon local token");
        }
        read_token_file(&path, "daemon local token")
    }

    /// Moves what an earlier version kept at the top of the home into [`Self::admin_dir`].
    /// The daemon does it as it starts, holding the home's lock. The store moves with its
    /// `-wal` and `-shm`, and a store already in `admin/` stops the move rather than be
    /// replaced; the other entries the daemon writes anew, so one left beside them goes.
    pub fn migrate_admin(&self) -> Result<()> {
        let admin = self.admin_dir();
        self.create_owner_dir(&admin)?;
        let legacy_store = self.root.join(STORE_FILE);
        if exists(&legacy_store)? {
            if exists(&self.store_path())? {
                bail!(
                    "two stores are in the Feld home, {} and {}; move the one not in use aside",
                    legacy_store.display(),
                    self.store_path().display()
                );
            }
            for suffix in ["-wal", "-shm", ""] {
                let name = format!("{STORE_FILE}{suffix}");
                move_entry(&self.root.join(&name), &admin.join(&name))?;
            }
        }
        for name in [TOKEN_FILE, COLLECTOR_CONFIG_FILE, UI_PAIR_DIR] {
            let legacy = self.root.join(name);
            if !exists(&legacy)? {
                continue;
            }
            let target = admin.join(name);
            if exists(&target)? {
                remove_entry(&legacy)?;
            } else {
                move_entry(&legacy, &target)?;
            }
        }
        Ok(())
    }
}

/// The directory of the Feld home only the Admin reads.
pub const ADMIN_DIR: &str = "admin";
/// The directory of the Feld home that keeps the tokens of the `attached` Sessions.
pub const TOKENS_DIR: &str = "tokens";
const STORE_FILE: &str = "feld.db";
const TOKEN_FILE: &str = "api-token";
const COLLECTOR_CONFIG_FILE: &str = "collector.json";
const UI_PAIR_DIR: &str = "ui-pair";

fn exists(path: &Path) -> Result<bool> {
    Ok(absent_as_none(fs::symlink_metadata(path))
        .with_context(|| format!("inspect {}", path.display()))?
        .is_some())
}

fn move_entry(from: &Path, to: &Path) -> Result<()> {
    absent_as_none(fs::rename(from, to))
        .map(|_| ())
        .with_context(|| format!("move {} to {}", from.display(), to.display()))
}

fn remove_entry(path: &Path) -> Result<()> {
    let removed = if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    absent_as_none(removed)
        .map(|_| ())
        .with_context(|| format!("remove {}", path.display()))
}

/// The variables of the daemon's environment that name the configuration roots of the
/// Runtimes it starts, which the daemon leaves in its record for a preparation made in
/// another environment to compare with.
pub const RECORDED_ENVIRONMENT: [&str; 3] = ["HOME", "CODEX_HOME", "CLAUDE_CONFIG_DIR"];

/// What a running daemon leaves in the Feld home: its instance, its process, its API, and
/// the variables of [`RECORDED_ENVIRONMENT`] it has.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonRecord {
    pub instance_id: String,
    pub pid: u32,
    pub api: String,
    /// The API port named with `--port`, which the later starts listen on or fail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The socket the daemon serves the parts beside a Runtime on, when it is not
    /// [`FeldHome::socket_path`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub environment: std::collections::BTreeMap<String, String>,
}

impl DaemonRecord {
    /// The variables of [`RECORDED_ENVIRONMENT`] this process has.
    pub fn recorded_environment() -> std::collections::BTreeMap<String, String> {
        RECORDED_ENVIRONMENT
            .into_iter()
            .filter_map(|name| {
                let value = std::env::var(name).ok().filter(|value| !value.is_empty())?;
                Some((name.to_owned(), value))
            })
            .collect()
    }
}

/// The daemon API without a probe: the caller's own environment, else the record a running
/// daemon left in the Feld home. The parts that run beside a Runtime reach the daemon this
/// way, outside any async runtime of their own.
pub fn recorded_api(home: &FeldHome) -> Result<String> {
    if let Ok(api) = std::env::var(FELD_API_ENV)
        && !api.is_empty()
    {
        return Ok(api);
    }
    home_api(home)
}

/// The socket the daemon of the Feld home serves the parts beside a Runtime on.
pub fn daemon_socket(home: &FeldHome) -> PathBuf {
    read_daemon_record(home)
        .ok()
        .and_then(|record| record.socket)
        .unwrap_or_else(|| home.socket_path())
}

/// The API of the daemon the Feld home last recorded. A daemon listens on the same address
/// again when it restarts.
pub fn home_api(home: &FeldHome) -> Result<String> {
    Ok(read_daemon_record(home)?.api)
}

/// The record a running daemon left in the Feld home.
pub fn read_daemon_record(home: &FeldHome) -> Result<DaemonRecord> {
    read_json(&home.record_path(), "daemon record")
}

/// Answers None for a file or directory that does not exist.
pub fn absent_as_none<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// The file the symlinks from `path` lead to, which may not exist yet.
fn symlink_target(path: &Path) -> Result<PathBuf> {
    let mut target = path.to_path_buf();
    for _ in 0..40 {
        let metadata = absent_as_none(fs::symlink_metadata(&target))
            .with_context(|| format!("inspect {}", target.display()))?;
        if !metadata.is_some_and(|metadata| metadata.file_type().is_symlink()) {
            return Ok(target);
        }
        let link = fs::read_link(&target)
            .with_context(|| format!("read the link {}", target.display()))?;
        target = target
            .parent()
            .context("a symlink sits in a directory")?
            .join(link);
    }
    bail!("too many symlinks from {}", path.display())
}

/// Reads the JSON file at `path`, which `what` names in the errors.
pub fn read_json<T: DeserializeOwned>(path: &Path, what: &str) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("read {what}"))?;
    serde_json::from_slice(&bytes).with_context(|| format!("decode {what}"))
}

/// [`read_json`] for a file that may not exist, which answers None.
pub fn read_json_if_present<T: DeserializeOwned>(path: &Path, what: &str) -> Result<Option<T>> {
    absent_as_none(fs::read(path))
        .with_context(|| format!("read {what}"))?
        .map(|bytes| serde_json::from_slice(&bytes).with_context(|| format!("decode {what}")))
        .transpose()
}

/// Reads a token file, which holds one non-empty token.
pub fn read_token_file(path: &Path, what: &str) -> Result<String> {
    let token = fs::read_to_string(path).with_context(|| format!("read {what}"))?;
    let token = token.trim().to_owned();
    if token.is_empty() {
        bail!("{what} is empty");
    }
    Ok(token)
}

/// Opens an owner-only lock file, keeping whatever it holds.
pub fn open_lock_file(path: &Path, what: &str) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("open {what} {}", path.display()))?;
    #[cfg(unix)]
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| format!("secure {what} {}", path.display()))?;
    Ok(file)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn the_admin_files_of_an_earlier_home_move_into_admin() {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
        home.create().unwrap();
        let root = home.root();
        for name in ["feld.db", "feld.db-wal", "api-token", "collector.json"] {
            fs::write(root.join(name), name).unwrap();
        }
        fs::create_dir(root.join("ui-pair")).unwrap();
        fs::write(home.collector_config_path(), "new").unwrap();

        home.migrate_admin().unwrap();
        home.migrate_admin().unwrap();

        for name in ["feld.db", "feld.db-wal", "api-token"] {
            assert!(!root.join(name).exists());
            assert_eq!(
                fs::read_to_string(home.admin_dir().join(name)).unwrap(),
                name
            );
        }
        assert!(!root.join("collector.json").exists());
        assert_eq!(
            fs::read_to_string(home.collector_config_path()).unwrap(),
            "new"
        );
        assert!(!root.join("ui-pair").exists());
        assert!(home.ui_pair_dir().is_dir());
        assert_eq!(mode(&home.admin_dir()), 0o700);
    }

    #[test]
    fn a_store_on_both_sides_stops_the_move() {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
        home.create().unwrap();
        fs::write(home.root().join("feld.db"), "earlier").unwrap();
        fs::write(home.store_path(), "current").unwrap();

        assert!(home.migrate_admin().is_err());
        assert_eq!(
            fs::read_to_string(home.root().join("feld.db")).unwrap(),
            "earlier"
        );
        assert_eq!(fs::read_to_string(home.store_path()).unwrap(), "current");
    }

    #[test]
    fn the_local_token_of_an_earlier_daemon_reads() {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
        home.create().unwrap();
        fs::write(home.root().join("api-token"), "earlier\n").unwrap();
        assert_eq!(home.read_token().unwrap(), "earlier");
        fs::write(home.token_path(), "current").unwrap();
        assert_eq!(home.read_token().unwrap(), "current");
    }

    #[test]
    fn a_person_file_keeps_its_symlink_and_mode_and_directory() {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
        let config = temporary.path().join("config");
        let dotfiles = temporary.path().join("dotfiles");
        fs::create_dir(&config).unwrap();
        fs::create_dir(&dotfiles).unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o755)).unwrap();
        let real = dotfiles.join("settings.json");
        fs::write(&real, "{}").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o644)).unwrap();
        let link = config.join("settings.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        home.create_person_dir(&config).unwrap();
        home.write_person_file(&link, b"{\"a\":1}").unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "{\"a\":1}");
        assert_eq!(mode(&real), 0o644);
        assert_eq!(mode(&config), 0o755);
        assert_eq!(fs::read_dir(&dotfiles).unwrap().count(), 1);
    }

    #[test]
    fn a_new_person_file_is_the_owners_alone() {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
        let path = temporary.path().join("codex/hooks.json");

        home.write_person_file(&path, b"{}").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[test]
    fn a_failed_write_leaves_the_person_file_as_it_was() {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().join("feld"))).unwrap();
        let config = temporary.path().join("config");
        fs::create_dir(&config).unwrap();
        let path = config.join("settings.json");
        fs::write(&path, "{\"mine\":true}").unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o500)).unwrap();

        let written = home.write_person_file(&path, b"{}");
        fs::set_permissions(&config, fs::Permissions::from_mode(0o700)).unwrap();

        assert!(written.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"mine\":true}");
        assert_eq!(fs::read_dir(&config).unwrap().count(), 1);
    }
}
