//! Stand-in commands: shell scripts a test puts where Feld looks for a product, such as
//! on `PATH`, to answer in its place.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Puts at `path` a stand-in that runs `script`, replacing the one already there.
///
/// The stand-in is a symlink to one runner, and `script` sits beside it as `<path>.sh`.
pub fn write_stand_in(path: &Path, script: &str) {
    let mut script_path = path.as_os_str().to_owned();
    script_path.push(".sh");
    fs::write(script_path, script).expect("write a stand-in");
    if fs::read_link(path).ok().as_deref() != Some(runner()) {
        let _ = fs::remove_file(path);
        std::os::unix::fs::symlink(runner(), path).expect("link a stand-in");
    }
}

/// The one executable every stand-in links to; it runs the script beside the stand-in.
///
/// macOS checks a new executable on its first run, which under the load of the tests
/// outlasts how long Feld waits for a product to tell its version. The runner stays in the
/// target directory across runs and has run once before any stand-in links to it, so a
/// stand-in answers at once.
fn runner() -> &'static Path {
    const SCRIPT: &str = "#!/bin/sh\n. \"$0.sh\"\n";
    static RUNNER: OnceLock<PathBuf> = OnceLock::new();
    RUNNER.get_or_init(|| {
        let runner = Path::new(env!("CARGO_TARGET_TMPDIR")).join("feld-stand-in");
        if !runner.exists() {
            // Another test binary may make and run the runner meanwhile; a hard link never
            // replaces the one it made, so a stand-in keeps the runner that has run.
            let written = runner.with_extension(std::process::id().to_string());
            fs::write(&written, SCRIPT).expect("write the runner");
            fs::set_permissions(&written, fs::Permissions::from_mode(0o700))
                .expect("make the runner runnable");
            match fs::hard_link(&written, &runner) {
                Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
                    panic!("put the runner in place: {error}")
                }
                _ => {}
            }
            let _ = fs::remove_file(&written);
        }
        Command::new(&runner).output().expect("run the runner once");
        runner
    })
}
