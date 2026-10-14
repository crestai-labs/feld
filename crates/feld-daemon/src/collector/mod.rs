use std::fs;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};

use feld_core::home::{FeldHome, absent_as_none};
use feld_core::telemetry::{ENDPOINT_FILE, endpoint_address};

mod child;
mod queue;
mod receiver;
mod tokens;

pub use child::run_collector_child;
pub(crate) use receiver::{MAX_ENVELOPE_BYTES, MAX_REQUEST_BYTES};

const ORPHAN_RELEASE_TIMEOUT: Duration = Duration::from_secs(2);
const PARENT_CHECK_INTERVAL: Duration = Duration::from_millis(25);

/// A daemon-owned child process that receives OTLP/HTTP on loopback.
pub struct Collector {
    endpoint: String,
    child: Option<Child>,
}

impl Collector {
    pub async fn start(home: &FeldHome, daemon_api: &str, internal_token: &str) -> Result<Self> {
        if internal_token.is_empty() {
            bail!("collector internal token is empty");
        }
        validate_loopback_url(daemon_api).context("validate collector daemon API")?;

        home.create()?;
        let endpoint_path = home.root().join(ENDPOINT_FILE);
        let configured_endpoint = absent_as_none(fs::read_to_string(&endpoint_path))
            .context("read persisted collector endpoint")?
            .map(|value| value.trim().to_owned());

        let (endpoint, address) = match configured_endpoint.as_deref() {
            Some(endpoint) => (
                endpoint.to_owned(),
                endpoint_address(endpoint)
                    .with_context(|| format!("read persisted collector endpoint {endpoint}"))?,
            ),
            None => {
                let reservation = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                    .context("reserve collector endpoint")?;
                let address = reservation
                    .local_addr()
                    .context("read collector endpoint")?;
                (format!("http://{address}"), address)
            }
        };
        wait_for_endpoint(address, configured_endpoint.is_some()).await?;
        if configured_endpoint.is_none() {
            home.write_owner_file_atomic(&endpoint_path, endpoint.as_bytes())
                .context("persist collector endpoint")?;
        }

        let config = CollectorConfig {
            endpoint: endpoint.clone(),
            daemon_api: daemon_api.trim_end_matches('/').to_owned(),
            internal_token: internal_token.to_owned(),
            parent_pid: std::process::id(),
        };
        let config_bytes = serde_json::to_vec(&config).context("encode collector config")?;
        home.write_owner_file_atomic(&home.collector_config_path(), &config_bytes)
            .context("write collector config")?;

        let executable = std::env::current_exe().context("resolve feld executable")?;
        let mut child = Command::from(feld_core::process::command(executable))
            .arg("--home")
            .arg(home.root())
            .arg("collector-run")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("start collector child process")?;

        let ready = async {
            loop {
                if let Some(status) = child.try_wait().context("inspect collector child")? {
                    bail!("collector child exited during startup with {status}");
                }
                if tokio::net::TcpStream::connect(address).await.is_ok() {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        match tokio::time::timeout(Duration::from_secs(2), ready).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(error);
            }
            Err(error) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(error).context("collector child did not become ready");
            }
        }

        Ok(Self {
            endpoint,
            child: Some(child),
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub async fn wait_for_exit(&mut self) -> Result<ExitStatus> {
        self.child
            .as_mut()
            .context("Collector child is unavailable")?
            .wait()
            .await
            .context("wait for Collector child")
    }

    pub async fn shutdown(mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.start_kill();
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectorConfig {
    endpoint: String,
    daemon_api: String,
    internal_token: String,
    parent_pid: u32,
}

async fn wait_for_endpoint(address: SocketAddr, tolerate_orphan_race: bool) -> Result<()> {
    let deadline = Instant::now() + ORPHAN_RELEASE_TIMEOUT;
    loop {
        match std::net::TcpListener::bind(address) {
            Ok(listener) => {
                drop(listener);
                return Ok(());
            }
            Err(error)
                if tolerate_orphan_race
                    && error.kind() == ErrorKind::AddrInUse
                    && Instant::now() < deadline =>
            {
                tokio::time::sleep(PARENT_CHECK_INTERVAL).await;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("collector endpoint {address} is unavailable"));
            }
        }
    }
}

fn validate_loopback_url(value: &str) -> Result<()> {
    let url = Url::parse(value).context("parse URL")?;
    if url.scheme() != "http" {
        bail!("collector daemon API must use http");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("collector daemon API must not contain a user or password");
    }
    let host = url.host_str().context("collector daemon API has no host")?;
    let address: IpAddr = host
        .parse()
        .context("collector daemon API host is not an IP address")?;
    if !address.is_loopback() {
        bail!("collector daemon API must be loopback");
    }
    if url.port_or_known_default().is_none() {
        bail!("collector daemon API has no port");
    }
    Ok(())
}
