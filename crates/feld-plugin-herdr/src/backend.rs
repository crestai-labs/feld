//! The operations of the Plugin contract, on herdr.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use feld_core::plugin::protocol::{Candidate, ObservationKind};
use feld_core::plugin::sdk::provider::{
    self, CleanupResult, NOTHING_TO_KILL, ProviderBackend, ResourceOrigin, acts_on,
    already_cleaned, observation, option_text, ps_failed, retained, selector_text,
};
use feld_core::process::quote_for_display;

use crate::commands::*;
use crate::placement::*;
use crate::snapshot::*;

impl ProviderBackend for HerdrProvider {
    const NAME: &'static str = "herdr";
    type Endpoint = HerdrEndpoint;
    type PrepareRequest = PrepareRequest;
    type Placement = HerdrPlacement;
    type Prepared = HerdrPlacement;
    type Partial = HerdrPlacement;
    type Current = CurrentNames;
    type Snapshot = Snapshot;
    type Error = HerdrProviderError;

    /// `self`: where the caller sits on herdr, or nothing when the caller is outside herdr.
    ///
    /// herdr puts `HERDR_SOCKET_PATH` in every pane process, so the endpoint a pane answers is
    /// the path of the server that owns it.
    fn self_selector() -> Option<Value> {
        let read = |name: &str| std::env::var(name).ok();
        selector_of(
            read("HERDR_ENV").as_deref(),
            read("HERDR_SOCKET_PATH").as_deref(),
            read("HERDR_PANE_ID").as_deref(),
        )
    }

    /// `resolve`: fill the options the caller left out from its own position and refuse the
    /// combinations herdr cannot place. The answer is what the launchSpec keeps.
    fn resolve_options(
        options: Option<&Value>,
        caller: Option<&Value>,
    ) -> std::result::Result<Value, String> {
        let empty = Map::new();
        let given = match options {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(object)) => object,
            Some(_) => return Err("herdr providerOptions must be a JSON object".to_owned()),
        };
        for key in given.keys() {
            if !matches!(
                key.as_str(),
                "session" | "unit" | "workspace" | "pane" | "split"
            ) {
                return Err(format!("herdr does not take the option '{key}'"));
            }
        }
        let session = option_text("herdr", given, "session")?
            .or_else(|| selector_text(caller, "session"))
            .unwrap_or_else(|| DEFAULT_SESSION.to_owned());
        let unit = option_text("herdr", given, "unit")?.unwrap_or_else(|| "workspace".to_owned());
        let workspace = option_text("herdr", given, "workspace")?;
        let pane = option_text("herdr", given, "pane")?;
        let split = option_text("herdr", given, "split")?;
        match unit.as_str() {
            "workspace" if workspace.is_none() && pane.is_none() => {}
            "tab" if workspace.is_some() && pane.is_none() => {}
            "pane" if pane.is_some() && workspace.is_none() => {}
            "workspace" => return Err("herdr unit workspace takes no borrowed selector".to_owned()),
            "tab" => return Err("herdr unit tab takes the borrowed workspace only".to_owned()),
            "pane" => return Err("herdr unit pane takes the borrowed pane only".to_owned()),
            other => {
                return Err(format!(
                    "herdr unit must be workspace, tab, or pane, not '{other}'"
                ));
            }
        }
        if unit != "pane" && split.is_some() {
            return Err("herdr takes a split only for the unit pane".to_owned());
        }
        let mut resolved = Map::from_iter([
            ("session".to_owned(), Value::String(session)),
            ("unit".to_owned(), Value::String(unit.clone())),
        ]);
        if let Some(workspace) = workspace {
            resolved.insert("workspace".to_owned(), Value::String(workspace));
        }
        if let Some(pane) = pane {
            resolved.insert("pane".to_owned(), Value::String(pane));
        }
        if unit == "pane" {
            let split = split.unwrap_or_else(|| "right".to_owned());
            if !matches!(split.as_str(), "right" | "down") {
                return Err(format!("herdr split must be right or down, not '{split}'"));
            }
            resolved.insert("split".to_owned(), Value::String(split));
        }
        Ok(Value::Object(resolved))
    }

    /// The launch herdr prepares for the resolved options.
    fn prepare_request(options: Option<&Value>, workdir: &Path) -> Result<PrepareRequest> {
        let resolved =
            Self::resolve_options(options, None).map_err(HerdrProviderError::InvalidRequest)?;
        let session = resolved["session"].as_str().expect("resolved session");
        let unit = match resolved["unit"].as_str().expect("resolved unit") {
            "workspace" => HerdrUnit::Workspace,
            "tab" => HerdrUnit::Tab,
            _ => HerdrUnit::Pane,
        };
        let borrowed = match unit {
            HerdrUnit::Workspace => None,
            HerdrUnit::Tab => resolved.get("workspace"),
            HerdrUnit::Pane => resolved.get("pane"),
        }
        .and_then(Value::as_str)
        .map(str::to_owned);
        let split = match resolved.get("split").and_then(Value::as_str) {
            Some("down") => Some(HerdrSplit::Down),
            Some(_) => Some(HerdrSplit::Right),
            None => None,
        };
        Ok(PrepareRequest {
            endpoint: endpoint(session),
            unit,
            split,
            borrowed,
            workdir: workdir.to_owned(),
        })
    }

    /// `attach`: the pane a selector names, with the server the caller sits on when the
    /// selector leaves it out.
    fn attach_request(selector: &Value) -> std::result::Result<AttachRequest, String> {
        let given = selector
            .as_object()
            .ok_or_else(|| "a herdr selector must be a JSON object".to_owned())?;
        for key in given.keys() {
            if !matches!(key.as_str(), "session" | "pane") {
                return Err(format!("herdr attach does not take the selector '{key}'"));
            }
        }
        let pane = option_text("herdr", given, "pane")?
            .ok_or_else(|| "herdr attach requires --herdr-pane".to_owned())?;
        let session = option_text("herdr", given, "session")?
            .or_else(|| selector_text(Self::self_selector().as_ref(), "session"))
            .unwrap_or_else(|| DEFAULT_SESSION.to_owned());
        Ok(AttachRequest {
            endpoint: endpoint(&session),
            pane,
        })
    }

    /// `summary`: the Placement as a person reads it (`workspace w3 (feldtest, created)`). The
    /// ID is the record; the label herdr answers for it now is added when the reading found
    /// the pane still inside the recorded resource.
    fn summary(placement: &Value, current: Option<&CurrentNames>) -> String {
        let unit = placement
            .get("unit")
            .and_then(Value::as_str)
            .unwrap_or("herdr");
        let (id, origin) = match unit {
            "workspace" => ("workspaceId", "workspaceOrigin"),
            "tab" => ("tabId", "tabOrigin"),
            "pane" => ("paneId", "paneOrigin"),
            _ => ("", ""),
        };
        let id = placement
            .get(id)
            .and_then(Value::as_str)
            .unwrap_or("pending");
        let mut about = Vec::new();
        if let Some(current) = current.filter(|current| current.id_of(unit) == Some(id)) {
            about.push(current.name_of(unit));
        }
        if let Some(origin) = placement.get(origin).and_then(Value::as_str) {
            about.push(origin.to_owned());
        }
        let mut summary = format!("{unit} {id}");
        if !about.is_empty() {
            summary.push_str(&format!(" ({})", about.join(", ")));
        }
        summary
    }

    /// `attachCommand`: the herdr command that opens the stored pane's terminal.
    fn attach_command(placement: &Value, _current: Option<&CurrentNames>) -> Option<String> {
        let terminal = quote_for_display(placement.get("terminalId").and_then(Value::as_str)?);
        let endpoint = placement.get("endpoint")?;
        let value = quote_for_display(endpoint.get("value").and_then(Value::as_str)?);
        match endpoint.get("kind").and_then(Value::as_str)? {
            "session" => Some(format!(
                "herdr --session {value} terminal attach {terminal}"
            )),
            "path" => Some(format!(
                "HERDR_SOCKET_PATH={value} herdr terminal attach {terminal}"
            )),
            _ => None,
        }
    }

    /// `place`: the three axes, with the borrowed resource resolved to the IDs and the
    /// terminal id the launch re-checks. Nothing is created yet, so the Placement carries
    /// no incarnation.
    fn prepare(&self, request: PrepareRequest) -> Result<HerdrPlacement> {
        validate_prepare_request(&request)?;
        let mut placement = HerdrPlacement {
            endpoint: request.endpoint,
            unit: request.unit,
            split: request.split,
            borrowed: None,
            workdir: request.workdir,
            workspace_id: None,
            workspace_origin: None,
            tab_id: None,
            tab_origin: None,
            pane_id: None,
            pane_origin: None,
            terminal_id: None,
            pane_process: None,
            program: None,
            process: None,
        };
        let selector = request.borrowed.unwrap_or_default();
        match request.unit {
            HerdrUnit::Workspace => {}
            HerdrUnit::Tab => {
                let answer = self.call(&placement.endpoint, ["workspace", "get", &selector])?;
                let workspace = text(&answer, &["workspace", "workspace_id"])?;
                placement.workspace_id = Some(workspace.clone());
                placement.workspace_origin = Some(ResourceOrigin::Existing);
                placement.borrowed = Some(Borrowed {
                    selector,
                    workspace_id: workspace,
                    tab_id: None,
                    pane_id: None,
                    terminal_id: None,
                });
            }
            HerdrUnit::Pane => {
                let answer = self.call(&placement.endpoint, ["pane", "get", &selector])?;
                let workspace = text(&answer, &["pane", "workspace_id"])?;
                let tab = text(&answer, &["pane", "tab_id"])?;
                let pane = text(&answer, &["pane", "pane_id"])?;
                let terminal = text(&answer, &["pane", "terminal_id"])?;
                placement.workspace_id = Some(workspace.clone());
                placement.workspace_origin = Some(ResourceOrigin::Existing);
                placement.tab_id = Some(tab.clone());
                placement.tab_origin = Some(ResourceOrigin::Existing);
                placement.borrowed = Some(Borrowed {
                    selector,
                    workspace_id: workspace,
                    tab_id: Some(tab),
                    pane_id: Some(pane),
                    terminal_id: Some(terminal),
                });
            }
        }
        Ok(placement)
    }

    /// `attach`: the existing pane a selector names, fixed to the incarnation every
    /// observation that follows is judged against. The pane, its tab, and its workspace
    /// are all `existing`, and the Provider's operations on them are readings alone.
    ///
    /// Whether the process in front of the pane is the Runtime the caller asked for is the
    /// Runtime Adapter's `match` to answer.
    fn attach(&self, request: AttachRequest) -> Result<AttachedPane> {
        if request.pane.is_empty() {
            return Err(HerdrProviderError::InvalidRequest(
                "herdr pane selector must not be empty".to_owned(),
            ));
        }
        let endpoint = request.endpoint;
        let pane = pane_of(&self.call(&endpoint, ["pane", "get", request.pane.as_str()])?)?;
        // The Runtime runs in the pane's foreground, so its own directory is the one the
        // Session works in; the pane keeps the directory it was created in.
        let workdir = pane
            .foreground_cwd
            .clone()
            .or_else(|| pane.cwd.clone())
            .ok_or_else(|| {
                HerdrProviderError::Protocol(
                    "herdr answered no working directory for this pane".to_owned(),
                )
            })?;
        let info = self.call(
            &endpoint,
            ["pane", "process-info", "--pane", pane.pane_id.as_str()],
        )?;
        let pane_process =
            feld_core::process::incarnation(number(&info, &["process_info", "shell_pid"])?)
                .map_err(ps_failed)?
                .ok_or_else(|| {
                    HerdrProviderError::Protocol("the herdr pane has no process".to_owned())
                })?;
        // A shell puts the program it starts in front of itself, so the leader of the
        // pane's foreground process group is the Runtime a person started in it.
        let leader = number(&info, &["process_info", "foreground_process_group_id"])?;
        let process = feld_core::process::incarnation(leader)
            .map_err(ps_failed)?
            .ok_or_else(|| {
                HerdrProviderError::InvalidRequest("the Runtime process is not running".to_owned())
            })?;
        let socket = self.socket_path(&endpoint)?;
        Ok(AttachedPane {
            current: self.current_names(&endpoint, &pane),
            selector: pane_selector(&socket.display().to_string(), &pane.pane_id),
            placement: HerdrPlacement {
                endpoint,
                unit: HerdrUnit::Pane,
                split: None,
                borrowed: Some(Borrowed {
                    selector: request.pane,
                    workspace_id: pane.workspace_id.clone(),
                    tab_id: Some(pane.tab_id.clone()),
                    pane_id: Some(pane.pane_id.clone()),
                    terminal_id: Some(pane.terminal_id.clone()),
                }),
                workdir: PathBuf::from(workdir),
                workspace_id: Some(pane.workspace_id),
                workspace_origin: Some(ResourceOrigin::Existing),
                tab_id: Some(pane.tab_id),
                tab_origin: Some(ResourceOrigin::Existing),
                pane_id: Some(pane.pane_id),
                pane_origin: Some(ResourceOrigin::Existing),
                terminal_id: Some(pane.terminal_id),
                pane_process: Some(pane_process),
                program: None,
                process: Some(process),
            },
        })
    }

    /// `candidates`: the panes of the server the caller sits on, each with the process in
    /// its foreground. Whether that process is a Runtime is the Runtime Adapter's `match`.
    fn candidates(&self, selector: Option<&Value>) -> std::result::Result<Vec<Candidate>, String> {
        let session = selector_text(selector, "session")
            .or_else(|| selector_text(Self::self_selector().as_ref(), "session"))
            .unwrap_or_else(|| DEFAULT_SESSION.to_owned());
        let endpoint = endpoint(&session);
        let reading = self.reading(&mut Snapshot::default(), &endpoint).map_err(
            |(ReadFailure::ServerDown(reason) | ReadFailure::Unreadable(reason))| reason,
        )?;
        let socket = self
            .socket_path(&endpoint)
            .map_err(|error| error.to_string())?
            .display()
            .to_string();
        let mut candidates = Vec::new();
        for pane in &reading.panes {
            let Some(workdir) = pane.foreground_cwd.as_ref().or(pane.cwd.as_ref()) else {
                continue;
            };
            // A pane closed since the reading answers nothing, and one without the shell
            // or the process in front that an attach records is no pane an attach can
            // take. Either leaves the other panes listed.
            let Ok(info) = self.call(
                &endpoint,
                ["pane", "process-info", "--pane", pane.pane_id.as_str()],
            ) else {
                continue;
            };
            let (Ok(shell), Ok(leader)) = (
                number(&info, &["process_info", "shell_pid"]),
                number(&info, &["process_info", "foreground_process_group_id"]),
            ) else {
                continue;
            };
            let read = |pid| {
                feld_core::process::snapshot(pid)
                    .map_err(|error| format!("read the processes of {}: {error}", pane.pane_id))
            };
            let (Some(_), Some(process)) = (read(shell)?, read(leader)?) else {
                continue;
            };
            candidates.push(Candidate {
                options: pane_selector(&socket, &pane.pane_id),
                summary: format!(
                    "pane {} ({})",
                    pane.pane_id,
                    reading.current_names(pane).name_of("pane")
                ),
                workdir: workdir.clone(),
                process,
            });
        }
        Ok(candidates)
    }

    /// `launch`: create the resource the unit names, put the Runtime's command into the
    /// pane it holds, and report what the first observation saw. A failure answers with
    /// the observation it is sure of, so that core can close the Session rather than treat
    /// the Provider as unreachable.
    fn launch(&self, request: LaunchRequest) -> std::result::Result<LaunchResult, LaunchFailure> {
        if let Err(error) = validate_launch_request(&request) {
            return Err(LaunchFailure::new(ObservationKind::Gone, None, None, error));
        }
        if let Err(error) = self.verify_prepared(&request.prepared) {
            return Err(LaunchFailure::new(launch_kind(&error), None, None, error));
        }
        let env = match launch_env(&request.env) {
            Ok(env) => env,
            Err(error) => return Err(LaunchFailure::new(ObservationKind::Gone, None, None, error)),
        };
        let created = match self.create(&request, &env) {
            Ok(created) => created,
            Err(error) => return Err(LaunchFailure::new(launch_kind(&error), None, None, error)),
        };
        let (workspace_origin, tab_origin) = origins(request.prepared.unit);
        let placement = HerdrPlacement {
            workspace_id: Some(created.workspace_id.clone()),
            workspace_origin: Some(workspace_origin),
            tab_id: Some(created.tab_id.clone()),
            tab_origin: Some(tab_origin),
            pane_id: Some(created.pane_id.clone()),
            pane_origin: Some(ResourceOrigin::Created),
            terminal_id: Some(created.terminal_id.clone()),
            ..request.prepared.clone()
        };
        match self.finish_launch(placement.clone(), &env) {
            Ok(result) => Ok(result),
            Err(error) => Err(self.failure_after_create(placement, error)),
        }
    }

    /// `stop` for a `program` and for a Runtime whose Adapter declares no `stopInput`:
    /// SIGTERM to the pane's program.
    fn stop(&self, placement: &HerdrPlacement) -> ControlResult {
        self.control(placement, "TERM", SIGTERM_ATTEMPT)
    }

    /// `kill`: SIGKILL to the pane's program. The start time of the pid is read on both
    /// sides of the signal, so that a pid the OS handed to another process in between is
    /// answered as `unknown` rather than as a program this Provider ended.
    fn kill(&self, placement: &HerdrPlacement) -> ControlResult {
        let before = self.observe(placement);
        let Some(program) = acts_on(&before).then(|| program_of(placement)).flatten() else {
            return ControlResult {
                attempt: NOTHING_TO_KILL.to_owned(),
                observed: before,
            };
        };
        if let Err(error) = self.signal(program.pid, "KILL") {
            return self.unsure_kill(error.to_string(), before);
        }
        self.after_kill(placement, program, before)
    }

    /// The order of `provider/herdr.md`: the attached Runtime, the server, the pane, the
    /// pane's own process, and last the program.
    fn observe_checked(
        &self,
        placement: &HerdrPlacement,
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
        let Some(pane_id) = placement.pane_id.clone() else {
            return Ok(observation(
                ObservationKind::Lost,
                None,
                "the launch was interrupted before it held a herdr pane".to_owned(),
                None,
            ));
        };
        let reading = match self.reading(snapshot, &placement.endpoint) {
            Ok(reading) => reading,
            Err(ReadFailure::Unreadable(reason)) => {
                return Ok(observation(ObservationKind::Unknown, None, reason, None));
            }
            Err(ReadFailure::ServerDown(reason)) => {
                return self.without_server(snapshot, placement, &reason);
            }
        };
        let found = placement
            .terminal_id
            .as_deref()
            .and_then(|terminal_id| reading.pane_of_terminal(terminal_id));
        let Some(pane) = found else {
            // The recorded ID may still be there under another terminal, which a restored
            // pane is: the ID survives a server restart and the terminal does not.
            if let Some(namesake) = reading.pane(&pane_id) {
                return Ok(observation(
                    ObservationKind::Mismatched,
                    None,
                    "the pane ID holds a terminal this Placement never recorded".to_owned(),
                    Some(reading.current_names(namesake)),
                ));
            }
            return Ok(match self.program_is_running(snapshot, placement)? {
                Some(true) => observation(
                    ObservationKind::Lost,
                    None,
                    "the herdr pane is gone while its program is still running".to_owned(),
                    None,
                ),
                Some(false) => observation(
                    ObservationKind::Gone,
                    None,
                    "the herdr pane and its program are gone".to_owned(),
                    None,
                ),
                None => observation(
                    ObservationKind::Unknown,
                    None,
                    "the herdr pane is gone and this Placement holds no program".to_owned(),
                    None,
                ),
            });
        };
        let current = Some(reading.current_names(pane));
        // A launch records the pane's own process before anything else of the pane, and
        // refuses a pane herdr answers no `shell_pid` for, so a Placement without it is one
        // a daemon that stopped in the middle of a launch wrote.
        let Some(pane_process) = placement.pane_process.as_ref() else {
            return Ok(observation(
                ObservationKind::Lost,
                None,
                "the launch was interrupted before it recorded the pane process".to_owned(),
                current,
            ));
        };
        match self.incarnation(snapshot, pane_process.pid)? {
            Some(process) if process == *pane_process => {}
            _ => {
                return Ok(observation(
                    ObservationKind::Mismatched,
                    None,
                    "the pane process incarnation does not match".to_owned(),
                    current,
                ));
            }
        }
        let exit_code = exit_code_of(pane.terminal_title.as_deref());
        Ok(match self.program_is_running(snapshot, placement)? {
            Some(true) => {
                let relocated = placement.pane_id.as_deref() != Some(pane.pane_id.as_str())
                    || placement.tab_id.as_deref() != Some(pane.tab_id.as_str())
                    || placement.workspace_id.as_deref() != Some(pane.workspace_id.as_str());
                observation(
                    if relocated {
                        ObservationKind::Relocated
                    } else {
                        ObservationKind::Running
                    },
                    None,
                    if relocated {
                        format!(
                            "the recorded pane is now {} in tab {} of workspace {}",
                            pane.pane_id, pane.tab_id, pane.workspace_id
                        )
                    } else {
                        "the recorded herdr incarnation is running".to_owned()
                    },
                    current,
                )
            }
            Some(false) => observation(
                ObservationKind::Gone,
                exit_code.or_else(|| self.settled_exit_code(&placement.endpoint, &pane.pane_id)),
                "the pane's program exited".to_owned(),
                current,
            ),
            // The launch found no program because `<command>` had already finished, which
            // is what the exit code in the pane's terminal title says. Without that code
            // the launch never recorded a program at all, which a launch that ran to its
            // end does not leave behind: the Placement is one a daemon that stopped in the
            // middle of a launch wrote, and `lost` is what core recovers from.
            None => match exit_code {
                Some(code) => observation(
                    ObservationKind::Gone,
                    Some(code),
                    "the pane's program exited before it was identified".to_owned(),
                    current,
                ),
                None => observation(
                    ObservationKind::Lost,
                    None,
                    "the launch was interrupted before it recorded a program".to_owned(),
                    current,
                ),
            },
        })
    }

    fn cleanup_checked(&self, placement: &HerdrPlacement) -> Result<CleanupResult> {
        let (resource, id, origin) = unit_resource(placement);
        if origin != Some(ResourceOrigin::Created) {
            return Ok(retained("the herdr resources of this Session are borrowed"));
        }
        let Some(id) = id else {
            return Ok(already_cleaned("the launch created no herdr resource"));
        };
        let reading = match self.reading(&mut Snapshot::default(), &placement.endpoint) {
            Ok(reading) => reading,
            // A server that is not running holds nothing to close, and brings its panes
            // back with the layout it saved when it starts again.
            Err(ReadFailure::ServerDown(reason)) => return Ok(retained(&reason)),
            Err(ReadFailure::Unreadable(reason)) => {
                return Ok(CleanupResult::unknown(reason));
            }
        };
        let alive = match placement.pane_process.as_ref() {
            Some(process) => {
                feld_core::process::incarnation(process.pid)
                    .map_err(ps_failed)?
                    .as_ref()
                    == Some(process)
            }
            None => false,
        };
        Ok(match cleanup_plan(placement, &reading, id, alive) {
            Cleanup::Close => self.close_resource(&placement.endpoint, resource, id),
            Cleanup::Answer(result) => result,
        })
    }

    fn workdir(placement: &HerdrPlacement) -> &Path {
        &placement.workdir
    }

    fn signalled(placement: &HerdrPlacement) -> Option<u32> {
        program_of(placement).map(|program| program.pid)
    }

    /// One line into the pane: the text with Enter, or the Enter alone for an empty input.
    /// `pane run` is `pane.send_input` with both, and `pane send-keys` is the Enter.
    fn send_input(&self, placement: &HerdrPlacement, input: &str) -> Result<()> {
        let pane_id = placement.pane_id.as_deref().ok_or_else(|| {
            HerdrProviderError::InvalidRequest("this Placement holds no herdr pane".to_owned())
        })?;
        match input.is_empty() {
            true => self.send(&placement.endpoint, ["pane", "send-keys", pane_id, "enter"]),
            false => self.send(&placement.endpoint, ["pane", "run", pane_id, input]),
        }
    }

    /// Re-observes until the program is gone or the wait is spent. A program still there
    /// when the wait ends leaves the Session open for `feld session kill`.
    fn observe_after_signal(&self, placement: &HerdrPlacement) -> ObservationResult {
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

    fn signal(&self, pid: u32, signal: &str) -> Result<()> {
        provider::signal(pid, signal)?.map_err(HerdrProviderError::Protocol)
    }
}
