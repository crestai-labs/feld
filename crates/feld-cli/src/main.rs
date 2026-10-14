//! The `feld` command: the command line, the daemon client it speaks with, and what it
//! prints.

use std::ffi::OsString;
use std::process::ExitCode;
use std::sync::{Once, OnceLock};

use chrono_tz::Tz;
use clap::error::ErrorKind;
use clap::{CommandFactory, FromArgMatches};
use feld_core::home::{FELD_WORKSPACE_ENV, FeldHome};
use feld_core::plugin::Registry;

/// `println!` through [`output::write_stdout`], which ends the command quietly once the
/// reader of stdout has gone.
macro_rules! println {
    () => {
        $crate::output::write_stdout(format_args!("\n"))
    };
    ($($arg:tt)*) => {
        $crate::output::write_stdout(format_args!("{}\n", format_args!($($arg)*)))
    };
}

/// A refusal: the daemon or this CLI declined the request, which exits 1.
macro_rules! rejected {
    ($($arg:tt)*) => {
        $crate::CliError::Rejected(format!($($arg)*))
    };
}

/// A local step that failed, which exits 2.
macro_rules! operational {
    ($($arg:tt)*) => {
        $crate::CliError::Operational(anyhow::anyhow!($($arg)*))
    };
}

pub mod args;
mod ask;
mod client;
mod codex_sandbox;
mod commands;
mod format;
mod launch;
mod output;
mod provider_options;
mod shell_file;
mod spool;
mod style;

use args::{Cli, Command, DoctorArgs, EnvArgs, ProviderCommand, RecordCommand};
use output::print_error;
use provider_options::{ProviderOption, split_provider_options};

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("{0}")]
    Rejected(String),
    #[error(transparent)]
    Operational(#[from] anyhow::Error),
    /// An error with the step a person takes next, which the output shows as `next:`.
    #[error("{0}")]
    WithNext(Box<CliError>, String),
}

impl CliError {
    fn after_commit(self, stage: &str) -> Self {
        match self {
            Self::Rejected(message) => Self::Rejected(format!("{stage} committed; {message}")),
            Self::Operational(error) => {
                Self::Operational(error.context(format!("{stage} committed")))
            }
            Self::WithNext(error, next) => {
                Self::WithNext(Box::new(error.after_commit(stage)), next)
            }
        }
    }

    /// This error, with the step a person takes next.
    fn with_next(self, next: impl Into<String>) -> Self {
        Self::WithNext(Box::new(self), next.into())
    }

    /// The kind `--json` names, the message, and the exit code.
    fn report(&self) -> (&'static str, String, u8) {
        match self {
            Self::Rejected(message) => ("rejected", message.clone(), 1),
            Self::Operational(error) => ("operational", format!("{error:#}"), 2),
            Self::WithNext(error, _) => error.report(),
        }
    }
}

type CliResult<T> = std::result::Result<T, CliError>;

/// What every command runs with: the Feld home, the Workspace and output the command line
/// chose, and what the output reads Plugins and times with.
struct Context {
    home: FeldHome,
    workspace: String,
    json: bool,
    /// The installed Plugins this process reads, for the places that only display what a
    /// Provider says. The commands that call an operation load their own registry with the
    /// daemon API, which a Plugin needs in its environment.
    plugins: Registry,
    /// The timezone the human-readable output reads times in. A daemon names its own local
    /// timezone in its status, and this machine answers for it until one does.
    timezone: OnceLock<Tz>,
    /// Whether this command has told that the daemon runs another version than this CLI.
    version_warned: Once,
}

impl Context {
    fn new(home: FeldHome, workspace: String, json: bool) -> Self {
        let plugins = Registry::load(&home, None);
        Self {
            home,
            workspace,
            json,
            plugins,
            timezone: OnceLock::new(),
            version_warned: Once::new(),
        }
    }

    /// Reads times in the timezone the daemon runs in, from here on.
    fn adopt_timezone(&self, timezone: &str) {
        if let Ok(timezone) = timezone.parse::<Tz>() {
            let _ = self.timezone.set(timezone);
        }
    }

    fn timezone(&self) -> Tz {
        *self.timezone.get_or_init(|| {
            feld_core::time::local_timezone()
                .and_then(|name| name.parse().ok())
                .unwrap_or(Tz::UTC)
        })
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    run().await
}

async fn run() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().collect();
    let json = arguments
        .iter()
        .take_while(|argument| *argument != "--")
        .any(|argument| argument == "--json");
    let (arguments, provider_options) = match split_provider_options(arguments) {
        Ok(split) => split,
        // An argument error, told as clap tells one.
        Err(error) => {
            print_error(json, "operational", &format!("{error:#}"), None);
            return ExitCode::from(2);
        }
    };
    // An empty FELD_WORKSPACE reads as unset, so the Workspace falls back to `default`.
    let mut command = Cli::command();
    if std::env::var_os(FELD_WORKSPACE_ENV).is_some_and(|value| value.is_empty()) {
        command = command.mut_arg("workspace", |arg| arg.env(None));
    }
    let mut cli = match parse(command, arguments) {
        Ok(cli) => cli,
        Err(error) if json && error.exit_code() != 0 => {
            print_error(true, "operational", &error.to_string(), None);
            return ExitCode::from(2);
        }
        Err(error) => {
            let exit_code = error.exit_code();
            let _ = error.print();
            return ExitCode::from(exit_code as u8);
        }
    };
    let json = cli.json;
    // A bare `feld` shows the help, which leaves the hidden internal commands out.
    let Some(command) = cli.command.take() else {
        if json {
            print_error(
                true,
                "operational",
                "a command is required; see `feld --help`",
                None,
            );
        } else {
            eprint!("{}", Cli::command().render_help());
        }
        return ExitCode::from(2);
    };
    match execute(cli, command, provider_options).await {
        Ok(success) => success,
        Err(error) => report_error(json, &error),
    }
}

/// Prints `error` as every command does, and answers the exit code it ends with.
fn report_error(json: bool, error: &CliError) -> ExitCode {
    let (kind, message, code) = error.report();
    let next = match error {
        CliError::WithNext(_, next) => Some(next.as_str()),
        _ => None,
    };
    print_error(json, kind, &message, next);
    ExitCode::from(code)
}

/// Parses the command line. clap names the options that hold a value in the Usage line of an
/// error, the ones read from the environment too, so an error that also arises without the
/// environment is told with that parse's Usage line, which keeps to what the command line gave.
/// The help keeps the environment it shows.
fn parse(command: clap::Command, arguments: Vec<OsString>) -> Result<Cli, clap::Error> {
    fn parse_from(command: clap::Command, arguments: Vec<OsString>) -> Result<Cli, clap::Error> {
        command
            .try_get_matches_from(arguments)
            .and_then(|mut matches| Cli::from_arg_matches_mut(&mut matches))
    }
    fn without_env(command: clap::Command) -> clap::Command {
        command
            .mut_args(|arg| arg.env(None))
            .mut_subcommands(without_env)
    }

    let error = match parse_from(command.clone(), arguments.clone()) {
        Ok(cli) => return Ok(cli),
        Err(error) => error,
    };
    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            | ErrorKind::DisplayVersion
    ) {
        return Err(error);
    }
    match parse_from(without_env(command), arguments) {
        Err(bare) if bare.kind() == error.kind() => Err(bare),
        _ => Err(error),
    }
}

async fn execute(cli: Cli, command: Command, options: Vec<ProviderOption>) -> CliResult<ExitCode> {
    let ctx = Context::new(FeldHome::resolve(cli.home)?, cli.workspace, cli.json);
    let ctx = &ctx;
    match command {
        Command::Init(args) => commands::init::run(ctx, args).await,
        Command::Uninstall(args) => commands::uninstall::run(ctx, args).await,
        Command::Doctor(DoctorArgs) => commands::doctor::run(ctx).await,
        Command::Daemon(args) => commands::daemon::run(ctx, args.command).await,
        Command::Workspace(args) => commands::workspace::run(ctx, args.command).await,
        Command::Actor(args) => commands::actor::run(ctx, args.command, &options).await,
        Command::Record(args) => match args.command {
            RecordCommand::List(args) => commands::record::list(ctx, args).await,
        },
        Command::Message(args) => commands::message::run(ctx, args.command).await,
        Command::Ui(args) => commands::ui::run(ctx, args).await,
        Command::Browser(args) => commands::browser::run(ctx, args.command).await,
        Command::Env(EnvArgs) => commands::env::run(ctx),
        Command::Session(args) => commands::session::run(ctx, args.command).await,
        Command::Runtime(args) => commands::runtime::run(ctx, args.command).await,
        Command::Provider(args) => match args.command {
            ProviderCommand::List => commands::provider::list(ctx).await,
        },
        Command::SelfSession(args) => commands::self_session::run(ctx, args.command).await,
        Command::CollectorRun => {
            feld_daemon::collector::run_collector_child(&ctx.home).await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::ProgramCapture(args) => {
            commands::program_capture::run(args)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::args::Cli;
    use clap::CommandFactory;

    /// Every command and option a reader can see answers what it is for, so that the CLI
    /// explains itself without the reference.
    #[test]
    fn every_visible_command_and_option_carries_help() {
        fn walk(command: &clap::Command, path: &str) {
            assert!(command.get_about().is_some(), "{path} has no about");
            for argument in command.get_arguments() {
                assert!(
                    argument.is_hide_set() || argument.get_help().is_some(),
                    "{path} <{}> has no help",
                    argument.get_id()
                );
            }
            for subcommand in command.get_subcommands() {
                if subcommand.is_hide_set() {
                    continue;
                }
                walk(subcommand, &format!("{path} {}", subcommand.get_name()));
            }
        }

        walk(&Cli::command(), "feld");
    }

    /// `ls` reaches every `list`, so that a hand used to docker and gh lands on the same table.
    #[test]
    fn every_list_command_answers_to_ls() {
        let mut seen = 0;
        for noun in Cli::command().get_subcommands() {
            let Some(list) = noun.find_subcommand("list") else {
                continue;
            };
            assert!(
                list.get_visible_aliases().any(|alias| alias == "ls"),
                "feld {} list has no ls alias",
                noun.get_name()
            );
            seen += 1;
        }
        assert_eq!(seen, 8);
    }
}
