//! `feld doctor`.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};
use std::time::{Duration, Instant};

use feld_core::home::FeldHome;
use feld_core::plugin::protocol::PrepareRequest;
use feld_core::plugin::{PluginKind, Plugins, Registry};
use feld_daemon::api::DaemonStatus;
use feld_daemon::bundled::{self, BUNDLED, BundledPlugin};
use feld_daemon::daemon::{self, Presence, Unresponsive};
use feld_daemon::store::schema::{self, SCHEMA_VERSION, Support};
use serde::Serialize;

use crate::output::print_json;
use crate::shell_file;
use crate::style::Style;
use crate::{CliResult, Context};

/// The fix of a check a sandbox kept from looking.
const OUTSIDE_SANDBOX: &str = "run `feld doctor` outside the sandbox to check it";

/// How long a product's executable has to print its version.
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// How wide the name of a check is, so that the details line up.
const NAME_WIDTH: usize = "runtime-settings".len();

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Verdict {
    Ok,
    Warn,
    Fail,
}

/// One check: what it looked at, how that went, and how to fix it when it did not.
#[derive(Serialize)]
pub struct Check {
    name: &'static str,
    result: Verdict,
    detail: String,
    next: Option<String>,
}

impl Check {
    fn ok(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            result: Verdict::Ok,
            detail: detail.into(),
            next: None,
        }
    }

    fn warn(name: &'static str, detail: impl Into<String>, next: impl Into<String>) -> Self {
        Self {
            name,
            result: Verdict::Warn,
            detail: detail.into(),
            next: Some(next.into()),
        }
    }

    fn fail(name: &'static str, detail: impl Into<String>, next: impl Into<String>) -> Self {
        Self {
            name,
            result: Verdict::Fail,
            detail: detail.into(),
            next: Some(next.into()),
        }
    }
}

/// `feld doctor`: every check, one line each, and exit 1 when one of them fails.
pub async fn run(ctx: &Context) -> CliResult<ExitCode> {
    let checks = checks(&ctx.home).await;
    print(ctx.json, &checks)?;
    Ok(exit_code(&checks))
}

/// The checks `feld init` runs when it fails, under a heading of their own.
pub async fn after_failure(home: &FeldHome, json: bool) -> CliResult<()> {
    let checks = checks(home).await;
    if !json {
        let style = Style::stdout(json);
        println!();
        println!("{}", style.bold("feld doctor"));
    }
    print(json, &checks)
}

fn exit_code(checks: &[Check]) -> ExitCode {
    if checks.iter().any(|check| check.result == Verdict::Fail) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn print(json: bool, checks: &[Check]) -> CliResult<()> {
    if json {
        print_json(&serde_json::json!({ "checks": checks }))?;
        return Ok(());
    }
    let style = Style::stdout(json);
    for check in checks {
        let result = match check.result {
            Verdict::Ok => style.ok("ok  "),
            Verdict::Warn => style.warn("warn"),
            Verdict::Fail => style.error("fail"),
        };
        println!("{result}  {:<NAME_WIDTH$}  {}", check.name, check.detail);
        if let Some(next) = &check.next {
            println!("        {}  {next}", style.dim("next:"));
        }
    }
    Ok(())
}

/// Runs every check against `home`. None of them writes anything.
pub async fn checks(home: &FeldHome) -> Vec<Check> {
    let installed = Registry::load(home, None);
    let presence = daemon::probe(home).await;
    let status = match &presence {
        Ok(Presence::Running(status)) => Some(status.as_ref()),
        _ => None,
    };
    vec![
        executables(&installed),
        feld_home(home, &installed),
        store(home, &presence),
        daemon_check(home, &presence),
        products(home, PluginKind::Provider, &installed),
        products(home, PluginKind::Runtime, &installed),
        runtime_settings(home, &installed),
        sandbox(&installed),
        shell(home),
        collector(home, &presence, status),
        browser(),
    ]
}

/// The `feld init` that installs the defaults of `kinds`, or the plain one before there is
/// a Feld home.
fn init_next(home: &FeldHome, kinds: &[PluginKind]) -> String {
    if home.root().exists() {
        feld_core::plugin::default_init(kinds)
    } else {
        "feld init".to_owned()
    }
}

/// The names of the installed Plugins of `kind`.
fn installed_of(installed: &Registry, kind: PluginKind) -> Vec<String> {
    match kind {
        PluginKind::Runtime => installed.runtimes(),
        PluginKind::Provider => installed.providers(),
    }
    .into_iter()
    .map(|plugin| plugin.name)
    .collect()
}

/// Check 1: The shipped Plugin executables sit beside `feld` and are of its version.
fn executables(installed: &Registry) -> Check {
    const NAME: &str = "executables";
    let version = env!("CARGO_PKG_VERSION");
    let beside = match bundled::executables_directory() {
        Ok(beside) => beside,
        Err(error) => return Check::fail(NAME, format!("{error:#}"), "reinstall Feld"),
    };
    let is_installed = |plugin: &BundledPlugin| {
        installed_of(installed, plugin.kind)
            .iter()
            .any(|name| name == plugin.name)
    };
    let mut current = Vec::new();
    let mut missing = Vec::new();
    let mut problems = Vec::new();
    let mut verdict = Verdict::Ok;
    let mut fix = Vec::new();
    for plugin in BUNDLED {
        let executable = plugin.executable(&beside);
        if !executable.is_file() {
            verdict = verdict.max(if is_installed(plugin) {
                Verdict::Fail
            } else {
                Verdict::Warn
            });
            missing.push(plugin.name);
            fix.push(plugin.name);
            continue;
        }
        let expected = format!("feld-plugin-{} {version}", plugin.name);
        match version_line(&executable, &["--version"]) {
            Some(line) if line == expected => current.push(plugin.name),
            Some(line) => {
                verdict = Verdict::Fail;
                problems.push(line);
                fix.push(plugin.name);
            }
            None => {
                verdict = Verdict::Fail;
                problems.push(format!("feld-plugin-{} answers no version", plugin.name));
                fix.push(plugin.name);
            }
        }
    }
    // A kind with nothing installed yet has only these executables for `feld init` to
    // install from.
    for kind in [PluginKind::Runtime, PluginKind::Provider] {
        if installed_of(installed, kind).is_empty()
            && bundled::of_kind(kind).all(|plugin| missing.contains(&plugin.name))
        {
            verdict = Verdict::Fail;
        }
    }
    let mut detail = vec![format!("feld {version}")];
    if !current.is_empty() {
        detail.push(format!("feld-plugin-{} {version}", current.join(", ")));
    }
    if !missing.is_empty() {
        detail.push(format!("feld-plugin-{} missing", missing.join(", ")));
    }
    detail.extend(problems);
    detail.push(beside.display().to_string());
    let detail = detail.join(" \u{b7} ");
    match verdict {
        Verdict::Ok => Check::ok(NAME, detail),
        verdict => {
            let next = bundled::reinstall_next(&fix);
            Check {
                name: NAME,
                result: verdict,
                detail,
                next: Some(next),
            }
        }
    }
}

/// Check 2: The Feld home is there, writable, with a Plugin of each kind installed.
fn feld_home(home: &FeldHome, installed: &Registry) -> Check {
    const NAME: &str = "home";
    let root = home.root();
    if !root.exists() {
        return Check::fail(
            NAME,
            format!("{} does not exist", root.display()),
            "feld init",
        );
    }
    if !root.is_dir() {
        return Check::fail(
            NAME,
            format!("{} is not a directory", root.display()),
            "choose another Feld home with --home or FELD_HOME",
        );
    }
    if !writable(root) {
        return Check::fail(
            NAME,
            format!("{} is not writable", root.display()),
            format!(
                "make it writable (`chmod u+w {}`), or choose another Feld home with --home",
                feld_core::process::quote_for_display(&root.display().to_string())
            ),
        );
    }
    let kinds = [PluginKind::Runtime, PluginKind::Provider];
    let listed = |kind: PluginKind| {
        let names = installed_of(installed, kind);
        if names.is_empty() {
            "(none)".to_owned()
        } else {
            names.join(", ")
        }
    };
    let detail = format!(
        "{} \u{b7} runtimes: {} \u{b7} providers: {}",
        root.display(),
        listed(PluginKind::Runtime),
        listed(PluginKind::Provider)
    );
    let missing: Vec<PluginKind> = kinds
        .into_iter()
        .filter(|kind| installed_of(installed, *kind).is_empty())
        .collect();
    if !missing.is_empty() {
        return Check::fail(NAME, detail, init_next(home, &missing));
    }
    let drifted = drifted_plugins(home, installed);
    if drifted.is_empty() {
        Check::ok(NAME, detail)
    } else {
        Check::fail(
            NAME,
            format!(
                "{detail} \u{b7} {} {} from feld {}",
                drifted.join(", "),
                if drifted.len() == 1 {
                    "differs"
                } else {
                    "differ"
                },
                env!("CARGO_PKG_VERSION")
            ),
            crate::client::VERSION_NEXT,
        )
    }
}

/// The installed shipped Plugins whose executable answers another version than `feld`, as
/// the executable answers it: `feld-plugin-tmux 0.14.0`. Only a Plugin whose files are not
/// those beside `feld` is asked, so that a copy `feld init` just made is not run here first.
fn drifted_plugins(home: &FeldHome, installed: &Registry) -> Vec<String> {
    let version = env!("CARGO_PKG_VERSION");
    let beside = bundled::executables_directory().ok();
    BUNDLED
        .iter()
        .filter(|plugin| {
            installed_of(installed, plugin.kind)
                .iter()
                .any(|name| name == plugin.name)
        })
        .filter(|plugin| {
            !beside
                .as_deref()
                .is_some_and(|beside| plugin.is_current(home, beside))
        })
        .filter_map(|plugin| {
            let executable = plugin
                .directory(home)
                .join(feld_core::plugin::manifest::EXECUTABLE_FILE);
            let expected = format!("feld-plugin-{} {version}", plugin.name);
            match version_line(&executable, &["--version"]) {
                Some(line) if line == expected => None,
                Some(line) => Some(line),
                None => Some(format!("feld-plugin-{} (no version)", plugin.name)),
            }
        })
        .collect()
}

fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is a NUL-terminated string that outlives the call.
    unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
}

/// Check 3: The store opens with this version, migrated if it is older.
fn store(home: &FeldHome, presence: &anyhow::Result<Presence>) -> Check {
    const NAME: &str = "store";
    let path = home.existing_store_path();
    let aside = || {
        let quoted = feld_core::process::quote_for_display(&path.display().to_string());
        format!("move the store aside to start a new one: `mv {quoted} {quoted}.old`")
    };
    // A daemon that answered opened the store at its start, which refuses a store it cannot
    // read; reading its write-ahead log here would write beside the store.
    if let Ok(Presence::Running(_)) = presence
        && path.exists()
    {
        return Check::ok(NAME, "open in the running daemon");
    }
    // A process holding the daemon lock that did not answer may hold the store open too.
    if path.exists() && daemon::lock_held(home).unwrap_or(true) {
        let next = match presence {
            Err(error) if crate::client::denied_by_sandbox(error) => OUTSIDE_SANDBOX,
            _ => "fix the daemon first, and run `feld doctor` again",
        };
        return Check::warn(
            NAME,
            "not checked, as the process holding the daemon lock did not answer",
            next,
        );
    }
    if let Some(log) = schema::write_ahead_log(&path) {
        return Check::warn(
            NAME,
            format!(
                "not checked, as {} holds writes the daemon has not read back",
                log.display()
            ),
            "feld daemon start, which reads them, and run `feld doctor` again",
        );
    }
    match schema::read_version(&path) {
        Ok(None) => Check::ok(NAME, "none yet; the daemon creates it when it starts"),
        Ok(Some(version)) => match schema::support(version) {
            Support::Empty => Check::ok(NAME, "none yet; the daemon creates it when it starts"),
            Support::Current => Check::ok(NAME, format!("schema {version}")),
            Support::Migrated => Check::ok(
                NAME,
                format!(
                    "schema {version}; the daemon migrates it to {SCHEMA_VERSION} when it starts"
                ),
            ),
            Support::Unsupported(reason) => Check::fail(NAME, reason, aside()),
        },
        Err(error) => Check::fail(NAME, format!("{error:#}"), aside()),
    }
}

/// Check 4: The daemon runs, or the port it starts on is free.
fn daemon_check(home: &FeldHome, presence: &anyhow::Result<Presence>) -> Check {
    const NAME: &str = "daemon";
    let start = crate::client::start_daemon(home);
    match presence {
        Ok(Presence::Running(status)) => {
            let version = crate::client::daemon_version(status);
            let detail = format!(
                "running \u{b7} {version} \u{b7} pid {} \u{b7} {}",
                status.pid, status.api
            );
            if crate::client::version_drifts(status) {
                Check::fail(
                    NAME,
                    format!(
                        "{detail} \u{b7} differs from feld {}",
                        env!("CARGO_PKG_VERSION")
                    ),
                    crate::client::VERSION_NEXT,
                )
            } else {
                Check::ok(NAME, detail)
            }
        }
        Ok(Presence::NotRunning) => {
            match daemon::start_port(home).map(|port| (port, daemon::port_free(port))) {
                Some((port, Ok(false))) => Check::fail(
                    NAME,
                    format!("not running \u{b7} port {port} is in use on 127.0.0.1"),
                    format!(
                        "stop the process that holds port {port} ({} names it), or run `feld daemon start --port <N>`",
                        lsof(port)
                    ),
                ),
                Some((port, Ok(true))) => Check::warn(
                    NAME,
                    format!("not running \u{b7} port {port} is free"),
                    start,
                ),
                Some((port, Err(error))) => Check::warn(
                    NAME,
                    format!("not running \u{b7} port {port} not checked here ({error:#})"),
                    start,
                ),
                None => Check::warn(NAME, "not running", start),
            }
        }
        // A Runtime's sandbox denies TCP to a daemon that runs, so whether it answers is
        // left unchecked; the lock and the daemon record say which daemon it is, without
        // writing a spool request.
        Err(error) if crate::client::denied_by_sandbox(error) => {
            match feld_core::home::read_daemon_record(home) {
                Ok(record) => Check::warn(
                    NAME,
                    format!(
                        "pid {} holds the daemon lock \u{b7} {} \u{b7} not checked, as this sandbox denies TCP",
                        record.pid, record.api
                    ),
                    OUTSIDE_SANDBOX,
                ),
                Err(error) => Check::fail(
                    NAME,
                    format!("{error:#}"),
                    format!("read {}", home.log_path().display()),
                ),
            }
        }
        Err(error) => match error.downcast_ref::<Unresponsive>() {
            Some(unresponsive) => Check::fail(
                NAME,
                format!(
                    "unresponsive \u{b7} pid {} \u{b7} {}",
                    unresponsive.pid, unresponsive.api
                ),
                format!("kill {} \u{b7} feld daemon start", unresponsive.pid),
            ),
            None => Check::fail(
                NAME,
                format!("{error:#}"),
                format!("read {}", home.log_path().display()),
            ),
        },
    }
}

fn lsof(port: u16) -> String {
    format!("`lsof -nP -iTCP:{port} -sTCP:LISTEN`")
}

/// Check 5 and 6: The product of each installed Plugin of `kind` is on `PATH` and tells its
/// version.
fn products(home: &FeldHome, kind: PluginKind, installed: &Registry) -> Check {
    let name = match kind {
        PluginKind::Provider => "provider",
        PluginKind::Runtime => "runtime",
    };
    // A Session runs on a Provider, so one that cannot run fails; a missing Runtime leaves
    // the other Runtimes and the Jobs working.
    let missing_verdict = match kind {
        PluginKind::Provider => Verdict::Fail,
        PluginKind::Runtime => Verdict::Warn,
    };
    let names = installed_of(installed, kind);
    if names.is_empty() {
        let none = format!("no {} is installed", kind_label(kind));
        let next = init_next(home, &[kind]);
        return match kind {
            PluginKind::Provider => Check::fail(name, none, next),
            PluginKind::Runtime => Check::warn(name, none, next),
        };
    }
    let mut verdict = Verdict::Ok;
    let mut items = Vec::new();
    let mut next = Vec::new();
    for plugin_name in names {
        let Some(plugin) = bundled::find(kind, &plugin_name) else {
            items.push(format!(
                "{plugin_name} (not shipped with Feld; not checked)"
            ));
            continue;
        };
        let Some(path) = on_path(plugin.command) else {
            verdict = verdict.max(missing_verdict);
            items.push(format!("{} is not on PATH", plugin.command));
            next.push(plugin.install_next());
            continue;
        };
        let Some(line) = version_line(&path, plugin.version_args) else {
            verdict = verdict.max(Verdict::Warn);
            items.push(format!("{} answers no version", plugin.command));
            next.push(format!(
                "check that `{} {}` runs",
                plugin.command,
                plugin.version_args.join(" ")
            ));
            continue;
        };
        let shown = if line.starts_with(plugin.command) {
            line.clone()
        } else {
            format!("{} {line}", plugin.command)
        };
        if let Some(problem) = plugin.unsupported(&line) {
            verdict = verdict.max(missing_verdict);
            items.push(problem);
            next.push(format!(
                "install {} {} or later",
                plugin.label(),
                plugin.oldest
            ));
            continue;
        }
        items.push(shown);
    }
    Check {
        name,
        result: verdict,
        detail: items.join(" \u{b7} "),
        next: (!next.is_empty()).then(|| next.join("; ")),
    }
}

fn kind_label(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Runtime => "Runtime Adapter",
        PluginKind::Provider => "Execution Provider",
    }
}

/// Check 7: What `feld init` prepared in the configuration root of each installed Runtime is
/// there, as this version writes it.
fn runtime_settings(home: &FeldHome, installed: &Registry) -> Check {
    const NAME: &str = "runtime-settings";
    let names = installed_of(installed, PluginKind::Runtime);
    if names.is_empty() {
        return Check::warn(
            NAME,
            "no Runtime Adapter is installed",
            init_next(home, &[PluginKind::Runtime]),
        );
    }
    let mut items = Vec::new();
    let mut unprepared = Vec::new();
    for name in names {
        let Some(adapter) = installed.runtime(&name) else {
            continue;
        };
        match adapter.prepare(&PrepareRequest {
            apply: false,
            collector: None,
        }) {
            Ok(response) if response.prepared => items.push(format!("{name}: prepared")),
            Ok(response) => {
                let keys: Vec<&str> = response
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.action != feld_core::plugin::protocol::PrepareAction::Keep
                    })
                    .map(|entry| entry.key.as_str())
                    .collect();
                items.push(format!("{name}: not prepared ({})", keys.join(", ")));
                unprepared.push(name);
            }
            Err(error) => {
                items.push(format!("{name}: prepare did not answer ({error:#})"));
                unprepared.push(name);
            }
        }
    }
    let detail = items.join(" \u{b7} ");
    if unprepared.is_empty() {
        Check::ok(NAME, detail)
    } else {
        let runtimes: Vec<String> = unprepared
            .iter()
            .map(|name| format!("--runtime {name}"))
            .collect();
        Check::warn(NAME, detail, format!("feld init {}", runtimes.join(" ")))
    }
}

/// Check 8: The Codex sandbox stays closed to the network where a pane of tmux or herdr
/// carries the socket of its server.
fn sandbox(installed: &Registry) -> Check {
    use crate::codex_sandbox::{self, NETWORK_RISK, SOCKET_PROVIDERS};
    const NAME: &str = "sandbox";
    if !installed_of(installed, PluginKind::Runtime)
        .iter()
        .any(|name| name == "codex")
    {
        return Check::ok(NAME, "codex is not installed");
    }
    let providers: Vec<String> = installed_of(installed, PluginKind::Provider)
        .into_iter()
        .filter(|name| SOCKET_PROVIDERS.contains(&name.as_str()))
        .collect();
    if providers.is_empty() {
        return Check::ok(
            NAME,
            "codex: no installed Provider puts a socket in its panes",
        );
    }
    let Some(path) = codex_sandbox::config_path() else {
        return Check::warn(
            NAME,
            "codex: the configuration root is unknown",
            "set CODEX_HOME or HOME",
        );
    };
    let Some(config) = codex_sandbox::config_sandbox(&path) else {
        return Check::warn(
            NAME,
            format!("codex: {} is not readable as TOML", path.display()),
            format!("fix {}", path.display()),
        );
    };
    if config.network_access == Some(true) {
        Check::warn(
            NAME,
            format!(
                "codex: {} sets sandbox_workspace_write.network_access = true on {}; {NETWORK_RISK}",
                path.display(),
                providers.join(", ")
            ),
            codex_sandbox::network_fix(&path.display().to_string()),
        )
    } else {
        Check::ok(
            NAME,
            "codex: the workspace-write sandbox is closed to the network",
        )
    }
}

/// Check 9: The shell settings read `feld env`, and this shell has read it.
fn shell(home: &FeldHome) -> Check {
    const NAME: &str = "shell";
    let line = shell_file::FELD_ENV_LINE;
    match shell_file::shell() {
        Err(shell) => Check::warn(
            NAME,
            format!("{shell} is not zsh or bash"),
            format!("add `{line}` to the settings file of your shell"),
        ),
        Ok(shell) => match shell.reading_feld_env() {
            None => Check::warn(
                NAME,
                format!("{line} is not in {}", shell.interactive.display()),
                format!("add `{line}` to {}", shell.interactive.display()),
            ),
            Some(path) if super::env::connected(home) => {
                Check::ok(NAME, format!("{line} in {} is read", path.display()))
            }
            Some(path) => Check::warn(
                NAME,
                format!(
                    "{line} is in {}, but this shell has not read it",
                    path.display()
                ),
                format!(
                    "source {}",
                    feld_core::process::quote_for_display(&path.display().to_string())
                ),
            ),
        },
    }
}

/// Check 10: The Collector receives telemetry, or its port is free for the daemon to start it on.
fn collector(
    home: &FeldHome,
    presence: &anyhow::Result<Presence>,
    status: Option<&DaemonStatus>,
) -> Check {
    const NAME: &str = "collector";
    if let Some(status) = status {
        return match &status.otlp {
            Some(endpoint) => Check::ok(NAME, format!("receiving at {endpoint}")),
            None => Check::warn(
                NAME,
                "the daemon runs without its Collector",
                format!(
                    "feld daemon restart, and read {} if it stays so",
                    home.log_path().display()
                ),
            ),
        };
    }
    if let Err(error) = presence
        && crate::client::denied_by_sandbox(error)
    {
        return Check::warn(
            NAME,
            "not checked, as this sandbox denies TCP",
            OUTSIDE_SANDBOX,
        );
    }
    if presence.is_err() {
        return Check::warn(
            NAME,
            "not checked while the daemon does not answer",
            "fix the daemon first",
        );
    }
    let Some(endpoint) = feld_core::telemetry::persisted_endpoint(home) else {
        return Check::ok(NAME, "the daemon chooses a port when it starts");
    };
    let address = match feld_core::telemetry::endpoint_address(&endpoint) {
        Ok(address) => address,
        Err(error) => {
            return Check::warn(
                NAME,
                format!("{endpoint}: {error:#}"),
                format!(
                    "remove {} so that the daemon chooses a new one",
                    home.root()
                        .join(feld_core::telemetry::ENDPOINT_FILE)
                        .display()
                ),
            );
        }
    };
    match std::net::TcpListener::bind(address) {
        Ok(_) => Check::ok(NAME, format!("{endpoint} is free")),
        Err(error) if error.kind() != std::io::ErrorKind::AddrInUse => Check::warn(
            NAME,
            format!("{endpoint} not checked here ({error})"),
            OUTSIDE_SANDBOX,
        ),
        Err(_) => Check::warn(
            NAME,
            format!("{endpoint} is in use"),
            format!(
                "stop the process that holds port {} ({} names it)",
                address.port(),
                lsof(address.port())
            ),
        ),
    }
}

/// Check 11: `feld ui` has a browser to open.
fn browser() -> Check {
    const NAME: &str = "browser";
    let (program, _) = super::ui::browser_command();
    let from_variable = std::env::var_os("BROWSER").is_some();
    let shown = if from_variable {
        format!("BROWSER={program}")
    } else {
        program.clone()
    };
    if on_path(&program).is_some() {
        Check::ok(NAME, shown)
    } else {
        Check::warn(
            NAME,
            format!("{shown} is not on PATH"),
            "set BROWSER to a command that opens a URL, or open the URL `feld ui --print-url` prints",
        )
    }
}

/// Where `command` runs from: itself when it names a path, else the first directory on
/// `PATH` that holds it as an executable.
fn on_path(command: &str) -> Option<PathBuf> {
    if command.contains('/') {
        let path = PathBuf::from(command);
        return is_executable(&path).then_some(path);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| {
            if directory.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                directory
            }
        })
        .map(|directory| directory.join(command))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// The first line `program` prints to `arguments` when it exits 0 within
/// [`VERSION_TIMEOUT`].
pub(super) fn version_line(program: &Path, arguments: &[&str]) -> Option<String> {
    let mut child = ProcessCommand::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + VERSION_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let mut output = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut output).ok()?;
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}
