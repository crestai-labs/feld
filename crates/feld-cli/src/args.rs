//! The command line: what `feld` takes, and the help it gives.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use feld_core::home::{FELD_HOME_ENV, FELD_WORKSPACE_ENV};
use feld_core::id::valid_key;
use feld_core::model::{ActorKind, Grant, RecordKind};
use feld_daemon::api::allowed_host_name;

pub const EXIT_CODE_HELP: &str = "Exit codes:
  0  succeeded, or stopped once the reader of stdout closed (`| head`)
  1  rejected: the daemon or this CLI refused the request
  2  operational: bad arguments, no reachable daemon, or a local step that failed";

pub const PROVIDER_OPTION_HELP: &str = "Execution Provider options are given as --<provider>-<key> <value>; the Provider defines its keys.
  tmux:   --tmux-socket <NAME|PATH>, --tmux-unit <session|window|pane>, --tmux-session <NAME>, --tmux-window <ID>
  herdr:  --herdr-session <NAME|PATH>, --herdr-unit <workspace|tab|pane>, --herdr-workspace <ID>, --herdr-pane <ID>, --herdr-split <right|down>";

pub const ATTACH_PROVIDER_OPTION_HELP: &str =
    "Execution Provider options name the pane to attach, as --<provider>-<key> <value>.
  tmux:   --tmux-pane <ID>, --tmux-socket <NAME|PATH>
  herdr:  --herdr-pane <ID>, --herdr-session <NAME|PATH>";

/// The help heading of the options every command takes.
pub const GLOBAL_OPTIONS: &str = "Global Options";

/// What the `read` state of a Message says, in the words the Web UI uses for it.
pub const READ_STATE_NOTE: &str =
    "A read means the Message was handed to a Session, not that it was acted on.";

/// The foot of a command's help: what to type, then how the command exits.
pub fn help_footer(body: &str) -> String {
    format!("{body}\n\n{EXIT_CODE_HELP}")
}

/// [`help_footer`] for the commands that place a Session on an Execution Provider.
pub fn launch_footer(body: &str) -> String {
    format!("{body}\n\n{PROVIDER_OPTION_HELP}\n\n{EXIT_CODE_HELP}")
}

/// Agent Loop Control Plane.
///
/// Feld declares Actors — an `agent` that runs a Runtime, a `job` that runs a program — starts
/// their Sessions on an Execution Provider, and keeps every start, observation, and Message of a
/// Workspace on one timeline. Most commands talk to the daemon of the Feld home, which
/// `feld daemon start` puts there.
#[derive(Debug, Parser)]
#[command(
    name = "feld",
    version = env!("FELD_BUILD_VERSION"),
    override_usage = "feld [OPTIONS] <COMMAND>",
    after_help = help_footer(
        "Getting started:
  feld init --yes                                    install the Plugins for what is on PATH and start the daemon
  feld actor create hello --kind job -- echo hello   create a Job
  feld actor start hello                             run it
  feld actor list -a                                 see every Actor and its latest Session
  feld record list --actor hello                     read what happened"
    )
)]
pub struct Cli {
    /// Feld state directory holding the store, the daemon, and the Plugins [default: ~/.feld]
    #[arg(
        long,
        env = FELD_HOME_ENV,
        global = true,
        value_name = "PATH",
        help_heading = GLOBAL_OPTIONS
    )]
    pub home: Option<PathBuf>,
    /// Workspace of the Actors a command acts on; Actor names are unique within one Workspace
    #[arg(
        long,
        env = FELD_WORKSPACE_ENV,
        default_value = "default",
        global = true,
        value_name = "KEY",
        value_parser = parse_workspace_key,
        help_heading = GLOBAL_OPTIONS
    )]
    pub workspace: String,
    /// Print the result as JSON on stdout, and an error as {"error":{"kind","message","next"}} on stderr
    #[arg(long, global = true, help_heading = GLOBAL_OPTIONS)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init(InitArgs),
    Uninstall(UninstallArgs),
    Doctor(DoctorArgs),
    Daemon(DaemonArgs),
    Workspace(WorkspaceArgs),
    Actor(ActorArgs),
    Session(SessionArgs),
    Record(RecordArgs),
    #[command(visible_alias = "msg")]
    Message(MessageArgs),
    Runtime(RuntimeArgs),
    Provider(ProviderArgs),
    #[command(name = "self")]
    SelfSession(SelfArgs),
    Ui(UiArgs),
    Browser(BrowserArgs),
    Env(EnvArgs),
    #[command(hide = true)]
    CollectorRun,
    #[command(hide = true)]
    ProgramCapture(ProgramCaptureArgs),
}

/// Make the Feld home, install the Plugins Feld ships into it, and start the daemon.
///
/// A terminal is asked which of the bundled Plugins to install, and `--runtime`, `--provider`,
/// and `--yes` answer in its place. Without an answer, the Plugins checked are the installed
/// ones, else those whose products (claude, codex, tmux, herdr) are on PATH, with claude-code
/// when no Runtime is there. With no Provider on PATH, none named, and none installed, nothing
/// is installed and the run ends with exit 1. The list confirms with a check in each kind.
/// The directory of every chosen Plugin is written again, and an installed Plugin left out
/// stays as it is. A daemon reads its Plugins at start, so
/// `feld daemon restart` puts the new ones to work.
///
/// Each chosen Runtime Adapter then prepares the Runtime a person starts themselves for
/// attach. What would chain or replace a setting of their own is shown first and written
/// only when a terminal answers yes or `--yes` is given. A run with neither a terminal nor
/// `--yes` leaves those settings as they are and ends with exit 1.
///
/// Then `eval "$(feld env)"` is added to the file an interactive zsh (`~/.zshrc`) or bash
/// (`~/.bashrc`) reads, unless a file the shell reads has it already; a terminal is asked
/// first, and `--yes` answers yes. Without a terminal or `--yes`, and for any other shell, a
/// note names the line to add.
///
/// Last, a terminal is asked whether to start the daemon, or to restart a running one so that
/// it reads the Plugins; `--yes` answers yes. A run with neither, and `--json`, leave the daemon
/// as it is and name the command that starts it.
#[derive(Debug, Args)]
#[command(after_help = help_footer(
    "Examples:
  feld init --yes                              install the Plugins for what is on PATH and start the daemon
  feld init --runtime codex --provider herdr   install those two instead"
))]
pub struct InitArgs {
    /// A Runtime Adapter to install, claude-code or codex, in place of the answer a TTY is asked for
    #[arg(long, value_name = "NAME")]
    pub runtime: Vec<String>,
    /// An Execution Provider to install, tmux or herdr, in place of the answer a TTY is asked for
    #[arg(long, value_name = "NAME")]
    pub provider: Vec<String>,
    /// Install the Plugins for what is on PATH without asking, consent to the Runtime settings it changes and to the line added to the shell settings, and start the daemon
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// Take out what `feld init` and the daemon put in this environment.
///
/// The daemon stops, after the Sessions it runs are killed. Each installed Runtime Adapter
/// takes back what it prepared in the Runtime a person starts themselves, from the prepare
/// record the Feld home keeps, and the Feld home gives up its settings: the Plugins, the
/// prepare records, the tokens, the Collector endpoint, and the locks. `sessions/` and the
/// store stay unless `--purge` is given. Everything is listed first and removed only when a
/// terminal answers yes or `--yes` is given; a run with neither removes nothing and ends
/// with exit 1.
#[derive(Debug, Args)]
#[command(after_help = help_footer(
    "Examples:
  feld uninstall                  list what goes, and ask
  feld uninstall --yes            remove it without asking
  feld uninstall --purge --yes    remove the whole Feld home as well"
))]
pub struct UninstallArgs {
    /// Remove the whole Feld home, sessions/ and the store included
    #[arg(long)]
    pub purge: bool,
    /// Consent to everything listed without asking
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// Check what Feld needs to run, one line per check, without writing anything.
///
/// The checks are: the Plugin executables beside feld and their version, the Feld home and
/// its Plugins, the store, the daemon or the port it starts on, the Execution Providers and
/// Runtimes on PATH, the Runtime settings `feld init` prepares, the `feld env` line of the
/// shell settings, the Collector port, and the browser `feld ui` opens. Each is `ok`, `warn`
/// when part of Feld misses out, or `fail` when the daemon, a Session, or an attach cannot
/// start, with `next:` naming the fix. It runs before `feld init` and while the daemon is
/// stopped, and the sign-in of a Runtime is not checked.
#[derive(Debug, Args)]
#[command(after_help = "Examples:
  feld doctor          check this environment
  feld doctor --json   the checks as JSON

Exit codes:
  0  no check failed
  1  a check failed
  2  operational: bad arguments")]
pub struct DoctorArgs;

/// Read and operate on Sessions, the runs of an Actor.
#[derive(Debug, Args)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub command: SessionCommand,
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// Show one Session, ended ones included.
    ///
    /// The start, end, outcome, execution, placement, and last observation are shown, with the
    /// spawner of a started Session, the requester and result of an accepted stop or kill, and
    /// the Sub Agent Sessions a Runtime reported.
    #[command(after_help = help_footer(
        "Examples:
  feld session get reviewer          the current Session of an Actor
  feld session get session_01J...    one Session by ID"
    ))]
    Get(SessionReadTarget),
    /// List Sessions, newest first.
    ///
    /// Every Session of the Workspace is listed unless an option narrows it; `--start` and
    /// `--end` keep the Sessions that overlap the range.
    #[command(after_help = help_footer(
        "Examples:
  feld session list --actor reviewer
  feld session list --kind job --current
  feld session list --start 2026-09-13T10:00:00+09:00 --end 2026-09-13T12:00:00+09:00"
    ))]
    #[command(visible_alias = "ls")]
    List(SessionListArgs),
    /// Ask a Session to end the way its Runtime does.
    ///
    /// The Execution Provider sends the stop input of the Runtime Adapter to the pane — `/exit`
    /// for Claude Code, `/quit` for Codex — and observes after each one. An execution without a
    /// stop input, a program among them, is sent SIGTERM. `feld session kill` ends a Session
    /// that stays. An attached Session is rejected.
    #[command(after_help = help_footer(
        "Examples:\n  feld session stop reviewer\n  feld session stop session_01J..."
    ))]
    Stop(SessionTarget),
    /// End a Session at once, with SIGKILL on its Execution Provider.
    ///
    /// The Runtime gets no chance to finish its turn or to write its state. `feld session stop`
    /// is the orderly end. An attached Session is rejected.
    #[command(after_help = help_footer(
        "Examples:\n  feld session kill hello\n  feld session kill session_01J..."
    ))]
    Kill(SessionTarget),
    /// Have the Execution Provider observe a Session now, and commit what it saw.
    ///
    /// The observation and the previous one are shown. An `observation` record is committed
    /// too when the kind differs from the previous one.
    #[command(after_help = help_footer(
        "Examples:\n  feld session observe reviewer\n  feld session observe session_01J..."
    ))]
    Observe(SessionTarget),
    /// Read what a Session put out: the transcript of an agent Runtime, the output of a program.
    ///
    /// A transcript lists the transcript entries in the common shape, and `--view all` adds the
    /// transcript entries the Conversation view leaves out. An output lists the stdout and stderr lines.
    /// The `next cursor:` line holds the value to give `--after` next time.
    #[command(after_help = help_footer(
        "Examples:
  feld session log reviewer --tail 20
  feld session log dispatcher --follow
  feld session log session_01J... --after 42"
    ))]
    Log(SessionLogArgs),
    /// Show the Session metrics, one row per metric, over the Session or a range.
    ///
    /// Each row holds the status and source of the metric and its value over the whole range.
    /// `--json` adds the series in bins of `--bucket` seconds.
    #[command(after_help = help_footer(
        "Examples:
  feld session metrics reviewer
  feld session metrics session_01J... --json --bucket 300"
    ))]
    Metrics(SessionMetricsArgs),
}

#[derive(Debug, Args)]
pub struct SessionListArgs {
    /// Show only the Sessions of this Actor
    #[arg(long, value_name = "ACTOR")]
    pub actor: Option<String>,
    /// Show only the Sessions of Actors of this kind: agent or job
    #[arg(long, value_name = "KIND", value_parser = parse_actor_kind)]
    pub kind: Option<ActorKind>,
    /// Show only current Sessions, the ones without an end
    #[arg(long)]
    pub current: bool,
    /// Keep the Sessions that end after this RFC 3339 time
    #[arg(long, value_name = "TIME")]
    pub start: Option<String>,
    /// Keep the Sessions that start before this RFC 3339 time
    #[arg(long, value_name = "TIME")]
    pub end: Option<String>,
}

#[derive(Debug, Args)]
pub struct SessionLogArgs {
    /// Session ID (session_...), or an Actor name for its current Session, else its latest one
    #[arg(value_name = "TARGET", value_parser = parse_session_target)]
    pub target: String,
    /// Keep printing what the Session adds until it ends; with --json, one JSON entry or line per line
    #[arg(long)]
    pub follow: bool,
    /// Print only the last N transcript entries or lines, at most 1000; conflicts with --after
    #[arg(long, value_name = "N", conflicts_with = "after")]
    pub tail: Option<usize>,
    /// Print only what comes after this cursor, as `next cursor:` gives it
    #[arg(long, value_name = "CURSOR")]
    pub after: Option<String>,
    /// Transcript entries to print: conversation, or all to add the rest
    #[arg(long, value_name = "VIEW", default_value = "conversation", value_parser = ["conversation", "all"])]
    pub view: String,
}

#[derive(Debug, Args)]
pub struct SessionMetricsArgs {
    /// Session ID (session_...), or an Actor name for its current Session, else its latest one
    #[arg(value_name = "TARGET", value_parser = parse_session_target)]
    pub target: String,
    /// Start of the range, an RFC 3339 time [default: the start of the Session]
    #[arg(long, value_name = "TIME")]
    pub start: Option<String>,
    /// End of the range, an RFC 3339 time [default: the end of the Session, else now]
    #[arg(long, value_name = "TIME")]
    pub end: Option<String>,
    /// Seconds per bin of the series `--json` prints, at least 60
    #[arg(long, value_name = "SECONDS", value_parser = parse_bucket)]
    pub bucket: Option<u64>,
}

/// Read the installed Runtime Adapters and their Runtime metrics.
#[derive(Debug, Args)]
pub struct RuntimeArgs {
    #[command(subcommand)]
    pub command: RuntimeCommand,
}

#[derive(Debug, Subcommand)]
pub enum RuntimeCommand {
    /// List the installed Runtimes, with how many of their Sessions are running.
    #[command(after_help = help_footer("Examples:\n  feld runtime list"))]
    #[command(visible_alias = "ls")]
    List,
    /// Show the Runtime metrics: the account quota, and the Session metrics of every Workspace
    /// aggregated per Runtime and account.
    ///
    /// Each metric aggregates the Sessions that reported it, by the rule of the metric. The range
    /// is the last 5 hours unless `--since` or `--start` sets it, and the first line shows it.
    #[command(after_help = help_footer(
        "Examples:
  feld runtime metrics
  feld runtime metrics --since 1h
  feld runtime metrics claude-code --start 2026-09-13T00:00:00+09:00
  feld runtime metrics codex --json --bucket 300"
    ))]
    Metrics(RuntimeMetricsArgs),
}

#[derive(Debug, Args)]
pub struct RuntimeMetricsArgs {
    /// Runtime name [default: every installed Runtime]
    #[arg(value_name = "RUNTIME")]
    pub runtime: Option<String>,
    /// Read the span up to now, a number and a unit: 30m, 5h, 7d [default: 5h]
    #[arg(long, value_name = "DURATION", value_parser = parse_since, conflicts_with_all = ["start", "end"])]
    pub since: Option<Since>,
    /// Start of the range, an RFC 3339 time [default: 5 hours before the end]
    #[arg(long, value_name = "TIME")]
    pub start: Option<String>,
    /// End of the range, an RFC 3339 time [default: now]
    #[arg(long, value_name = "TIME")]
    pub end: Option<String>,
    /// Seconds per bin of the series `--json` prints, at least 60
    #[arg(long, value_name = "SECONDS", value_parser = parse_bucket)]
    pub bucket: Option<u64>,
}

/// Read the installed Execution Providers.
#[derive(Debug, Args)]
pub struct ProviderArgs {
    #[command(subcommand)]
    pub command: ProviderCommand,
}

#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// List the installed Execution Providers, with their Nodes and running Sessions.
    #[command(after_help = help_footer("Examples:\n  feld provider list"))]
    #[command(visible_alias = "ls")]
    List,
}

/// Open the Feld Web UI of the Workspace in a browser, signed in.
///
/// The daemon issues a pairing code named `local`, and the browser opens the Sign in page with
/// the code in the URL fragment. The page signs in by trading the code for a browser token in a
/// cookie and moves on to the Actors page of the Workspace; a browser token the browser already
/// holds is replaced. The browser is $BROWSER, else the one this platform opens links with.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
#[command(after_help = help_footer(
    "Examples:
  feld ui
  feld ui --print-url        print the URL, which works once within 5 minutes
  feld ui pair --name laptop issue a code to sign in on another device"
))]
pub struct UiArgs {
    #[command(subcommand)]
    pub command: Option<UiCommand>,
    /// Print the URL instead of opening a browser
    #[arg(long)]
    pub print_url: bool,
}

#[derive(Debug, Subcommand)]
pub enum UiCommand {
    /// Issue a code to sign in to the Web UI from the browser of another device.
    ///
    /// Enter the code on the Sign in page of the Web UI the other device opens through the
    /// HTTPS proxy (`tailscale serve`). The code works once within 5 minutes, and the browser
    /// gets a browser token of the given name. The daemon does not know the address the proxy
    /// serves, so no URL is printed.
    #[command(after_help = help_footer(
        "Examples:
  feld ui pair
  feld ui pair --name laptop"
    ))]
    Pair {
        /// Name of the browser token the code is traded for [default: browser]
        #[arg(long)]
        name: Option<String>,
    },
}

/// List and revoke the browser tokens of signed-in browsers.
///
/// The commands speak with the local token, so they run on the device of the daemon.
#[derive(Debug, Args)]
pub struct BrowserArgs {
    #[command(subcommand)]
    pub command: BrowserCommand,
}

#[derive(Debug, Subcommand)]
pub enum BrowserCommand {
    /// List the browser tokens in force.
    #[command(after_help = help_footer("Examples:\n  feld browser list"))]
    #[command(visible_alias = "ls")]
    List,
    /// Revoke browser tokens, which signs those browsers out.
    ///
    /// The next request of that browser is refused, and its Web UI returns to the Sign in page.
    #[command(after_help = help_footer(
        "Examples:
  feld browser revoke laptop                                     every token named laptop
  feld browser revoke browser_5c1f0e9a2b7d4c86a3e1f4b2d9c07a51   one token"
    ))]
    Revoke {
        /// The id of a browser token, or a name, which revokes every token in force of that name
        #[arg(value_name = "NAME|ID")]
        target: String,
    },
}

/// Print the environment that connects a shell to the daemon of the Feld home.
///
/// The output is `export` statements of FELD_HOME, FELD_API, and the OpenTelemetry
/// variables that send a Runtime's telemetry to the daemon's Collector. A Claude Code started
/// from a shell that ran `eval "$(feld env)"` exports its metrics to this daemon once it is
/// attached. The values come from the Feld home, so the daemon need not be running; a home
/// whose daemon never started gives FELD_HOME alone.
#[derive(Debug, Args)]
#[command(after_help = help_footer(
    "Examples:
  eval \"$(feld env)\"                in ~/.zshrc or ~/.bashrc, which `feld init` adds
  feld --home /tmp/feld-test env   the environment of another Feld home"
))]
pub struct EnvArgs;

/// Wrap the program of a Session and write what it prints as output records.
#[derive(Debug, Args)]
pub struct ProgramCaptureArgs {
    /// File the output records are appended to
    #[arg(long, value_name = "PATH")]
    pub output: PathBuf,
    /// FIFO the program's stdout is read from
    #[arg(long, value_name = "PATH")]
    pub stdout_fifo: PathBuf,
    /// FIFO the program's stderr is read from
    #[arg(long, value_name = "PATH")]
    pub stderr_fifo: PathBuf,
    /// File created once every line has been written
    #[arg(long, value_name = "PATH")]
    pub done: PathBuf,
}

/// Take a Runtime that already runs in a pane as the Session of an Actor.
///
/// Nothing is started and no signal is sent. Feld only checks that the configuration root the
/// Runtime reads holds the skill and settings `feld init` prepared (a `note:` line points to
/// `feld init` when they are missing), confirms that the pane runs `--runtime`, and keeps the
/// Session token where `feld` inside that pane finds it, so that it speaks as the Session.
/// A Runtime that already has a Session has its token rotated. Either `--self` or an Execution
/// Provider option naming the pane is required.
#[derive(Debug, Args)]
#[command(after_help = format!(
    "Examples:
  feld actor attach --self --runtime claude-code            the pane this CLI runs in
  feld actor attach codex-dev --tmux-pane %47 --runtime codex
  feld actor attach dev --herdr-pane w3:p1 --runtime claude-code

{ATTACH_PROVIDER_OPTION_HELP}

{EXIT_CODE_HELP}"
))]
pub struct AttachArgs {
    /// Actor name to attach as; with --self a name is derived from the pane's workdir
    #[arg(required_unless_present = "self_", value_name = "ACTOR", value_parser = parse_actor_name)]
    pub name: Option<String>,
    /// Attach the Runtime this CLI runs in; an option names its pane when the Runtime runs this conversation outside the pane
    #[arg(long = "self")]
    pub self_: bool,
    /// Runtime running in the pane: claude-code or codex
    #[arg(long, value_name = "RUNTIME")]
    pub runtime: String,
}

/// Read the facts Feld itself recorded.
#[derive(Debug, Args)]
pub struct RecordArgs {
    #[command(subcommand)]
    pub command: RecordCommand,
}

#[derive(Debug, Subcommand)]
pub enum RecordCommand {
    /// List records in the order they were committed.
    ///
    /// Starts, spawns, observations, Messages, lifecycle requests, and skipped ticks are listed
    /// oldest first, with their sequence number in SEQ. The `next cursor:` line that ends the
    /// table holds the value to give `--after` next time.
    #[command(after_help = help_footer(
        "Examples:
  feld record list --session session_01J...          the records of one Session
  feld record list --actor nightly --kind schedule.skipped
  feld record list --kind spawned --follow           keep printing new spawns"
    ))]
    #[command(visible_alias = "ls")]
    List(RecordListArgs),
}

#[derive(Debug, Args)]
pub struct RecordListArgs {
    /// Show only the records of this Actor
    #[arg(long, value_name = "ACTOR")]
    pub actor: Option<String>,
    /// Show only the records of this Session (session_...)
    #[arg(long, value_name = "SESSION")]
    pub session: Option<String>,
    /// Show only the records of this kind, such as spawned or schedule.skipped
    #[arg(long, value_name = "KIND", value_parser = parse_record_kind)]
    pub kind: Option<RecordKind>,
    /// Keep the records at or after this RFC 3339 time
    #[arg(long, value_name = "TIME")]
    pub start: Option<String>,
    /// Keep the records before this RFC 3339 time
    #[arg(long, value_name = "TIME")]
    pub end: Option<String>,
    /// Show only the records after this sequence number, as SEQ and `next cursor:` give it
    #[arg(long, value_name = "SEQ")]
    pub after: Option<u64>,
    /// Keep printing the records Feld commits; with --json, one JSON record per line
    #[arg(long)]
    pub follow: bool,
}

/// Run and inspect the daemon that holds the store and serves the API.
#[derive(Debug, Args)]
pub struct DaemonArgs {
    #[command(subcommand)]
    pub command: DaemonCommand,
}

#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// Start the daemon in the background and wait up to 10 seconds for its API.
    ///
    /// The daemon restores the Actors, open Sessions, and Schedules of the Feld home and logs
    /// to FELD_HOME/daemon.log. A daemon that already runs is reported as it stands.
    ///
    /// The API listens on 127.0.0.1, and on [::1] where IPv6 is available, on the same port.
    /// A port named with --port stays the daemon's port across stop, start, and restart.
    /// Without one, the default Feld home (~/.feld) listens on 7381. Either fails the start
    /// while another process holds it. Another Feld home listens on the port it last
    /// listened on while that is free.
    ///
    /// The daemon answers requests sent to 127.0.0.1, localhost, or [::1] on its port, and
    /// to the host names allowed with --allow-host. To open the Web UI from another device,
    /// relay the port with `tailscale serve` and allow the name it serves.
    #[command(after_help = help_footer(
        "Examples:\n  feld daemon start\n  feld daemon start --port 7390\n  tailscale serve --bg 7381\n  feld daemon start --allow-host <host>.<tailnet>.ts.net"
    ))]
    Start(DaemonStartArgs),
    /// Show the running daemon: pid, API, Web UI, allowed host names, timezone, and installed
    /// Plugins.
    ///
    /// The exit code is 1 while no daemon runs for the Feld home, and 2 while a daemon holds
    /// the Feld home but does not answer within 5 seconds, which is reported as unresponsive
    /// with its pid.
    #[command(after_help = help_footer(
        "Examples:\n  feld daemon status\n  feld --json daemon status"
    ))]
    Status,
    /// Ask the daemon to shut down. The Sessions keep running on their Providers.
    ///
    /// A daemon that is not running is reported as `Daemon is not running.` with exit code 0.
    /// A daemon that does not answer within 5 seconds is left as it is, and the error names
    /// its pid to stop it with `kill`.
    #[command(after_help = help_footer("Examples:\n  feld daemon stop"))]
    Stop,
    /// Stop the daemon and start it again, so that it reads the installed Plugins anew.
    ///
    /// The daemon listens on the port of the daemon it stops; --port names another port.
    /// It answers the host names of the daemon it stops, and those --allow-host adds.
    /// A daemon that does not answer within 5 seconds is left as it is, and the error names
    /// its pid to stop it with `kill`.
    #[command(after_help = help_footer(
        "Examples:\n  feld daemon restart\n  feld daemon restart --port 7390\n  feld daemon restart --allow-host <host>.<tailnet>.ts.net"
    ))]
    Restart(DaemonStartArgs),
    #[command(hide = true)]
    Run {
        /// Keep the local token read from stdin, which `feld daemon restart` hands over.
        #[arg(long)]
        local_token_stdin: bool,
        /// Listen on this port of 127.0.0.1, or fail
        #[arg(long)]
        port: Option<u16>,
        /// Listen on this port of 127.0.0.1, or fail, without keeping it for the later starts
        #[arg(long, conflicts_with = "port")]
        carried_port: Option<u16>,
        /// Answer requests sent to this host name too
        #[arg(long, value_parser = allowed_host_name)]
        allow_host: Vec<String>,
    },
}

#[derive(Debug, Args)]
pub struct DaemonStartArgs {
    /// API port on 127.0.0.1, kept for the later starts
    #[arg(long, env = "FELD_API_PORT", value_parser = clap::value_parser!(u16).range(1..))]
    pub port: Option<u16>,
    /// Host name of a proxy such as `tailscale serve` whose requests the daemon answers
    /// (repeatable)
    #[arg(long, value_name = "HOST", value_parser = allowed_host_name)]
    pub allow_host: Vec<String>,
}

/// Group Actors into Workspaces, which each hold their own Actor names.
#[derive(Debug, Args)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    pub command: WorkspaceCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceCommand {
    /// List the Workspaces with their Actor count.
    #[command(after_help = help_footer(
        "Examples:\n  feld workspace list\n  feld workspace list --include-deleted"
    ))]
    #[command(visible_alias = "ls")]
    List {
        /// Also list the deleted Workspaces, with a DELETED column
        #[arg(long)]
        include_deleted: bool,
    },
    /// Create a Workspace.
    #[command(after_help = help_footer("Examples:\n  feld workspace create team-a"))]
    Create {
        /// Workspace key of lowercase letters, digits, and `-`, starting with a letter or a digit
        #[arg(value_name = "KEY")]
        key: String,
    },
    /// Delete a Workspace other than `default` that has no live Actor; its key can then name a new
    /// Workspace.
    #[command(after_help = help_footer("Examples:\n  feld workspace delete team-a"))]
    Delete {
        /// Key of the live Workspace to delete
        #[arg(value_name = "KEY")]
        key: String,
    },
    /// Show a Workspace; after a delete, the one deleted last.
    #[command(after_help = help_footer("Examples:\n  feld workspace get team-a"))]
    Get {
        /// Workspace key
        #[arg(value_name = "KEY")]
        key: String,
    },
}

/// Create, change, and read the Actors of the Workspace.
#[derive(Debug, Args)]
pub struct ActorArgs {
    #[command(subcommand)]
    pub command: ActorCommand,
}

#[derive(Debug, Subcommand)]
pub enum ActorCommand {
    Create(ActorCreateArgs),
    Update(ActorUpdateArgs),
    /// Delete an Actor, keeping its history.
    ///
    /// An Actor whose current Session runs is rejected. The name is free for a new Actor at
    /// once, and the deleted one stays readable by its ID with `feld actor get <ID>` and in
    /// `feld actor list --include-deleted`.
    #[command(after_help = help_footer("Examples:\n  feld actor delete reviewer"))]
    Delete(ActorTarget),
    Get(ActorGetArgs),
    #[command(visible_alias = "ls")]
    List(ActorListArgs),
    /// Start a new Session from the launchSpec of an Actor.
    ///
    /// The new Session is the next generation of the Actor, and it replaces a current Session
    /// that has ended. While the current Session still runs the start is rejected with exit code
    /// 1: end it with `feld session stop` or `feld actor restart`, or observe it again with
    /// `feld session observe` when Feld has yet to see that it is gone. The placement and the
    /// outcome are printed.
    #[command(after_help = help_footer("Examples:\n  feld actor start hello"))]
    Start(ActorTarget),
    Attach(AttachArgs),
    /// Stop the running current Session of an Actor and start its next generation.
    ///
    /// The stop is the one `feld session stop` asks for, and once the Execution Provider
    /// observes the Session end, Feld starts the next generation from the launchSpec. An Actor without a
    /// running current Session, or whose current Session is attached, is rejected.
    #[command(after_help = help_footer("Examples:\n  feld actor restart reviewer"))]
    Restart(ActorTarget),
    /// Keep the Schedule of a Job from firing, as an update of its `enabled`.
    ///
    /// No tick fires until `feld actor resume`, and a running Session is untouched. A Job
    /// without a Schedule is rejected.
    #[command(after_help = help_footer("Examples:\n  feld actor pause nightly"))]
    Pause(ActorTarget),
    /// Let the Schedule of a Job fire again, as an update of its `enabled`.
    ///
    /// The next run is computed from now, and the ticks of the pause are not caught up.
    #[command(after_help = help_footer("Examples:\n  feld actor resume nightly"))]
    Resume(ActorTarget),
}

/// Create an Actor: an `agent` that runs a Runtime, or a `job` that runs a program.
///
/// A live Actor of the same name keeps its ID and has its declaration replaced: the
/// launchSpec becomes the one given, and the Schedule follows `--cron` with its enabled state
/// kept, or goes without it. A different kind, an agent without `--runtime` or a job without
/// `-- <COMMAND>...` is rejected before anything is written. An agent starts a Session at once
/// and a job waits; `--start` and `--no-start` turn that around. The creation commits first,
/// so a start that then fails answers `Actor declaration committed; <reason>` with exit code 1.
/// The grants, which only the Admin gives, stay with a replaced declaration without `--grant`,
/// and the accepted senders without `--accept-from`.
#[derive(Debug, Args)]
#[command(after_help = launch_footer(
    r#"Examples:
  feld actor create reviewer --kind agent --runtime claude-code
  feld actor create hello --kind job -- sh -c 'printf "Hello\n"'
  feld actor create hello --kind job --start -- echo hello               create it and run it now
  feld actor create nightly --kind job --cron '0 9 * * *' -- ./nightly   run it every day at 09:00
  feld actor create lead --kind agent --runtime claude-code --grant read --grant control
  feld actor create worker --kind agent --runtime codex --accept-from lead"#
))]
pub struct ActorCreateArgs {
    /// Actor name, unique among the live Actors of the Workspace
    #[arg(value_parser = parse_actor_name)]
    pub name: String,
    /// Kind of the Actor, agent or job; it stays for the life of the Actor
    #[arg(long, value_name = "KIND", value_parser = parse_actor_kind)]
    pub kind: ActorKind,
    /// 5-feld cron expression a Schedule of the Job fires from, such as '0 9 * * *'
    #[arg(long, value_name = "CRON")]
    pub cron: Option<String>,
    /// IANA timezone the Schedule reads the cron in [default: the daemon's timezone]
    #[arg(long, requires = "cron", value_name = "TIMEZONE")]
    pub timezone: Option<String>,
    /// Start a Session once the creation commits, the default for an agent
    #[arg(long, conflicts_with = "no_start")]
    pub start: bool,
    /// Create the Actor without starting a Session, the default for a job
    #[arg(long)]
    pub no_start: bool,
    /// Grant the Actor's Sessions an operation on the other Actors of the Workspace: read, control, manage-agent or manage-job; repeat for more
    #[arg(long = "grant", value_name = "GRANT", value_parser = parse_grant)]
    pub grants: Vec<Grant>,
    /// Accept Messages only from this Actor of the Workspace, besides the Admin; repeat for more [default: every Actor of the Workspace]
    #[arg(long = "accept-from", value_name = "ACTOR", value_parser = parse_actor_name)]
    pub accept_from: Vec<String>,
    #[command(flatten)]
    pub launch: LaunchArgs,
}

/// Change the name, the launchSpec or the Schedule of an Actor.
///
/// `--name` renames the Actor, which keeps its ID, Sessions and mailbox, and is rejected while
/// its current Session runs. Only the given fields change and they hold from the next start or tick, so the current
/// Session runs on untouched. An agent keeps a Runtime and a job a program, so an update that
/// gives the other is rejected. One Execution Provider option replaces every Provider option.
/// The Schedule keeps its enabled state and, without `--timezone`, its timezone; a Job without
/// a Schedule gets one from `--cron`. `--grant` replaces every grant and `--no-grants` removes
/// them. `--accept-from` replaces the accepted senders and `--no-accept-from` accepts every
/// Actor of the Workspace again. At least one option is required.
#[derive(Debug, Args)]
#[command(after_help = launch_footer(
    "Examples:
  feld actor update reviewer --workdir ../other
  feld actor update reviewer --name reviewer-14
  feld actor update nightly --cron '30 2 * * *' --timezone UTC
  feld actor update lead --grant read
  feld actor update worker --accept-from lead --accept-from reviewer"
))]
pub struct ActorUpdateArgs {
    /// Actor to update
    #[arg(value_name = "ACTOR", value_parser = parse_actor_name)]
    pub actor: String,
    /// New name of the Actor
    #[arg(long, value_name = "NAME", value_parser = parse_actor_name)]
    pub name: Option<String>,
    /// 5-feld cron expression of the Schedule; a Job without a Schedule gets one
    #[arg(long, value_name = "CRON")]
    pub cron: Option<String>,
    /// IANA timezone the Schedule reads the cron in
    #[arg(long, value_name = "TIMEZONE")]
    pub timezone: Option<String>,
    /// Grant that replaces every grant of the Actor: read, control, manage-agent or manage-job; repeat for more
    #[arg(long = "grant", value_name = "GRANT", value_parser = parse_grant)]
    pub grants: Vec<Grant>,
    /// Remove every grant of the Actor
    #[arg(long, conflicts_with = "grants")]
    pub no_grants: bool,
    /// Actor of the Workspace that replaces the accepted senders, besides the Admin; repeat for more
    #[arg(long = "accept-from", value_name = "ACTOR", value_parser = parse_actor_name)]
    pub accept_from: Vec<String>,
    /// Accept Messages from every Actor of the Workspace again
    #[arg(long, conflicts_with = "accept_from")]
    pub no_accept_from: bool,
    #[command(flatten)]
    pub launch: LaunchArgs,
}

/// Show one Actor: its ID, launchSpec, Schedule, and Sessions.
///
/// A name reads the live Actor. An ID (actor_...) reads the Actor even after its delete. The
/// Sessions are listed newest first, the current one marked `*`; `feld session get <SESSION>`
/// shows one of them in full.
#[derive(Debug, Args)]
#[command(after_help = help_footer(
    "Examples:
  feld actor get dispatcher
  feld actor get actor_0c93f4..."
))]
pub struct ActorGetArgs {
    /// Actor name, or an Actor ID (actor_...)
    #[arg(value_name = "ACTOR", value_parser = parse_actor_target)]
    pub target: String,
}

/// List the Actors of the Workspace with their latest Session.
///
/// By default only the Actors holding a current Session and the Jobs with an enabled Schedule;
/// `-a` lists every Actor. One row per Actor holds its current Session, else its latest one, with
/// that Session's generation, start kind, outcome, and last observation, the number of waiting
/// Messages, the marks `mail` and `skip`, and the next run.
#[derive(Debug, Args)]
#[command(after_help = help_footer(
    "Examples:
  feld actor list
  feld actor list -a                  every Actor, the stopped ones too
  feld actor list --kind job
  feld actor list --refresh           observe every current Session on its Provider first
  feld actor list -a --include-deleted
                                       the deleted Actors too, with a DELETED column"
))]
pub struct ActorListArgs {
    /// Show every Actor, not only those with a current Session or an enabled Schedule
    #[arg(short = 'a', long)]
    pub all: bool,
    /// Show only the Actors of this kind: agent or job
    #[arg(long, value_name = "KIND", value_parser = parse_actor_kind)]
    pub kind: Option<ActorKind>,
    /// Observe every current Session on its Execution Provider before printing
    #[arg(long)]
    pub refresh: bool,
    /// Include the deleted Actors, together with -a
    #[arg(long)]
    pub include_deleted: bool,
}

pub fn parse_env_variable(value: &str) -> std::result::Result<(String, String), String> {
    value
        .split_once('=')
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .ok_or_else(|| "expected KEY=VALUE".to_owned())
}

pub fn parse_actor_kind(value: &str) -> std::result::Result<ActorKind, String> {
    value.parse()
}

pub fn parse_grant(value: &str) -> std::result::Result<Grant, String> {
    value.parse()
}

pub fn parse_record_kind(value: &str) -> std::result::Result<RecordKind, String> {
    value.parse()
}

#[derive(Clone, Debug, Default, Args)]
pub struct LaunchArgs {
    /// Runtime Adapter the Session runs: claude-code or codex; conflicts with -- <COMMAND>
    #[arg(long, conflicts_with = "command", value_name = "RUNTIME")]
    pub runtime: Option<String>,
    /// Settings the Runtime Adapter reads, as one JSON object; conflicts with -- <COMMAND>
    #[arg(long, value_name = "JSON", conflicts_with = "command")]
    pub runtime_settings: Option<String>,
    /// Let the Runtime read the settings and hooks the workdir itself carries without asking; conflicts with -- <COMMAND>
    #[arg(long, conflicts_with_all = ["command", "no_trust_workdir"])]
    pub trust_workdir: bool,
    /// On update, stop letting the Runtime read the workdir's own settings without asking
    #[arg(long, conflicts_with = "command")]
    pub no_trust_workdir: bool,
    /// Working directory of the Session; a relative path resolves from here [default: the current directory]
    #[arg(long, value_name = "PATH")]
    pub workdir: Option<PathBuf>,
    /// Environment variable the Session starts in, as KEY=VALUE; repeat for more. On update, the given ones replace every declared variable
    #[arg(long = "env", value_name = "KEY=VALUE", value_parser = parse_env_variable)]
    pub env: Vec<(String, String)>,
    /// Execution Provider that places the Session, tmux or herdr [default: the Provider an option names, else the one this CLI runs in, else the only installed one]
    #[arg(long, value_name = "PROVIDER")]
    pub provider: Option<String>,
    /// Node the Session runs on [default: local]
    #[arg(long, value_name = "NODE")]
    pub node: Option<String>,
    /// Program argv to run, given after `--`
    #[arg(last = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ActorTarget {
    /// Actor name in the Workspace
    #[arg(value_name = "ACTOR", value_parser = parse_actor_name)]
    pub actor: String,
}

#[derive(Debug, Args)]
pub struct SessionTarget {
    /// Session ID (session_...), or an Actor name for its current Session
    #[arg(value_name = "TARGET", value_parser = parse_session_target)]
    pub target: String,
}

#[derive(Debug, Args)]
pub struct SessionReadTarget {
    /// Session ID (session_...), or an Actor name for its current Session, else its latest one
    #[arg(value_name = "TARGET", value_parser = parse_session_target)]
    pub target: String,
}

/// Act as the Session this command runs inside, through the Runtime API: show it and receive
/// its Messages.
///
/// The Session is the one of the caller's Session token, from FELD_SESSION_TOKEN,
/// else from the self index of the Execution Provider this process runs in. Outside a Session
/// standard output stays empty, the reason goes to standard error, and the exit code is 1.
#[derive(Debug, Args)]
pub struct SelfArgs {
    #[command(subcommand)]
    pub command: SelfCommand,
}

#[derive(Debug, Subcommand)]
pub enum SelfCommand {
    /// Show the caller's Session.
    ///
    /// A closed Session is refused as session closed, with exit code 1.
    #[command(after_help = help_footer("Examples:\n  feld self get"))]
    Get,
    /// Print the Session token of the caller.
    ///
    /// A Runtime helper uses it to call the daemon API itself. The daemon is not called.
    #[command(after_help = help_footer("Examples:\n  feld self token"))]
    Token,
    /// Receive and read the Messages addressed to the caller.
    Message(SelfMessageArgs),
}

#[derive(Debug, Args)]
pub struct SelfMessageArgs {
    #[command(subcommand)]
    pub command: SelfMessageCommand,
}

#[derive(Debug, Subcommand)]
pub enum SelfMessageCommand {
    /// Read the Messages waiting for the caller's Actor and commit them as read.
    ///
    /// A Session token is required, so this runs inside a Feld Session. Every Message
    /// printed is committed as read, and the next call sees only what arrived after it.
    #[command(after_help = help_footer(
        "Examples:
  feld self message recv
  feld self message recv --wait   hold until a Message arrives"
    ))]
    Recv {
        /// Wait until a Message arrives when none is waiting
        #[arg(long)]
        wait: bool,
    },
}

/// Send, delete, read, and list the Messages between Actors.
#[derive(Debug, Args)]
pub struct MessageArgs {
    #[command(subcommand)]
    pub command: MessageCommand,
}

#[derive(Debug, Subcommand)]
pub enum MessageCommand {
    /// Send a Message to the mailbox of an Actor.
    ///
    /// The Message is committed to the durable mailbox of the Actor and waits there until a
    /// Session of that Actor reads it. The sender is the Actor of the Session token this CLI
    /// resolves, else the Admin.
    #[command(after_help = help_footer(
        "Examples:
  feld message send developer 'please fix the failing test'
  feld message send developer < review.md   the body from standard input
  feld message send developer --label nudge 'is the PR ready?'"
    ))]
    Send {
        /// Actor the Message goes to
        #[arg(value_name = "ACTOR", value_parser = parse_actor_name)]
        actor: String,
        /// Label that names the kind of the Message, in the grammar of Actor names
        #[arg(long, value_name = "LABEL", value_parser = parse_label)]
        label: Option<String>,
        /// Body of the Message [default: standard input]
        #[arg(value_name = "BODY")]
        body: Option<String>,
    },
    /// Delete a Message that no Session has read.
    ///
    /// A Message with a read stays, and the delete is rejected.
    #[command(after_help = help_footer("Examples:\n  feld message delete message_01J..."))]
    Delete(MessageTarget),
    /// Show one Message in full: its route, read state, and body.
    ///
    /// Nothing is recorded, so a Message waiting in the mailbox stays waiting.
    #[command(after_help = help_footer(&format!(
        "{READ_STATE_NOTE}\n\nExamples:\n  feld message get message_01J..."
    )))]
    Get(MessageTarget),
    /// List Messages oldest first, leaving every Message as it is.
    ///
    /// One row per Message holds its sender, recipient, accepted time, read state, and preview.
    /// The mailbox of an Actor is the list narrowed by `--to`.
    #[command(after_help = help_footer(&format!(
        "{READ_STATE_NOTE}

Examples:
  feld message list --to developer             the mailbox of developer
  feld message list --to developer --waiting   the Messages no Session has read"
    )))]
    #[command(visible_alias = "ls")]
    List(MessageListArgs),
}

#[derive(Debug, Args)]
pub struct MessageTarget {
    /// Message ID (message_...)
    #[arg(value_name = "MESSAGE")]
    pub message: String,
}

#[derive(Debug, Args)]
pub struct MessageListArgs {
    /// Show only the Messages this Actor sent
    #[arg(long, value_name = "ACTOR")]
    pub from: Option<String>,
    /// Show only the Messages addressed to this Actor
    #[arg(long, value_name = "ACTOR")]
    pub to: Option<String>,
    /// Show only the Messages sent with this label
    #[arg(long, value_name = "LABEL", value_parser = parse_label)]
    pub label: Option<String>,
    /// Show only the Messages without a read
    #[arg(long)]
    pub waiting: bool,
    /// Keep the Messages accepted at or after this RFC 3339 time
    #[arg(long, value_name = "TIME")]
    pub start: Option<String>,
    /// Keep the Messages accepted before this RFC 3339 time
    #[arg(long, value_name = "TIME")]
    pub end: Option<String>,
}

pub fn parse_bucket(value: &str) -> std::result::Result<u64, String> {
    value
        .parse()
        .ok()
        .filter(|seconds| *seconds >= 60)
        .ok_or_else(|| "the bucket must be a number of seconds, at least 60".to_owned())
}

/// A span back from now, as `--since` takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Since {
    amount: u32,
    unit: char,
}

impl Since {
    /// The range of `feld runtime metrics` unless an option sets it: the window of the 5-hour
    /// quota.
    pub const FIVE_HOURS: Self = Self {
        amount: 5,
        unit: 'h',
    };

    pub fn duration(self) -> chrono::TimeDelta {
        let amount = i64::from(self.amount);
        match self.unit {
            'm' => chrono::TimeDelta::minutes(amount),
            'h' => chrono::TimeDelta::hours(amount),
            _ => chrono::TimeDelta::days(amount),
        }
    }
}

impl std::fmt::Display for Since {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.amount, self.unit)
    }
}

/// A positive number of minutes, hours, or days, up to 100 years: `30m`, `5h`, `7d`.
pub fn parse_since(value: &str) -> std::result::Result<Since, String> {
    value
        .char_indices()
        .last()
        .and_then(|(at, unit)| {
            let amount = value[..at].parse().ok().filter(|amount| *amount > 0)?;
            matches!(unit, 'm' | 'h' | 'd').then_some(Since { amount, unit })
        })
        .filter(|since| since.duration() <= chrono::TimeDelta::days(36_500))
        .ok_or_else(|| {
            "the duration must be a positive number and a unit, m, h, or d: 30m, 5h, 7d".to_owned()
        })
}

pub fn parse_workspace_key(value: &str) -> std::result::Result<String, String> {
    valid_key(value)
        .then(|| value.to_owned())
        .ok_or_else(|| "Workspace keys must match ^[a-z0-9][a-z0-9-]*$".to_owned())
}

pub fn parse_actor_name(value: &str) -> std::result::Result<String, String> {
    valid_key(value)
        .then(|| value.to_owned())
        .ok_or_else(|| "Actor names must match ^[a-z0-9][a-z0-9-]*$".to_owned())
}

/// An Actor name, or an Actor ID (`actor_...`).
pub fn parse_actor_target(value: &str) -> std::result::Result<String, String> {
    parse_name_or_id(value, "actor_")
}

/// An Actor name, or a Session ID (`session_...`).
pub fn parse_session_target(value: &str) -> std::result::Result<String, String> {
    parse_name_or_id(value, "session_")
}

/// The path encodes an ID, so an ID only needs its prefix to stay in its own path segment.
fn parse_name_or_id(value: &str, prefix: &str) -> std::result::Result<String, String> {
    (valid_key(value) || value.strip_prefix(prefix).is_some_and(|id| !id.is_empty()))
        .then(|| value.to_owned())
        .ok_or_else(|| {
            format!("the target must be an Actor name, ^[a-z0-9][a-z0-9-]*$, or an ID, {prefix}...")
        })
}

pub fn parse_label(value: &str) -> std::result::Result<String, String> {
    valid_key(value)
        .then(|| value.to_owned())
        .ok_or_else(|| "Message labels must match ^[a-z0-9][a-z0-9-]*$".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_names_and_targets_follow_the_key_grammar() {
        for value in [
            "..",
            "../../other/actors/foo",
            "a/b",
            "a?b",
            "a#b",
            "",
            "Dev",
        ] {
            assert!(parse_actor_name(value).is_err(), "{value}");
            assert!(parse_actor_target(value).is_err(), "{value}");
            assert!(parse_session_target(value).is_err(), "{value}");
        }
        for value in ["session_", "actor_x"] {
            assert!(parse_session_target(value).is_err(), "{value}");
        }
        assert!(parse_actor_target("session_0c93").is_err());
        assert!(parse_actor_name("dev-2").is_ok());
        assert!(parse_actor_target("actor_0c93").is_ok());
        assert!(parse_session_target("dev-2").is_ok());
        assert!(parse_session_target("session_0c93_subagent_child-7").is_ok());
    }

    #[test]
    fn since_takes_a_positive_number_and_a_unit() {
        for (value, minutes) in [("30m", 30), ("5h", 300), ("7d", 10_080)] {
            let since = parse_since(value).unwrap();
            assert_eq!(since.to_string(), value);
            assert_eq!(since.duration().num_minutes(), minutes);
        }
        for value in [
            "",
            "5",
            "h",
            "0h",
            "-1h",
            "1.5h",
            "5s",
            "5 h",
            "5H",
            "99999999d",
        ] {
            assert!(parse_since(value).is_err(), "{value}");
        }
    }
}
