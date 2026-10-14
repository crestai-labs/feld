//! `feld provider`.

use std::process::ExitCode;

use feld_daemon::api::ProviderList;

use crate::client::Daemon;
use crate::output::print_table_or;
use crate::{CliResult, Context};

pub async fn list(ctx: &Context) -> CliResult<ExitCode> {
    let daemon = Daemon::connect(ctx).await?;
    let listing: ProviderList = daemon.get("/v1/providers").await?;
    ctx.print(&listing, |listing| {
        let rows: Vec<Vec<String>> = listing
            .providers
            .iter()
            .map(|provider| {
                vec![
                    provider.plugin.name.clone(),
                    provider.plugin.label.clone(),
                    provider.nodes.join(","),
                    provider.running_sessions.to_string(),
                ]
            })
            .collect();
        print_table_or(
            "No Execution Providers are installed.",
            &["NAME", "LABEL", "NODES", "RUNNING_SESSIONS"],
            &rows,
        );
    })?;
    Ok(ExitCode::SUCCESS)
}
