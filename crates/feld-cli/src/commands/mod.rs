//! The commands, one module per noun.

pub mod actor;
pub mod attach;
pub mod browser;
pub mod daemon;
pub mod doctor;
pub mod env;
pub mod init;
pub mod message;
pub mod program_capture;
pub mod provider;
pub mod record;
pub mod runtime;
pub mod self_session;
pub mod session;
pub mod ui;
pub mod uninstall;
pub mod workspace;

use super::{CliResult, Context};

/// How often `--follow` asks for what was added.
const FOLLOW_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// `--bucket` shapes the series `--json` prints; the table shows the whole range as one value.
/// The check sits here because clap's `requires` misses a global `--json` given before the
/// subcommand.
fn check_bucket(ctx: &Context, bucket: Option<u64>) -> CliResult<()> {
    if bucket.is_some() && !ctx.json {
        return Err(operational!(
            "--bucket shapes the series --json prints; add --json"
        ));
    }
    Ok(())
}

/// The query of a metrics read: the range, and the bucket of its series.
fn metrics_query(
    start: Option<&String>,
    end: Option<&String>,
    bucket: Option<u64>,
) -> Vec<(&'static str, String)> {
    let mut query = Vec::new();
    if let Some(start) = start {
        query.push(("start", start.clone()));
    }
    if let Some(end) = end {
        query.push(("end", end.clone()));
    }
    if let Some(bucket) = bucket {
        query.push(("bucket", bucket.to_string()));
    }
    query
}
