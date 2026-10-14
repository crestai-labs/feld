use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use feld_core::model::Session;
use feld_core::process;

use super::SessionController;

impl SessionController {
    pub(super) fn prepare_output_capture(&self, session: &str) -> Result<OutputCapture> {
        let directory = self.home.session_console_dir(session);
        self.home.create_owner_dir(&directory)?;
        self.home
            .open_owner_file(&self.home.session_output_path(session), false)?;
        Ok(OutputCapture {
            recorder: std::env::current_exe().context("resolve Feld output recorder")?,
            record: self.home.session_output_path(session),
            stdout_fifo: self.home.session_stdout_fifo(session),
            stderr_fifo: self.home.session_stderr_fifo(session),
            done: self.home.session_output_done(session),
        })
    }

    pub(super) fn await_output_capture(&self, session: &Session) {
        if session.execution.execution_type != "program" {
            return;
        }
        let done = self.home.session_output_done(&session.id);
        let output = self.home.session_output_path(&session.id);
        let started = std::time::Instant::now();
        let deadline = started + Duration::from_secs(5);
        while !done.is_file() && std::time::Instant::now() < deadline {
            if started.elapsed() >= Duration::from_millis(100)
                && output.metadata().is_ok_and(|metadata| metadata.len() == 0)
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Notifies readers when a `program` Session's captured output grew since the
    /// previous poll. A Session seen for the first time counts as having had
    /// nothing, so output written before the first poll still wakes readers, and a
    /// closed Session drops out of the cache. A Runtime Session has no such poll:
    /// its records grow when the Runtime Adapter's helper reaches
    /// `POST /v1/self/runtime`, and that arrival is the cursor
    /// change core notifies.
    pub(super) fn notify_console_growth(&self, session: &Session) {
        if session.execution.execution_type != "program" {
            return;
        }
        let mut lengths = self
            .console_lengths
            .lock()
            .expect("Console length cache poisoned");
        if session.end.is_some() {
            lengths.remove(&session.id);
            return;
        }
        let Some(length) = file_length(&self.home.session_output_path(&session.id)) else {
            return;
        };
        let previous = lengths.insert(session.id.clone(), length);
        drop(lengths);
        if previous != Some(length) {
            self.store
                .notify("log", &session.id, Some(&session.workspace));
        }
    }

    /// Drops the Console cursors of Sessions that are no longer open, whichever
    /// way they closed.
    pub(super) fn forget_closed_console_cursors(&self, open: &HashSet<String>) {
        self.console_lengths
            .lock()
            .expect("Console length cache poisoned")
            .retain(|session, _| open.contains(session));
    }
}

/// The files core's capture helper uses for one `program`.
pub(super) struct OutputCapture {
    recorder: PathBuf,
    record: PathBuf,
    stdout_fifo: PathBuf,
    stderr_fifo: PathBuf,
    done: PathBuf,
}

/// The command a `program` runs so that the Console can show its output: the capture helper
/// reads two FIFOs into the output record while the command itself replaces the wrapper, so
/// the Provider still sees one process with the command's own exit status.
pub(super) fn capture_wrapper(capture: &OutputCapture, command: &[String]) -> Vec<String> {
    let quote = |value: &Path| process::quote(&value.to_string_lossy());
    let stdout = quote(&capture.stdout_fifo);
    let stderr = quote(&capture.stderr_fifo);
    let mut script = format!(
        "{CLEAR_CLAUDE_CODE_ENVIRONMENT}; mkfifo {stdout} {stderr}; nohup {} program-capture --output {} --stdout-fifo {stdout} --stderr-fifo {stderr} --done {} </dev/null >/dev/null 2>&1 & exec 3>{stdout} 4>{stderr}; exec",
        quote(&capture.recorder),
        quote(&capture.record),
        quote(&capture.done),
    );
    for argument in command {
        script.push(' ');
        script.push_str(&process::quote(argument));
    }
    script.push_str(&format!(" 3>&- 4>&- >{stdout} 2>{stderr}"));
    vec!["/bin/sh".to_owned(), "-c".to_owned(), script]
}

/// Clears what a Claude Code the daemon or the Provider's server was started from puts in
/// its child processes' environment: every `CLAUDE*` variable, the parent markers among them,
/// except the configuration root the daemon was given. The Provider only sets variables, so
/// they are cleared where the program runs.
const CLEAR_CLAUDE_CODE_ENVIRONMENT: &str = "for name in $(env | sed -n 's/^\\(CLAUDE[A-Za-z0-9_]*\\)=.*/\\1/p'); do [ \"$name\" = CLAUDE_CONFIG_DIR ] || unset \"$name\"; done";

fn file_length(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|metadata| metadata.len())
}
