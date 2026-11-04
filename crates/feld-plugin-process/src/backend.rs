//! The operations of the Plugin contract, on OS processes.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use feld_core::home::{FELD_HOME_ENV, FeldHome};
use feld_core::plugin::protocol::ObservationKind;
use feld_core::plugin::sdk::provider::{
    self, AttachRequest, AttachedPane, CleanupResult, CleanupStatus, CommandFailed, ControlResult,
    LaunchFailure, LaunchRequest, LaunchResult, Launched, ProviderBackend, RUN_DEADLINE, acts_on,
    already_cleaned, observation, ps_failed, retained,
};
use feld_core::process::ProcessIncarnation;

use crate::supervisor::{self, Started};

pub const SIGTERM_ATTEMPT: &str = "process sent SIGTERM to the program";
pub const SIGKILL_ATTEMPT: &str = "process killed the program (SIGKILL)";
pub const STILL_PRESENT: &str = "the program is still running";
const NOT_A_TERMINAL: &str =
    "the process Provider runs a command without a terminal; it launches Jobs only";

pub type ObservationResult = provider::ObservationResult<()>;
type Result<T> = std::result::Result<T, ProcessError>;

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("{0}")]
    InvalidRequest(String),
    #[error("{0}")]
    Io(String),
    #[error(transparent)]
    Command(#[from] CommandFailed),
}

/// What `place` resolves: the workdir the program starts in.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prepared {
    pub workdir: PathBuf,
}

/// The Placement core stores: where the program runs, the state directory its supervisor
/// records the exit in, and the two processes told apart from later ones by start time.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPlacement {
    pub workdir: PathBuf,
    pub state: PathBuf,
    pub supervisor: ProcessIncarnation,
    /// The program's process, which a program that ended before the launch read it lacks.
    pub program: Option<ProcessIncarnation>,
}

/// One reading of the process table, shared by the Placements one `observe` judges.
#[derive(Debug, Default)]
pub struct Snapshot {
    processes: HashMap<u32, Option<ProcessIncarnation>>,
}

impl Snapshot {
    fn running(&mut self, process: &ProcessIncarnation) -> Result<bool> {
        let found = match self.processes.get(&process.pid) {
            Some(found) => found.clone(),
            None => {
                let found = feld_core::process::incarnation(process.pid).map_err(ps_failed)?;
                self.processes.insert(process.pid, found.clone());
                found
            }
        };
        Ok(found.as_ref() == Some(process))
    }
}

pub struct ProcessProvider {
    /// The executable started as the supervisor, which is this Plugin itself.
    pub(crate) executable: PathBuf,
    /// The directory the state directories of launches go under.
    pub(crate) state_root: PathBuf,
    pub(crate) settle_timeout: Duration,
    /// Reads the program's start time once the supervisor answers its pid.
    pub(crate) program_incarnation: fn(u32) -> std::io::Result<Option<ProcessIncarnation>>,
}

impl Default for ProcessProvider {
    fn default() -> Self {
        let home = std::env::var_os(FELD_HOME_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                FeldHome::resolve(None)
                    .ok()
                    .map(|home| home.root().to_owned())
            })
            .unwrap_or_else(std::env::temp_dir);
        Self {
            executable: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("plugin")),
            state_root: home.join("providers").join("process"),
            settle_timeout: Duration::from_secs(2),
            program_incarnation: feld_core::process::incarnation,
        }
    }
}

impl ProviderBackend for ProcessProvider {
    const NAME: &'static str = "process";
    type Endpoint = ();
    type PrepareRequest = Prepared;
    type Prepared = Prepared;
    type Placement = ProcessPlacement;
    type Partial = Prepared;
    type Current = ();
    type Snapshot = Snapshot;
    type Error = ProcessError;

    /// A process launched here has no place a caller can sit in.
    fn self_selector() -> Option<Value> {
        None
    }

    fn resolve_options(
        options: Option<&Value>,
        _caller: Option<&Value>,
    ) -> std::result::Result<Value, String> {
        match options {
            None | Some(Value::Null) => {}
            Some(Value::Object(given)) => {
                if let Some(key) = given.keys().next() {
                    return Err(format!("process does not take the option '{key}'"));
                }
            }
            Some(_) => return Err("process providerOptions must be a JSON object".to_owned()),
        }
        Ok(Value::Object(Map::new()))
    }

    fn prepare_request(options: Option<&Value>, workdir: &Path) -> Result<Prepared> {
        Self::resolve_options(options, None).map_err(ProcessError::InvalidRequest)?;
        if !workdir.is_absolute() {
            return Err(ProcessError::InvalidRequest(format!(
                "the workdir of a process launch must be absolute: {}",
                workdir.display()
            )));
        }
        Ok(Prepared {
            workdir: workdir.to_owned(),
        })
    }

    fn attach_request(_selector: &Value) -> std::result::Result<AttachRequest<()>, String> {
        Err(NOT_A_TERMINAL.to_owned())
    }

    fn summary(placement: &Value, _current: Option<&()>) -> String {
        match placement
            .get("program")
            .and_then(|program| program.get("pid"))
            .and_then(Value::as_u64)
        {
            Some(pid) => format!("process {pid}"),
            None => "process".to_owned(),
        }
    }

    fn attach_command(_placement: &Value, _current: Option<&()>) -> Option<String> {
        None
    }

    fn workdir(placement: &ProcessPlacement) -> &Path {
        &placement.workdir
    }

    /// A workdir that cannot be entered refuses the start before anything is created.
    fn prepare(&self, request: Prepared) -> Result<Prepared> {
        if !request.workdir.is_dir() {
            return Err(ProcessError::InvalidRequest(format!(
                "the workdir {} is not a directory",
                request.workdir.display()
            )));
        }
        Ok(request)
    }

    fn launch(&self, request: LaunchRequest<Prepared>) -> Launched<ProcessPlacement, Prepared, ()> {
        let gone =
            |error: String| Err(LaunchFailure::new(ObservationKind::Gone, None, None, error));
        let state = self.state_root.join(format!(
            "{}-{}",
            request.resource_name,
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        ));
        if let Err(error) = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&state)
        {
            return gone(format!("create {}: {error}", state.display()));
        }
        let started = self.start_supervisor(&state, &request);
        let (supervisor, program) = match started {
            Ok(started) => started,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&state);
                return gone(error);
            }
        };
        let placement = ProcessPlacement {
            workdir: request.prepared.workdir,
            state,
            supervisor,
            program,
        };
        let observation = self.observe(&placement);
        Ok(LaunchResult {
            placement,
            observation,
        })
    }

    fn attach(&self, _request: AttachRequest<()>) -> Result<AttachedPane<ProcessPlacement, ()>> {
        Err(ProcessError::InvalidRequest(NOT_A_TERMINAL.to_owned()))
    }

    /// The order of `provider/process.md`: the exit record, the program, the supervisor.
    fn observe_checked(
        &self,
        placement: &ProcessPlacement,
        snapshot: &mut Snapshot,
    ) -> Result<ObservationResult> {
        if let Some(observed) = recorded(placement)? {
            return Ok(observed);
        }
        if let Some(program) = &placement.program
            && snapshot.running(program)?
        {
            return Ok(observation(
                ObservationKind::Running,
                None,
                "the recorded program is running".to_owned(),
                None,
            ));
        }
        // The program has ended; its supervisor records the exit a moment later.
        if snapshot.running(&placement.supervisor)? {
            let deadline = Instant::now() + self.settle_timeout;
            while Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
                if let Some(observed) = recorded(placement)? {
                    return Ok(observed);
                }
            }
            if feld_core::process::matches(&placement.supervisor).map_err(ps_failed)? {
                return Ok(observation(
                    ObservationKind::Unknown,
                    None,
                    "the program is gone but its supervisor has not recorded the exit".to_owned(),
                    None,
                ));
            }
        }
        Ok(match recorded(placement)? {
            Some(observed) => observed,
            None => observation(
                ObservationKind::Gone,
                None,
                "the program and its supervisor are gone without an exit record".to_owned(),
                None,
            ),
        })
    }

    fn stop(&self, placement: &ProcessPlacement) -> ControlResult<()> {
        self.control(placement, "TERM", SIGTERM_ATTEMPT)
    }

    fn kill(&self, placement: &ProcessPlacement) -> ControlResult<()> {
        self.control(placement, "KILL", SIGKILL_ATTEMPT)
    }

    /// A program without a terminal takes no typed input, so a `stopInput` column is a
    /// SIGTERM here as well.
    fn graceful_stop(&self, placement: &ProcessPlacement, _inputs: &[String]) -> ControlResult<()> {
        self.stop(placement)
    }

    /// Removes the state directory once nothing runs that writes into it.
    fn cleanup_checked(&self, placement: &ProcessPlacement) -> Result<CleanupResult> {
        let observed = self.observe(placement);
        if acts_on(&observed) || observed.kind == ObservationKind::Unknown {
            return Ok(retained(&format!(
                "the state directory {} is kept: {}",
                placement.state.display(),
                observed.detail
            )));
        }
        match std::fs::remove_dir_all(&placement.state) {
            Ok(()) => Ok(CleanupResult {
                cleaned: true,
                status: CleanupStatus::Removed,
                detail: "the state directory of the program was removed".to_owned(),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(already_cleaned(
                "the state directory of the program is already gone",
            )),
            Err(error) => Err(ProcessError::Io(format!(
                "remove {}: {error}",
                placement.state.display()
            ))),
        }
    }

    fn send_input(&self, _placement: &ProcessPlacement, _input: &str) -> Result<()> {
        Err(ProcessError::InvalidRequest(NOT_A_TERMINAL.to_owned()))
    }

    /// Re-observes until the program is gone or the wait is spent. A program still there
    /// when the wait ends leaves the Session open for `feld session kill`.
    fn observe_after_signal(&self, placement: &ProcessPlacement) -> ObservationResult {
        let deadline = Instant::now() + self.settle_timeout;
        loop {
            let observed = self.observe(placement);
            if !acts_on(&observed) {
                return observed;
            }
            if Instant::now() >= deadline {
                return ObservationResult {
                    detail: STILL_PRESENT.to_owned(),
                    ..observed
                };
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn signalled(placement: &ProcessPlacement) -> Option<u32> {
        placement.program.as_ref().map(|program| program.pid)
    }

    fn signal(&self, pid: u32, signal: &str) -> Result<()> {
        provider::signal(pid, signal)?.map_err(ProcessError::Io)
    }
}

impl ProcessProvider {
    /// Starts the supervisor in a session of its own, hands it the command and the
    /// environment, and reads back the program's pid. A supervisor that does not answer is
    /// killed with whatever it started, so that a failed launch leaves nothing running.
    fn start_supervisor(
        &self,
        state: &Path,
        request: &LaunchRequest<Prepared>,
    ) -> std::result::Result<(ProcessIncarnation, Option<ProcessIncarnation>), String> {
        let mut command = feld_core::process::command(&self.executable);
        command
            .arg(supervisor::OPERATION)
            .arg(state)
            // The supervisor holds no directory a person may remove.
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // SAFETY: setsid is async-signal-safe and touches no memory of the parent.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("start the supervisor: {error}"))?;
        // The supervisor waits on its stdin until the request arrives, so it is still the
        // process this reads.
        let supervisor = match feld_core::process::incarnation(child.id()) {
            Ok(Some(supervisor)) => supervisor,
            Ok(None) => return Err(abandon(&mut child, "the supervisor exited on start")),
            Err(error) => {
                return Err(abandon(
                    &mut child,
                    &format!("read the supervisor's start time: {error}"),
                ));
            }
        };
        let body = serde_json::to_vec(&supervisor::Request {
            command: request.command.clone(),
            env: request.env.clone(),
            workdir: request.prepared.workdir.clone(),
        })
        .expect("encode the launch request");
        let written = child.stdin.take().expect("piped stdin").write_all(&body);
        if let Err(error) = written {
            return Err(abandon(
                &mut child,
                &format!("hand the command to the supervisor: {error}"),
            ));
        }
        let stdout = child.stdout.take().expect("piped stdout");
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let read = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _ = sender.send(read);
        });
        let line = match receiver.recv_timeout(RUN_DEADLINE) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => {
                return Err(abandon(
                    &mut child,
                    &format!("read the supervisor's answer: {error}"),
                ));
            }
            Err(_) => return Err(abandon(&mut child, "the supervisor did not answer")),
        };
        match serde_json::from_str::<Started>(line.trim()) {
            // A program that has already ended has no start time to read. One that cannot be
            // read may be running, and a Placement without it could not be stopped, so the
            // launch ends it with the supervisor instead.
            Ok(Started::Pid(pid)) => match (self.program_incarnation)(pid) {
                Ok(program) => Ok((supervisor, program)),
                Err(error) => Err(abandon(
                    &mut child,
                    &format!("read the program's start time: {error}"),
                )),
            },
            Ok(Started::Error(error)) => {
                let _ = child.wait();
                Err(error)
            }
            Err(_) => Err(abandon(
                &mut child,
                &format!("the supervisor answered {:?}", line.trim()),
            )),
        }
    }
}

/// Kills a supervisor that failed the launch, with the program it may have started in its
/// process group, and says why.
fn abandon(child: &mut Child, reason: &str) -> String {
    // SAFETY: kill has no memory effects. The supervisor leads its own process group.
    unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    let _ = child.kill();
    let _ = child.wait();
    reason.to_owned()
}

/// The observation the exit record settles, if the supervisor has written one.
fn recorded(placement: &ProcessPlacement) -> Result<Option<ObservationResult>> {
    let record = supervisor::read_record(&placement.state).map_err(|error| {
        ProcessError::Io(format!(
            "read the exit record in {}: {error}",
            placement.state.display()
        ))
    })?;
    Ok(record.map(|record| {
        observation(
            ObservationKind::Gone,
            Some(record.exit_code),
            "the program exited".to_owned(),
            None,
        )
    }))
}
