//! `feld upgrade`: install.sh, run in the directory of this feld, then the new feld's
//! `feld init --yes`.

use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};
use std::process::{Command, ExitCode, ExitStatus, Stdio};

use anyhow::Context as _;
use feld_core::process::quote_for_display;
use serde_json::{Value, json};

use crate::args::UpgradeArgs;
use crate::output::print_json;
use crate::{CliResult, Context};

/// The install script of this version. Fetching, checking, and replacing the executables
/// are written there alone, for a first install and an upgrade both.
const INSTALL_SCRIPT: &str = include_str!("../../../../install.sh");

/// Where a person fetches install.sh, to finish an upgrade that stopped partway.
const INSTALL_URL: &str = "https://raw.githubusercontent.com/crestai-labs/feld/main/install.sh";

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
    let from = env!("CARGO_PKG_VERSION");
    let to = new_version(&feld).unwrap_or_else(|| "?".to_owned());
    if !ctx.json {
        println!("Upgraded feld {from} → {to} in {}", directory.display());
        println!();
        // The new feld aligns the Feld home, the Runtime settings, and the daemon with itself.
        let status = new_feld(ctx, &feld, &["init", "--yes"])
            .status()
            .with_context(|| format!("run {} init --yes", feld.display()))?;
        return Ok(exit_code(status));
    }

    // `init --json` leaves the daemon as it is and names the command that runs it, which
    // the new feld runs next. Both answers go out as one object.
    let (init, status) = new_feld_json(ctx, &feld, &["init", "--yes"])?;
    if !status.success() {
        print_json(&init)?;
        return Ok(exit_code(status));
    }
    let mut daemon = Value::Null;
    let mut status = status;
    if let Some(command) = daemon_command(&init) {
        (daemon, status) = new_feld_json(ctx, &feld, &["daemon", command])?;
    }
    print_json(&json!({ "from": from, "to": to, "init": init, "daemon": daemon }))?;
    Ok(exit_code(status))
}

/// The `feld daemon` command that `feld init --json` names as the next step, which puts the
/// new daemon to work. A home still missing a kind of Plugin names `feld init` instead.
fn daemon_command(init: &Value) -> Option<&'static str> {
    match init.get("next").and_then(Value::as_str) {
        Some("feld daemon restart") => Some("restart"),
        Some("feld daemon start") => Some("start"),
        _ => None,
    }
}

/// The new `feld` with this command's Feld home.
fn new_feld(ctx: &Context, feld: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(feld);
    command.arg("--home").arg(ctx.home.root()).args(arguments);
    command
}

/// What the new `feld --json <arguments>` prints on stdout, with how it ended. Its errors
/// go to stderr as they are.
fn new_feld_json(ctx: &Context, feld: &Path, arguments: &[&str]) -> CliResult<(Value, ExitStatus)> {
    let output = new_feld(ctx, feld, arguments)
        .arg("--json")
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("run {} {}", feld.display(), arguments.join(" ")))?;
    let value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(_) if !output.status.success() => Value::Null,
        Err(error) => {
            return Err(operational!(
                "{} {} printed no JSON: {error}",
                feld.display(),
                arguments.join(" ")
            ));
        }
    };
    Ok((value, output.status))
}

fn exit_code(status: ExitStatus) -> ExitCode {
    ExitCode::from(
        status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(2),
    )
}

/// Whether `feld` is in the Cellar of Homebrew, whose formula owns it.
fn installed_by_homebrew(feld: &Path) -> bool {
    feld.components()
        .any(|component| component == Component::Normal("Cellar".as_ref()))
}

/// Runs install.sh into `directory`. Its own output names the next step as `feld init`,
/// which this command takes, so only a failure is shown.
fn install(directory: &Path, version: Option<&str>) -> CliResult<()> {
    let feld = directory.join("feld");
    let before = std::fs::metadata(&feld).map(|metadata| metadata.ino()).ok();
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
    let said = String::from_utf8_lossy(&output.stderr);
    let details: Vec<&str> = said
        .trim()
        .lines()
        .map(|line| line.strip_prefix("install.sh: ").unwrap_or(line))
        .collect();
    let details = details.join("\n");
    // install.sh renames feld into place before the Plugins, so feld tells whether it got
    // that far.
    let after = std::fs::metadata(&feld).map(|metadata| metadata.ino()).ok();
    if after == before {
        return Err(operational!(
            "the upgrade failed, and feld stays at {}\n{details}",
            env!("CARGO_PKG_VERSION")
        ));
    }
    let mut again = format!(
        "FELD_INSTALL_DIR={}",
        quote_for_display(&directory.display().to_string())
    );
    if let Some(version) = version {
        again.push_str(&format!(" FELD_VERSION={}", quote_for_display(version)));
    }
    Err(operational!(
        "the upgrade stopped partway: the executables in {} are partly replaced\n{details}",
        directory.display()
    )
    .with_next(format!(
        "curl -fsSL {INSTALL_URL} | {again} sh && feld init"
    )))
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

    use serde_json::json;

    use super::{daemon_command, installed_by_homebrew};

    /// `--json` runs the daemon command `init --json` leaves as its next step, so that the
    /// daemon runs the new version as it does without `--json`.
    #[test]
    fn the_daemon_step_init_json_leaves_is_taken() {
        assert_eq!(
            daemon_command(&json!({ "next": "feld daemon restart" })),
            Some("restart")
        );
        assert_eq!(
            daemon_command(&json!({ "next": "feld daemon start" })),
            Some("start")
        );
        assert_eq!(
            daemon_command(&json!({ "next": "feld init --provider tmux" })),
            None
        );
    }

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
