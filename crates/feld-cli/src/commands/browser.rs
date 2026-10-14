//! `feld browser`.

use std::process::ExitCode;

use feld_core::model::Browser;
use feld_daemon::api::{BrowserList, BrowserResponse};

use crate::args::BrowserCommand;
use crate::client::{Daemon, percent_encode};
use crate::output::print_table_or;
use crate::{CliResult, Context};

pub async fn run(ctx: &Context, command: BrowserCommand) -> CliResult<ExitCode> {
    let daemon = Daemon::connect_local(ctx).await?;
    match command {
        BrowserCommand::List => {
            let listing: BrowserList = daemon.get("/v1/ui/browsers").await?;
            ctx.print(&listing, |listing| {
                let rows: Vec<Vec<String>> = listing
                    .browsers
                    .iter()
                    .map(|browser| {
                        vec![
                            browser.id.clone(),
                            browser.name.clone(),
                            ctx.local_time(&browser.paired_at),
                            ctx.local_time(&browser.last_used_at),
                            ctx.local_time(&browser.expires_at),
                        ]
                    })
                    .collect();
                print_table_or(
                    "No browsers are signed in.",
                    &["ID", "NAME", "SIGNED_IN", "LAST_USED", "EXPIRES"],
                    &rows,
                );
            })?;
        }
        BrowserCommand::Revoke { target } => {
            let listing: BrowserList = daemon.get("/v1/ui/browsers").await?;
            let ids = revoke_targets(&listing.browsers, &target);
            if ids.is_empty() {
                return Err(rejected!(
                    "No browser token in force has the id or name '{target}'"
                ));
            }
            let mut revoked = Vec::with_capacity(ids.len());
            for id in ids {
                let response: BrowserResponse = daemon
                    .delete(&format!("/v1/ui/browsers/{}", percent_encode(&id)))
                    .await?;
                revoked.push(response.browser);
            }
            let revoked = BrowserList { browsers: revoked };
            ctx.print(&revoked, |revoked| {
                for browser in &revoked.browsers {
                    println!("Browser signed out.");
                    println!("  id:         {}", browser.id);
                    println!("  name:       {}", browser.name);
                    println!(
                        "  revoked_at: {}",
                        ctx.local_time_or_dash(browser.revoked_at.as_deref())
                    );
                }
            })?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The ids `feld browser revoke <target>` revokes: the token whose id is the target, else
/// every token named the target.
fn revoke_targets(browsers: &[Browser], target: &str) -> Vec<String> {
    if let Some(browser) = browsers.iter().find(|browser| browser.id == target) {
        return vec![browser.id.clone()];
    }
    browsers
        .iter()
        .filter(|browser| browser.name == target)
        .map(|browser| browser.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser(id: &str, name: &str) -> Browser {
        Browser {
            id: id.to_owned(),
            name: name.to_owned(),
            paired_at: "2026-10-01T01:42:31Z".to_owned(),
            last_used_at: "2026-10-01T02:05:12Z".to_owned(),
            expires_at: "2026-10-08T02:05:12Z".to_owned(),
            revoked_at: None,
        }
    }

    #[test]
    fn revoke_targets_an_id_or_every_token_of_a_name() {
        let browsers = [
            browser("browser_a", "laptop"),
            browser("browser_b", "local"),
            browser("browser_c", "laptop"),
            browser("browser_d", "browser_b"),
        ];
        assert_eq!(revoke_targets(&browsers, "browser_a"), ["browser_a"]);
        assert_eq!(
            revoke_targets(&browsers, "laptop"),
            ["browser_a", "browser_c"]
        );
        assert_eq!(revoke_targets(&browsers, "browser_b"), ["browser_b"]);
        assert!(revoke_targets(&browsers, "phone").is_empty());
    }
}
