//! The operations of the Plugin contract, on tmux.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use feld_core::plugin::protocol::{Candidate, ObservationKind};
use feld_core::plugin::sdk::provider::{
    self, CleanupResult, CleanupStatus, NOTHING_TO_KILL, ProviderBackend, ResourceOrigin,
    WAIT_COMMAND, acts_on, already_cleaned, observation, option_text, retained, selector_text,
};
use feld_core::process::{quote, quote_for_display};

use crate::commands::*;
use crate::placement::*;
use crate::snapshot::*;

/// A pane as `self` answers it from inside: the socket path and `%n`.
fn pane_selector(socket: &str, pane: &str) -> Value {
    Value::Object(Map::from_iter([
        ("pane".to_owned(), Value::String(pane.to_owned())),
        ("socket".to_owned(), Value::String(socket.to_owned())),
    ]))
}

impl ProviderBackend for TmuxProvider {
    const NAME: &'static str = "tmux";
    type Endpoint = TmuxEndpoint;
    type PrepareRequest = PrepareRequest;
    type Placement = TmuxPlacement;
    type Prepared = PreparedLaunch;
    type Partial = PartialTmuxPlacement;
    type Current = CurrentNames;
    type Snapshot = Snapshot;
    type Error = TmuxProviderError;

    /// `self`: where the caller sits on tmux, or nothing when the caller is outside tmux.
    fn self_selector() -> Option<Value> {
        let socket = socket_path(&std::env::var("TMUX").ok()?)?;
        let pane = std::env::var("TMUX_PANE")
            .ok()
            .filter(|pane| !pane.is_empty())?;
        Some(pane_selector(&socket, &pane))
    }

    /// `resolve`: fill the options the caller left out from its own position and refuse the
    /// combinations tmux cannot place. The answer is what the launchSpec keeps.
    fn resolve_options(
        options: Option<&Value>,
        caller: Option<&Value>,
    ) -> std::result::Result<Value, String> {
        let empty = Map::new();
        let given = match options {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(object)) => object,
            Some(_) => return Err("tmux providerOptions must be a JSON object".to_owned()),
        };
        for key in given.keys() {
            if !matches!(key.as_str(), "socket" | "unit" | "session" | "window") {
                return Err(format!("tmux does not take the option '{key}'"));
            }
        }
        let socket = option_text("tmux", given, "socket")?
            .or_else(|| selector_text(caller, "socket"))
            .unwrap_or_else(|| DEFAULT_SOCKET.to_owned());
        let unit = option_text("tmux", given, "unit")?.unwrap_or_else(|| "session".to_owned());
        let session = option_text("tmux", given, "session")?;
        let window = option_text("tmux", given, "window")?;
        match unit.as_str() {
            "session" if session.is_none() && window.is_none() => {}
            "window" if session.is_some() && window.is_none() => {}
            "pane" if window.is_some() && session.is_none() => {}
            "session" => {
                return Err("tmux unit session takes no borrowed selector".to_owned());
            }
            "window" => return Err("tmux unit window takes the borrowed session only".to_owned()),
            "pane" => return Err("tmux unit pane takes the borrowed window only".to_owned()),
            other => {
                return Err(format!(
                    "tmux unit must be session, window, or pane, not '{other}'"
                ));
            }
        }
        let mut resolved = Map::from_iter([
            ("socket".to_owned(), Value::String(socket)),
            ("unit".to_owned(), Value::String(unit)),
        ]);
        if let Some(session) = session {
            resolved.insert("session".to_owned(), Value::String(session));
        }
        if let Some(window) = window {
            resolved.insert("window".to_owned(), Value::String(window));
        }
        Ok(Value::Object(resolved))
    }

    /// The launch tmux prepares for the resolved options.
    fn prepare_request(options: Option<&Value>, workdir: &Path) -> Result<PrepareRequest> {
        let resolved =
            Self::resolve_options(options, None).map_err(TmuxProviderError::InvalidRequest)?;
        let socket = resolved["socket"].as_str().expect("resolved socket");
        let unit = match resolved["unit"].as_str().expect("resolved unit") {
            "session" => TmuxUnit::Session,
            "window" => TmuxUnit::Window,
            _ => TmuxUnit::Pane,
        };
        let borrowed = match unit {
            TmuxUnit::Session => None,
            TmuxUnit::Window => resolved.get("session"),
            TmuxUnit::Pane => resolved.get("window"),
        }
        .and_then(Value::as_str)
        .map(str::to_owned);
        Ok(PrepareRequest {
            endpoint: endpoint(socket),
            unit,
            borrowed,
            workdir: workdir.to_owned(),
        })
    }

    /// `attach`: the pane a selector names, with the socket the caller sits on when the
    /// selector leaves it out.
    fn attach_request(selector: &Value) -> std::result::Result<AttachRequest, String> {
        let given = selector
            .as_object()
            .ok_or_else(|| "a tmux selector must be a JSON object".to_owned())?;
        for key in given.keys() {
            if !matches!(key.as_str(), "socket" | "pane") {
                return Err(format!("tmux does not take the selector '{key}'"));
            }
        }
        let pane = option_text("tmux", given, "pane")?
            .ok_or_else(|| "tmux attach requires --tmux-pane".to_owned())?;
        let socket = option_text("tmux", given, "socket")?
            .or_else(|| selector_text(Self::self_selector().as_ref(), "socket"))
            .unwrap_or_else(|| DEFAULT_SOCKET.to_owned());
        Ok(AttachRequest {
            endpoint: endpoint(&socket),
            pane,
        })
    }

    /// `summary`: the Placement as a person reads it (`pane %3 (main:1, existing)`). The stable
    /// ID is the record; the name tmux answers for it now is added when the reading found the
    /// pane still inside it.
    fn summary(placement: &Value, current: Option<&CurrentNames>) -> String {
        let unit = placement
            .get("unit")
            .and_then(Value::as_str)
            .unwrap_or("tmux");
        let (stable_id, origin) = match unit {
            "session" => ("sessionId", "sessionOrigin"),
            "window" => ("windowId", "windowOrigin"),
            "pane" => ("paneId", "paneOrigin"),
            _ => ("", ""),
        };
        let stable_id = placement
            .get(stable_id)
            .and_then(Value::as_str)
            .unwrap_or("pending");
        let mut about = Vec::new();
        if let Some(current) = current.filter(|current| current.id_of(unit) == Some(stable_id)) {
            about.push(current.name_of(unit));
        }
        if let Some(origin) = placement.get(origin).and_then(Value::as_str) {
            about.push(origin.to_owned());
        }
        let mut summary = format!("{unit} {stable_id}");
        if !about.is_empty() {
            summary.push_str(&format!(" ({})", about.join(", ")));
        }
        summary
    }

    /// `attachCommand`: the tmux command that puts a terminal on the stored Placement, or on
    /// the pane itself when the reading found it moved out of the recorded unit.
    fn attach_command(placement: &Value, current: Option<&CurrentNames>) -> Option<String> {
        let endpoint = placement.get("endpoint")?;
        let value = endpoint.get("value").and_then(Value::as_str)?;
        let selector = match endpoint.get("kind").and_then(Value::as_str)? {
            "name" => format!("-L {}", quote_for_display(value)),
            "path" => format!("-S {}", quote_for_display(value)),
            _ => return None,
        };
        let unit = placement.get("unit").and_then(Value::as_str)?;
        let recorded = match unit {
            "session" => placement.get("sessionId"),
            "window" => placement.get("windowId"),
            "pane" => placement.get("paneId"),
            _ => None,
        }?
        .as_str()?;
        // A pane moved out of the recorded unit is reached by its own ID, which opens the
        // window and pane it is in now.
        let target = match current {
            Some(current) if current.id_of(unit) != Some(recorded) => current.pane_id.as_str(),
            _ => recorded,
        };
        Some(format!(
            "tmux {selector} attach-session -t {}",
            quote(target)
        ))
    }

    fn prepare(&self, request: PrepareRequest) -> Result<PreparedLaunch> {
        validate_prepare_request(&request)?;
        let server = self.server_incarnation(&request.endpoint)?;

        let (borrowed, session, window_id) = match request.unit {
            TmuxUnit::Session => (None, None, None),
            TmuxUnit::Window | TmuxUnit::Pane => {
                let server = server.as_ref().ok_or_else(|| {
                    TmuxProviderError::InvalidRequest(
                        "the borrowed tmux resource has no running server".to_owned(),
                    )
                })?;
                let selector = request
                    .borrowed
                    .as_deref()
                    .expect("validated borrowed selector");
                let kind = match request.unit {
                    TmuxUnit::Window => "session",
                    TmuxUnit::Pane => "window",
                    TmuxUnit::Session => unreachable!(),
                };
                let pane = self.find_pane(&request.endpoint, kind, selector)?;
                if pane.server_pid != server.pid {
                    return Err(TmuxProviderError::Protocol(
                        "tmux returned inconsistent server PIDs".to_owned(),
                    ));
                }
                let session = PreparedSession {
                    stable_id: pane.session_id,
                    created_at: pane.session_created_at,
                };
                let window = (request.unit == TmuxUnit::Pane).then_some(pane.window_id);
                let borrowed = match request.unit {
                    TmuxUnit::Window => Some(session.stable_id.clone()),
                    TmuxUnit::Pane => window.clone(),
                    TmuxUnit::Session => unreachable!(),
                };
                (borrowed, Some(session), window)
            }
        };

        Ok(PreparedLaunch {
            endpoint: request.endpoint,
            unit: request.unit,
            borrowed,
            workdir: request.workdir,
            server,
            session,
            window_id,
        })
    }

    /// Identifies the tmux resources a selector points at and the process in the pane's
    /// foreground. Whether that process is the Runtime the caller asked for is the Runtime
    /// Adapter's `match` to answer.
    fn attach(&self, request: AttachRequest) -> Result<AttachedPane> {
        if request.pane.is_empty() {
            return Err(TmuxProviderError::InvalidRequest(
                "tmux pane selector must not be empty".to_owned(),
            ));
        }
        let pane = self.find_pane(&request.endpoint, "pane", &request.pane)?;
        if pane.dead {
            return Err(TmuxProviderError::InvalidRequest(
                "the selected tmux pane is not running".to_owned(),
            ));
        }
        let server = self
            .process_incarnation(pane.server_pid)?
            .ok_or_else(|| TmuxProviderError::Protocol("tmux server disappeared".to_owned()))?;
        let pane_pid = pane.live_pid()?;
        let pane_process = self.process_incarnation(pane_pid)?.ok_or_else(|| {
            TmuxProviderError::Protocol("tmux pane process disappeared".to_owned())
        })?;
        let runtime_pid = self.foreground_process_group(pane_pid)?;
        let runtime_process = self.process_incarnation(runtime_pid)?.ok_or_else(|| {
            TmuxProviderError::InvalidRequest("the Runtime process is not running".to_owned())
        })?;
        Ok(AttachedPane {
            current: pane.current_names(),
            selector: pane_selector(&pane.socket_path.display().to_string(), &pane.pane_id),
            placement: TmuxPlacement {
                endpoint: TmuxEndpoint::Path(pane.socket_path),
                unit: TmuxUnit::Pane,
                borrowed: Some(pane.window_id.clone()),
                workdir: pane.current_path,
                server,
                session_id: pane.session_id,
                session_created_at: pane.session_created_at,
                session_origin: ResourceOrigin::Existing,
                window_id: pane.window_id,
                window_origin: ResourceOrigin::Existing,
                pane_id: pane.pane_id,
                pane_origin: ResourceOrigin::Existing,
                pane_process,
                program: None,
                process: Some(runtime_process),
            },
        })
    }

    /// The live panes of the server the caller sits on, each with the process in its
    /// foreground. Whether that process is a Runtime is the Runtime Adapter's `match`.
    fn candidates(&self, selector: Option<&Value>) -> std::result::Result<Vec<Candidate>, String> {
        let socket = selector_text(selector, "socket")
            .or_else(|| selector_text(Self::self_selector().as_ref(), "socket"))
            .unwrap_or_else(|| DEFAULT_SOCKET.to_owned());
        let panes = self
            .list_panes(&endpoint(&socket))
            .map_err(|error| error.to_string())?;
        let mut candidates = Vec::new();
        for pane in panes.into_iter().filter(|pane| !pane.dead) {
            let Some(Ok(pid)) = pane.pane_pid.map(|pid| self.foreground_process_group(pid)) else {
                continue;
            };
            let Some(process) = feld_core::process::snapshot(pid).map_err(|error| {
                format!("read the foreground process of {}: {error}", pane.pane_id)
            })?
            else {
                continue;
            };
            let names = pane.current_names();
            candidates.push(Candidate {
                options: pane_selector(&pane.socket_path.display().to_string(), &pane.pane_id),
                summary: format!(
                    "pane {} ({}:{}.{})",
                    pane.pane_id, names.session, names.window_index, names.pane_index
                ),
                workdir: pane.current_path.display().to_string(),
                process,
            });
        }
        Ok(candidates)
    }

    fn launch(&self, request: LaunchRequest) -> std::result::Result<LaunchResult, LaunchFailure> {
        if let Err(error) = validate_launch_request(&request) {
            return Err(LaunchFailure::new(ObservationKind::Gone, None, None, error));
        }
        if let Err(error) = self.verify_prepared(&request.prepared) {
            return Err(LaunchFailure::new(ObservationKind::Gone, None, None, error));
        }

        match self.create_and_launch(&request) {
            Ok(result) => Ok(result),
            Err(Attempt::Failed(failure)) => Err(failure),
            // tmux 3.2a to 3.7c leaves the pane of a failed respawn without the input state
            // its next respawn frees, and that respawn crashes the server. The launch closes
            // the resource it created and creates it once more instead.
            Err(Attempt::Unrespawned(partial, error)) => {
                let cleanup = self.cleanup_partial(&partial);
                if !closed(&cleanup) {
                    return Err(self.failure_after_cleanup(*partial, cleanup, error));
                }
                thread::sleep(RESPAWN_RETRY_DELAY);
                self.create_and_launch(&request)
                    .map_err(|attempt| match attempt {
                        Attempt::Failed(failure) => failure,
                        Attempt::Unrespawned(partial, error) => {
                            self.failure_after_create(*partial, error)
                        }
                    })
            }
        }
    }

    fn stop(&self, placement: &TmuxPlacement) -> ControlResult {
        self.control(placement, "TERM", "tmux sent SIGTERM to the pane's process")
    }

    fn kill(&self, placement: &TmuxPlacement) -> ControlResult {
        let before = self.observe(placement);
        if !acts_on(&before) {
            return ControlResult {
                attempt: NOTHING_TO_KILL.to_owned(),
                observed: before,
            };
        }
        let pid = placement.pane_process.pid;
        if let Err(error) = self.signal(pid, "KILL") {
            return ControlResult {
                attempt: "tmux killed the pane's process (SIGKILL)".to_owned(),
                observed: observation(
                    ObservationKind::Unknown,
                    None,
                    error.to_string(),
                    before.current,
                ),
            };
        }
        match self.process_incarnation(pid) {
            Ok(Some(current)) if current != placement.pane_process => ControlResult {
                attempt: "tmux killed the pane's process (SIGKILL)".to_owned(),
                observed: observation(
                    ObservationKind::Unknown,
                    None,
                    "the process PID changed immediately after SIGKILL".to_owned(),
                    before.current,
                ),
            },
            Err(error) => ControlResult {
                attempt: "tmux killed the pane's process (SIGKILL)".to_owned(),
                observed: observation(
                    ObservationKind::Unknown,
                    None,
                    error.to_string(),
                    before.current,
                ),
            },
            _ => ControlResult {
                attempt: "tmux killed the pane's process (SIGKILL)".to_owned(),
                observed: self.observe_after_signal(placement),
            },
        }
    }

    fn observe_checked(
        &self,
        placement: &TmuxPlacement,
        snapshot: &mut Snapshot,
    ) -> Result<ObservationResult> {
        if let Some(expected) = &placement.process {
            match self.incarnation(snapshot, expected.pid)? {
                Some(process) if process == *expected => {}
                _ => {
                    return Ok(observation(
                        ObservationKind::Gone,
                        None,
                        "the attached Runtime process exited".to_owned(),
                        None,
                    ));
                }
            }
        }
        match self.incarnation(snapshot, placement.server.pid)? {
            Some(current) if current == placement.server => {}
            _ => {
                return Ok(observation(
                    ObservationKind::Lost,
                    None,
                    "the tmux server incarnation is no longer running".to_owned(),
                    None,
                ));
            }
        }

        let panes = match self.panes(snapshot, &placement.endpoint) {
            Ok(panes) => panes,
            Err(reason) => {
                return Ok(observation(ObservationKind::Unknown, None, reason, None));
            }
        };
        if !self.panes_match(snapshot, &panes, &placement.server)? {
            return Ok(observation(
                ObservationKind::Lost,
                None,
                "the endpoint is connected to a different tmux server".to_owned(),
                None,
            ));
        }
        let matches = panes
            .iter()
            .filter(|pane| pane.pane_id == placement.pane_id)
            .collect::<Vec<_>>();
        if matches.is_empty() {
            // When the pane of an `attached` Session disappears, check its Runtime process.
            let process = placement
                .process
                .as_ref()
                .unwrap_or(&placement.pane_process);
            let kind = match self.incarnation(snapshot, process.pid)? {
                Some(current) if current == *process => ObservationKind::Lost,
                _ => ObservationKind::Gone,
            };
            return Ok(observation(
                kind,
                None,
                if kind == ObservationKind::Lost {
                    "the pane disappeared while its recorded process is still running"
                } else {
                    "the pane and its recorded process are gone"
                }
                .to_owned(),
                None,
            ));
        }

        let outside = matches
            .iter()
            .find(|pane| {
                pane.session_id != placement.session_id
                    || pane.window_id != placement.window_id
                    || pane.session_created_at != placement.session_created_at
            })
            .copied();
        let preferred = outside.unwrap_or(matches[0]);
        let current = Some(preferred.current_names());
        if preferred.dead {
            let exit_code = if preferred.reaped() {
                preferred.exit_code()
            } else {
                self.settled_exit_code(&placement.endpoint, &preferred.pane_id)
            };
            return Ok(observation(
                ObservationKind::Gone,
                exit_code,
                "the pane process exited".to_owned(),
                current,
            ));
        }
        if preferred.pane_pid != Some(placement.pane_process.pid) {
            return Ok(observation(
                ObservationKind::Mismatched,
                None,
                "the pane now contains a different process PID".to_owned(),
                current,
            ));
        }
        // A pid the pane still holds but the process table no longer lists is a process
        // tmux has not yet reaped: it is still exiting until the pane turns dead.
        let exiting = match self.incarnation(snapshot, placement.pane_process.pid)? {
            Some(process) if process == placement.pane_process => false,
            None => true,
            Some(_) => {
                return Ok(observation(
                    ObservationKind::Mismatched,
                    None,
                    "the pane process incarnation does not match".to_owned(),
                    current,
                ));
            }
        };
        // A relocated pane always names where it runs now, exiting or not.
        let location = outside.map(|pane| {
            let names = pane.current_names();
            format!(
                "the recorded pane is now in window {} ({}) of session {} ({})",
                names.window_id,
                names.name_of("window"),
                names.session_id,
                names.session
            )
        });
        Ok(observation(
            if location.is_some() {
                ObservationKind::Relocated
            } else {
                ObservationKind::Running
            },
            None,
            match (exiting, location) {
                (true, Some(location)) => format!("{EXITING}; {location}"),
                (true, None) => EXITING.to_owned(),
                (false, Some(location)) => location,
                (false, None) => "the recorded tmux incarnation is running".to_owned(),
            },
            current,
        ))
    }

    fn cleanup_checked(&self, placement: &TmuxPlacement) -> Result<CleanupResult> {
        if placement.session_origin == ResourceOrigin::Existing
            && placement.window_origin == ResourceOrigin::Existing
            && placement.pane_origin == ResourceOrigin::Existing
        {
            return Ok(retained("the tmux resources are borrowed"));
        }
        if self.process_incarnation(placement.server.pid)?.as_ref() != Some(&placement.server) {
            return Ok(retained("the tmux server incarnation no longer matches"));
        }
        match placement.unit {
            TmuxUnit::Session => {
                let sessions = self.list_sessions(&placement.endpoint)?;
                if !self.connected_sessions_match(&sessions, &placement.server)? {
                    return Ok(retained(
                        "the endpoint is connected to a different tmux server",
                    ));
                }
                let Some(session) = sessions.iter().find(|session| {
                    session.stable_id == placement.session_id
                        && session.created_at == placement.session_created_at
                }) else {
                    return Ok(already_cleaned(ALREADY_ABSENT));
                };
                if !self.run_guarded_kill(
                    &placement.endpoint,
                    placement.server.pid,
                    "kill-session",
                    &session.stable_id,
                )? {
                    return Ok(retained("the endpoint changed tmux servers before cleanup"));
                }
            }
            TmuxUnit::Window => {
                let panes = self.list_panes(&placement.endpoint)?;
                if !self.connected_panes_match(&panes, &placement.server)? {
                    return Ok(retained(
                        "the endpoint is connected to a different tmux server",
                    ));
                }
                if !panes
                    .iter()
                    .any(|pane| pane.window_id == placement.window_id)
                {
                    return Ok(already_cleaned(ALREADY_ABSENT));
                }
                if !self.run_guarded_kill(
                    &placement.endpoint,
                    placement.server.pid,
                    "kill-window",
                    &placement.window_id,
                )? {
                    return Ok(retained("the endpoint changed tmux servers before cleanup"));
                }
            }
            TmuxUnit::Pane => {
                let panes = self.list_panes(&placement.endpoint)?;
                if !self.connected_panes_match(&panes, &placement.server)? {
                    return Ok(retained(
                        "the endpoint is connected to a different tmux server",
                    ));
                }
                let Some(pane) = panes.iter().find(|pane| pane.pane_id == placement.pane_id) else {
                    return Ok(already_cleaned(ALREADY_ABSENT));
                };
                // tmux 3.8 and later answer no PID for a dead pane; the pane ID, which a
                // server never reuses, then names the incarnation alone.
                if pane
                    .pane_pid
                    .is_some_and(|pid| pid != placement.pane_process.pid)
                {
                    return Ok(retained("the pane process incarnation no longer matches"));
                }
                if !pane.dead
                    && self.process_incarnation(pane.live_pid()?)?.as_ref()
                        != Some(&placement.pane_process)
                {
                    return Ok(retained("the pane process incarnation no longer matches"));
                }
                if holds_borrowed_window(&panes, pane, &placement.window_id) {
                    return Ok(retained(
                        "the created pane is the borrowed window's last pane",
                    ));
                }
                if !self.run_guarded_kill(
                    &placement.endpoint,
                    placement.server.pid,
                    "kill-pane",
                    &placement.pane_id,
                )? {
                    return Ok(retained("the endpoint changed tmux servers before cleanup"));
                }
            }
        }
        Ok(CleanupResult {
            cleaned: true,
            status: CleanupStatus::Removed,
            detail: "the created tmux resource was removed".to_owned(),
        })
    }

    fn workdir(placement: &TmuxPlacement) -> &Path {
        &placement.workdir
    }

    fn signalled(placement: &TmuxPlacement) -> Option<u32> {
        Some(placement.pane_process.pid)
    }

    /// Types one input into the pane and presses Enter, as long as the pane still holds
    /// the recorded incarnation. An empty input is the Enter alone.
    fn send_input(&self, placement: &TmuxPlacement, input: &str) -> Result<()> {
        let condition = format!(
            "#{{&&:#{{==:#{{pid}},{}}},#{{==:#{{pane_pid}},{}}}}}",
            placement.server.pid, placement.pane_process.pid
        );
        let typing = match input.is_empty() {
            true => String::new(),
            false => format!(
                "send-keys -t {} -l -- {} ; ",
                placement.pane_id,
                quote(input)
            ),
        };
        let operation = format!("{typing}send-keys -t {} Enter", placement.pane_id);
        self.run_tmux(
            &placement.endpoint,
            [
                "if-shell",
                "-t",
                placement.pane_id.as_str(),
                "-F",
                condition.as_str(),
                operation.as_str(),
                "display-message -p FELD_INCARNATION_MISMATCH",
            ],
        )
        .and_then(|output| {
            if String::from_utf8_lossy(&output.stdout).contains("FELD_INCARNATION_MISMATCH") {
                Err(TmuxProviderError::Protocol(
                    "the tmux incarnation changed before graceful stop".to_owned(),
                ))
            } else {
                Ok(())
            }
        })
    }

    /// Re-observes until the pane settles on something other than its process, or the wait
    /// is spent. A process on its way out can briefly read as `mismatched`, so that answer
    /// counts as settled only when the wait ends on it.
    fn observe_after_signal(&self, placement: &TmuxPlacement) -> ObservationResult {
        let deadline = Instant::now() + self.settle_timeout;
        loop {
            let observed = self.observe(placement);
            if !matches!(
                observed.kind,
                ObservationKind::Running | ObservationKind::Relocated | ObservationKind::Mismatched
            ) {
                return observed;
            }
            if Instant::now() >= deadline {
                if observed.kind == ObservationKind::Mismatched {
                    return observed;
                }
                return ObservationResult {
                    detail: "the pane and its process are still present".to_owned(),
                    ..observed
                };
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn signal(&self, pid: u32, signal: &str) -> Result<()> {
        provider::signal(pid, signal)?.map_err(TmuxProviderError::Protocol)
    }
}

/// Why one create of a launch did not put a program in place.
enum Attempt {
    Failed(LaunchFailure),
    /// `respawn-pane` failed in the pane it created, which takes no other tmux command
    /// but a kill.
    Unrespawned(Box<PartialTmuxPlacement>, TmuxProviderError),
}

impl TmuxProvider {
    /// Creates the resource of a launch and starts its command in it.
    fn create_and_launch(
        &self,
        request: &LaunchRequest,
    ) -> std::result::Result<LaunchResult, Attempt> {
        let endpoint = &request.prepared.endpoint;
        let mut args: Vec<String> = match request.prepared.unit {
            TmuxUnit::Session => vec![
                "new-session".into(),
                "-d".into(),
                "-s".into(),
                request.resource_name.clone(),
            ],
            TmuxUnit::Window => vec![
                "new-window".into(),
                "-d".into(),
                "-t".into(),
                format!(
                    "{}:",
                    request
                        .prepared
                        .session
                        .as_ref()
                        .expect("validated prepared session")
                        .stable_id
                ),
                "-n".into(),
                request.resource_name.clone(),
            ],
            TmuxUnit::Pane => vec![
                "split-window".into(),
                "-d".into(),
                "-t".into(),
                request
                    .prepared
                    .window_id
                    .clone()
                    .expect("validated prepared window"),
            ],
        };
        args.extend([
            "-c".into(),
            request.prepared.workdir.to_string_lossy().into_owned(),
            "-P".into(),
            "-F".into(),
            CREATED_FORMAT.into(),
            "--".into(),
            WAIT_COMMAND.into(),
        ]);

        let output = match self.run_tmux_raw(endpoint, args.iter().map(String::as_str)) {
            // A client that cannot create the server's socket reports it on stderr and
            // still exits 0, with nothing created and so nothing printed.
            Ok(output)
                if output.status.success()
                    && output.stdout.trim_ascii().is_empty()
                    && !output.stderr.trim_ascii().is_empty()
                    && create_was_rejected(&output) =>
            {
                return Err(Attempt::Failed(LaunchFailure::new(
                    ObservationKind::Gone,
                    None,
                    None,
                    tmux_failure("tmux create", output),
                )));
            }
            // tmux 3.2 says nothing at all when it cannot create the socket, so the socket
            // missing after an empty answer is what shows nothing was created.
            Ok(output)
                if output.status.success()
                    && output.stdout.trim_ascii().is_empty()
                    && socket_is_missing(endpoint) =>
            {
                return Err(Attempt::Failed(LaunchFailure::new(
                    ObservationKind::Gone,
                    None,
                    None,
                    TmuxProviderError::Protocol(format!(
                        "tmux created nothing and left no socket at {}",
                        endpoint.label()
                    )),
                )));
            }
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                let kind = if create_was_rejected(&output) {
                    ObservationKind::Gone
                } else {
                    ObservationKind::Lost
                };
                return Err(Attempt::Failed(LaunchFailure::new(
                    kind,
                    None,
                    None,
                    tmux_failure("tmux create", output),
                )));
            }
            Err(error) => {
                return Err(Attempt::Failed(LaunchFailure::new(
                    create_run_failure_kind(&error),
                    None,
                    None,
                    error,
                )));
            }
        };
        let created = match single_line(&output.stdout).and_then(CreatedPane::parse) {
            Ok(created) => created,
            Err(error) => {
                return Err(Attempt::Failed(self.failure_after_unread_create(
                    request,
                    &output.stdout,
                    error,
                )));
            }
        };
        let origins = origins(request.prepared.unit);
        let mut partial = PartialTmuxPlacement {
            endpoint: endpoint.clone(),
            unit: request.prepared.unit,
            borrowed: request.prepared.borrowed.clone(),
            workdir: request.prepared.workdir.clone(),
            server_pid: created.server_pid,
            server: None,
            session_id: created.session_id.clone(),
            session_created_at: created.session_created_at,
            session_origin: origins.0,
            window_id: created.window_id.clone(),
            window_origin: origins.1,
            pane_id: created.pane_id.clone(),
            pane_origin: ResourceOrigin::Created,
            waiting_pane_pid: created.waiting_pane_pid,
        };

        match self.process_incarnation(created.server_pid) {
            Ok(Some(server)) => partial.server = Some(server),
            Ok(None) => {
                return Err(Attempt::Failed(self.failure_after_create(
                    partial,
                    TmuxProviderError::Protocol(
                        "created tmux server process disappeared".to_owned(),
                    ),
                )));
            }
            Err(error) => return Err(Attempt::Failed(self.failure_after_create(partial, error))),
        }
        if let Err(error) = verify_created(&request.prepared, &partial) {
            return Err(Attempt::Failed(self.failure_after_create(partial, error)));
        }
        match self.finish_launch(request, partial.clone()) {
            Ok(result) => Ok(result),
            Err(Unfinished::Unrespawned(error)) => {
                Err(Attempt::Unrespawned(Box::new(partial), error))
            }
            Err(Unfinished::Failed(error)) => {
                Err(Attempt::Failed(self.failure_after_create(partial, error)))
            }
        }
    }
}

/// Whether the socket a `-S` path names is absent. A `-L` name leaves the path to tmux, so
/// it never counts as missing.
fn socket_is_missing(endpoint: &TmuxEndpoint) -> bool {
    match endpoint {
        TmuxEndpoint::Path(path) => !path.exists(),
        TmuxEndpoint::Name(_) => false,
    }
}
