//! The gauges a Runtime Adapter's parts make of what the Runtime's OTel leaves out, and
//! their delivery to the Collector of the Feld home, as `metrics.md` describes.

use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use prost::Message;
use reqwest::header::CONTENT_TYPE;
use serde_json::Value;

use crate::home::FeldHome;
use crate::telemetry::{ENDPOINT_FILE, endpoint_address, proto, telemetry_token};

/// One gauge a Plugin helper reports to the Collector. The instrument names are the
/// Plugin's; what core provides is the delivery.
#[derive(Clone, Debug)]
pub struct GaugePoint {
    pub name: String,
    pub value: f64,
    pub attributes: Vec<(String, String)>,
}

impl GaugePoint {
    pub fn new(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value,
            attributes: Vec::new(),
        }
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.push((key.into(), value.into()));
        self
    }

    /// Names the account a quota report belongs to, when the configuration root tells it.
    pub fn with_account(self, account: Option<&Account>) -> Self {
        let Some(account) = account else {
            return self;
        };
        let point = self.with(ACCOUNT_ID, account.id.as_str());
        match &account.name {
            Some(name) => point.with(ACCOUNT_NAME, name.as_str()),
            None => point,
        }
    }
}

/// The attributes a quota gauge names its account by.
pub const ACCOUNT_ID: &str = "account.id";
pub const ACCOUNT_NAME: &str = "account.name";

/// The account a Runtime runs under, as its configuration root tells it: an ID, and a
/// name a person reads it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub name: Option<String>,
}

impl Account {
    /// An account from the values a file holds, when it names an ID.
    pub fn of(id: Option<&Value>, name: Option<&Value>) -> Option<Self> {
        let text = |value: Option<&Value>| {
            value
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        Some(Self {
            id: text(id)?,
            name: text(name),
        })
    }
}

/// Sends gauges as one OTLP/HTTP metrics export to the Collector this Feld home runs,
/// signed with the telemetry token of the Session token `token`, which the Collector
/// attributes by.
pub fn send_gauges(home: &FeldHome, token: &str, scope: &str, points: &[GaugePoint]) -> Result<()> {
    if token.is_empty() {
        bail!("session token is empty");
    }
    if points.is_empty() {
        return Ok(());
    }
    let endpoint = fs::read_to_string(home.root().join(ENDPOINT_FILE))
        .context("read persisted collector endpoint")?;
    let endpoint = endpoint.trim();
    endpoint_address(endpoint).context("validate persisted collector endpoint")?;

    let request = gauge_request(scope, points)?;
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(5))
        .build()
        .context("build status gauge client")?
        .post(format!("{endpoint}/v1/metrics"))
        .bearer_auth(telemetry_token(token))
        .header(CONTENT_TYPE.as_str(), "application/x-protobuf")
        .body(request.encode_to_vec())
        .send()
        .context("send status gauges")?;
    if !response.status().is_success() {
        bail!(
            "collector rejected status gauges with {}",
            response.status()
        );
    }
    Ok(())
}

pub(crate) fn gauge_request(
    scope: &str,
    points: &[GaugePoint],
) -> Result<proto::ExportMetricsServiceRequest> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_nanos()
        .try_into()
        .context("status gauge timestamp exceeds OTLP range")?;
    let metrics = points
        .iter()
        .map(|point| {
            let data_point = point.attributes.iter().fold(
                proto::NumberDataPoint::double(0, timestamp, point.value),
                |data_point, (key, value)| data_point.with(key.as_str(), value.as_str()),
            );
            proto::Metric::gauge(point.name.as_str(), vec![data_point])
        })
        .collect();
    Ok(proto::ExportMetricsServiceRequest::new(
        Some(proto::Resource::default()),
        Some(proto::InstrumentationScope::new(
            scope,
            env!("CARGO_PKG_VERSION"),
        )),
        metrics,
    ))
}

/// A number the Runtime reported, when it is one.
pub fn finite(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

/// A percentage the Runtime reported, when it is one within 0 to 100.
pub fn percentage(value: Option<&Value>) -> Option<f64> {
    finite(value).filter(|value| (0.0..=100.0).contains(value))
}
