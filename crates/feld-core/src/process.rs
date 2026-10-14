//! What Feld knows about an OS process without a Plugin: the incarnation that tells one
//! process from another that reused its pid, the snapshot a Plugin receives as `process`,
//! the ancestry the token resolution walks, the `Command` every spawn starts from, and the
//! quoting of the words a shell reads.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Command, Output};

use serde::{Deserialize, Serialize};

/// The variables a Claude Code puts in its child processes' environment to tell them it is
/// their parent. A daemon or Provider server started from inside a Claude Code hands them on
/// to every Session it launches, where they would name that Claude Code as the Session's
/// parent. The Claude Code Adapter clears them with empty values in the launch environment.
pub const CLAUDE_CODE_PARENT_MARKERS: [&str; 5] = [
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_PID",
];

/// One word a POSIX shell reads back as `value`, always in single quotes, where a single
/// quote itself closes, escapes, and reopens.
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// [`quote`] for a human to read: words made only of characters every shell leaves alone
/// stay bare.
pub fn quote_for_display(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c))
    {
        return word.to_owned();
    }
    quote(word)
}

/// An argv as one line for a human to read: every word carries the quoting a POSIX shell
/// needs to split the line back into the same argv, so the display can be copied into a
/// terminal. The Web UI writes a command the same way.
pub fn command_line(argv: &[String]) -> String {
    argv.iter()
        .map(|word| quote_for_display(word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A `Command` whose child inherits only stdin, stdout and stderr.
///
/// std marks a pipe close-on-exec only after creating it, so a child that another thread
/// spawns in between inherits the pipe and holds it open for as long as it lives, and the
/// reader of that pipe never sees its end. The child here marks every descriptor past stderr
/// close-on-exec before it runs the program.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    // SAFETY: the closure makes system calls only, which is what may run between fork and
    // exec.
    unsafe {
        command.pre_exec(close_inherited_on_exec);
    }
    command
}

#[cfg(target_os = "macos")]
fn close_inherited_on_exec() -> io::Result<()> {
    const CAPACITY: usize = 4096;
    let mut descriptors = [libc::proc_fdinfo {
        proc_fd: 0,
        proc_fdtype: 0,
    }; CAPACITY];
    // SAFETY: the buffer holds the byte count passed, and the kernel writes at most that.
    let size = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            descriptors.as_mut_ptr().cast(),
            std::mem::size_of_val(&descriptors) as libc::c_int,
        )
    };
    if size <= 0 {
        return Err(io::Error::last_os_error());
    }
    let count = size as usize / std::mem::size_of::<libc::proc_fdinfo>();
    if count >= CAPACITY {
        return Err(io::Error::other(format!(
            "more than {CAPACITY} open descriptors to keep from the child"
        )));
    }
    for descriptor in &descriptors[..count] {
        if descriptor.proc_fd > libc::STDERR_FILENO {
            // SAFETY: fcntl on a descriptor number has no memory effects.
            unsafe { libc::fcntl(descriptor.proc_fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn close_inherited_on_exec() -> io::Result<()> {
    // SAFETY: close_range takes plain integers.
    let result = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            (libc::STDERR_FILENO + 1) as libc::c_uint,
            libc::c_uint::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    };
    if result == 0 {
        return Ok(());
    }
    // A kernel before 5.11 knows no close_range or its CLOEXEC flag.
    let error = io::Error::last_os_error();
    if !matches!(error.raw_os_error(), Some(libc::ENOSYS | libc::EINVAL)) {
        return Err(error);
    }
    // SAFETY: sysconf and fcntl take plain integers.
    let limit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
    for descriptor in
        libc::STDERR_FILENO + 1..limit.clamp(0, libc::c_int::MAX as libc::c_long) as libc::c_int
    {
        unsafe { libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn close_inherited_on_exec() -> io::Result<()> {
    Ok(())
}

/// One process, distinguished from a later process that reuses its pid by its start time.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessIncarnation {
    pub pid: u32,
    pub start_time: String,
}

/// The `process` a Plugin receives: the incarnation together with what it is running.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub started_at: String,
    pub executable: String,
    pub argv: Vec<String>,
}

impl ProcessSnapshot {
    pub fn incarnation(&self) -> ProcessIncarnation {
        ProcessIncarnation {
            pid: self.pid,
            start_time: self.started_at.clone(),
        }
    }
}

/// Runs `ps` with `args`. A sandbox around this process, such as the one a Runtime puts
/// around the commands it runs, can refuse to start `ps`; the error then holds a
/// [`TableDenied`], which says how to get out of it.
pub fn ps<I, S>(args: I) -> io::Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    command("ps").args(args).output().map_err(|error| {
        if error.kind() == io::ErrorKind::PermissionDenied {
            io::Error::new(error.kind(), TableDenied(error))
        } else {
            error
        }
    })
}

/// A sandbox around this process denied it reading the process table.
#[derive(Debug)]
pub struct TableDenied(io::Error);

impl std::fmt::Display for TableDenied {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}; a sandbox around this process denies reading the process table, \
             so start the Runtime without its sandbox and run the command again",
            self.0
        )
    }
}

impl std::error::Error for TableDenied {}

/// Whether `error` comes of a sandbox that denies this process reading the process table.
pub fn table_denied(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause.is::<TableDenied>()
            || cause
                .downcast_ref::<io::Error>()
                .and_then(io::Error::get_ref)
                .is_some_and(|inner| inner.is::<TableDenied>())
    })
}

/// One `ps` field of a process, or None when the process is gone.
pub fn field(pid: u32, field: &str) -> io::Result<Option<String>> {
    let output = ps(["-o", field, "-p", &pid.to_string()])?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!value.is_empty()).then_some(value))
}

pub fn incarnation(pid: u32) -> io::Result<Option<ProcessIncarnation>> {
    Ok(field(pid, "lstart=")?.map(|start_time| ProcessIncarnation { pid, start_time }))
}

pub fn matches(expected: &ProcessIncarnation) -> io::Result<bool> {
    Ok(incarnation(expected.pid)?.as_ref() == Some(expected))
}

pub fn snapshot(pid: u32) -> io::Result<Option<ProcessSnapshot>> {
    let (Some(started_at), Some(executable)) = (field(pid, "lstart=")?, field(pid, "comm=")?)
    else {
        return Ok(None);
    };
    let argv = match arguments(pid) {
        Some(argv) => argv,
        // `ps` joins the words with spaces, so a word that holds one comes back split.
        None => match field(pid, "args=")? {
            Some(argv) => argv.split_whitespace().map(str::to_owned).collect(),
            None => return Ok(None),
        },
    };
    Ok(Some(ProcessSnapshot {
        pid,
        started_at,
        executable,
        argv,
    }))
}

/// The argv a process started with, word by word, or None when it cannot be read (the
/// process is gone or belongs to another user). Linux reads `/proc/<pid>/cmdline`, macOS the
/// `KERN_PROCARGS2` sysctl.
#[cfg(target_os = "linux")]
fn arguments(pid: u32) -> Option<Vec<String>> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let mut words: Vec<String> = cmdline
        .split(|byte| *byte == 0)
        .map(|word| String::from_utf8_lossy(word).into_owned())
        .collect();
    // The NUL that ends the last word; a kernel thread or a zombie has no words at all.
    if words.last().is_some_and(String::is_empty) {
        words.pop();
    }
    (!words.is_empty()).then_some(words)
}

#[cfg(target_os = "macos")]
fn arguments(pid: u32) -> Option<Vec<String>> {
    let (arguments, _) = process_arguments(pid)?;
    (!arguments.is_empty()).then_some(arguments)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn arguments(_pid: u32) -> Option<Vec<String>> {
    None
}

/// The environment a process started with, or None when it cannot be read (the process is
/// gone or belongs to another user). Linux reads `/proc/<pid>/environ`, macOS the
/// `KERN_PROCARGS2` sysctl.
pub fn environment(pid: u32) -> Option<BTreeMap<String, String>> {
    let block = environment_block(pid)?;
    Some(
        block
            .split(|byte| *byte == 0)
            .filter_map(|entry| {
                let entry = String::from_utf8_lossy(entry);
                let (key, value) = entry.split_once('=')?;
                (!key.is_empty()).then(|| (key.to_owned(), value.to_owned()))
            })
            .collect(),
    )
}

#[cfg(target_os = "linux")]
fn environment_block(pid: u32) -> Option<Vec<u8>> {
    std::fs::read(format!("/proc/{pid}/environ")).ok()
}

#[cfg(target_os = "macos")]
fn environment_block(pid: u32) -> Option<Vec<u8>> {
    process_arguments(pid).map(|(_, environment)| environment)
}

/// The argument strings and the environment block of `KERN_PROCARGS2`: argc, the executable
/// path and its padding, argc argument strings, then the environment strings up to an empty
/// one.
#[cfg(target_os = "macos")]
fn process_arguments(pid: u32) -> Option<(Vec<String>, Vec<u8>)> {
    let mut argmax: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    // SAFETY: the buffer is the c_int `size` names.
    let read = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&mut argmax as *mut libc::c_int).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 || argmax <= 0 {
        return None;
    }
    let mut buffer = vec![0u8; argmax as usize];
    let mut size = buffer.len();
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    // SAFETY: the buffer holds `size` bytes, and sysctl writes at most that many.
    let read = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 || size < std::mem::size_of::<libc::c_int>() {
        return None;
    }
    buffer.truncate(size);
    let (argc, rest) = buffer.split_at(std::mem::size_of::<libc::c_int>());
    let argc = libc::c_int::from_ne_bytes(argc.try_into().ok()?).max(0) as usize;
    // The executable path, then the NULs that pad it.
    let mut position = rest.iter().position(|byte| *byte == 0)?;
    while rest.get(position) == Some(&0) {
        position += 1;
    }
    let mut arguments = Vec::with_capacity(argc);
    for _ in 0..argc {
        let length = rest.get(position..)?.iter().position(|byte| *byte == 0)?;
        arguments.push(String::from_utf8_lossy(&rest[position..position + length]).into_owned());
        position += length + 1;
    }
    let environment = rest.get(position..)?;
    let end = environment
        .windows(2)
        .position(|pair| pair == [0, 0])
        .map_or(environment.len(), |end| end + 1);
    Some((arguments, environment[..end].to_vec()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn environment_block(_pid: u32) -> Option<Vec<u8>> {
    None
}

/// Whether `ancestor` is `child` or one of the processes it descends from.
pub fn is_descendant(child: u32, ancestor: u32) -> io::Result<bool> {
    Ok(child == ancestor || find_ancestor(child, |pid| Ok(pid == ancestor))?.is_some())
}

/// The nearest process `pid` descends from that `wanted` accepts, read one parent at a
/// time so that the walk ends at the first one.
pub fn find_ancestor(
    mut pid: u32,
    mut wanted: impl FnMut(u32) -> io::Result<bool>,
) -> io::Result<Option<u32>> {
    for _ in 0..256 {
        let parent = field(pid, "ppid=")?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        if parent == 0 || parent == pid {
            break;
        }
        if wanted(parent)? {
            return Ok(Some(parent));
        }
        pid = parent;
    }
    Ok(None)
}

/// The pids of the processes one process is the parent of, youngest pid last. A pane's
/// program is the child of the shell the Provider left in the pane.
pub fn children(pid: u32) -> io::Result<Vec<u32>> {
    let output = ps(["-Ao", "pid=,ppid="])?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let mut children: Vec<u32> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let child = fields.next()?.parse::<u32>().ok()?;
            let parent = fields.next()?.parse::<u32>().ok()?;
            (parent == pid).then_some(child)
        })
        .collect();
    children.sort_unstable();
    Ok(children)
}

/// The pids of `pid` and every process that descends from it, `pid` first.
pub fn descendants(pid: u32) -> io::Result<Vec<u32>> {
    let output = ps(["-Ao", "pid=,ppid="])?;
    let pairs: Vec<(u32, u32)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
        .collect();
    let mut found = vec![pid];
    let mut next = 0;
    while let Some(&parent) = found.get(next) {
        next += 1;
        found.extend(
            pairs
                .iter()
                .filter(|(child, of)| *of == parent && *child != parent)
                .map(|(child, _)| *child),
        );
    }
    Ok(found)
}

/// The paths of the files the processes hold open, or none for a process that is gone or
/// belongs to another user. Linux reads `/proc/<pid>/fd`, macOS asks `lsof`.
#[cfg(target_os = "linux")]
pub fn open_files(pids: &[u32]) -> io::Result<Vec<std::path::PathBuf>> {
    Ok(pids
        .iter()
        .filter_map(|pid| std::fs::read_dir(format!("/proc/{pid}/fd")).ok())
        .flatten()
        .filter_map(|entry| std::fs::read_link(entry.ok()?.path()).ok())
        .collect())
}

#[cfg(not(target_os = "linux"))]
pub fn open_files(pids: &[u32]) -> io::Result<Vec<std::path::PathBuf>> {
    if pids.is_empty() {
        return Ok(Vec::new());
    }
    let pids: Vec<String> = pids.iter().map(u32::to_string).collect();
    // `lsof` exits 1 when one of the processes is gone, and lists the others all the same.
    let output = command("lsof")
        .args(["-n", "-P", "-w", "-Fn", "-p", &pids.join(",")])
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .map(std::path::PathBuf::from)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A process finds itself among the processes it descends from, and the files it holds
    /// open by their paths.
    #[test]
    fn a_process_holds_its_open_files() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("held.jsonl");
        let _held = std::fs::File::create(&path).unwrap();
        let own = std::process::id();
        assert_eq!(descendants(own).unwrap().first(), Some(&own));
        let path = path.canonicalize().unwrap();
        assert!(
            open_files(&[own])
                .unwrap()
                .iter()
                .any(|open| open.canonicalize().ok().as_ref() == Some(&path)),
            "{path:?}"
        );
    }

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    /// What a shell makes of a line [`command_line`] wrote, one word per NUL.
    fn shell_argv(line: &str) -> Vec<String> {
        let output = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("printf '%s\\0' {line}"))
            .output()
            .expect("the shell runs");
        assert!(output.status.success(), "{line}");
        let printed = String::from_utf8(output.stdout).expect("the words are UTF-8");
        let mut words: Vec<String> = printed.split('\0').map(str::to_owned).collect();
        words.pop();
        words
    }

    #[test]
    fn plain_words_stay_bare() {
        assert_eq!(command_line(&argv(&["echo", "hello"])), "echo hello");
        assert_eq!(
            command_line(&argv(&["python3", "triage.py", "--project", "10"])),
            "python3 triage.py --project 10"
        );
    }

    #[test]
    fn words_a_shell_would_split_or_read_are_quoted() {
        assert_eq!(
            command_line(&argv(&["sh", "-c", "printf \"Hello from Feld\\n\""])),
            "sh -c 'printf \"Hello from Feld\\n\"'"
        );
        assert_eq!(command_line(&argv(&["say", "it's"])), "say 'it'\\''s'");
        assert_eq!(command_line(&argv(&["sh", "-c", ""])), "sh -c ''");
    }

    /// The acceptance the display owes a reader: copying the line into a shell gives back
    /// the argv it was written from.
    #[test]
    fn a_shell_splits_the_line_back_into_the_argv() {
        for words in [
            vec!["echo", "hello"],
            vec!["sh", "-c", "printf \"Hello from Feld\\n\""],
            vec!["sh", "-c", "cd /tmp && ls; echo done"],
            vec!["say", "it's a $HOME `date`"],
            vec!["echo", "", "two  spaces", "tab\there"],
            vec!["echo", "back\\slash", "*", "~", "#comment"],
        ] {
            let expected = argv(&words);
            assert_eq!(shell_argv(&command_line(&expected)), expected);
        }
    }

    #[test]
    fn a_child_inherits_no_descriptor_past_stderr() {
        // A pipe without close-on-exec, as std leaves one for a moment while it spawns.
        let mut ends = [0; 2];
        assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
        // The child itself reports whether it holds the write end, so a child that another
        // test spawns meanwhile and that inherits the pipe leaves the answer alone. The probe
        // is `test` without a shell, as a shell keeps its own descriptors from 10 up while it
        // redirects, where a number past 9 that the pipe gets among the parallel tests finds
        // one of them open.
        let probe = |mut command: Command| {
            command
                .arg("-e")
                .arg(format!("/dev/fd/{}", ends[1]))
                .status()
                .unwrap()
                .success()
        };
        let plain = probe(Command::new("test"));
        let kept = probe(command("test"));
        unsafe { libc::close(ends[0]) };
        unsafe { libc::close(ends[1]) };
        assert!(plain, "a plain Command passes the pipe on");
        assert!(!kept, "the child holds the write end");
    }

    #[test]
    fn the_environment_a_process_started_with_is_read() {
        // macOS hides the environment of its own platform binaries such as `sleep`, so the
        // process read is this test binary, waiting in the test below.
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "process::tests::waits_to_be_read",
                "--ignored",
                "--quiet",
            ])
            .env("FELD_PROCESS_TEST", "one=two")
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // Right after the spawn the child may not have run `exec` yet.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let environment = loop {
            let environment = environment(child.id());
            if environment
                .as_ref()
                .is_some_and(|environment| environment.contains_key("FELD_PROCESS_TEST"))
                || std::time::Instant::now() > deadline
            {
                break environment;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        child.kill().unwrap();
        child.wait().unwrap();
        let environment = environment.expect("a child of this user has a readable environment");
        assert_eq!(environment["FELD_PROCESS_TEST"], "one=two");
        assert!(environment.contains_key("PATH"));
        assert!(!environment.contains_key("--exact"));
    }

    /// A word that holds a space stays one word, as in a Runtime installed under
    /// `~/Library/Application Support`.
    #[test]
    fn the_argv_a_process_started_with_is_read_word_by_word() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("Application Support");
        std::fs::create_dir(&directory).unwrap();
        let program = directory.join("feld process test");
        std::os::unix::fs::symlink(std::env::current_exe().unwrap(), &program).unwrap();
        let program = program.to_str().unwrap().to_owned();
        let mut child = Command::new(&program)
            .args([
                "--exact",
                "process::tests::waits_to_be_read",
                "--ignored",
                "--quiet",
            ])
            .env("FELD_PROCESS_TEST", "argv")
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // Right after the spawn the child may not have run `exec` yet.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let snapshot = loop {
            let snapshot = snapshot(child.id()).unwrap();
            if snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.argv.first() == Some(&program))
                || std::time::Instant::now() > deadline
            {
                break snapshot;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        child.kill().unwrap();
        child.wait().unwrap();
        let snapshot = snapshot.expect("a child of this user has a snapshot");
        assert_eq!(
            snapshot.argv,
            [
                program.as_str(),
                "--exact",
                "process::tests::waits_to_be_read",
                "--ignored",
                "--quiet"
            ]
        );
    }

    #[test]
    #[ignore = "the process the environment and argv tests read"]
    fn waits_to_be_read() {
        if std::env::var_os("FELD_PROCESS_TEST").is_some() {
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    }
}
