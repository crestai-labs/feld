//! The CLI's side of the spool (`docs/spec/design/spool.md`): inside a Runtime's sandbox that
//! denies TCP, one request of the daemon API is written as a file and its response read back
//! from another.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use feld_core::home::FELD_SESSION_ID_ENV;
use feld_core::home::FeldHome;
use feld_core::spool::{self as wire, Credential, Directory, Source, State};
use feld_daemon::daemon;
use fs2::FileExt;
use tokio::signal::unix::{SignalKind, signal};

use super::CliResult;

/// How often a waiting CLI checks that the daemon still runs.
const LIVENESS_INTERVAL: Duration = Duration::from_secs(1);
const FIRST_POLL: Duration = Duration::from_millis(2);
const LAST_POLL: Duration = Duration::from_millis(100);

/// The request file this CLI waits on, `<id>.request`, which a signal withdraws before the
/// CLI ends.
static WAITING: Mutex<Option<PathBuf>> = Mutex::new(None);
static SIGNALS: Once = Once::new();

/// The spool of a Feld home, the directory of it the caller's Session writes, and the
/// Session tokens a request carries.
#[derive(Clone)]
pub struct Spool {
    home: FeldHome,
    directory: Directory,
    credentials: Vec<Credential>,
}

impl Spool {
    /// The spool of an `owned` Session is its own directory, named by `FELD_SESSION_ID` beside
    /// the token of the environment; the `attached` Sessions share one.
    pub fn new(home: FeldHome, credentials: Vec<Credential>) -> Self {
        let session = std::env::var(FELD_SESSION_ID_ENV)
            .ok()
            .filter(|session| !session.is_empty());
        let directory = match (credentials.first(), session) {
            (Some(credential), Some(session)) if credential.source == Source::Environment => {
                Directory::Session(session)
            }
            _ => Directory::Shared,
        };
        Self {
            home,
            directory,
            credentials,
        }
    }

    /// Hands one request to the daemon and waits for its response.
    pub async fn exchange(
        &self,
        method: &str,
        path: &str,
        content_type: Option<String>,
        body: Option<String>,
    ) -> CliResult<wire::Response> {
        let request = wire::Request {
            version: wire::VERSION,
            method: method.to_owned(),
            path: path.to_owned(),
            content_type,
            body,
            credentials: self.credentials.clone(),
            caller: wire::Caller::current(),
        };
        end_on_signals();
        let pending = Pending::write(self.directory.path(&self.home), &self.directory, &request)?;
        pending.wait(&self.home).await
    }
}

/// A request written to the spool, whose lock tells the daemon this CLI still waits. Dropped
/// before its response, it withdraws the request the daemon has not taken.
struct Pending {
    spool: PathBuf,
    id: String,
    _lock: File,
    answered: bool,
}

impl Pending {
    fn write(spool: PathBuf, directory: &Directory, request: &wire::Request) -> CliResult<Self> {
        let id = wire::fresh_id()?;
        let partial = spool.join(wire::partial_name(&id, State::Request));
        let bytes = serde_json::to_vec(request).context("encode the spool request")?;
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&partial)
            .and_then(|mut file| {
                file.try_lock_exclusive()?;
                file.write_all(&bytes)?;
                fs::rename(&partial, spool.join(wire::file_name(&id, State::Request)))?;
                Ok(file)
            });
        match written {
            Ok(lock) => {
                let pending = Self {
                    spool,
                    id,
                    _lock: lock,
                    answered: false,
                };
                *WAITING.lock().unwrap_or_else(|poison| poison.into_inner()) =
                    Some(pending.path(State::Request));
                Ok(pending)
            }
            Err(error) => {
                let _ = fs::remove_file(&partial);
                Err(unwritable(&spool, directory, error))
            }
        }
    }

    fn path(&self, state: State) -> PathBuf {
        self.spool.join(wire::file_name(&self.id, state))
    }

    /// Withdraws the request unless the daemon took it first: whether it withdrew.
    fn withdraw(&self) -> bool {
        withdraw(&self.path(State::Request))
    }

    async fn wait(mut self, home: &FeldHome) -> CliResult<wire::Response> {
        let response = self.path(State::Response);
        let mut poll = FIRST_POLL;
        let mut checked = Instant::now();
        loop {
            match fs::read(&response) {
                Ok(bytes) => {
                    let _ = fs::remove_file(&response);
                    self.answered = true;
                    return Ok(serde_json::from_slice(&bytes).context("decode the spool response")?);
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(anyhow::Error::from(error)
                        .context(format!("read {}", response.display()))
                        .into());
                }
            }
            if checked.elapsed() >= LIVENESS_INTERVAL {
                checked = Instant::now();
                if !daemon::lock_held(home)? && !response.exists() {
                    self.answered = true;
                    return Err(if self.withdraw() {
                        operational!(
                            "the Feld daemon stopped before taking the request; nothing was applied"
                        )
                    } else {
                        operational!(
                            "the Feld daemon stopped while handling the request; it may have been applied"
                        )
                    });
                }
            }
            tokio::time::sleep(poll).await;
            poll = (poll * 2).min(LAST_POLL);
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        WAITING
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        if !self.answered {
            self.withdraw();
        }
    }
}

/// Renames a request to withdrawn and removes it: whether the daemon had not taken it.
fn withdraw(request: &std::path::Path) -> bool {
    let withdrawn = request.with_extension("withdrawn");
    if fs::rename(request, &withdrawn).is_err() {
        return false;
    }
    let _ = fs::remove_file(withdrawn);
    true
}

/// Ends the CLI on SIGINT or SIGTERM from the first spool request on, as the signals would
/// without a handler: a request not yet taken is withdrawn first, and one the daemon handles
/// is cancelled as its lock goes with the process.
fn end_on_signals() {
    SIGNALS.call_once(|| {
        let (Ok(mut interrupt), Ok(mut terminate)) = (
            signal(SignalKind::interrupt()),
            signal(SignalKind::terminate()),
        ) else {
            return;
        };
        tokio::spawn(async move {
            let signal = tokio::select! {
                _ = interrupt.recv() => libc::SIGINT,
                _ = terminate.recv() => libc::SIGTERM,
            };
            if let Some(request) = WAITING
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .take()
            {
                withdraw(&request);
            }
            std::process::exit(128 + signal);
        });
    });
}

/// Why a request could not be written to the spool.
fn unwritable(
    spool: &std::path::Path,
    directory: &Directory,
    error: std::io::Error,
) -> super::CliError {
    if error.kind() == ErrorKind::NotFound {
        return rejected!(
            "the running Feld daemon has no spool at {}; run `feld daemon restart` outside the sandbox",
            spool.display()
        );
    }
    let named = spool.canonicalize().unwrap_or_else(|_| spool.to_path_buf());
    let hint = crate::codex_sandbox::Sandbox::of_environment(&named)
        .unwritable_hint(&named, *directory == Directory::Shared);
    match hint {
        Some(hint) => rejected!(
            "cannot write a request to the Feld spool {}: {error}; {hint}",
            spool.display()
        ),
        None => rejected!(
            "cannot write a request to the Feld spool {}: {error}; the sandbox has to allow writing there",
            spool.display()
        ),
    }
}
