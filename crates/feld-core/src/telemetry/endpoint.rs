use std::fs;
use std::net::{IpAddr, SocketAddr};

use anyhow::{Context, Result, bail};
use reqwest::Url;

use crate::home::FeldHome;

/// The file in the Feld home that keeps the Collector endpoint.
pub const ENDPOINT_FILE: &str = "collector-endpoint";

/// The Collector endpoint the Feld home keeps across daemon restarts, or None before a
/// daemon first started its Collector there.
pub fn persisted_endpoint(home: &FeldHome) -> Option<String> {
    fs::read_to_string(home.root().join(ENDPOINT_FILE))
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The loopback socket address of the Collector endpoint `endpoint`.
pub fn endpoint_address(endpoint: &str) -> Result<SocketAddr> {
    let url = Url::parse(endpoint).context("parse URL")?;
    if url.scheme() != "http"
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("collector endpoint must be an HTTP origin");
    }
    let host = url.host_str().context("collector endpoint has no host")?;
    let address: IpAddr = host
        .parse()
        .context("collector endpoint host is not an IP address")?;
    if !address.is_loopback() {
        bail!("collector endpoint must be loopback");
    }
    let port = url.port().context("collector endpoint has no port")?;
    Ok(SocketAddr::new(address, port))
}
