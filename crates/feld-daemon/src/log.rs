//! The daemon log: `FELD_HOME/daemon.log` and the generations it rotates into, kept to a
//! bounded size while the daemon runs.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use feld_core::home::FeldHome;

/// The size at which the current file is rotated.
pub const LOG_LIMIT: u64 = 10 * 1024 * 1024;
/// How many files the log keeps: the current one and `daemon.log.1` to `daemon.log.4`.
pub const LOG_FILES: usize = 5;
/// How often the running daemon looks at the size of the current file.
const LOG_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// The `generation`-th file the log was rotated into, `daemon.log.1` being the newest.
pub fn generation_path(path: &Path, generation: usize) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{generation}"));
    PathBuf::from(name)
}

/// Rotates the log at `path` into `files` files: the oldest generation goes, each one
/// moves one up, and the current file becomes `.1`.
pub fn rotate_generations(path: &Path, files: usize) -> Result<()> {
    let oldest = generation_path(path, files - 1);
    match fs::remove_file(&oldest) {
        Err(error) if error.kind() != ErrorKind::NotFound => {
            return Err(error).with_context(|| format!("remove {}", oldest.display()));
        }
        _ => {}
    }
    for generation in (1..files - 1).rev() {
        let from = generation_path(path, generation);
        let to = generation_path(path, generation + 1);
        match fs::rename(&from, &to) {
            Err(error) if error.kind() != ErrorKind::NotFound => {
                return Err(error).with_context(|| format!("rotate {}", from.display()));
            }
            _ => {}
        }
    }
    let newest = generation_path(path, 1);
    fs::rename(path, &newest).with_context(|| format!("rotate {}", path.display()))
}

/// Keeps the daemon log rotated while this process runs, when its stdout and stderr are
/// the current file, as `feld daemon start` and `restart` give them. A rotation opens a
/// new current file and points stdout and stderr at it.
#[cfg(unix)]
pub fn keep_rotated(home: &FeldHome) -> Result<()> {
    let path = home.log_path();
    if !stderr().is_some_and(|stderr| stderr.is(&path)) {
        return Ok(());
    }
    rotate(home, &path)?;
    let home = home.clone();
    std::thread::Builder::new()
        .name("daemon-log".to_owned())
        .spawn(move || {
            // A rotation that keeps failing the same way is logged once.
            let mut failing = None;
            loop {
                std::thread::sleep(LOG_CHECK_INTERVAL);
                match rotate(&home, &path) {
                    Ok(()) => failing = None,
                    Err(error) => {
                        let error = format!("{error:#}");
                        if failing.as_ref() != Some(&error) {
                            eprintln!("Feld daemon: the log was not rotated: {error}");
                            failing = Some(error);
                        }
                    }
                }
            }
        })
        .context("start daemon log rotation")?;
    Ok(())
}

#[cfg(not(unix))]
pub fn keep_rotated(_home: &FeldHome) -> Result<()> {
    Ok(())
}

/// Rotates once the file stderr writes to has reached the limit. That file is measured
/// rather than `path`, so a current file removed by hand, or one rotated whose successor
/// could not be opened, is replaced by a new current file all the same.
#[cfg(unix)]
fn rotate(home: &FeldHome, path: &Path) -> Result<()> {
    use std::os::fd::AsRawFd;

    let Some(stderr) = stderr() else {
        return Ok(());
    };
    if stderr.size < LOG_LIMIT {
        return Ok(());
    }
    if stderr.is(path) {
        rotate_generations(path, LOG_FILES)?;
    }
    let current = home.open_owner_file(path, true)?;
    for target in [libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        // SAFETY: both descriptors are open; dup2 replaces the target atomically.
        if unsafe { libc::dup2(current.as_raw_fd(), target) } < 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("write to {}", path.display()));
        }
    }
    Ok(())
}

/// The file stderr writes to.
#[cfg(unix)]
struct Stderr {
    device: u64,
    inode: u64,
    size: u64,
}

#[cfg(unix)]
impl Stderr {
    /// Whether this is the file at `path`.
    fn is(&self, path: &Path) -> bool {
        use std::os::unix::fs::MetadataExt;

        fs::metadata(path).is_ok_and(|file| file.dev() == self.device && file.ino() == self.inode)
    }
}

#[cfg(unix)]
fn stderr() -> Option<Stderr> {
    // SAFETY: fstat writes into the zeroed struct it is given.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(libc::STDERR_FILENO, &mut stat) } != 0 {
        return None;
    }
    Some(Stderr {
        device: stat.st_dev as u64,
        inode: stat.st_ino as u64,
        size: stat.st_size as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn a_full_log_moves_every_generation_up_and_drops_the_oldest() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("daemon.log");
        for generation in 1..=4 {
            fs::write(
                generation_path(&path, generation),
                format!("gen-{generation}"),
            )
            .unwrap();
        }
        fs::write(&path, "current!").unwrap();

        rotate_generations(&path, 5).unwrap();
        assert!(!path.exists());
        assert_eq!(read(&generation_path(&path, 1)), "current!");
        assert_eq!(read(&generation_path(&path, 2)), "gen-1");
        assert_eq!(read(&generation_path(&path, 3)), "gen-2");
        assert_eq!(read(&generation_path(&path, 4)), "gen-3");
        assert!(!generation_path(&path, 5).exists());
    }

    #[test]
    fn a_log_without_generations_rotates_into_the_first() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("daemon.log");
        fs::write(&path, "one").unwrap();
        rotate_generations(&path, 5).unwrap();
        assert_eq!(read(&generation_path(&path, 1)), "one");
        assert!(!generation_path(&path, 2).exists());
    }
}
