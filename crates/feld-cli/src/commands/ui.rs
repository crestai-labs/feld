//! `feld ui` and `feld ui pair`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use chrono::{DateTime, Utc};
use feld_core::home::FeldHome;
use feld_core::model::PairingCode;
use feld_daemon::api::WorkspaceResponse;
use serde::Serialize;

use crate::args::{UiArgs, UiCommand};
use crate::client::{Daemon, percent_encode};
use crate::{CliResult, Context};

/// The name of the browser token `feld ui` pairs the browser of this device with.
const LOCAL_BROWSER_NAME: &str = "local";

#[derive(Serialize)]
struct PairingCodeRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
}

pub async fn run(ctx: &Context, args: UiArgs) -> CliResult<ExitCode> {
    match args.command {
        Some(UiCommand::Pair { name }) => pair(ctx, name.as_deref()).await,
        None => open(ctx, args.print_url).await,
    }
}

async fn issue_code(daemon: &Daemon, name: Option<&str>) -> CliResult<PairingCode> {
    daemon
        .post_json("/v1/ui/pairing-codes", &PairingCodeRequest { name })
        .await
}

async fn pair(ctx: &Context, name: Option<&str>) -> CliResult<ExitCode> {
    let daemon = Daemon::connect_local(ctx).await?;
    let issued = issue_code(&daemon, name).await?;
    ctx.print(&issued, |issued| {
        println!("Sign-in code issued.");
        println!("  code:       {}", issued.code);
        println!("  name:       {}", issued.name);
        println!("  expires_at: {}", ctx.local_time(&issued.expires_at));
        println!(
            "Sign in to the Web UI of the other device with the code within 5 minutes. It works once."
        );
    })?;
    Ok(ExitCode::SUCCESS)
}

async fn open(ctx: &Context, print_url: bool) -> CliResult<ExitCode> {
    let workspace = &ctx.workspace;
    let daemon = Daemon::connect_local(ctx).await?;
    let response: WorkspaceResponse = daemon
        .get(&format!("/v1/workspaces/{}", percent_encode(workspace)))
        .await?;
    if response.workspace.deleted_at.is_some() {
        return Err(rejected!("Workspace '{workspace}' is deleted"));
    }
    let issued = issue_code(&daemon, Some(LOCAL_BROWSER_NAME)).await?;
    let url = format!(
        "{}/ui/pair?workspace={}#code={}",
        daemon.api,
        percent_encode(workspace),
        percent_encode(&issued.code)
    );
    remove_expired_pages(&ctx.home.ui_pair_dir());
    if !print_url {
        let page = write_pairing_page(&ctx.home, &url, &issued.expires_at)?;
        open_browser(&page)?;
    }
    ctx.print(
        &serde_json::json!({ "url": url, "opened": !print_url }),
        |_| {
            if print_url {
                println!("{url}");
            } else {
                println!("Opened Feld Web UI.");
            }
        },
    )?;
    Ok(ExitCode::SUCCESS)
}

/// The command `feld ui` opens a URL with, and the arguments that go before the URL:
/// `BROWSER`, else the opener of the platform.
pub fn browser_command() -> (String, &'static [&'static str]) {
    if let Ok(browser) = std::env::var("BROWSER") {
        (browser, &[])
    } else if cfg!(target_os = "macos") {
        ("open".to_owned(), &[])
    } else if cfg!(target_os = "windows") {
        ("cmd".to_owned(), &["/C", "start", ""])
    } else {
        ("xdg-open".to_owned(), &[])
    }
}

/// Writes an owner-only page that moves the browser to `url`, so the pairing code stays out
/// of the browser's command line, which other users of the machine can read. The page is
/// named by the expiry of its code.
fn write_pairing_page(home: &FeldHome, url: &str, expires_at: &str) -> Result<PathBuf> {
    let dir = home.ui_pair_dir();
    home.create_owner_dir(&dir)?;
    let now = unix_now();
    let expires = DateTime::parse_from_rfc3339(expires_at)
        .map(|time| time.with_timezone(&Utc).timestamp())
        .unwrap_or(now.as_secs() as i64);
    let page = dir.join(format!(
        "{expires}-{}-{}.html",
        std::process::id(),
        now.subsec_nanos()
    ));
    let url = html_escape(url);
    let contents = format!(
        "<!doctype html>\n<meta charset=\"utf-8\">\n<meta name=\"referrer\" content=\"no-referrer\">\n\
         <meta http-equiv=\"refresh\" content=\"0;url={url}\">\n<title>Feld</title>\n\
         <a href=\"{url}\">Open Feld Web UI</a>\n"
    );
    home.write_owner_file(&page, contents.as_bytes())?;
    Ok(page)
}

fn unix_now() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

/// Removes the pages under `dir` whose code has expired.
fn remove_expired_pages(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = unix_now().as_secs();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let expires = name
            .to_str()
            .and_then(|name| name.split('-').next())
            .and_then(|expires| expires.parse::<u64>().ok());
        if expires.is_none_or(|expires| expires < now) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn open_browser(page: &Path) -> Result<()> {
    let (program, extra_args) = browser_command();
    ProcessCommand::new(&program)
        .args(extra_args)
        .arg(page)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("open Web UI with {program}"))?;
    Ok(())
}
