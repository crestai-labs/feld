//! The daemon as a person reads it.

use feld_daemon::api::DaemonStatus;

use crate::Context;
use crate::client::plugin_names;
use crate::output::listed;
use crate::style::Style;

pub fn print_daemon_started(status: &DaemonStatus, style: &Style) {
    let line = |key: &str, value: &str| {
        println!(
            "  {}{value}",
            style.dim(&format!("{:<12}", format!("{key}:")))
        );
    };
    line("pid", &status.pid.to_string());
    line("home", &status.home);
    line("api", &status.api);
    if let Some(web_ui) = &status.web_ui {
        line("ui", &web_ui_line(web_ui, style));
    }
    if !status.allowed_hosts.is_empty() {
        line("allow_host", &status.allowed_hosts.join(", "));
    }
    line("timezone", &status.timezone);
    line(
        "restored",
        &format!(
            "{} actors, {} open sessions, {} schedules",
            status.restored.actors, status.restored.running_sessions, status.restored.schedules
        ),
    );
    // A kind with nothing installed is shown, so that the next step is not a refusal.
    if status.runtimes.is_empty() {
        line("runtimes", "(none)");
    }
    if status.providers.is_empty() {
        line("providers", "(none)");
    }
}

/// The Web UI's URL, and how a browser that has not opened it signs in. The pairing code
/// stays out of it: `feld ui` issues one for the browser it opens.
pub fn web_ui_line(web_ui: &str, style: &Style) -> String {
    format!(
        "{web_ui}  {}  {}",
        style.dim("\u{b7}"),
        style.dim("first time in this browser: run `feld ui` to sign in")
    )
}

pub fn print_daemon_status(ctx: &Context, status: &DaemonStatus) {
    println!("  state:      {}", status.state);
    println!("  version:    {}", status.version);
    println!("  pid:        {}", status.pid);
    println!("  home:       {}", status.home);
    println!("  api:        {}", status.api);
    if let Some(web_ui) = &status.web_ui {
        println!("  web_ui:     {web_ui}");
    }
    println!("  allow_host: {}", listed(status.allowed_hosts.clone()));
    if let Some(otlp) = &status.otlp {
        println!("  otlp:       {otlp}");
    }
    println!("  timezone:   {}", status.timezone);
    println!("  runtimes:   {}", listed(plugin_names(&status.runtimes)));
    println!("  providers:  {}", listed(plugin_names(&status.providers)));
    if !status.nodes.is_empty() {
        println!("  nodes:      {}", status.nodes.join(", "));
    }
    let scheduler = &status.scheduler;
    let restarts = match &scheduler.last_restart {
        Some(last) => format!(
            "restarts {} (last {}: {})",
            scheduler.restarts,
            ctx.local_time(&last.at),
            last.reason
        ),
        None => format!("restarts {}", scheduler.restarts),
    };
    println!(
        "  scheduler:  woke {} · {restarts}",
        ctx.local_time_or_dash(scheduler.woke_at.as_deref())
    );
    let ticks: Vec<String> = scheduler
        .planned
        .iter()
        .map(|tick| {
            format!(
                "{}/{} {}",
                tick.workspace,
                tick.actor,
                ctx.local_time(&tick.planned)
            )
        })
        .collect();
    println!("  ticks:      {}", listed(ticks));
}
