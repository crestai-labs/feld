//! The Collector child process, which `feld collector-run` starts.

use anyhow::{Context, Result, bail};
use tokio::net::TcpListener;

use feld_core::home::{FeldHome, read_json};
use feld_core::telemetry::endpoint_address;

use super::{CollectorConfig, PARENT_CHECK_INTERVAL, receiver, validate_loopback_url};

/// Runs the hidden Collector child process entry point.
pub async fn run_collector_child(home: &FeldHome) -> Result<()> {
    let config: CollectorConfig = read_json(&home.collector_config_path(), "collector config")?;
    if direct_parent_pid() != config.parent_pid {
        bail!("collector parent daemon changed before startup");
    }
    validate_loopback_url(&config.daemon_api).context("validate collector daemon API")?;
    let address = endpoint_address(&config.endpoint).context("validate collector endpoint")?;
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("bind collector endpoint {address}"))?;
    receiver::serve(listener, config).await
}

pub(super) async fn wait_for_parent_exit(expected_parent: u32) {
    loop {
        if direct_parent_pid() != expected_parent {
            return;
        }
        tokio::time::sleep(PARENT_CHECK_INTERVAL).await;
    }
}

fn direct_parent_pid() -> u32 {
    std::os::unix::process::parent_id()
}
