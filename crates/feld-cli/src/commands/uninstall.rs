//! `feld uninstall`.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::FileTypeExt as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context as _, Result, anyhow};
use feld_core::home::{FeldHome, absent_as_none, read_daemon_record};
use feld_core::model::Ownership;
use feld_core::plugin::protocol::{RestoreRequest, RestoreResponse};
use feld_core::plugin::{Plugins, Registry};
use feld_daemon::api::{LifecycleResponse, SessionList, WorkspaceList};
use feld_daemon::bundled;
use feld_daemon::daemon::{self, Presence};
use feld_daemon::store::schema::{self, OpenSession};
use serde::Serialize;
use serde_json::Value;

use crate::args::UninstallArgs;
use crate::ask;
use crate::client::{Daemon, session_path};
use crate::output::print_json;
use crate::{CliResult, Context};

/// What Feld keeps in its home to be used: the Plugins, the prepare records, the tokens,
/// the Collector endpoint and configuration, the daemon record, the daemon log and its
/// generations, the daemon's socket, the spool, the pairing pages of `feld ui`, and the
/// locks. `feld uninstall` removes them. The names at the top of the home that `admin/`
/// took over are those of a home a daemon of this version has not yet started in.
const HOME_SETTINGS: [&str; 22] = [
    "plugins",
    "prepared",
    "admin/api-token",
    "api-token",
    "tokens",
    "self-index",
    "monitor-locks",
    "collector-endpoint",
    "admin/collector.json",
    "collector.json",
    "daemon.json",
    "daemon.log",
    "daemon.log.1",
    "daemon.log.2",
    "daemon.log.3",
    "daemon.log.4",
    "daemon.lock",
    "daemon.lifecycle.lock",
    "daemon.sock",
    "spool",
    "admin/ui-pair",
    "ui-pair",
];

/// What Feld records in its home: the store and the files of the Sessions. `feld
/// uninstall` keeps them unless `--purge` is given.
const HOME_RECORDS: [&str; 7] = [
    "sessions",
    "admin/feld.db",
    "admin/feld.db-wal",
    "admin/feld.db-shm",
    "feld.db",
    "feld.db-wal",
    "feld.db-shm",
];

/// What only a Feld home holds, each an owner-only file Feld writes. `feld uninstall`
/// removes nothing from a directory with none of them.
const HOME_MARKS: [&str; 5] = [
    "admin/api-token",
    "admin/feld.db",
    "api-token",
    "daemon.lock",
    "feld.db",
];

/// How a SQLite database begins, as the store does.
const SQLITE_HEADER: &[u8] = b"SQLite format 3\0";

/// How long `feld uninstall` waits for the Sessions it killed to end.
const UNINSTALL_KILL_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

/// A running Session `feld uninstall` finds, whether it kills it (an attached one is the
/// person's own process and stays), and whether it has killed it.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UninstallSession {
    id: String,
    workspace: String,
    actor: String,
    execution: String,
    kill: bool,
    killed: bool,
}

/// What one Runtime Adapter takes back, and whether this Feld home keeps a prepare record
/// for it.
#[derive(Serialize)]
struct UninstallRuntime {
    name: String,
    #[serde(skip)]
    label: String,
    prepared: bool,
    #[serde(flatten)]
    restore: RestoreResponse,
}

/// `feld uninstall`: the reverse of `feld init`, in the reverse order. Nothing is removed
/// until everything is listed and consented to, and the Feld home goes last because the
/// Runtime Adapters read their prepare records from it. The one write before that brings
/// the installed Plugins to this version, as a daemon start does, so that they answer
/// `restore`.
pub async fn run(ctx: &Context, args: UninstallArgs) -> CliResult<ExitCode> {
    let (home, json) = (&ctx.home, ctx.json);
    if !home.root().is_dir() {
        return Err(rejected!(
            "there is no Feld home at {}",
            home.root().display()
        ));
    }
    if !HOME_MARKS
        .iter()
        .any(|mark| is_home_mark(home.root(), mark))
    {
        return Err(rejected!(
            "{} is not a Feld home: it has none of the owner-only files Feld writes ({})",
            home.root().display(),
            HOME_MARKS.join(", ")
        ));
    }
    let (daemon_pid, mut sessions) = match daemon::probe(home).await? {
        Presence::Running(status) => (Some(status.pid), running_sessions(ctx).await?),
        Presence::NotRunning => {
            refuse_open_sessions(home)?;
            (None, Vec::new())
        }
    };
    // The installed Plugins answer `restore` in the shape of this version.
    if let Ok(beside) = bundled::executables_directory() {
        bundled::update_installed(home, &beside);
    }

    let installed = Registry::load(home, None);
    let mut runtimes = Vec::new();
    for summary in installed.runtimes() {
        let adapter = installed
            .runtime(&summary.name)
            .expect("an installed Runtime Adapter is in the registry");
        let record = prepare_record(home, &summary.name)?;
        let prepared = record.is_some();
        let restore = adapter
            .restore(&RestoreRequest {
                apply: false,
                record,
            })
            .with_context(|| format!("ask the {} Runtime Adapter what to restore", summary.name))?;
        runtimes.push(UninstallRuntime {
            name: summary.name,
            label: summary.label,
            prepared,
            restore,
        });
    }
    let spare = spare_sockets(home);
    let (removed, kept) = home_entries(home, args.purge, &spare)?;

    if !json {
        println!("Feld home: {}", home.root().display());
        match daemon_pid {
            Some(pid) => {
                let kill = sessions.iter().filter(|session| session.kill).count();
                println!("daemon: running (pid {pid}) \u{b7} running sessions to kill: {kill}");
                for session in &sessions {
                    let note = if session.kill {
                        ""
                    } else {
                        "  (attached, stays)"
                    };
                    println!(
                        "  {}  {:<12} {}{note}",
                        session.id, session.execution, session.actor
                    );
                }
            }
            None => println!("daemon: not running"),
        }
        for runtime in &runtimes {
            println!();
            println!("{}", runtime.label);
            if runtime.restore.entries.is_empty() {
                println!("  nothing to restore");
            }
            for entry in &runtime.restore.entries {
                let detail = match (entry.action, &entry.next, &entry.current) {
                    (
                        feld_core::plugin::protocol::RestoreAction::Restore
                        | feld_core::plugin::protocol::RestoreAction::Unchain,
                        Some(next),
                        _,
                    ) => format!("to: {next}"),
                    (feld_core::plugin::protocol::RestoreAction::Keep, _, Some(current)) => {
                        format!("current: {current}")
                    }
                    (_, _, Some(current))
                        if ["hooks", "writable_roots"].contains(&entry.key.as_str()) =>
                    {
                        current.clone()
                    }
                    _ => entry.path.clone(),
                };
                println!("  {:<20} {:<8}  {detail}", entry.key, entry.action.as_str());
            }
            let takes_back = runtime
                .restore
                .entries
                .iter()
                .any(|entry| entry.action != feld_core::plugin::protocol::RestoreAction::Keep);
            if !runtime.prepared && takes_back {
                println!(
                    "  warning: `feld init` of this Feld home did not put these in place; taking them out stops the attach of the other Feld homes too. To put them back, run `feld init` in the Feld home that uses them"
                );
            }
        }
        println!();
        println!("Feld home");
        if !removed.is_empty() {
            println!("  {}   remove", removed.join(" "));
        }
        if !kept.is_empty() {
            println!("  {}   keep   (use --purge to remove)", kept.join(" "));
        }
        println!();
    }

    let next = uninstall_next();
    let consented = if args.yes {
        true
    } else if !json && ask::is_available() {
        ask::confirm("Remove these?")?
    } else {
        if json {
            print_json(&serde_json::json!({
                "home": home.root(),
                "applied": false,
                "daemon": {"pid": daemon_pid, "sessions": sessions},
                "runtimes": runtimes,
                "remove": removed,
                "keep": kept,
                "next": next,
            }))?;
        } else {
            println!("run `feld uninstall --yes` to apply");
        }
        return Ok(ExitCode::from(1));
    };
    if !consented {
        println!("Nothing was removed.");
        return Ok(ExitCode::from(1));
    }

    let mut left = Vec::new();
    if daemon_pid.is_some() {
        kill_sessions(ctx, &mut sessions).await?;
        daemon::stop(home).await?;
        left = left_open(home, &sessions);
    }
    for runtime in &mut runtimes {
        let adapter = installed
            .runtime(&runtime.name)
            .expect("an installed Runtime Adapter is in the registry");
        runtime.restore = adapter
            .restore(&RestoreRequest {
                apply: true,
                record: prepare_record(home, &runtime.name)?,
            })
            .with_context(|| format!("restore the {} Runtime settings", runtime.name))?;
    }
    for name in &removed {
        // Stopping the daemon takes some of them out already. The socket outside the home
        // is named by its absolute path, which the join keeps as it is.
        let path = home.root().join(name.trim_end_matches('/'));
        let removal = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        absent_as_none(removal).with_context(|| format!("remove {}", path.display()))?;
    }
    // An `admin/` left empty goes with them, and so does a directory of spare sockets.
    let _ = fs::remove_dir(home.admin_dir());
    for socket in &spare {
        if let Some(directory) = socket.parent() {
            let _ = fs::remove_dir(directory);
        }
    }
    // A home left with nothing of anyone else's goes too.
    let home_removal = args.purge.then(|| fs::remove_dir(home.root()));
    let home_error = match &home_removal {
        Some(Err(error)) if error.kind() != ErrorKind::DirectoryNotEmpty => Some(error),
        _ => None,
    };

    if json {
        if let Some(error) = home_error {
            eprintln!("warning: remove {}: {error}", home.root().display());
        }
        print_json(&serde_json::json!({
            "home": home.root(),
            "applied": true,
            "daemon": {"pid": daemon_pid, "sessions": sessions, "left": left},
            "runtimes": runtimes,
            "remove": removed,
            "keep": kept,
            "next": next,
        }))?;
        return Ok(ExitCode::SUCCESS);
    }
    println!("Uninstalled");
    println!(
        "  daemon:       {}",
        if daemon_pid.is_some() {
            "stopped"
        } else {
            "not running"
        }
    );
    for session in &left {
        println!(
            "  left open:    {}  {:<12} {}",
            session.id, session.execution, session.actor
        );
    }
    for runtime in &runtimes {
        let mut paths: Vec<&str> = runtime
            .restore
            .entries
            .iter()
            .filter(|entry| entry.action != feld_core::plugin::protocol::RestoreAction::Keep)
            .map(|entry| entry.path.as_str())
            .collect();
        paths.dedup();
        let summary = if paths.is_empty() {
            "nothing to restore".to_owned()
        } else {
            format!("restored ({})", paths.join(", "))
        };
        println!("  {:<13} {summary}", format!("{}:", runtime.name));
    }
    if let Some(Ok(())) = home_removal {
        println!("  feld home:    removed {}", home.root().display());
    } else if let Some(error) = home_error {
        println!(
            "  feld home:    Feld's files removed \u{b7} {} stays: {error}",
            home.root().display()
        );
    } else if args.purge {
        println!(
            "  feld home:    Feld's files removed \u{b7} {} holds files that are not Feld's and stays",
            home.root().display()
        );
    } else if kept.is_empty() {
        println!("  feld home:    settings removed");
    } else {
        println!(
            "  feld home:    settings removed \u{b7} {} kept at {}",
            kept.join(" "),
            home.root().display()
        );
    }
    for line in &next {
        println!("  next:         {line}");
    }
    for note in runtimes.iter().flat_map(|runtime| &runtime.restore.notes) {
        println!("  note:         {note}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Whether `name` in `root` is the file Feld writes there: a regular file only its owner
/// may read, the daemon lock empty and the store a SQLite database.
fn is_home_mark(root: &Path, name: &str) -> bool {
    let path = root.join(name);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return false;
        }
    }
    if !metadata.is_file() {
        return false;
    }
    match name.rsplit('/').next() {
        Some("daemon.lock") => metadata.len() == 0,
        Some("feld.db") => {
            let mut header = [0u8; SQLITE_HEADER.len()];
            fs::File::open(&path)
                .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut header))
                .is_ok_and(|()| header == SQLITE_HEADER)
        }
        _ => true,
    }
}

/// The prepare record the Feld home keeps for one Runtime Adapter, or None when there is
/// none. One that does not read stops the uninstall, as its Runtime's settings are only
/// given back with it.
fn prepare_record(home: &FeldHome, runtime: &str) -> Result<Option<Value>> {
    let path = home.root().join("prepared").join(format!("{runtime}.json"));
    absent_as_none(fs::read(&path))
        .with_context(|| format!("read {}", path.display()))?
        .map(|bytes| {
            serde_json::from_slice::<Value>(&bytes)
                .map_err(|error| error.to_string())
                .and_then(|record| {
                    record
                        .is_object()
                        .then_some(record)
                        .ok_or_else(|| "it is not a JSON object".to_owned())
                })
                .map_err(|error| {
                    anyhow!(
                        "the prepare record {} does not read: {error}; fix it or move it aside, then run `feld uninstall` again",
                        path.display()
                    )
                })
        })
        .transpose()
}

/// What of the Feld home goes and what stays, by name, directories with a `/`, and the
/// `spare` sockets by their paths. Only the names Feld writes go; anything else in the home
/// is not Feld's and stays.
fn home_entries(
    home: &FeldHome,
    purge: bool,
    spare: &[PathBuf],
) -> Result<(Vec<String>, Vec<String>)> {
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    for name in HOME_SETTINGS.iter().chain(&HOME_RECORDS) {
        let path = home.root().join(name);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        let shown = if metadata.is_dir() {
            format!("{name}/")
        } else {
            (*name).to_owned()
        };
        if purge || HOME_SETTINGS.contains(name) {
            removed.push(shown);
        } else {
            kept.push(shown);
        }
    }
    for socket in spare {
        removed.push(socket.display().to_string());
    }
    Ok((removed, kept))
}

/// The sockets a daemon of a home too long for one left in a temporary directory: the one
/// its record names, which it made under the temporary directory it ran with, and the one
/// under this process's. A recorded path counts only when it bears this home's names.
fn spare_sockets(home: &FeldHome) -> Vec<PathBuf> {
    let short = home.short_socket_path();
    let names = |path: &Path| {
        (
            path.file_name().map(ToOwned::to_owned),
            path.parent()
                .and_then(Path::file_name)
                .map(ToOwned::to_owned),
        )
    };
    let recorded = read_daemon_record(home)
        .ok()
        .and_then(|record| record.socket)
        .filter(|socket| socket.is_absolute() && names(socket) == names(&short));
    let mut sockets: Vec<PathBuf> = recorded.into_iter().chain([short]).collect();
    sockets.dedup();
    sockets.retain(|socket| {
        fs::symlink_metadata(socket).is_ok_and(|metadata| metadata.file_type().is_socket())
    });
    sockets
}

/// Every running Session the daemon knows, Workspace by Workspace.
async fn running_sessions(ctx: &Context) -> Result<Vec<UninstallSession>> {
    let daemon = Daemon::connect(ctx).await?;
    let workspaces: WorkspaceList = daemon
        .get("/v1/workspaces")
        .await
        .map_err(|error| anyhow!("{error}"))?;
    let mut running = Vec::new();
    for workspace in workspaces.workspaces {
        let listing: SessionList = daemon
            .get_with(
                "/v1/sessions",
                &[("workspace", workspace.key.as_str()), ("current", "true")],
            )
            .await
            .map_err(|error| anyhow!("{error}"))?;
        running.extend(
            listing
                .sessions
                .into_iter()
                .filter(|session| session.end.is_none())
                .map(|session| UninstallSession {
                    kill: session.ownership != Ownership::Attached,
                    killed: false,
                    execution: session
                        .execution
                        .runtime
                        .clone()
                        .unwrap_or_else(|| session.execution.execution_type.clone()),
                    id: session.id,
                    workspace: session.workspace,
                    actor: session.actor,
                }),
        );
    }
    Ok(running)
}

/// Stops `feld uninstall` before it changes anything when the store of a daemon that does
/// not run holds open Sessions: their processes run on with nothing to take their hooks,
/// and only the daemon kills them.
fn refuse_open_sessions(home: &FeldHome) -> CliResult<()> {
    let start = "run `feld daemon start`, then `feld uninstall` again";
    let open = schema::read_open_sessions(&home.existing_store_path())
        .map_err(|error| rejected!("the open Sessions are not known: {error:#}; {start}"))?;
    if open.is_empty() {
        return Ok(());
    }
    let listed: Vec<String> = open
        .iter()
        .map(|session| {
            format!(
                "\n  {}  {:<12} {}",
                session.id, session.execution, session.actor
            )
        })
        .collect();
    Err(rejected!(
        "the daemon is not running, and its store holds these open Sessions:{}\n{start}",
        listed.concat()
    ))
}

/// The Sessions the store holds open after the daemon stopped, but the attached ones
/// `feld uninstall` listed to stay: the killed ones that did not end in time, and the
/// ones that started after the listing.
fn left_open(home: &FeldHome, sessions: &[UninstallSession]) -> Vec<UninstallSession> {
    let open = match schema::read_open_sessions(&home.existing_store_path()) {
        Ok(open) => open,
        Err(error) => {
            eprintln!("warning: the Sessions left open are not known: {error:#}");
            return Vec::new();
        }
    };
    open.into_iter()
        .filter_map(|session: OpenSession| {
            match sessions.iter().find(|listed| listed.id == session.id) {
                Some(listed) if !listed.kill => None,
                Some(listed) => Some(listed.clone()),
                None => Some(UninstallSession {
                    kill: session.ownership != Ownership::Attached,
                    killed: false,
                    id: session.id,
                    workspace: session.workspace,
                    actor: session.actor,
                    execution: session.execution,
                }),
            }
        })
        .collect()
}

/// Kills the Sessions `feld uninstall` listed to kill, and waits a while for them to end
/// so that the daemon observes their end before it stops.
async fn kill_sessions(ctx: &Context, sessions: &mut [UninstallSession]) -> Result<()> {
    if !sessions.iter().any(|session| session.kill) {
        return Ok(());
    }
    let daemon = Daemon::connect(ctx).await?;
    for session in sessions.iter_mut().filter(|session| session.kill) {
        let _: LifecycleResponse = daemon
            .post(&format!("{}/kill", session_path(&session.id)))
            .await
            .map_err(|error| anyhow!("kill {}: {error}", session.id))?;
        session.killed = true;
    }
    let until = std::time::Instant::now() + UNINSTALL_KILL_WAIT;
    for session in sessions.iter().filter(|session| session.killed) {
        while std::time::Instant::now() < until {
            let response = daemon
                .get_session(&session.id)
                .await
                .map_err(|error| anyhow!("{error}"))?;
            if response.session.end.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
    Ok(())
}

/// What `feld uninstall` leaves for a person to take out: the `feld env` line of a shell
/// settings file, and the executables.
fn uninstall_next() -> Vec<String> {
    let mut next = Vec::new();
    if let Some(user_home) = std::env::var_os("HOME").map(PathBuf::from) {
        for profile in crate::shell_file::SETTINGS_FILES
            .into_iter()
            .chain([".config/fish/config.fish"])
        {
            let path = user_home.join(profile);
            if crate::shell_file::mentions_feld_env(&path) {
                next.push(format!(
                    "remove `eval \"$(feld env)\"` from {}",
                    path.display()
                ));
            }
        }
    }
    if let Ok(feld) = std::env::current_exe() {
        let directory = feld.parent().map(Path::to_path_buf).unwrap_or_default();
        next.push(format!(
            "remove {} and the feld-plugin-* beside it",
            feld.display()
        ));
        if directory.ends_with(".cargo/bin") {
            next.pop();
            let packages: Vec<String> = std::iter::once("feld".to_owned())
                .chain(
                    bundled::BUNDLED
                        .iter()
                        .filter(|plugin| plugin.executable(&directory).is_file())
                        .map(|plugin| format!("feld-plugin-{}", plugin.name)),
                )
                .collect();
            next.push(format!("cargo uninstall {}", packages.join(" ")));
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use feld_core::home::FeldHome;
    use feld_core::telemetry::ENDPOINT_FILE;
    use feld_daemon::log;

    use super::{HOME_RECORDS, HOME_SETTINGS};

    /// Every path Feld writes in its home is under a name `feld uninstall` lists.
    #[test]
    fn the_home_names_cover_every_path_feld_writes() {
        let home = FeldHome::resolve(Some("/feld-home".into())).unwrap();
        let root = home.root();
        let mut paths = vec![
            home.store_path(),
            home.lock_path(),
            home.lifecycle_lock_path(),
            home.record_path(),
            home.token_path(),
            home.spool_dir(),
            home.log_path(),
            home.plugins_dir(),
            home.session_runtime_dir("session_1"),
            home.session_output_path("session_1"),
            home.monitor_lock_path("session_1"),
            home.token_dir(),
            home.self_index_dir(),
            home.collector_config_path(),
            home.ui_pair_dir(),
            root.join("prepared").join("codex.json"),
            root.join(ENDPOINT_FILE),
        ];
        paths.extend((1..log::LOG_FILES).map(|n| log::generation_path(&home.log_path(), n)));
        for path in paths {
            let path = path.strip_prefix(root).unwrap();
            assert!(
                HOME_SETTINGS
                    .iter()
                    .chain(&HOME_RECORDS)
                    .any(|name| path.starts_with(name)),
                "{} is not listed",
                path.display()
            );
        }
    }
}
