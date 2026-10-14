//! The herdr commands a Placement is prepared, launched, observed and closed with.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Output;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use feld_core::plugin::protocol::ObservationKind;
use feld_core::plugin::sdk::launch_env::LaunchEnv;
use feld_core::plugin::sdk::provider::{
    self, CleanupResult, CleanupStatus, ProviderBackend, ResourceOrigin, WAIT_COMMAND,
    already_cleaned, observation, ps_failed, retained, valid_env_name,
};
use feld_core::process::ProcessIncarnation;
use feld_core::process::quote;

use crate::placement::*;
use crate::snapshot::*;

/// The prefix of the terminal title the launch script writes the exit code into.
const EXIT_TITLE: &str = "feld exit ";

/// The environment variable the launch script travels to the pane in.
pub(super) const LAUNCH_SCRIPT: &str = "FELD_LAUNCH_SCRIPT";

/// The line the launch types into the pane's shell. A pane takes input through a terminal,
/// whose canonical line holds a few hundred bytes, and a Runtime's argv with the wrapper
/// core puts around it is longer than that. So the script travels in the environment the
/// pane is created with and the line only names it.
pub(super) const LAUNCH_TEXT: &str = "exec sh -c \"$FELD_LAUNCH_SCRIPT\"";

/// What herdr answers when the endpoint has no server behind it.
const SERVER_NOT_RUNNING: &str = "server_not_running";

/// What herdr answers while its server is shutting down, on the way to having no server.
const SERVER_UNAVAILABLE: &str = "server_unavailable";

/// What herdr answers when a workspace belongs to a worktree group that has to be closed
/// with it. Feld creates its workspaces outside any group, so a workspace that answers
/// this is one cleanup leaves alone with the reason.
const WORKSPACE_GROUP: &str = "workspace_group_close_required";

/// The wait a launch gives the pane: the shell has to be in front, read the line the launch
/// typed, and hand the pane over to the launch script. An interactive shell takes as long
/// over that as its own startup does, so the budget is wider than the one a stop gives a
/// program it has signalled. Both waits share it, which keeps the whole launch well inside
/// the deadline core gives the operation.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(10);

/// What a launch reports when the line it typed never reached the launch script. Nothing
/// was started, so the launch closes what it created and the Session closes with it.
const LINE_NOT_TAKEN: &str =
    "the created herdr pane never ran the launch script: its shell did not take the line";

pub(super) const SIGTERM_ATTEMPT: &str = "herdr sent SIGTERM to the pane's program";

pub(super) const SIGKILL_ATTEMPT: &str = "herdr killed the pane's program (SIGKILL)";

/// What a stop records when the wait after it is spent and the program is still there.
pub(super) const STILL_PRESENT: &str = "the pane and its program are still present";

#[derive(Clone, Debug)]
pub struct HerdrProvider {
    herdr: PathBuf,
    pub(super) settle_timeout: Duration,
    launch_timeout: Duration,
}

impl Default for HerdrProvider {
    fn default() -> Self {
        Self::new("herdr")
    }
}

impl HerdrProvider {
    pub fn new(herdr: impl Into<PathBuf>) -> Self {
        Self {
            herdr: herdr.into(),
            settle_timeout: Duration::from_secs(2),
            launch_timeout: LAUNCH_TIMEOUT,
        }
    }

    /// The wait a stop or a kill gives the program before it reports it still present.
    #[cfg(test)]
    pub(super) fn settling(mut self, timeout: Duration) -> Self {
        self.settle_timeout = timeout;
        self
    }

    /// The wait a launch gives the pane before it reports the line was not taken.
    #[cfg(test)]
    pub(super) fn launching(mut self, timeout: Duration) -> Self {
        self.launch_timeout = timeout;
        self
    }

    /// The pane exists; this puts the command in it. The pane's own process is the shell
    /// herdr started, which `exec` hands to `sh` without changing its pid or start time,
    /// and the program is the child that `sh` runs `<command>` as. The children the pane
    /// already has are read first, because the shell that reads the line forks for its own
    /// prompt and `exec` leaves those children under the same pid. `env` is handed to the
    /// receiver the launch script runs as a child before the program.
    pub(super) fn finish_launch(
        &self,
        mut placement: HerdrPlacement,
        env: &LaunchEnv,
    ) -> Result<LaunchResult> {
        let endpoint = placement.endpoint.clone();
        let pane_id = placement.pane_id.clone().expect("a created pane");
        let deadline = Instant::now() + self.launch_timeout;
        let shell = self.settled_shell(&endpoint, &pane_id, deadline)?;
        let before = feld_core::process::children(shell.pid).map_err(ps_failed)?;
        self.send(&endpoint, ["pane", "run", &pane_id, LAUNCH_TEXT])?;
        if let Err(error) = env.hand_over(shell.pid, deadline) {
            return Err(match self.launch_script_argv(shell.pid)? {
                Some(_) => HerdrProviderError::LaunchEnv(error),
                None => HerdrProviderError::Protocol(LINE_NOT_TAKEN.to_owned()),
            });
        }
        placement.program =
            self.settle_program(&endpoint, &pane_id, &shell, &before, env, deadline)?;
        placement.pane_process = Some(shell);
        let observation = self.observe(&placement);
        Ok(LaunchResult {
            placement,
            observation,
        })
    }

    /// Creates the resource the unit names and answers the pane it holds.
    pub(super) fn create(&self, request: &LaunchRequest, env: &LaunchEnv) -> Result<PaneInfo> {
        let args = create_args(request, env)?;
        created_pane(&self.call(&request.prepared.endpoint, args)?)
    }

    /// The pane's own process, once the shell herdr started is the one in front of it. The
    /// launch types into that shell, so anything else in front would take the command.
    fn settled_shell(
        &self,
        endpoint: &HerdrEndpoint,
        pane_id: &str,
        deadline: Instant,
    ) -> Result<ProcessIncarnation> {
        loop {
            let answer = self.call(endpoint, ["pane", "process-info", "--pane", pane_id])?;
            let shell = number(&answer, &["process_info", "shell_pid"])?;
            if number(&answer, &["process_info", "foreground_process_group_id"])? == shell {
                return feld_core::process::incarnation(shell)
                    .map_err(ps_failed)?
                    .ok_or_else(|| {
                        HerdrProviderError::Protocol(
                            "the created herdr pane has no process".to_owned(),
                        )
                    });
            }
            if Instant::now() >= deadline {
                return Err(HerdrProviderError::Protocol(
                    "the created herdr pane is running something other than its shell".to_owned(),
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// The process `<command>` runs as: the one new child of the launch script's shell
    /// that [`is_program`] accepts. A command that finished before it
    /// could be read leaves its exit code in the pane's terminal title, which the
    /// observation reads instead.
    ///
    /// A pane that holds neither by the end of the wait was never handed to the launch
    /// script, so nothing was started and the launch fails: a Placement without a program
    /// is one no observation can judge and no stop or kill can reach.
    fn settle_program(
        &self,
        endpoint: &HerdrEndpoint,
        pane_id: &str,
        shell: &ProcessIncarnation,
        before: &[u32],
        env: &LaunchEnv,
        deadline: Instant,
    ) -> Result<Option<ProcessIncarnation>> {
        loop {
            if self.exited_in_pane(endpoint, pane_id)? {
                return Ok(None);
            }
            if let Some(script) = self.launch_script_argv(shell.pid)? {
                let mut arrived = Vec::new();
                for child in feld_core::process::children(shell.pid).map_err(ps_failed)? {
                    if !before.contains(&child) && is_program(child, &script, env)? {
                        arrived.push(child);
                    }
                }
                if let [program] = arrived[..]
                    && let Some(program) =
                        feld_core::process::incarnation(program).map_err(ps_failed)?
                {
                    return Ok(Some(program));
                }
            }
            if Instant::now() >= deadline {
                // The program may have started and ended inside the last reading, which
                // leaves its exit code in the title a moment later.
                return match self.exited_in_pane(endpoint, pane_id)? {
                    true => Ok(None),
                    false => Err(HerdrProviderError::Protocol(LINE_NOT_TAKEN.to_owned())),
                };
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Whether the launch script has already written `<command>`'s exit code into the
    /// pane's terminal title.
    fn exited_in_pane(&self, endpoint: &HerdrEndpoint, pane_id: &str) -> Result<bool> {
        let pane = pane_of(&self.call(endpoint, ["pane", "get", pane_id])?)?;
        Ok(exit_code_of(pane.terminal_title.as_deref()).is_some())
    }

    /// The argv of the pane's own process once it is the shell of the launch script. Until
    /// the line the launch typed has replaced the shell that read it, the children under
    /// that pid are the ones that shell forked for its own prompt, and the program is not
    /// there yet.
    fn launch_script_argv(&self, pid: u32) -> Result<Option<Vec<String>>> {
        let runs_script = feld_core::process::field(pid, "args=")
            .map_err(ps_failed)?
            .is_some_and(|argv| argv.contains(LAUNCH_SCRIPT));
        if !runs_script {
            return Ok(None);
        }
        Ok(feld_core::process::snapshot(pid)
            .map_err(ps_failed)?
            .map(|shell| shell.argv))
    }

    /// Re-checks the borrowed resource right before anything is created. A borrowed pane
    /// has to answer with the terminal id the placement resolved, since a restored pane
    /// keeps the ID and takes a new terminal.
    pub(super) fn verify_prepared(&self, placement: &HerdrPlacement) -> Result<()> {
        let Some(borrowed) = placement.borrowed.as_ref() else {
            return Ok(());
        };
        let changed = |what: &str| {
            Err(HerdrProviderError::InvalidRequest(format!(
                "the borrowed herdr {what} changed after placement"
            )))
        };
        match placement.unit {
            HerdrUnit::Workspace => Ok(()),
            HerdrUnit::Tab => {
                let answer = self.call(
                    &placement.endpoint,
                    ["workspace", "get", &borrowed.workspace_id],
                )?;
                if text(&answer, &["workspace", "workspace_id"])? == borrowed.workspace_id {
                    Ok(())
                } else {
                    changed("workspace")
                }
            }
            HerdrUnit::Pane => {
                let pane_id = borrowed_pane(placement)?;
                let pane = pane_of(&self.call(&placement.endpoint, ["pane", "get", pane_id])?)?;
                if pane.pane_id == pane_id
                    && pane.workspace_id == borrowed.workspace_id
                    && Some(&pane.tab_id) == borrowed.tab_id.as_ref()
                    && Some(&pane.terminal_id) == borrowed.terminal_id.as_ref()
                {
                    Ok(())
                } else {
                    changed("pane")
                }
            }
        }
    }

    /// Closes what a failed launch created. Borrowed resources are left alone, and the
    /// created pane of a `pane` launch is never a borrowed tab's last one.
    pub(super) fn failure_after_create(
        &self,
        placement: HerdrPlacement,
        error: HerdrProviderError,
    ) -> LaunchFailure {
        let cleanup = self.close_created(&placement);
        let kind = if matches!(
            cleanup.status,
            CleanupStatus::Removed | CleanupStatus::AlreadyAbsent
        ) {
            ObservationKind::Gone
        } else {
            ObservationKind::Lost
        };
        LaunchFailure::new(kind, Some(placement), Some(cleanup), error)
    }

    fn close_created(&self, placement: &HerdrPlacement) -> CleanupResult {
        let (resource, id, _) = unit_resource(placement);
        let Some(id) = id else {
            return already_cleaned("the launch created no herdr resource");
        };
        self.close_resource(&placement.endpoint, resource, id)
    }

    /// What the pid says right after SIGKILL: nothing, or the program on its way out, is
    /// the kill this Provider made; another incarnation under that pid is not.
    pub(super) fn after_kill(
        &self,
        placement: &HerdrPlacement,
        program: &ProcessIncarnation,
        before: ObservationResult,
    ) -> ControlResult {
        match feld_core::process::incarnation(program.pid) {
            Ok(Some(current)) if current != *program => self.unsure_kill(
                "the program PID changed immediately after SIGKILL".to_owned(),
                before,
            ),
            Err(error) => self.unsure_kill(ps_failed(error).to_string(), before),
            _ => ControlResult {
                attempt: SIGKILL_ATTEMPT.to_owned(),
                observed: self.observe_after_signal(placement),
            },
        }
    }

    pub(super) fn unsure_kill(&self, detail: String, before: ObservationResult) -> ControlResult {
        ControlResult {
            attempt: SIGKILL_ATTEMPT.to_owned(),
            observed: observation(ObservationKind::Unknown, None, detail, before.current),
        }
    }

    /// Closes one herdr resource and says what became of it. A resource herdr no longer
    /// holds is already cleaned, and a workspace herdr keeps for the worktree group it
    /// belongs to is left with the reason herdr gave for keeping it.
    pub(super) fn close_resource(
        &self,
        endpoint: &HerdrEndpoint,
        resource: &str,
        id: &str,
    ) -> CleanupResult {
        match self.send(endpoint, [resource, "close", id]) {
            Ok(()) => CleanupResult {
                cleaned: true,
                status: CleanupStatus::Removed,
                detail: "the created herdr resource was closed".to_owned(),
            },
            Err(HerdrProviderError::Herdr { code, message, .. })
                if code.ends_with("_not_found") =>
            {
                already_cleaned(&message)
            }
            Err(HerdrProviderError::Herdr { code, message, .. }) if code == WORKSPACE_GROUP => {
                retained(&message)
            }
            Err(error) => CleanupResult::unknown(error.to_string()),
        }
    }

    /// The socket path of the server an endpoint names, as the server reports it, which is
    /// what herdr puts in `HERDR_SOCKET_PATH` of its panes.
    pub(super) fn socket_path(&self, endpoint: &HerdrEndpoint) -> Result<PathBuf> {
        let (command, output) = self.run(endpoint, ["status", "server", "--json"])?;
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
            .find_map(|status| status.get("socket")?.as_str().map(PathBuf::from))
            .ok_or_else(|| {
                HerdrProviderError::Protocol(format!("herdr {command} answered no socket"))
            })
    }

    /// The labels herdr answers for a pane's tab and workspace right now. A server that
    /// will not answer them leaves the IDs, which is what the summary falls back to.
    pub(super) fn current_names(&self, endpoint: &HerdrEndpoint, pane: &PaneInfo) -> CurrentNames {
        self.reading(&mut Snapshot::default(), endpoint)
            .map(|reading| reading.current_names(pane))
            .unwrap_or_else(|_| CurrentNames {
                workspace_id: pane.workspace_id.clone(),
                tab_id: pane.tab_id.clone(),
                pane_id: pane.pane_id.clone(),
                workspace: pane.workspace_id.clone(),
                tab_number: None,
            })
    }

    /// The exit code the launch script leaves in the pane's terminal title, read again for
    /// a program that has just exited. One observation reads herdr before it reads the
    /// process table, so a program that ends between the two leaves a title the first
    /// reading was too early for.
    pub(super) fn settled_exit_code(&self, endpoint: &HerdrEndpoint, pane_id: &str) -> Option<i32> {
        let deadline = Instant::now() + self.settle_timeout;
        loop {
            let title = self
                .call(endpoint, ["pane", "get", pane_id])
                .ok()
                .and_then(|answer| pane_of(&answer).ok())
                .and_then(|pane| pane.terminal_title);
            if let Some(code) = exit_code_of(title.as_deref()) {
                return Some(code);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// The server is not answering, so the pane cannot be read. The program still can be:
    /// herdr ends a pane's processes when it stops, so most of these are `gone`.
    pub(super) fn without_server(
        &self,
        snapshot: &mut Snapshot,
        placement: &HerdrPlacement,
        reason: &str,
    ) -> Result<ObservationResult> {
        Ok(match self.program_is_running(snapshot, placement)? {
            Some(true) => observation(
                ObservationKind::Lost,
                None,
                format!("{reason}; the recorded program is still running"),
                None,
            ),
            Some(false) => observation(
                ObservationKind::Gone,
                None,
                format!("{reason}; the recorded program is gone"),
                None,
            ),
            None => observation(
                ObservationKind::Unknown,
                None,
                format!("{reason}; this Placement holds no program"),
                None,
            ),
        })
    }

    /// Whether the program this Placement recorded is the one running under its pid, or
    /// None when the Placement holds no program at all.
    pub(super) fn program_is_running(
        &self,
        snapshot: &mut Snapshot,
        placement: &HerdrPlacement,
    ) -> Result<Option<bool>> {
        let Some(program) = program_of(placement) else {
            return Ok(None);
        };
        Ok(Some(
            self.incarnation(snapshot, program.pid)?.as_ref() == Some(program),
        ))
    }

    /// One endpoint, read once per `observe`. `session.snapshot` answers the panes, tabs,
    /// and workspaces together, which is everything an observation and a summary need.
    pub(super) fn reading(
        &self,
        snapshot: &mut Snapshot,
        endpoint: &HerdrEndpoint,
    ) -> std::result::Result<HerdrReading, ReadFailure> {
        let key = serde_json::to_string(endpoint).expect("an endpoint re-encodes");
        if let Some(known) = snapshot.readings.get(&key) {
            return known.clone();
        }
        let read =
            self.call(endpoint, ["api", "snapshot"])
                .map_err(|error| match &error {
                    HerdrProviderError::Herdr { code, .. }
                        if code == SERVER_NOT_RUNNING || code == SERVER_UNAVAILABLE =>
                    {
                        ReadFailure::ServerDown(error.to_string())
                    }
                    _ => ReadFailure::Unreadable(error.to_string()),
                })
                .and_then(|answer| {
                    serde_json::from_value(answer.get("snapshot").cloned().unwrap_or_default())
                        .map_err(|error| {
                            ReadFailure::Unreadable(format!(
                                "herdr answered no session snapshot: {error}"
                            ))
                        })
                });
        snapshot.readings.insert(key, read.clone());
        read
    }

    /// One process, read once per `observe`.
    pub(super) fn incarnation(
        &self,
        snapshot: &mut Snapshot,
        pid: u32,
    ) -> Result<Option<ProcessIncarnation>> {
        if let Some(known) = snapshot.processes.get(&pid) {
            return Ok(known.clone());
        }
        let read = feld_core::process::incarnation(pid).map_err(ps_failed)?;
        snapshot.processes.insert(pid, read.clone());
        Ok(read)
    }

    /// One herdr command that answers nothing when it succeeds, which `pane run` and the
    /// `close` commands do.
    pub(super) fn send<I, S>(&self, endpoint: &HerdrEndpoint, args: I) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.reply(endpoint, args).map(|_| ())
    }

    /// One herdr command, with its `{"id":…,"result":…}` answer read back. An
    /// `{"error":{"code":…}}` answer is this Provider's error, which the operation turns
    /// into a refusal.
    pub(super) fn call<I, S>(&self, endpoint: &HerdrEndpoint, args: I) -> Result<Value>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let (command, output) = self.run(endpoint, args)?;
        answer(&command, &output)
    }

    fn reply<I, S>(&self, endpoint: &HerdrEndpoint, args: I) -> Result<Option<Value>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let (command, output) = self.run(endpoint, args)?;
        reply(&command, &output)
    }

    fn run<I, S>(&self, endpoint: &HerdrEndpoint, args: I) -> Result<(String, Output)>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args: Vec<String> = args
            .into_iter()
            .map(|argument| argument.as_ref().to_string_lossy().into_owned())
            .collect();
        let output = provider::run(&self.herdr, |command| {
            endpoint.append_args(command);
            command.args(&args);
        })?;
        // Every command is `<group> <verb>`; the rest carries the launch script and is too
        // long to name the command by.
        Ok((args[..args.len().min(2)].join(" "), output))
    }
}

pub(super) fn validate_prepare_request(request: &PrepareRequest) -> Result<()> {
    let borrowed = request.borrowed.as_deref().unwrap_or_default();
    match request.unit {
        HerdrUnit::Workspace if borrowed.is_empty() => {}
        HerdrUnit::Tab | HerdrUnit::Pane if !borrowed.is_empty() => {}
        unit => {
            return Err(HerdrProviderError::InvalidRequest(format!(
                "herdr unit {} takes {} borrowed selector",
                unit.as_str(),
                if unit == HerdrUnit::Workspace {
                    "no"
                } else {
                    "a"
                }
            )));
        }
    }
    if !request.workdir.is_absolute() {
        return Err(HerdrProviderError::InvalidRequest(format!(
            "the workdir of a herdr launch must be absolute: {}",
            request.workdir.display()
        )));
    }
    Ok(())
}

pub(super) fn origins(unit: HerdrUnit) -> (ResourceOrigin, ResourceOrigin) {
    match unit {
        HerdrUnit::Workspace => (ResourceOrigin::Created, ResourceOrigin::Created),
        HerdrUnit::Tab => (ResourceOrigin::Existing, ResourceOrigin::Created),
        HerdrUnit::Pane => (ResourceOrigin::Existing, ResourceOrigin::Existing),
    }
}

fn borrowed_workspace(placement: &HerdrPlacement) -> Result<&str> {
    placement
        .borrowed
        .as_ref()
        .map(|borrowed| borrowed.workspace_id.as_str())
        .ok_or_else(|| {
            HerdrProviderError::InvalidRequest(
                "a herdr tab launch needs the workspace it is added to".to_owned(),
            )
        })
}

fn borrowed_pane(placement: &HerdrPlacement) -> Result<&str> {
    placement
        .borrowed
        .as_ref()
        .and_then(|borrowed| borrowed.pane_id.as_deref())
        .ok_or_else(|| {
            HerdrProviderError::InvalidRequest(
                "a herdr pane launch needs the pane it splits".to_owned(),
            )
        })
}

/// The herdr command that creates the resource of one unit, with the workdir, the label,
/// and the launch script, which reads the environment the Runtime Adapter asked for from
/// `env`. The focus stays where it is.
pub(super) fn create_args(request: &LaunchRequest, env: &LaunchEnv) -> Result<Vec<String>> {
    let placement = &request.prepared;
    let mut args: Vec<String> = match placement.unit {
        HerdrUnit::Workspace => vec!["workspace".to_owned(), "create".to_owned()],
        HerdrUnit::Tab => vec![
            "tab".to_owned(),
            "create".to_owned(),
            "--workspace".to_owned(),
            borrowed_workspace(placement)?.to_owned(),
        ],
        HerdrUnit::Pane => vec![
            "pane".to_owned(),
            "split".to_owned(),
            borrowed_pane(placement)?.to_owned(),
            "--direction".to_owned(),
            placement
                .split
                .unwrap_or(HerdrSplit::Right)
                .as_str()
                .to_owned(),
        ],
    };
    args.push("--cwd".to_owned());
    args.push(placement.workdir.to_string_lossy().into_owned());
    // A split pane is shown under the label of the tab it was split in.
    if placement.unit != HerdrUnit::Pane {
        args.push("--label".to_owned());
        args.push(request.resource_name.clone());
    }
    args.push("--env".to_owned());
    args.push(format!(
        "{LAUNCH_SCRIPT}={}",
        launch_script(env, &request.command)
    ));
    args.push("--no-focus".to_owned());
    Ok(args)
}

pub(super) fn validate_launch_request(request: &LaunchRequest) -> Result<()> {
    if request.resource_name.is_empty() {
        return Err(HerdrProviderError::InvalidRequest(
            "resource name must not be empty".to_owned(),
        ));
    }
    if request.command.first().is_none_or(String::is_empty) {
        return Err(HerdrProviderError::InvalidRequest(
            "command must contain a non-empty argv".to_owned(),
        ));
    }
    if !request.prepared.workdir.is_absolute() || !request.prepared.workdir.is_dir() {
        return Err(HerdrProviderError::InvalidRequest(
            "prepared workdir must still be an existing absolute directory".to_owned(),
        ));
    }
    match request.prepared.unit {
        HerdrUnit::Workspace if request.prepared.borrowed.is_none() => Ok(()),
        HerdrUnit::Tab => borrowed_workspace(&request.prepared).map(|_| ()),
        HerdrUnit::Pane => borrowed_pane(&request.prepared).map(|_| ()),
        HerdrUnit::Workspace => Err(HerdrProviderError::InvalidRequest(
            "herdr unit workspace takes no borrowed selector".to_owned(),
        )),
    }
}

/// What a launch that failed before it created anything is sure of. herdr answering with
/// an error code means the server refused the command, so nothing was created; an answer
/// that is not one at all, or none by the deadline, leaves that unknown.
pub(super) fn launch_kind(error: &HerdrProviderError) -> ObservationKind {
    match error {
        HerdrProviderError::Io(failed) if failed.timed_out() => ObservationKind::Lost,
        HerdrProviderError::Herdr { .. }
        | HerdrProviderError::InvalidRequest(_)
        | HerdrProviderError::LaunchEnv(_)
        | HerdrProviderError::Io(_) => ObservationKind::Gone,
        HerdrProviderError::Protocol(_) => ObservationKind::Lost,
    }
}

/// What the pane runs. `exec` hands the pane's process to `sh` without changing its pid,
/// `sh` runs the Runtime's argv as its child, and what is left after it writes the exit
/// code into the pane's terminal title keeps the pane open. The script drops itself out of
/// the environment and takes the environment the Runtime Adapter asked for from `env`
/// before the program starts.
pub(super) fn launch_script(env: &LaunchEnv, command: &[String]) -> String {
    let argv = command
        .iter()
        .map(|argument| quote(argument))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "unset {LAUNCH_SCRIPT}; {}; {argv}; s=$?; printf '\\033]2;feld exit %d\\007' \"$s\"; {WAIT_COMMAND}",
        env.source()
    )
}

/// Whether a new child of the launch script's shell is the program. The shell runs the
/// receiver of `env` as a child, waits for it, and only then forks for `<command>`. So the
/// program is the child that has left the shell's own argv by `exec`, which a fork has
/// until then, is not that receiver, and has not ended.
pub(super) fn is_program(child: u32, script: &[String], env: &LaunchEnv) -> Result<bool> {
    let Some(snapshot) = feld_core::process::snapshot(child).map_err(ps_failed)? else {
        return Ok(false);
    };
    if snapshot.argv == script || snapshot.argv == env.receiver() {
        return Ok(false);
    }
    Ok(feld_core::process::field(child, "stat=")
        .map_err(ps_failed)?
        .is_some_and(|stat| !stat.starts_with('Z')))
}

/// The environment the Runtime Adapter asked for, which the launch script takes from this
/// Provider.
pub(super) fn launch_env(env: &[(String, String)]) -> Result<LaunchEnv> {
    if let Some((name, _)) = env.iter().find(|(name, _)| !valid_env_name(name)) {
        return Err(HerdrProviderError::InvalidRequest(format!(
            "invalid environment variable name '{name}'"
        )));
    }
    LaunchEnv::new(env, &receiver()?).map_err(HerdrProviderError::LaunchEnv)
}

/// The executable the launch script takes its environment from: this Plugin's own.
#[cfg(not(test))]
fn receiver() -> Result<PathBuf> {
    std::env::current_exe().map_err(HerdrProviderError::LaunchEnv)
}

/// A test runs as its own executable, so the launch script takes its environment from the
/// Plugin executable `cargo` builds beside the test's `deps` directory.
#[cfg(test)]
fn receiver() -> Result<PathBuf> {
    let test = std::env::current_exe().map_err(HerdrProviderError::LaunchEnv)?;
    Ok(test
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the target directory")
        .join(env!("CARGO_PKG_NAME")))
}

/// The exit code the launch script wrote into the pane's terminal title.
pub(super) fn exit_code_of(title: Option<&str>) -> Option<i32> {
    title?
        .trim()
        .strip_prefix(EXIT_TITLE)?
        .trim()
        .parse::<i32>()
        .ok()
}

/// The process a stop or a kill reaches: the `<command>` of an `owned` Session, or the
/// Runtime process of an `attached` one, which is what an observation calls the program.
pub(super) fn program_of(placement: &HerdrPlacement) -> Option<&ProcessIncarnation> {
    placement.program.as_ref().or(placement.process.as_ref())
}

/// The resource of a Placement's unit: what herdr calls it, the ID the Placement recorded,
/// and whether the launch created it. Cleanup closes this one and nothing else.
pub(super) fn unit_resource(
    placement: &HerdrPlacement,
) -> (&'static str, Option<&str>, Option<ResourceOrigin>) {
    let (id, origin) = match placement.unit {
        HerdrUnit::Workspace => (
            placement.workspace_id.as_deref(),
            placement.workspace_origin,
        ),
        HerdrUnit::Tab => (placement.tab_id.as_deref(), placement.tab_origin),
        HerdrUnit::Pane => (placement.pane_id.as_deref(), placement.pane_origin),
    };
    (placement.unit.as_str(), id, origin)
}

/// What cleanup does with the resource one launch created, from a reading of herdr and
/// whether the pane's own process is still the recorded one.
pub(super) enum Cleanup {
    Close,
    Answer(CleanupResult),
}

pub(super) fn cleanup_plan(
    placement: &HerdrPlacement,
    reading: &HerdrReading,
    id: &str,
    pane_process_alive: bool,
) -> Cleanup {
    let Some(terminal_id) = placement.terminal_id.as_deref() else {
        return Cleanup::Answer(retained("this Placement holds no terminal id to match"));
    };
    let Some(pane) = reading.pane_of_terminal(terminal_id) else {
        return Cleanup::Answer(match reading.holds(placement.unit, id) {
            true => retained("the recorded pane is gone from the resource the launch created"),
            false => already_cleaned("the created herdr resource is already gone"),
        });
    };
    if !pane_process_alive {
        return Cleanup::Answer(retained("the pane process incarnation does not match"));
    }
    let moved = match placement.unit {
        HerdrUnit::Workspace => pane.workspace_id != id,
        HerdrUnit::Tab => pane.tab_id != id,
        HerdrUnit::Pane => placement.tab_id.as_deref() != Some(pane.tab_id.as_str()),
    };
    if moved {
        return Cleanup::Answer(retained(
            "the recorded pane left the resource the launch created",
        ));
    }
    // A created pane that is the borrowed tab's last one stays, so that the tab does.
    if placement.unit == HerdrUnit::Pane
        && reading
            .panes
            .iter()
            .filter(|candidate| candidate.tab_id == pane.tab_id)
            .count()
            <= 1
    {
        return Cleanup::Answer(retained("the created pane is the borrowed tab's last pane"));
    }
    Cleanup::Close
}
