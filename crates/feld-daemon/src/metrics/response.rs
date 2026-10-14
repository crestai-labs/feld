use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricStatus {
    Reported,
    Unavailable,
    #[serde(rename = "n/a")]
    NotApplicable,
}

impl MetricStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reported => "reported",
            Self::Unavailable => "unavailable",
            Self::NotApplicable => "n/a",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetricsResponse {
    pub session: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub bucket: Option<u64>,
    pub metrics: Vec<MetricProjection>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricProjection {
    pub metric: String,
    pub status: MetricStatus,
    pub source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<MetricBin>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricBin {
    pub start: String,
    pub end: Option<String>,
    pub value: Value,
    /// The Session whose value a Runtime's context window bin took.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetricsResponse {
    pub runtime: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub bucket: Option<u64>,
    pub metrics: Vec<RuntimeMetricProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota: Option<RuntimeQuota>,
}

/// One metric aggregated over the `reported` Sessions of a Runtime, one series per account,
/// with how many Sessions had each status.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetricProjection {
    pub metric: String,
    pub status: MetricStatus,
    pub source: String,
    pub sessions: SessionCounts,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<AccountSeries>,
}

/// The `reported` Sessions of one account aggregated bin by bin.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSeries {
    /// The account of the Sessions; absent for the Sessions whose account is unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
    pub series: Vec<MetricBin>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SessionCounts {
    pub reported: u64,
    pub unavailable: u64,
    #[serde(rename = "n/a")]
    pub not_applicable: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeQuota {
    pub status: MetricStatus,
    pub source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<QuotaSeries>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSeries {
    /// The account of the reporting Sessions; absent when their Telemetry names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
    pub window: String,
    pub resets_at: String,
    pub end: String,
    pub points: Vec<QuotaPoint>,
    pub latest: QuotaPoint,
    pub reported_by: ReportedBy,
}

/// The account a series belongs to: its ID and the name a person reads it by.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Account {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaPoint {
    pub at: String,
    pub used_percent: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportedBy {
    pub session: String,
    pub workspace: String,
    pub actor: String,
}
