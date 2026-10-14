//! What tmux answers: the sessions and panes one reading holds, and the lines of one
//! command's output.

use std::collections::HashMap;
use std::path::PathBuf;

use feld_core::process::ProcessIncarnation;

use crate::placement::*;

/// What tmux 3.2 and later print unchanged in a format, and what a path or a name hardly
/// holds. tmux 3.4 and 3.5 print a control character such as U+001F as `\037`.
pub(super) const FIELD_SEPARATOR: char = '\u{e000}';
pub(super) const PANE_FORMAT: &str = "#{session_id}\u{e000}#{window_id}\u{e000}#{pane_id}\u{e000}#{pid}\u{e000}#{session_created}\u{e000}#{pane_pid}\u{e000}#{pane_dead}\u{e000}#{pane_dead_status}\u{e000}#{pane_dead_signal}\u{e000}#{session_name}\u{e000}#{window_name}\u{e000}#{window_index}\u{e000}#{pane_index}\u{e000}#{pane_current_path}\u{e000}#{socket_path}";
pub(super) const CREATED_FORMAT: &str = "#{session_id}\u{e000}#{window_id}\u{e000}#{pane_id}\u{e000}#{pid}\u{e000}#{session_created}\u{e000}#{pane_pid}";

/// What one `observe` read of tmux and the process table, so that a sequence of Placements
/// is judged against one consistent reading instead of one reading each.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub(super) panes: HashMap<String, std::result::Result<Vec<PaneSnapshot>, String>>,
    pub(super) processes: HashMap<u32, Option<ProcessIncarnation>>,
}

#[derive(Debug)]
pub(super) struct SessionSnapshot {
    pub(super) stable_id: String,
    pub(super) created_at: i64,
    pub(super) server_pid: u32,
}

#[derive(Debug)]
pub(super) struct CreatedPane {
    pub(super) session_id: String,
    pub(super) window_id: String,
    pub(super) pane_id: String,
    pub(super) server_pid: u32,
    pub(super) session_created_at: i64,
    pub(super) waiting_pane_pid: u32,
}

impl CreatedPane {
    pub(super) fn parse(line: &str) -> Result<Self> {
        let fields: Vec<_> = line.split(FIELD_SEPARATOR).collect();
        if fields.len() != 6 {
            return Err(TmuxProviderError::Protocol(format!(
                "created resource returned {} fields, expected 6",
                fields.len()
            )));
        }
        Ok(Self {
            session_id: fields[0].to_owned(),
            window_id: fields[1].to_owned(),
            pane_id: fields[2].to_owned(),
            server_pid: parse_field(fields[3], "server PID")?,
            session_created_at: parse_field(fields[4], "session creation time")?,
            waiting_pane_pid: parse_field(fields[5], "pane PID")?,
        })
    }
}

#[derive(Clone, Debug)]
pub(super) struct PaneSnapshot {
    pub(super) session_id: String,
    pub(super) window_id: String,
    pub(super) pane_id: String,
    pub(super) server_pid: u32,
    pub(super) session_created_at: i64,
    /// The pane's process. tmux 3.8 and later answer none for a dead pane.
    pub(super) pane_pid: Option<u32>,
    pub(super) dead: bool,
    dead_status: Option<i32>,
    dead_signal: Option<String>,
    session_name: String,
    window_name: String,
    window_index: u32,
    pane_index: u32,
    pub(super) current_path: PathBuf,
    pub(super) socket_path: PathBuf,
}

impl PaneSnapshot {
    pub(super) fn parse(line: &str) -> Result<Self> {
        let fields: Vec<_> = line.split(FIELD_SEPARATOR).collect();
        if fields.len() != 15 {
            return Err(TmuxProviderError::Protocol(format!(
                "pane returned {} fields, expected 15",
                fields.len()
            )));
        }
        Ok(Self {
            session_id: fields[0].to_owned(),
            window_id: fields[1].to_owned(),
            pane_id: fields[2].to_owned(),
            server_pid: parse_field(fields[3], "server PID")?,
            session_created_at: parse_field(fields[4], "session creation time")?,
            pane_pid: if fields[5].is_empty() {
                None
            } else {
                Some(parse_field(fields[5], "pane PID")?)
            },
            dead: fields[6] == "1",
            dead_status: if fields[7].is_empty() {
                None
            } else {
                Some(parse_field(fields[7], "pane exit status")?)
            },
            dead_signal: (!fields[8].is_empty()).then(|| fields[8].to_owned()),
            session_name: fields[9].to_owned(),
            window_name: fields[10].to_owned(),
            window_index: parse_field(fields[11], "window index")?,
            pane_index: parse_field(fields[12], "pane index")?,
            current_path: PathBuf::from(fields[13]),
            socket_path: PathBuf::from(fields[14]),
        })
    }

    pub(super) fn current_names(&self) -> CurrentNames {
        CurrentNames {
            session_id: self.session_id.clone(),
            window_id: self.window_id.clone(),
            pane_id: self.pane_id.clone(),
            session: self.session_name.clone(),
            window: self.window_name.clone(),
            window_index: self.window_index,
            pane_index: self.pane_index,
        }
    }

    /// The process of a pane that is not dead, which tmux always answers.
    pub(super) fn live_pid(&self) -> Result<u32> {
        self.pane_pid.ok_or_else(|| {
            TmuxProviderError::Protocol("tmux returned no PID for a live pane".to_owned())
        })
    }

    /// Whether tmux has recorded how the dead pane's process ended.
    pub(super) fn reaped(&self) -> bool {
        self.dead_status.is_some() || self.dead_signal.is_some()
    }

    pub(super) fn exit_code(&self) -> Option<i32> {
        self.dead_status
            .or_else(|| self.dead_signal.as_deref().and_then(signal_exit_code))
    }
}

/// The exit code a shell reports for a process `signal` ended. tmux names the signal, or
/// numbers it where the system has no signal names.
pub(super) fn signal_exit_code(signal: &str) -> Option<i32> {
    match signal {
        "kill" => Some(137),
        "term" => Some(143),
        number => number.parse::<i32>().ok().map(|number| 128 + number),
    }
}

/// The signal named by the line tmux draws on a dead pane once it has reaped the process,
/// `Pane is dead (signal term, <time>)`, when that line is the last on `screen`.
pub(super) fn drawn_dead_signal(screen: &str) -> Option<&str> {
    let last = screen.lines().rev().find(|line| !line.trim().is_empty())?;
    let (signal, _) = last
        .strip_prefix("Pane is dead (signal ")?
        .split_once(',')?;
    Some(signal)
}

pub(super) fn parse_field<T>(field: &str, description: &str) -> Result<T>
where
    T: std::str::FromStr,
{
    field
        .parse()
        .map_err(|_| TmuxProviderError::Protocol(format!("tmux returned an invalid {description}")))
}

pub(super) fn single_line(bytes: &[u8]) -> Result<&str> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| TmuxProviderError::Protocol("tmux returned non-UTF-8 output".to_owned()))?;
    let mut lines = text.lines();
    let line = lines
        .next()
        .ok_or_else(|| TmuxProviderError::Protocol("tmux returned empty output".to_owned()))?;
    if lines.any(|extra| !extra.is_empty()) {
        return Err(TmuxProviderError::Protocol(
            "tmux returned multiple resources for a single target".to_owned(),
        ));
    }
    Ok(line)
}

pub(super) fn lines(bytes: &[u8]) -> Result<Vec<&str>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| TmuxProviderError::Protocol("tmux returned non-UTF-8 output".to_owned()))?;
    Ok(text.lines().filter(|line| !line.is_empty()).collect())
}
