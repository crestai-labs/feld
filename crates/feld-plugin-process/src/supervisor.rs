//! The process that stays beside a launched program: it starts the program, tells the launch
//! the program's pid, waits for the program, and records its exit code where `observe`
//! reads it. It runs in a session of its own, so it outlives the launch, the daemon, and
//! the terminal the daemon was started from.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus, Stdio};

use serde::{Deserialize, Serialize};

use feld_core::home::{
    FELD_ACTOR_ENV, FELD_SESSION_ID_ENV, FELD_SESSION_TOKEN_ENV, FELD_WORKSPACE_ENV,
};

/// The operation a launch starts this executable with: `supervise <state directory>`.
pub const OPERATION: &str = "supervise";

/// The file in the state directory that holds how the program ended.
pub const EXIT_FILE: &str = "exit";

/// What the launch hands the supervisor on its stdin. The environment carries the Session
/// token, so it travels on a pipe rather than on a command line or in a file.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub command: Vec<String>,
    pub env: Vec<(String, String)>,
    pub workdir: PathBuf,
}

/// The one line the supervisor answers the launch with on its stdout.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Started {
    Pid(u32),
    Error(String),
}

/// How the program ended, as `observe` reads it.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitRecord {
    pub exit_code: i32,
}

/// The exit code of a status, with a signal read as a shell reads it: 128 + its number.
pub fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(-1)
}

pub fn run(mut arguments: impl Iterator<Item = String>) -> ExitCode {
    let Some(state) = arguments.next().map(PathBuf::from) else {
        eprintln!("usage: feld-plugin-process {OPERATION} <state directory>");
        return ExitCode::from(2);
    };
    let mut input = String::new();
    let request = std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| format!("read the launch request: {error}"))
        .and_then(|_| {
            serde_json::from_str::<Request>(&input)
                .map_err(|error| format!("decode the launch request: {error}"))
        });
    let child = request.and_then(|request| start(&request));
    let answer = match &child {
        Ok(child) => Started::Pid(child.id()),
        Err(error) => Started::Error(error.clone()),
    };
    let mut stdout = std::io::stdout();
    let _ = writeln!(
        stdout,
        "{}",
        serde_json::to_string(&answer).expect("encode the start")
    );
    let _ = stdout.flush();
    // The launch reads up to the end of this pipe, and waits on nothing that outlives it.
    release_stdout();
    let Ok(mut child) = child else {
        return ExitCode::FAILURE;
    };
    let code = match child.wait() {
        Ok(status) => exit_code(status),
        Err(_) => return ExitCode::FAILURE,
    };
    match record(&state, code) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

/// Starts the program in the workdir with the environment the launch gave, and nothing
/// that names one Session from the supervisor's own environment.
fn start(request: &Request) -> Result<std::process::Child, String> {
    let (program, arguments) = request
        .command
        .split_first()
        .ok_or_else(|| "the launch request holds no command".to_owned())?;
    let mut command = feld_core::process::command(program);
    // A shell reads where it starts from PWD, which names the daemon's directory otherwise.
    command
        .args(arguments)
        .current_dir(&request.workdir)
        .env("PWD", &request.workdir);
    for name in [
        FELD_SESSION_TOKEN_ENV,
        FELD_SESSION_ID_ENV,
        FELD_ACTOR_ENV,
        FELD_WORKSPACE_ENV,
    ] {
        command.env_remove(name);
    }
    command
        .envs(request.env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn().map_err(|error| {
        if !request.workdir.is_dir() {
            format!(
                "the workdir {} cannot be entered: {error}",
                request.workdir.display()
            )
        } else {
            format!("failed to start {program}: {error}")
        }
    })
}

fn release_stdout() {
    if let Ok(null) = std::fs::OpenOptions::new().write(true).open("/dev/null") {
        // SAFETY: dup2 takes two open descriptors and has no memory effects.
        unsafe { libc::dup2(null.as_raw_fd(), libc::STDOUT_FILENO) };
    }
}

/// Writes the exit record beside its place and renames it over, so that `observe` reads
/// either no record or a whole one.
pub fn record(state: &Path, exit_code: i32) -> std::io::Result<()> {
    let temporary = state.join(format!(".{EXIT_FILE}.tmp"));
    std::fs::write(
        &temporary,
        serde_json::to_vec(&ExitRecord { exit_code }).expect("encode the exit record"),
    )?;
    std::fs::rename(&temporary, state.join(EXIT_FILE))
}

/// The exit record of a state directory, or None while the program has not ended.
pub fn read_record(state: &Path) -> std::io::Result<Option<ExitRecord>> {
    match std::fs::read(state.join(EXIT_FILE)) {
        Ok(contents) => serde_json::from_slice(&contents)
            .map(Some)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
