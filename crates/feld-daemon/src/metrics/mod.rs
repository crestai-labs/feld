mod aggregate;
mod decode;
#[cfg(test)]
mod fixture;
mod instruments;
mod project;
mod quota;
mod response;
mod runtime;
mod scan;
mod window;

pub use aggregate::runtime_metrics;
pub use instruments::{InstrumentMaps, Instruments};
pub use project::{session_metrics, sub_agent_metrics};
pub use response::{
    Account, AccountSeries, MetricBin, MetricProjection, MetricStatus, QuotaPoint, QuotaSeries,
    ReportedBy, RuntimeMetricProjection, RuntimeMetricsResponse, RuntimeQuota, SessionCounts,
    SessionMetricsResponse,
};
pub use scan::ContextWindowBefore;

const METRICS: [&str; 8] = [
    "token_usage",
    "estimated_cost",
    "api_errors",
    "prompt_cache_hit_rate",
    "api_request_latency_p95",
    "tool_calls",
    "context_window_utilization",
    "active_time",
];

/// The account quota a Runtime reports, which the Runtime metrics carry beside the
/// aggregated Session metrics.
const QUOTA: &str = "quota";

#[derive(Clone, Debug, Default)]
pub struct MetricsQuery<'a> {
    pub start: Option<&'a str>,
    pub end: Option<&'a str>,
    pub bucket: Option<u64>,
    pub metric: Option<&'a str>,
}
