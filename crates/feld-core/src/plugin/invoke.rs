//! Running one operation of a Plugin.
//!
//! `<dir>/plugin <operation>` reads the request from stdin and writes the response to
//! stdout, both as one JSON object. Exit 0 means the Plugin answered, whatever the answer
//! says; any other exit means it could not answer and stderr holds the reason. A Plugin
//! that has not exited by the deadline is killed together with the processes of its process
//! group, and counts as one that could not answer.

use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::home::{FELD_API_ENV, FELD_HOME_ENV, FELD_PLUGIN_DIR_ENV};

/// What every Plugin finds in its environment on top of the caller's own.
#[derive(Clone, Debug)]
pub struct PluginEnvironment {
    pub home: PathBuf,
    pub api: Option<String>,
}

/// How long one operation may run before core kills the Plugin.
pub const CALL_DEADLINE: Duration = Duration::from_secs(30);

/// One Plugin directory, ready to be called.
#[derive(Clone, Debug)]
pub struct PluginProcess {
    pub name: String,
    pub directory: PathBuf,
    pub environment: PluginEnvironment,
    pub deadline: Duration,
}

impl PluginProcess {
    pub fn executable(&self) -> PathBuf {
        self.directory.join(super::manifest::EXECUTABLE_FILE)
    }

    pub fn call<Request: Serialize, Response: DeserializeOwned>(
        &self,
        operation: &str,
        request: &Request,
    ) -> Result<Response> {
        let answer = self.call_json(
            operation,
            &serde_json::to_value(request).context("encode Plugin request")?,
        )?;
        serde_json::from_value(answer)
            .with_context(|| format!("decode the {operation} response of Plugin '{}'", self.name))
    }

    pub fn call_json(&self, operation: &str, request: &Value) -> Result<Value> {
        let output = self
            .spawn(operation, request)
            .with_context(|| format!("run {} {operation}", self.executable().display()))?;
        if !output.status.success() {
            let reason = String::from_utf8_lossy(&output.stderr);
            let reason = reason.trim();
            bail!(
                "Plugin '{}' could not answer {operation}{}{}",
                self.name,
                if reason.is_empty() { "" } else { ": " },
                reason
            );
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let answer: Value = serde_json::from_str(stdout.trim()).with_context(|| {
            format!(
                "Plugin '{}' answered {operation} with something other than JSON",
                self.name
            )
        })?;
        if !answer.is_object() {
            bail!(
                "Plugin '{}' answered {operation} with a {} instead of a JSON object",
                self.name,
                json_type(&answer)
            );
        }
        Ok(answer)
    }

    fn spawn(&self, operation: &str, request: &Value) -> io::Result<Output> {
        let mut command = crate::process::command(self.executable());
        command
            .arg(operation)
            .current_dir(&self.directory)
            .env(FELD_HOME_ENV, &self.environment.home)
            .env(FELD_PLUGIN_DIR_ENV, &self.directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        match &self.environment.api {
            Some(api) => command.env(FELD_API_ENV, api),
            None => command.env_remove(FELD_API_ENV),
        };
        let mut child = command.spawn()?;
        let body = serde_json::to_vec(request).expect("a JSON value re-encodes");
        let exchanged = exchange(&mut child, &body, self.deadline);
        // The Plugin leads its own group and is not reaped yet, so the group is still its own.
        // What it started waits on it, such as a client of a server that does not answer, and
        // goes with it. A process that left the group, such as a server that detached itself,
        // stays.
        if exchanged.is_err() && matches!(child.try_wait(), Ok(None)) {
            // SAFETY: kill has no memory effects.
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = child.wait();
        }
        exchanged
    }
}

/// Writes `input` to the child's stdin and collects its stdout and stderr until it exits.
/// The answer is complete once the child has exited, even while a process it started still
/// holds its stdout or stderr open. A child that has not exited within `limit` answers
/// `TimedOut`, and is left running and unreaped for the caller to kill.
pub(crate) fn exchange(child: &mut Child, input: &[u8], limit: Duration) -> io::Result<Output> {
    let deadline = Instant::now() + limit;
    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    for descriptor in [
        stdin.as_ref().map(AsRawFd::as_raw_fd),
        stdout.as_ref().map(AsRawFd::as_raw_fd),
        stderr.as_ref().map(AsRawFd::as_raw_fd),
    ]
    .into_iter()
    .flatten()
    {
        set_nonblocking(descriptor)?;
    }
    let mut remaining = input;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    loop {
        if let Some(pipe) = stdin.as_mut() {
            match pipe.write(remaining) {
                Ok(written) => remaining = &remaining[written..],
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                // The Plugin closed its stdin without reading all of it.
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => remaining = &[],
                Err(error) => return Err(error),
            }
            if remaining.is_empty() {
                stdin = None;
            }
        }
        drain(&mut stdout, &mut out)?;
        drain(&mut stderr, &mut err)?;
        if let Some(status) = child.try_wait()? {
            drain(&mut stdout, &mut out)?;
            drain(&mut stderr, &mut err)?;
            return Ok(Output {
                status,
                stdout: out,
                stderr: err,
            });
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("no answer within {} seconds", limit.as_secs_f64()),
            ));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Reads what the pipe holds now, and forgets the pipe at its end.
fn drain(pipe: &mut Option<impl Read>, buffer: &mut Vec<u8>) -> io::Result<()> {
    let Some(reader) = pipe.as_mut() else {
        return Ok(());
    };
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => {
                *pipe = None;
                return Ok(());
            }
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

fn set_nonblocking(descriptor: std::os::fd::RawFd) -> io::Result<()> {
    // SAFETY: fcntl on a descriptor this process owns has no memory effects.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Whether a directory holds a Plugin core can run.
pub fn has_executable(directory: &Path) -> bool {
    let path = directory.join(super::manifest::EXECUTABLE_FILE);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(&path)
            .is_ok_and(|data| data.is_file() && data.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    path.is_file()
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(script: &str, deadline: Duration) -> (tempfile::TempDir, PluginProcess) {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory
            .path()
            .join(super::super::manifest::EXECUTABLE_FILE);
        std::fs::write(
            &executable,
            format!("#!/bin/sh\n[ -z \"$FELD_TEST_WARM_UP\" ] || exit 0\n{script}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        // macOS inspects an executable the first time it runs, which takes seconds while the
        // machine builds and runs others. Running it once here keeps that inspection out of
        // the deadline a test measures.
        let _ = std::process::Command::new(&executable)
            .env("FELD_TEST_WARM_UP", "1")
            .status();
        let process = PluginProcess {
            name: "test".to_owned(),
            directory: directory.path().to_path_buf(),
            environment: PluginEnvironment {
                home: directory.path().to_path_buf(),
                api: None,
            },
            deadline,
        };
        (directory, process)
    }

    #[test]
    fn the_answer_arrives_while_a_process_the_plugin_started_holds_its_stdout() {
        let (_directory, process) = plugin(
            "cat >/dev/null\nsleep 30 &\necho '{\"ok\": true}'",
            Duration::from_secs(10),
        );
        let started = Instant::now();
        let answer = process
            .call_json("observe", &serde_json::json!({}))
            .unwrap();
        assert_eq!(answer, serde_json::json!({"ok": true}));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_plugin_that_does_not_answer_by_the_deadline_is_killed() {
        let (directory, process) = plugin(
            "echo $$ > pid\nkill -STOP $$\necho '{}'",
            Duration::from_secs(3),
        );
        let started = Instant::now();
        let error = process
            .call_json("observe", &serde_json::json!({}))
            .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(format!("{error:#}").contains("no answer within 3 seconds"));
        let pid = std::fs::read_to_string(directory.path().join("pid")).unwrap();
        assert_eq!(
            crate::process::field(pid.trim().parse().unwrap(), "pid=").unwrap(),
            None
        );
    }

    #[test]
    fn what_a_plugin_started_goes_with_it_at_the_deadline() {
        let (directory, process) =
            plugin("sleep 300 &\necho $! > child\nwait", Duration::from_secs(3));
        let error = process
            .call_json("observe", &serde_json::json!({}))
            .unwrap_err();
        assert!(format!("{error:#}").contains("no answer within 3 seconds"));
        let pid: u32 = std::fs::read_to_string(directory.path().join("child"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // The child is no longer the Plugin's, so whoever adopts it reaps it a moment later.
        let deadline = Instant::now() + Duration::from_secs(5);
        while crate::process::field(pid, "pid=").unwrap().is_some() {
            assert!(Instant::now() < deadline, "process {pid} is still there");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
