//! `feld daemon`.

use std::io::Read;
use std::process::ExitCode;

use feld_daemon::daemon::{self, Presence, Unresponsive};

use crate::args::DaemonCommand;
use crate::client;
use crate::format::daemon::{print_daemon_started, print_daemon_status};
use crate::output::print_done;
use crate::style::Style;
use crate::{CliError, CliResult, Context};

pub async fn run(ctx: &Context, command: DaemonCommand) -> CliResult<ExitCode> {
    let (home, json) = (&ctx.home, ctx.json);
    match command {
        DaemonCommand::Start(args) => {
            let (status, already_running) =
                daemon::start(home, args.port, &args.allow_host).await?;
            if already_running {
                client::warn_version_drift(ctx, &status);
            }
            ctx.print(
                &serde_json::json!({
                    "alreadyRunning": already_running,
                    "daemon": status,
                    "ui": status.web_ui,
                }),
                |_| {
                    let style = Style::stdout(json);
                    if already_running {
                        print_done(&style, "Daemon already running.");
                    } else {
                        print_done(&style, "Daemon started.");
                    }
                    print_daemon_started(&status, &style);
                },
            )?;
            Ok(ExitCode::SUCCESS)
        }
        DaemonCommand::Status => match client::probe(ctx).await {
            Err(CliError::Operational(error)) if error.is::<Unresponsive>() => {
                let unresponsive = error.downcast::<Unresponsive>().expect("checked above");
                ctx.print(
                    &serde_json::json!({
                        "state": "unresponsive",
                        "pid": unresponsive.pid,
                        "home": home.root(),
                        "api": unresponsive.api,
                    }),
                    |_| {
                        println!("Daemon");
                        println!("  state:      unresponsive");
                        println!("  pid:        {}", unresponsive.pid);
                        println!("  home:       {}", home.root().display());
                        println!("  api:        {}", unresponsive.api);
                        println!(
                            "  next:       kill {} \u{b7} feld daemon start",
                            unresponsive.pid
                        );
                    },
                )?;
                Ok(ExitCode::from(2))
            }
            Err(error) => Err(error),
            Ok(Presence::Running(status)) => {
                client::warn_version_drift(ctx, &status);
                ctx.print(&status, |status| {
                    println!("Daemon");
                    print_daemon_status(ctx, status);
                })?;
                Ok(ExitCode::SUCCESS)
            }
            Ok(Presence::NotRunning) => {
                ctx.print(
                    &serde_json::json!({
                        "state": "stopped",
                        "home": home.root(),
                    }),
                    |_| {
                        println!("Daemon is not running.");
                        println!("  home:       {}", home.root().display());
                    },
                )?;
                Ok(ExitCode::from(1))
            }
        },
        DaemonCommand::Stop => {
            let stopped = daemon::stop(home).await?;
            ctx.print(&serde_json::json!({ "stopped": stopped }), |_| {
                if stopped {
                    println!("Daemon stopped. Sessions keep running on their providers.");
                } else {
                    println!("Daemon is not running.");
                }
            })?;
            Ok(ExitCode::SUCCESS)
        }
        DaemonCommand::Restart(args) => {
            let (stopped, status) = daemon::restart(home, args.port, &args.allow_host).await?;
            ctx.print(
                &serde_json::json!({ "stopped": stopped, "daemon": status, "ui": status.web_ui }),
                |_| {
                    let style = Style::stdout(json);
                    if stopped {
                        print_done(
                            &style,
                            "Daemon stopped. Sessions keep running on their providers.",
                        );
                    } else {
                        println!("Daemon is not running.");
                    }
                    print_done(&style, "Daemon started.");
                    print_daemon_started(&status, &style);
                },
            )?;
            Ok(ExitCode::SUCCESS)
        }
        DaemonCommand::Run {
            local_token_stdin,
            port,
            carried_port,
            allow_host,
        } => {
            let local_token = if local_token_stdin {
                let mut token = String::new();
                std::io::stdin()
                    .read_to_string(&mut token)
                    .map_err(anyhow::Error::from)?;
                let token = token.trim().to_owned();
                (!token.is_empty()).then_some(token)
            } else {
                None
            };
            daemon::run(
                home.clone(),
                env!("FELD_BUILD_VERSION"),
                local_token,
                port,
                carried_port,
                allow_host,
            )
            .await?;
            Ok(ExitCode::SUCCESS)
        }
    }
}
