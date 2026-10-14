//! The tmux commands a Placement is prepared, launched, observed and closed with.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::thread;
use std::time::{Duration, Instant};

use feld_core::plugin::protocol::ObservationKind;
use feld_core::plugin::sdk::launch_env::LaunchEnv;
use feld_core::plugin::sdk::provider::{
    self, CleanupResult, CleanupStatus, ProviderBackend, ResourceOrigin, already_cleaned,
    ps_failed, retained, valid_env_name,
};
use feld_core::process::ProcessIncarnation;
use feld_core::process::quote;

use crate::placement::*;
use crate::snapshot::*;

/// The detail of a `running` pane whose process tmux has not yet reaped.
pub(super) const EXITING: &str = "the pane process is exiting";
/// The detail of a cleanup that found its created resource gone already.
pub(super) const ALREADY_ABSENT: &str = "the created tmux resource is already absent";
/// How long a launch waits before it creates its resource again after a failed
/// `respawn-pane`.
pub(super) const RESPAWN_RETRY_DELAY: Duration = Duration::from_millis(100);
/// How long a launch waits for the launch script to take its environment.
const LAUNCH_ENV_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub struct TmuxProvider {
    pub(super) tmux: PathBuf,
    pub(super) settle_timeout: Duration,
}

/// Why a launch did not finish in the pane it created.
pub(super) enum Unfinished {
    /// `respawn-pane` failed, and the pane takes no other tmux command but a kill.
    Unrespawned(TmuxProviderError),
    Failed(TmuxProviderError),
}

impl From<TmuxProviderError> for Unfinished {
    fn from(error: TmuxProviderError) -> Self {
        Self::Failed(error)
    }
}

impl Default for TmuxProvider {
    fn default() -> Self {
        Self::new("tmux")
    }
}

impl TmuxProvider {
    pub fn new(tmux: impl Into<PathBuf>) -> Self {
        Self {
            tmux: tmux.into(),
            settle_timeout: Duration::from_secs(2),
        }
    }

    pub(super) fn finish_launch(
        &self,
        request: &LaunchRequest,
        partial: PartialTmuxPlacement,
    ) -> std::result::Result<LaunchResult, Unfinished> {
        let endpoint = &request.prepared.endpoint;
        self.run_tmux(
            endpoint,
            [
                "set-option",
                "-p",
                "-t",
                partial.pane_id.as_str(),
                "remain-on-exit",
                "on",
            ],
        )?;

        let gate = format!(
            "feld-launch-{}-{}",
            partial.server_pid,
            partial.pane_id.trim_start_matches('%')
        );
        let env = launch_env(&request.env)?;
        let command = launch_shell_command(&self.tmux, endpoint, &gate, &env, &request.command);
        let workdir = request.prepared.workdir.to_string_lossy();
        let respawn = [
            "respawn-pane",
            "-k",
            "-t",
            partial.pane_id.as_str(),
            "-c",
            workdir.as_ref(),
            "--",
            command.as_str(),
        ];
        // tmux fails a respawn whose pty it cannot get yet (`fork failed`) when many panes
        // start at once.
        self.run_tmux(endpoint, respawn)
            .map_err(Unfinished::Unrespawned)?;

        let pane = self.resolve_target(endpoint, &partial.pane_id)?;
        if pane.session_id != partial.session_id
            || pane.window_id != partial.window_id
            || pane.server_pid != partial.server_pid
            || pane.session_created_at != partial.session_created_at
        {
            return Err(TmuxProviderError::Protocol(
                "created tmux resource changed while launching".to_owned(),
            )
            .into());
        }
        let server = partial.server.clone().expect("verified partial server");
        if self.process_incarnation(pane.server_pid)?.as_ref() != Some(&server) {
            return Err(TmuxProviderError::Protocol(
                "created tmux server incarnation changed while launching".to_owned(),
            )
            .into());
        }
        let pane_process = self.process_incarnation(pane.live_pid()?)?.ok_or_else(|| {
            TmuxProviderError::Protocol("created pane process disappeared before exec".to_owned())
        })?;
        let placement = TmuxPlacement {
            endpoint: partial.endpoint,
            unit: partial.unit,
            borrowed: partial.borrowed,
            workdir: partial.workdir,
            server,
            session_id: partial.session_id,
            session_created_at: partial.session_created_at,
            session_origin: partial.session_origin,
            window_id: partial.window_id,
            window_origin: partial.window_origin,
            pane_id: partial.pane_id,
            pane_origin: partial.pane_origin,
            program: Some(pane_process.clone()),
            pane_process,
            process: None,
        };

        env.hand_over(pane.live_pid()?, Instant::now() + LAUNCH_ENV_TIMEOUT)
            .map_err(TmuxProviderError::LaunchEnv)?;
        self.run_tmux(endpoint, ["wait-for", "-S", gate.as_str()])?;
        // A command core wrapped for output capture performs a short FIFO handshake before
        // it execs. Waiting here keeps a fast program from being reported as the temporary
        // shell of that handshake.
        thread::sleep(Duration::from_millis(20));
        let observation = self.observe_until_settled(&placement);
        Ok(LaunchResult {
            placement,
            observation,
        })
    }

    pub(super) fn failure_after_create(
        &self,
        partial: PartialTmuxPlacement,
        error: TmuxProviderError,
    ) -> LaunchFailure {
        let cleanup = self.cleanup_partial(&partial);
        self.failure_after_cleanup(partial, cleanup, error)
    }

    /// A launch whose created resource `cleanup` closed is `gone`, and otherwise `lost`.
    pub(super) fn failure_after_cleanup(
        &self,
        partial: PartialTmuxPlacement,
        cleanup: CleanupResult,
        error: TmuxProviderError,
    ) -> LaunchFailure {
        let kind = if closed(&cleanup) {
            ObservationKind::Gone
        } else {
            ObservationKind::Lost
        };
        LaunchFailure::new(kind, Some(partial), Some(cleanup), error)
    }

    /// A create tmux answered in a form the Provider cannot read still made a resource. Its
    /// answer still names the created session `$n`, window `@n` and pane `%n`. The resource is
    /// closed as a created one only while that pane still sits in that session and window
    /// of the server the launch prepared on, and otherwise it is left to a person as `lost`.
    pub(super) fn failure_after_unread_create(
        &self,
        request: &LaunchRequest,
        stdout: &[u8],
        error: TmuxProviderError,
    ) -> LaunchFailure {
        match self.unread_partial(request, &String::from_utf8_lossy(stdout)) {
            Ok(Some(partial)) => self.failure_after_create(partial, error),
            Ok(None) => LaunchFailure::new(
                ObservationKind::Lost,
                None,
                Some(retained(
                    "the created resource remains because tmux's answer does not identify it",
                )),
                error,
            ),
            Err(cause) => LaunchFailure::new(
                ObservationKind::Lost,
                None,
                Some(CleanupResult::unknown(cause.to_string())),
                error,
            ),
        }
    }

    fn unread_partial(
        &self,
        request: &LaunchRequest,
        answer: &str,
    ) -> Result<Option<PartialTmuxPlacement>> {
        let prepared = &request.prepared;
        let Some(pane_id) = created_id(answer, '%') else {
            return Ok(None);
        };
        let panes = self.list_panes(&prepared.endpoint)?;
        let Some(pane) = panes.iter().find(|pane| pane.pane_id == pane_id) else {
            return Ok(None);
        };
        let Some(server) = self.process_incarnation(pane.server_pid)? else {
            return Ok(None);
        };
        // The created pane waits for its program, so a dead one is not the answer read.
        let Some(waiting_pane_pid) = pane.pane_pid else {
            return Ok(None);
        };
        // The pane reads now as the answer read when it was made, server PID and session
        // creation time included: a server started again on the endpoint numbers its own
        // session `$0` too.
        let fields = [
            pane.session_id.clone(),
            pane.window_id.clone(),
            pane.pane_id.clone(),
            pane.server_pid.to_string(),
            pane.session_created_at.to_string(),
            waiting_pane_pid.to_string(),
        ];
        if !answers_fields(answer.trim_end_matches('\n'), &fields)
            || prepared
                .server
                .as_ref()
                .is_some_and(|known| *known != server)
        {
            return Ok(None);
        }
        let origins = origins(prepared.unit);
        let partial = PartialTmuxPlacement {
            endpoint: prepared.endpoint.clone(),
            unit: prepared.unit,
            borrowed: prepared.borrowed.clone(),
            workdir: prepared.workdir.clone(),
            server_pid: server.pid,
            server: Some(server),
            session_id: pane.session_id.clone(),
            session_created_at: pane.session_created_at,
            session_origin: origins.0,
            window_id: pane.window_id.clone(),
            window_origin: origins.1,
            pane_id: pane.pane_id.clone(),
            pane_origin: ResourceOrigin::Created,
            waiting_pane_pid,
        };
        Ok(verify_created(prepared, &partial)
            .is_ok()
            .then_some(partial))
    }

    fn observe_until_settled(&self, placement: &TmuxPlacement) -> ObservationResult {
        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            let observed = self.observe(placement);
            let unsettled = observed.kind == ObservationKind::Mismatched
                || observed.detail.starts_with(EXITING);
            if !unsettled || Instant::now() >= deadline {
                return observed;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    pub(super) fn verify_prepared(&self, prepared: &PreparedLaunch) -> Result<()> {
        let current_server = self.server_incarnation(&prepared.endpoint)?;
        if current_server != prepared.server {
            return Err(TmuxProviderError::InvalidRequest(
                "the tmux server changed after placement preparation".to_owned(),
            ));
        }
        match prepared.unit {
            TmuxUnit::Session => Ok(()),
            TmuxUnit::Window => {
                let expected = prepared
                    .session
                    .as_ref()
                    .expect("validated prepared session");
                let pane = self.resolve_target(&prepared.endpoint, &expected.stable_id)?;
                if pane.server_pid == prepared.server.as_ref().expect("validated server").pid
                    && pane.session_id == expected.stable_id
                    && pane.session_created_at == expected.created_at
                {
                    Ok(())
                } else {
                    Err(TmuxProviderError::InvalidRequest(
                        "the borrowed tmux session changed after preparation".to_owned(),
                    ))
                }
            }
            TmuxUnit::Pane => {
                let expected_session = prepared
                    .session
                    .as_ref()
                    .expect("validated prepared session");
                let expected_window = prepared
                    .window_id
                    .as_deref()
                    .expect("validated prepared window");
                let pane = self.resolve_target(&prepared.endpoint, expected_window)?;
                if pane.server_pid == prepared.server.as_ref().expect("validated server").pid
                    && pane.session_id == expected_session.stable_id
                    && pane.session_created_at == expected_session.created_at
                    && pane.window_id == expected_window
                {
                    Ok(())
                } else {
                    Err(TmuxProviderError::InvalidRequest(
                        "the borrowed tmux window changed after preparation".to_owned(),
                    ))
                }
            }
        }
    }

    pub(super) fn server_incarnation(
        &self,
        endpoint: &TmuxEndpoint,
    ) -> Result<Option<ProcessIncarnation>> {
        let output = self.run_tmux_raw(endpoint, ["list-sessions", "-F", "#{pid}"])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if server_absent(&stderr) {
                return Ok(None);
            }
            return Err(tmux_failure("list-sessions", output));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        // The server answers with no sessions between closing its last session and
        // exiting, and holds nothing a launch could borrow, so it counts as no server.
        let Some(line) = stdout.lines().find(|line| !line.trim().is_empty()) else {
            return Ok(None);
        };
        let pid = line.trim().parse::<u32>().map_err(|_| {
            TmuxProviderError::Protocol("tmux returned an invalid server PID".to_owned())
        })?;
        self.process_incarnation(pid)
    }

    /// The exit code of a dead pane whose process tmux has not yet reaped. tmux marks the
    /// pane dead when its terminal reaches end of file and records the exit status when it
    /// handles the process's `SIGCHLD`, which comes later under load.
    pub(super) fn settled_exit_code(&self, endpoint: &TmuxEndpoint, pane_id: &str) -> Option<i32> {
        let deadline = Instant::now() + self.settle_timeout;
        loop {
            let pane = self.resolve_target(endpoint, pane_id).ok()?;
            if pane.reaped() {
                return pane.exit_code();
            }
            // tmux built with utempter before 3.6 drops a `SIGCHLD` that arrives while it
            // closes the dead pane's terminal, and then never reaps the process. Another
            // `SIGCHLD` makes tmux reap every child that has exited.
            let _ = provider::signal(pane.server_pid, "CHLD");
            // tmux 3.2 has no `pane_dead_signal` and leaves `pane_dead_status` empty for a
            // process a signal ended, so the signal is read from the line it draws.
            if let Some(code) = self.drawn_signal_exit_code(endpoint, pane_id) {
                return Some(code);
            }
            if Instant::now() >= deadline {
                return pane.exit_code();
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn drawn_signal_exit_code(&self, endpoint: &TmuxEndpoint, pane_id: &str) -> Option<i32> {
        let output = self
            .run_tmux(endpoint, ["capture-pane", "-p", "-J", "-t", pane_id])
            .ok()?;
        drawn_dead_signal(&String::from_utf8_lossy(&output.stdout)).and_then(signal_exit_code)
    }

    fn resolve_target(&self, endpoint: &TmuxEndpoint, target: &str) -> Result<PaneSnapshot> {
        let output = self.run_tmux(
            endpoint,
            ["display-message", "-p", "-t", target, PANE_FORMAT],
        )?;
        PaneSnapshot::parse(single_line(&output.stdout)?)
    }

    /// The pane a selector names, read from `list-panes -a`. `display-message -t` answers
    /// empty fields for a target that names nothing, and the current window of the session
    /// for a window index that names nothing, so `has-session -t` first asks tmux whether
    /// the whole target exists, and the pane is then looked up by its `%n`.
    /// `kind` names what the selector points at (`pane`, `window` or `session`) in the answer.
    pub(super) fn find_pane(
        &self,
        endpoint: &TmuxEndpoint,
        kind: &str,
        target: &str,
    ) -> Result<PaneSnapshot> {
        let not_found = || {
            TmuxProviderError::NotFound(format!(
                "{kind} {target} was not found on tmux socket {}",
                endpoint.label()
            ))
        };
        let pane_id = if target.starts_with('%') {
            target.to_owned()
        } else {
            let output = self.run_tmux_raw(endpoint, ["has-session", "-t", target])?;
            if !output.status.success() {
                if String::from_utf8_lossy(&output.stderr).starts_with("can't find") {
                    return Err(not_found());
                }
                return Err(tmux_failure("tmux", output));
            }
            let output = self.run_tmux(
                endpoint,
                ["display-message", "-p", "-t", target, "#{pane_id}"],
            )?;
            single_line(&output.stdout)?.to_owned()
        };
        self.list_panes(endpoint)?
            .into_iter()
            .find(|pane| !pane_id.is_empty() && pane.pane_id == pane_id)
            .ok_or_else(not_found)
    }

    pub(super) fn list_panes(&self, endpoint: &TmuxEndpoint) -> Result<Vec<PaneSnapshot>> {
        let output = self.run_tmux(endpoint, ["list-panes", "-a", "-F", PANE_FORMAT])?;
        lines(&output.stdout)?
            .into_iter()
            .map(PaneSnapshot::parse)
            .collect()
    }

    pub(super) fn list_sessions(&self, endpoint: &TmuxEndpoint) -> Result<Vec<SessionSnapshot>> {
        let output = self.run_tmux(
            endpoint,
            [
                "list-sessions",
                "-F",
                "#{session_id}\u{e000}#{session_created}\u{e000}#{pid}",
            ],
        )?;
        lines(&output.stdout)?
            .into_iter()
            .map(|line| {
                let fields: Vec<_> = line.split(FIELD_SEPARATOR).collect();
                if fields.len() != 3 {
                    return Err(TmuxProviderError::Protocol(
                        "invalid list-sessions field count".to_owned(),
                    ));
                }
                Ok(SessionSnapshot {
                    stable_id: fields[0].to_owned(),
                    created_at: parse_field(fields[1], "session creation time")?,
                    server_pid: parse_field(fields[2], "server PID")?,
                })
            })
            .collect()
    }

    pub(super) fn process_incarnation(&self, pid: u32) -> Result<Option<ProcessIncarnation>> {
        Ok(feld_core::process::incarnation(pid).map_err(ps_failed)?)
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
        let read = self.process_incarnation(pid)?;
        snapshot.processes.insert(pid, read.clone());
        Ok(read)
    }

    /// The panes of one endpoint, read once per `observe`. The reason a reading failed is
    /// remembered too, so that one broken endpoint costs one tmux call.
    pub(super) fn panes(
        &self,
        snapshot: &mut Snapshot,
        endpoint: &TmuxEndpoint,
    ) -> std::result::Result<Vec<PaneSnapshot>, String> {
        let key = serde_json::to_string(endpoint).expect("an endpoint re-encodes");
        if let Some(known) = snapshot.panes.get(&key) {
            return known.clone();
        }
        let read = self
            .run_tmux(endpoint, ["list-panes", "-a", "-F", PANE_FORMAT])
            .map_err(|error| error.to_string())
            .and_then(|output| {
                lines(&output.stdout)
                    .and_then(|lines| {
                        lines
                            .into_iter()
                            .map(PaneSnapshot::parse)
                            .collect::<Result<Vec<_>>>()
                    })
                    .map_err(|error| error.to_string())
            });
        snapshot.panes.insert(key, read.clone());
        read
    }

    pub(super) fn panes_match(
        &self,
        snapshot: &mut Snapshot,
        panes: &[PaneSnapshot],
        expected: &ProcessIncarnation,
    ) -> Result<bool> {
        if panes.is_empty() || panes.iter().any(|pane| pane.server_pid != expected.pid) {
            return Ok(false);
        }
        Ok(self.incarnation(snapshot, expected.pid)?.as_ref() == Some(expected))
    }

    pub(super) fn foreground_process_group(&self, pane_pid: u32) -> Result<u32> {
        let output = feld_core::process::ps(["-o", "tpgid=", "-p", &pane_pid.to_string()])
            .map_err(ps_failed)?;
        let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let pid: u32 = value.parse().unwrap_or(0);
        if pid == 0 {
            return Err(TmuxProviderError::InvalidRequest(
                "the selected pane has no foreground process group leader".to_owned(),
            ));
        }
        Ok(pid)
    }

    pub(super) fn run_tmux<I, S>(&self, endpoint: &TmuxEndpoint, args: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.run_tmux_raw(endpoint, args)?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(tmux_failure("tmux", output))
        }
    }

    pub(super) fn run_tmux_raw<I, S>(&self, endpoint: &TmuxEndpoint, args: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Ok(provider::run(&self.tmux, |command| {
            // Without a UTF-8 locale tmux turns FIELD_SEPARATOR into `_`; `-u` keeps it.
            command.arg("-u");
            endpoint.append_args(command);
            command.args(args);
        })?)
    }

    pub(super) fn connected_panes_match(
        &self,
        panes: &[PaneSnapshot],
        expected: &ProcessIncarnation,
    ) -> Result<bool> {
        if panes.is_empty() || panes.iter().any(|pane| pane.server_pid != expected.pid) {
            return Ok(false);
        }
        Ok(self.process_incarnation(expected.pid)?.as_ref() == Some(expected))
    }

    pub(super) fn connected_sessions_match(
        &self,
        sessions: &[SessionSnapshot],
        expected: &ProcessIncarnation,
    ) -> Result<bool> {
        if sessions.is_empty()
            || sessions
                .iter()
                .any(|session| session.server_pid != expected.pid)
        {
            return Ok(false);
        }
        Ok(self.process_incarnation(expected.pid)?.as_ref() == Some(expected))
    }

    pub(super) fn run_guarded_kill(
        &self,
        endpoint: &TmuxEndpoint,
        server_pid: u32,
        command: &str,
        target: &str,
    ) -> Result<bool> {
        let condition = format!("#{{==:#{{pid}},{server_pid}}}");
        let operation = format!("{command} -t {target}");
        let output = self.run_tmux(
            endpoint,
            [
                "if-shell",
                "-F",
                condition.as_str(),
                operation.as_str(),
                "display-message -p FELD_SERVER_MISMATCH",
            ],
        )?;
        Ok(!String::from_utf8_lossy(&output.stdout).contains("FELD_SERVER_MISMATCH"))
    }

    pub(super) fn cleanup_partial(&self, partial: &PartialTmuxPlacement) -> CleanupResult {
        match self.cleanup_partial_checked(partial) {
            Ok(result) => result,
            Err(error) => CleanupResult::unknown(error.to_string()),
        }
    }

    fn cleanup_partial_checked(&self, partial: &PartialTmuxPlacement) -> Result<CleanupResult> {
        let Some(server) = partial.server.as_ref() else {
            return Ok(retained(
                "the created resource remains because its server start time is unavailable",
            ));
        };
        if self.process_incarnation(server.pid)?.as_ref() != Some(server) {
            return Ok(retained("the created resource's server no longer matches"));
        }
        let target = match partial.unit {
            TmuxUnit::Session => {
                let sessions = self.list_sessions(&partial.endpoint)?;
                if !self.connected_sessions_match(&sessions, server)? {
                    return Ok(retained(
                        "the endpoint is connected to a different tmux server",
                    ));
                }
                if !sessions.iter().any(|session| {
                    session.stable_id == partial.session_id
                        && session.created_at == partial.session_created_at
                }) {
                    return Ok(already_cleaned(ALREADY_ABSENT));
                }
                ("kill-session", partial.session_id.as_str())
            }
            TmuxUnit::Window => {
                let panes = self.list_panes(&partial.endpoint)?;
                if !self.connected_panes_match(&panes, server)? {
                    return Ok(retained(
                        "the endpoint is connected to a different tmux server",
                    ));
                }
                if !panes.iter().any(|pane| {
                    pane.window_id == partial.window_id
                        && pane.session_created_at == partial.session_created_at
                }) {
                    return Ok(already_cleaned(ALREADY_ABSENT));
                }
                ("kill-window", partial.window_id.as_str())
            }
            TmuxUnit::Pane => {
                let panes = self.list_panes(&partial.endpoint)?;
                if !self.connected_panes_match(&panes, server)? {
                    return Ok(retained(
                        "the endpoint is connected to a different tmux server",
                    ));
                }
                let Some(pane) = panes.iter().find(|pane| pane.pane_id == partial.pane_id) else {
                    return Ok(already_cleaned(ALREADY_ABSENT));
                };
                if holds_borrowed_window(&panes, pane, &partial.window_id) {
                    return Ok(retained(
                        "the created pane is the borrowed window's last pane",
                    ));
                }
                ("kill-pane", partial.pane_id.as_str())
            }
        };
        if !self.run_guarded_kill(&partial.endpoint, server.pid, target.0, target.1)? {
            return Ok(retained("the endpoint changed tmux servers before cleanup"));
        }
        Ok(CleanupResult {
            cleaned: true,
            status: CleanupStatus::Removed,
            detail: "the created tmux resource was removed".to_owned(),
        })
    }
}

pub(super) fn validate_prepare_request(request: &PrepareRequest) -> Result<()> {
    match &request.endpoint {
        TmuxEndpoint::Name(name) if name.is_empty() => {
            return Err(TmuxProviderError::InvalidRequest(
                "tmux socket name must not be empty".to_owned(),
            ));
        }
        TmuxEndpoint::Path(path) if !path.is_absolute() => {
            return Err(TmuxProviderError::InvalidRequest(
                "tmux socket path must be absolute".to_owned(),
            ));
        }
        _ => {}
    }
    // Whether the workdir still exists is a launch-time condition, which the create
    // command reports as a `gone` Session rather than as a refused start.
    match (request.unit, request.borrowed.as_deref()) {
        (TmuxUnit::Session, None) => Ok(()),
        (TmuxUnit::Window | TmuxUnit::Pane, Some(selector)) if !selector.is_empty() => Ok(()),
        (TmuxUnit::Session, Some(_)) => Err(TmuxProviderError::InvalidRequest(
            "session unit does not accept a borrowed selector".to_owned(),
        )),
        _ => Err(TmuxProviderError::InvalidRequest(
            "window and pane units require a borrowed selector".to_owned(),
        )),
    }
}

pub(super) fn validate_launch_request(request: &LaunchRequest) -> Result<()> {
    if request.resource_name.is_empty() {
        return Err(TmuxProviderError::InvalidRequest(
            "resource name must not be empty".to_owned(),
        ));
    }
    if request.command.first().is_none_or(String::is_empty) {
        return Err(TmuxProviderError::InvalidRequest(
            "command must contain a non-empty argv".to_owned(),
        ));
    }
    if !request.prepared.workdir.is_absolute() || !request.prepared.workdir.is_dir() {
        return Err(TmuxProviderError::InvalidRequest(
            "prepared workdir must still be an existing absolute directory".to_owned(),
        ));
    }
    match request.prepared.unit {
        TmuxUnit::Session
            if request.prepared.borrowed.is_none()
                && request.prepared.session.is_none()
                && request.prepared.window_id.is_none() => {}
        TmuxUnit::Window
            if request.prepared.borrowed.is_some()
                && request.prepared.server.is_some()
                && request.prepared.session.is_some()
                && request.prepared.window_id.is_none() => {}
        TmuxUnit::Pane
            if request.prepared.borrowed.is_some()
                && request.prepared.server.is_some()
                && request.prepared.session.is_some()
                && request.prepared.window_id.is_some() => {}
        _ => {
            return Err(TmuxProviderError::InvalidRequest(
                "prepared placement is inconsistent with its tmux unit".to_owned(),
            ));
        }
    }
    Ok(())
}

pub(super) fn origins(unit: TmuxUnit) -> (ResourceOrigin, ResourceOrigin) {
    match unit {
        TmuxUnit::Session => (ResourceOrigin::Created, ResourceOrigin::Created),
        TmuxUnit::Window => (ResourceOrigin::Existing, ResourceOrigin::Created),
        TmuxUnit::Pane => (ResourceOrigin::Existing, ResourceOrigin::Existing),
    }
}

/// Whether closing the created `pane` would close the borrowed window: the pane is still in
/// it and the window holds no other pane. A pane moved to another window is closed wherever
/// it is, and tmux closes that window with it only when the pane was its last.
pub(super) fn holds_borrowed_window(
    panes: &[PaneSnapshot],
    pane: &PaneSnapshot,
    borrowed: &str,
) -> bool {
    pane.window_id == borrowed
        && panes
            .iter()
            .filter(|candidate| candidate.window_id == pane.window_id)
            .map(|candidate| candidate.pane_id.as_str())
            .collect::<HashSet<_>>()
            .len()
            <= 1
}

/// Whether a cleanup left none of the created resource.
pub(super) fn closed(cleanup: &CleanupResult) -> bool {
    matches!(
        cleanup.status,
        CleanupStatus::Removed | CleanupStatus::AlreadyAbsent
    )
}

pub(super) fn verify_created(
    prepared: &PreparedLaunch,
    partial: &PartialTmuxPlacement,
) -> Result<()> {
    if let Some(expected) = prepared.server.as_ref()
        && partial.server.as_ref() != Some(expected)
    {
        return Err(TmuxProviderError::InvalidRequest(
            "the create command reached a different tmux server".to_owned(),
        ));
    }
    match prepared.unit {
        TmuxUnit::Session => Ok(()),
        TmuxUnit::Window => {
            let expected = prepared
                .session
                .as_ref()
                .expect("validated prepared session");
            if partial.session_id == expected.stable_id
                && partial.session_created_at == expected.created_at
            {
                Ok(())
            } else {
                Err(TmuxProviderError::InvalidRequest(
                    "the create command reached a different borrowed session".to_owned(),
                ))
            }
        }
        TmuxUnit::Pane => {
            let expected_session = prepared
                .session
                .as_ref()
                .expect("validated prepared session");
            let expected_window = prepared
                .window_id
                .as_deref()
                .expect("validated prepared window");
            if partial.session_id == expected_session.stable_id
                && partial.session_created_at == expected_session.created_at
                && partial.window_id == expected_window
            {
                Ok(())
            } else {
                Err(TmuxProviderError::InvalidRequest(
                    "the create command reached a different borrowed window".to_owned(),
                ))
            }
        }
    }
}

/// The environment the Runtime Adapter asked for, which the launch script takes from this
/// Provider.
pub(super) fn launch_env(env: &[(String, String)]) -> Result<LaunchEnv> {
    if let Some((name, _)) = env.iter().find(|(name, _)| !valid_env_name(name)) {
        return Err(TmuxProviderError::InvalidRequest(format!(
            "invalid environment variable name '{name}'"
        )));
    }
    LaunchEnv::new(env, &receiver()?).map_err(TmuxProviderError::LaunchEnv)
}

/// The executable the launch script takes its environment from: this Plugin's own.
#[cfg(not(test))]
fn receiver() -> Result<PathBuf> {
    std::env::current_exe().map_err(TmuxProviderError::LaunchEnv)
}

/// A test runs as its own executable, so the launch script takes its environment from the
/// Plugin executable `cargo` builds beside the test's `deps` directory.
#[cfg(test)]
fn receiver() -> Result<PathBuf> {
    let test = std::env::current_exe().map_err(TmuxProviderError::LaunchEnv)?;
    Ok(test
        .parent()
        .and_then(Path::parent)
        .expect("the target directory")
        .join(env!("CARGO_PKG_NAME")))
}

pub(super) fn launch_shell_command(
    tmux: &Path,
    endpoint: &TmuxEndpoint,
    gate: &str,
    env: &LaunchEnv,
    command: &[String],
) -> String {
    let mut shell = env.source();
    shell.push_str("; ");
    shell.push_str(&quote(&tmux.to_string_lossy()));
    match endpoint {
        TmuxEndpoint::Name(name) => {
            shell.push_str(" -L ");
            shell.push_str(&quote(name));
        }
        TmuxEndpoint::Path(path) => {
            shell.push_str(" -S ");
            shell.push_str(&quote(&path.to_string_lossy()));
        }
    }
    shell.push_str(" wait-for ");
    shell.push_str(&quote(gate));
    shell.push_str("; exec");
    for argument in command {
        shell.push(' ');
        shell.push_str(&quote(argument));
    }
    shell
}

/// Whether `answer` is `fields` joined by one separator, whatever tmux printed it as.
pub(super) fn answers_fields(answer: &str, fields: &[String]) -> bool {
    let Some(rest) = answer.strip_prefix(fields[0].as_str()) else {
        return false;
    };
    rest.match_indices(fields[1].as_str())
        .next()
        .is_some_and(|(at, _)| at > 0 && fields.join(&rest[..at]) == answer)
}

/// The first stable ID with `prefix` (`$`, `@` or `%`) in a create answer.
fn created_id(answer: &str, prefix: char) -> Option<&str> {
    answer.match_indices(prefix).find_map(|(start, _)| {
        let digits = answer[start + 1..]
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(answer.len() - start - 1);
        (digits > 0).then(|| &answer[start..start + 1 + digits])
    })
}

pub(super) fn tmux_failure(command: &str, output: Output) -> TmuxProviderError {
    TmuxProviderError::Tmux {
        command: command.to_owned(),
        status: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    }
}

pub(super) fn create_was_rejected(output: &Output) -> bool {
    if output.status.code().is_none() {
        return false;
    }
    !server_lost(&String::from_utf8_lossy(&output.stderr))
}

/// What a create that tmux could not be run for is sure of. A client that never started
/// created nothing; one that answered nothing by the deadline may have reached the server.
pub(super) fn create_run_failure_kind(error: &TmuxProviderError) -> ObservationKind {
    match error {
        TmuxProviderError::Io(failed) if failed.timed_out() => ObservationKind::Lost,
        _ => ObservationKind::Gone,
    }
}

/// The client lost the server mid-command, as it does with a server that is exiting.
fn server_lost(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    [
        "server exited unexpectedly",
        "lost server",
        "connection lost",
        "broken pipe",
    ]
    .iter()
    .any(|message| stderr.contains(message))
}

fn server_absent(stderr: &str) -> bool {
    stderr.contains("no server running")
        || stderr.contains("(No such file or directory)")
        || stderr.contains("(Connection refused)")
        || server_lost(stderr)
}
