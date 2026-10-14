//! The environment a launch hands the program. The environment carries the Session token.
//! The command line of the product's client or of the pane's shell is one `ps` shows every
//! user of the machine, and a file in the temporary directory is one a sandboxed Runtime of
//! another Session can read and write. So the Provider keeps the environment in its own
//! memory and hands it over a Unix domain socket to the launch script of that pane alone.
//!
//! The launch script runs the Plugin executable as [`OPERATION`], which connects to the
//! socket and prints the environment as shell statements for the script to `eval`. Each
//! side checks the other: the Provider answers only a peer of its own user that descends
//! from the pane's process, and the receiver prints only what a peer with the Provider's
//! pid sent.

use std::fs::DirBuilder;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::process::quote;

/// The Plugin operation the launch script takes its environment with.
pub const OPERATION: &str = "launch-env";

/// What ends the environment on the socket, so that a receiver tells a whole one from a
/// connection that closed early.
const END: u8 = 0;

/// The longest socket path every platform binds: macOS keeps 104 bytes with the NUL.
const SOCKET_PATH_LIMIT: usize = 103;

/// The directory a socket goes under when the temporary directory's path leaves no room for
/// one. The socket's own directory is owner-only, and each side checks the other's pid, so
/// a directory every user writes is as good a place.
const SHORT_BASE: &str = "/tmp";

/// The environment of one launch, waiting on its socket for the launch script.
#[derive(Debug)]
pub struct LaunchEnv {
    directory: PathBuf,
    listener: UnixListener,
    statements: String,
    receiver: [String; 4],
}

impl LaunchEnv {
    /// Opens the socket for `env`, whose names a shell can assign. `program` is the Plugin
    /// executable the launch script runs as the receiver.
    pub fn new(env: &[(String, String)], program: &Path) -> io::Result<Self> {
        Self::under(&std::env::temp_dir(), env, program)
    }

    fn under(temporary: &Path, env: &[(String, String)], program: &Path) -> io::Result<Self> {
        let name = format!("feld-env-{}", &Uuid::new_v4().simple().to_string()[..16]);
        let fits = |base: &Path| base.join(&name).join("s").as_os_str().len() <= SOCKET_PATH_LIMIT;
        let base = if fits(temporary) {
            temporary
        } else {
            Path::new(SHORT_BASE)
        };
        let directory = base.join(name);
        DirBuilder::new().mode(0o700).create(&directory)?;
        let socket = directory.join("s");
        let listener = match bind(&socket) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&directory);
                return Err(error);
            }
        };
        let statements = env
            .iter()
            .map(|(name, value)| format!("export {name}={}\n", quote(value)))
            .collect();
        Ok(Self {
            directory,
            listener,
            statements,
            receiver: [
                program.to_string_lossy().into_owned(),
                OPERATION.to_owned(),
                socket.to_string_lossy().into_owned(),
                std::process::id().to_string(),
            ],
        })
    }

    /// The shell command that puts the environment in place. A shell whose receiver fails
    /// exits there.
    pub fn source(&self) -> String {
        let receiver = self
            .receiver
            .iter()
            .map(|word| quote(word))
            .collect::<Vec<_>>()
            .join(" ");
        format!("eval \"$({receiver} || echo exit 1)\"")
    }

    /// The argv of the receiver that [`source`](Self::source) runs, which is a child of the
    /// launch script's shell for as long as it takes.
    pub fn receiver(&self) -> &[String] {
        &self.receiver
    }

    /// Hands the environment to the receiver that `pane`, the pane's own process, runs, and
    /// closes every other connection. A receiver that has not come by `deadline` fails the
    /// launch.
    pub fn hand_over(&self, pane: u32, deadline: Instant) -> io::Result<()> {
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    // A peer that cannot be read is no reason for another process to stop
                    // the launch.
                    if !matches!(from_pane(&stream, pane), Ok(true)) {
                        continue;
                    }
                    stream.set_nonblocking(false)?;
                    stream.set_write_timeout(Some(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .max(Duration::from_millis(100)),
                    ))?;
                    stream.write_all(self.statements.as_bytes())?;
                    return stream.write_all(&[END]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "the launch script did not take its environment",
                        ));
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for LaunchEnv {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn bind(socket: &Path) -> io::Result<UnixListener> {
    let listener = UnixListener::bind(socket)?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// Whether the peer is a process of this user that descends from `pane`.
fn from_pane(stream: &UnixStream, pane: u32) -> io::Result<bool> {
    let peer = peer(stream)?;
    Ok(peer.uid == own_uid() && peer.pid != 0 && crate::process::is_descendant(peer.pid, pane)?)
}

/// [`OPERATION`]: prints the environment the Provider whose pid `args` names sends on the
/// socket `args` names.
pub fn receive(mut args: impl Iterator<Item = String>) -> ExitCode {
    let (Some(socket), Some(provider), None) = (args.next(), args.next(), args.next()) else {
        eprintln!("usage: {OPERATION} <socket> <provider pid>");
        return ExitCode::from(2);
    };
    match received(Path::new(&socket), &provider) {
        Ok(statements) => match io::stdout().write_all(&statements) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{OPERATION}: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("{OPERATION}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn received(socket: &Path, provider: &str) -> io::Result<Vec<u8>> {
    let provider: u32 = provider
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "not a pid"))?;
    let mut stream = UnixStream::connect(socket)?;
    let peer = peer(&stream)?;
    if peer.pid != provider || peer.uid != own_uid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the socket is not the launching Provider's",
        ));
    }
    let mut statements = Vec::new();
    stream.read_to_end(&mut statements)?;
    if statements.pop() != Some(END) {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the Provider closed the socket before the whole environment",
        ));
    }
    Ok(statements)
}

struct Peer {
    pid: u32,
    uid: u32,
}

fn own_uid() -> u32 {
    // SAFETY: geteuid has no memory effects.
    unsafe { libc::geteuid() }
}

#[cfg(target_os = "macos")]
fn peer(stream: &UnixStream) -> io::Result<Peer> {
    let fd = stream.as_raw_fd();
    let mut pid: libc::pid_t = 0;
    let mut length = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: pid and length are valid for the writes getsockopt makes.
    let read = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&raw mut pid).cast(),
            &raw mut length,
        )
    };
    if read != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: uid and gid are valid for the writes getpeereid makes.
    if unsafe { libc::getpeereid(fd, &raw mut uid, &raw mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Peer {
        pid: pid as u32,
        uid,
    })
}

#[cfg(target_os = "linux")]
fn peer(stream: &UnixStream) -> io::Result<Peer> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: credentials and length are valid for the writes getsockopt makes.
    let read = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    };
    if read != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Peer {
        pid: credentials.pid as u32,
        uid: credentials.uid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_the_environment_only_to_a_peer_under_the_pane() {
        let env = LaunchEnv::new(
            &[("FELD_VALUE".to_owned(), "a'b $(touch nope)".to_owned())],
            Path::new("unused"),
        )
        .unwrap();
        let socket = PathBuf::from(&env.receiver()[2]);
        // The socket is in a directory only its owner reaches, and the receiver's argv
        // carries no value of the environment.
        let mode = std::fs::metadata(socket.parent().unwrap()).unwrap();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777,
            0o700
        );
        assert!(!env.source().contains("touch nope"), "{}", env.source());

        // A peer that is not under the pane is closed and the wait goes on to the deadline.
        let stranger = thread::spawn({
            let socket = socket.clone();
            move || {
                let mut stream = UnixStream::connect(&socket).unwrap();
                let mut read = Vec::new();
                let _ = stream.read_to_end(&mut read);
                read
            }
        });
        let elsewhere = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let refused = env.hand_over(elsewhere.id(), Instant::now() + Duration::from_millis(200));
        assert_eq!(refused.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(stranger.join().unwrap().is_empty());
        kill(elsewhere);

        // This process descends from itself, so a receiver here takes the environment.
        let provider = std::process::id().to_string();
        let receiver = thread::spawn({
            let socket = socket.clone();
            move || received(&socket, &provider)
        });
        env.hand_over(std::process::id(), Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            String::from_utf8(receiver.join().unwrap().unwrap()).unwrap(),
            "export FELD_VALUE='a'\\''b $(touch nope)'\n"
        );

        // The directory goes with the environment.
        drop(env);
        assert!(!socket.parent().unwrap().exists());
    }

    #[test]
    fn a_temporary_directory_too_long_for_a_socket_leaves_it_under_a_short_base() {
        let long = std::env::temp_dir().join("x".repeat(SOCKET_PATH_LIMIT));
        let env = LaunchEnv::under(&long, &[], Path::new("unused")).unwrap();
        let socket = PathBuf::from(&env.receiver()[2]);
        assert!(socket.starts_with(SHORT_BASE), "{}", socket.display());
        assert!(socket.as_os_str().len() <= SOCKET_PATH_LIMIT);
        let provider = std::process::id().to_string();
        let receiver = thread::spawn(move || received(&socket, &provider));
        env.hand_over(std::process::id(), Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(receiver.join().unwrap().unwrap(), b"");
    }

    #[test]
    fn the_receiver_refuses_a_socket_another_process_listens_on() {
        let env = LaunchEnv::new(&[], Path::new("unused")).unwrap();
        let socket = PathBuf::from(&env.receiver()[2]);
        let elsewhere = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let refused = received(&socket, &elsewhere.id().to_string()).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied);
        kill(elsewhere);
    }

    #[test]
    fn the_receiver_refuses_an_environment_cut_short() {
        let env = LaunchEnv::new(&[], Path::new("unused")).unwrap();
        let socket = PathBuf::from(&env.receiver()[2]);
        let provider = std::process::id().to_string();
        let receiver = thread::spawn(move || received(&socket, &provider));
        // The Provider gave up before it wrote anything.
        let accepted = loop {
            match env.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(_) => thread::sleep(Duration::from_millis(5)),
            }
        };
        drop(accepted);
        assert_eq!(
            receiver.join().unwrap().unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    fn kill(mut child: std::process::Child) {
        let _ = child.kill();
        let _ = child.wait();
    }
}
