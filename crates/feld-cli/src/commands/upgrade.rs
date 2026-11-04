//! `feld upgrade`: install.sh, run in the directory of this feld, then the new feld's
//! `feld init --yes`.

use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::Context as _;

use crate::args::UpgradeArgs;
use crate::{CliResult, Context};

/// The install script of this version. Fetching, checking, and replacing the executables
/// are written there alone, for a first install and an upgrade both.
const INSTALL_SCRIPT: &str = include_str!("../../../../install.sh");

/// The tag a Release archive is built for, which `package.sh` passes to the build. A feld
/// built from source has none.
const RELEASE: Option<&str> = option_env!("FELD_RELEASE");

pub fn run(ctx: &Context, args: UpgradeArgs) -> CliResult<ExitCode> {
    if RELEASE.is_none() {
        return Err(rejected!(
            "this feld was built from source, so feld upgrade leaves it as it is"
        )
        .with_next("pull and build the new source, then run `feld init`"));
    }
    let feld = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .context("find this feld")?;
    if installed_by_homebrew(&feld) {
        return Err(
            rejected!("this feld was installed with Homebrew, which replaces it")
                .with_next("brew upgrade feld && feld init"),
        );
    }
    let directory = feld
        .parent()
        .map(Path::to_path_buf)
        .context("find the directory of this feld")?;

    install(&directory, args.version.as_deref())?;
    let feld = directory.join("feld");
    if !ctx.json {
        let version = new_version(&feld).unwrap_or_else(|| "?".to_owned());
        println!(
            "Upgraded feld {} → {version} in {}",
            env!("CARGO_PKG_VERSION"),
            directory.display()
        );
        println!();
    }

    // The new feld aligns the Feld home, the Runtime settings, and the daemon with itself.
    let mut init = Command::new(&feld);
    init.arg("--home").arg(ctx.home.root());
    if ctx.json {
        init.arg("--json");
    }
    let status = init
        .args(["init", "--yes"])
        .status()
        .with_context(|| format!("run {} init --yes", feld.display()))?;
    Ok(ExitCode::from(
        status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(2),
    ))
}

/// Whether `feld` is in the Cellar of Homebrew, whose formula owns it.
fn installed_by_homebrew(feld: &Path) -> bool {
    feld.components()
        .any(|component| component == Component::Normal("Cellar".as_ref()))
}

/// Runs install.sh into `directory`. Its own output names the next step as `feld init`,
/// which this command takes, so only a failure is shown.
fn install(directory: &PathBuf, version: Option<&str>) -> CliResult<()> {
    let mut script = Command::new("sh");
    script
        .arg("-c")
        .arg(INSTALL_SCRIPT)
        .arg("install.sh")
        .env("FELD_INSTALL_DIR", directory)
        .stdin(Stdio::null());
    match version {
        Some(version) => script.env("FELD_VERSION", version),
        None => script.env_remove("FELD_VERSION"),
    };
    let output = script.output().context("run the install script")?;
    if output.status.success() {
        return Ok(());
    }
    let reason = String::from_utf8_lossy(&output.stderr);
    let reason = reason.trim().trim_start_matches("install.sh: ");
    Err(operational!(
        "{reason}\nfeld stays at {}",
        env!("CARGO_PKG_VERSION")
    ))
}

/// The version `feld --version` of the new executable answers.
fn new_version(feld: &Path) -> Option<String> {
    let output = Command::new(feld).arg("--version").output().ok()?;
    let answer = String::from_utf8(output.stdout).ok()?;
    answer.split_whitespace().nth(1).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::installed_by_homebrew;

    #[test]
    fn a_feld_in_the_cellar_is_homebrews() {
        assert!(installed_by_homebrew(Path::new(
            "/opt/homebrew/Cellar/feld/0.2.1/bin/feld"
        )));
        assert!(installed_by_homebrew(Path::new(
            "/home/linuxbrew/.linuxbrew/Cellar/feld/0.2.1/bin/feld"
        )));
        assert!(!installed_by_homebrew(Path::new(
            "/Users/me/.local/bin/feld"
        )));
    }
}
