//! Metrics as a person reads them: each metric over the range, in the unit it is read in.

use serde_json::Value;

use crate::Context;
use crate::output::print_table;
use feld_daemon::metrics::{MetricStatus, QuotaSeries, RuntimeMetricsResponse, RuntimeQuota};

/// One line per account and window of a reported quota, each with the current series of
/// that account's window. The labels share the width of the longest.
fn quota_lines(quota: &RuntimeQuota, local_time: impl Fn(&str) -> String) -> Vec<String> {
    let mut current: Vec<(String, &QuotaSeries)> = Vec::new();
    for (window, label) in [("five_hour", "5-hour"), ("seven_day", "7-day")] {
        let first = current.len();
        // The series of an account's window come in reset order, so the last is the current
        // one.
        for series in quota.series.iter().filter(|series| series.window == window) {
            let id =
                |series: &QuotaSeries| series.account.as_ref().map(|account| account.id.clone());
            match current[first..]
                .iter_mut()
                .find(|(_, seen)| id(seen) == id(series))
            {
                Some((_, seen)) => *seen = series,
                None => current.push((String::new(), series)),
            }
        }
        for (text, series) in &mut current[first..] {
            *text = match &series.account {
                Some(account) => format!("{label} {}:", account.name),
                None => format!("{label}:"),
            };
        }
    }
    let width = current
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0)
        .max(23);
    current
        .iter()
        .map(|(label, series)| {
            format!(
                "  {label:<width$}  {:.0}% used · resets {}",
                series.latest.used_percent,
                local_time(&series.resets_at)
            )
        })
        .collect()
}

pub fn print_runtime_metrics(ctx: &Context, response: &RuntimeMetricsResponse) {
    println!("Runtime {}", response.runtime);
    // The daemon leaves the quota out only of a read narrowed to one metric, which the CLI
    // never asks for.
    if let Some(quota) = &response.quota {
        println!("Quota");
        if quota.status == MetricStatus::Reported {
            for line in quota_lines(quota, |at| ctx.local_time(at)) {
                println!("{line}");
            }
        } else {
            println!("  {} · {}", quota.status.as_str(), quota.source);
        }
    }
    print_table(
        &["METRIC", "ACCOUNT", "STATUS", "VALUE"],
        &metric_rows(response),
    );
}

/// One row per account of a reported metric, and one for a metric that is not reported.
fn metric_rows(response: &RuntimeMetricsResponse) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for metric in &response.metrics {
        let name = metric.metric.as_str();
        let row = |account: String, value: String| {
            vec![
                metric_label(name),
                account,
                metric.status.as_str().to_owned(),
                value,
            ]
        };
        if metric.status != MetricStatus::Reported || metric.accounts.is_empty() {
            let value = if metric.status == MetricStatus::Reported {
                "-".to_owned()
            } else {
                metric.source.clone()
            };
            rows.push(row("-".to_owned(), value));
            continue;
        }
        for account in &metric.accounts {
            let value = account.series.first().map_or_else(
                || "-".to_owned(),
                |bin| match name {
                    "context_window_utilization" => bin.value["used_percent"].as_f64().map_or_else(
                        || "-".to_owned(),
                        |used| format!("{used:.0}% (max of sessions)"),
                    ),
                    _ => metric_value(name, &bin.value),
                },
            );
            let label = account
                .account
                .as_ref()
                .map_or_else(|| "unknown".to_owned(), |account| account.name.clone());
            rows.push(row(label, value));
        }
    }
    rows
}

pub fn metric_label(metric: &str) -> String {
    match metric {
        "api_errors" => "API errors".to_owned(),
        "api_request_latency_p95" => "API request latency p95".to_owned(),
        "context_window_utilization" => "context window".to_owned(),
        _ => metric.replace('_', " "),
    }
}

/// One metric's value over the range, in the unit a person reads it in.
pub fn metric_value(metric: &str, value: &Value) -> String {
    let number = value.as_f64();
    match (metric, number) {
        ("token_usage", _) => value["total"]
            .as_f64()
            .map_or_else(|| value.to_string(), |total| format!("{total:.0} total")),
        ("estimated_cost", Some(cost)) => format!("${cost:.2}"),
        ("prompt_cache_hit_rate", Some(rate)) => format!("{rate:.0}%"),
        ("api_request_latency_p95", Some(latency)) => format!("{latency:.0} ms"),
        ("active_time", Some(seconds)) => {
            let seconds = seconds.round() as u64;
            format!("{}m {}s", seconds / 60, seconds % 60)
        }
        ("context_window_utilization", _) => {
            let used = value["used_percent"]
                .as_f64()
                .map_or_else(|| "-".to_owned(), |used| format!("{used:.0}%"));
            match value["size"].as_f64() {
                Some(size) => format!("{used} of {size:.0}"),
                None => used,
            }
        }
        (_, Some(number)) => format!("{number:.0}"),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use feld_daemon::metrics::{
        Account, AccountSeries, MetricBin, MetricStatus, QuotaPoint, QuotaSeries, ReportedBy,
        RuntimeMetricProjection, RuntimeMetricsResponse, RuntimeQuota, SessionCounts,
    };
    use serde_json::json;

    use super::{metric_rows, quota_lines};

    fn series(account: Option<&str>, window: &str, resets_at: &str, used: f64) -> QuotaSeries {
        let point = QuotaPoint {
            at: "2026-09-13T00:00:00Z".to_owned(),
            used_percent: used,
        };
        QuotaSeries {
            account: account.map(|name| Account {
                id: format!("id-{name}"),
                name: name.to_owned(),
            }),
            window: window.to_owned(),
            resets_at: resets_at.to_owned(),
            end: resets_at.to_owned(),
            points: vec![point.clone()],
            latest: point,
            reported_by: ReportedBy {
                session: "s1".to_owned(),
                workspace: "default".to_owned(),
                actor: "a".to_owned(),
            },
        }
    }

    #[test]
    fn quota_lines_show_the_current_series_of_each_account() {
        let quota = RuntimeQuota {
            status: MetricStatus::Reported,
            source: "status line".to_owned(),
            series: vec![
                series(None, "five_hour", "T1", 40.0),
                series(
                    Some("alice.longer.address@example.com"),
                    "five_hour",
                    "T1",
                    90.0,
                ),
                series(
                    Some("alice.longer.address@example.com"),
                    "five_hour",
                    "T2",
                    64.0,
                ),
                series(Some("bot@example.com"), "five_hour", "T3", 18.0),
                series(Some("bot@example.com"), "seven_day", "T4", 9.0),
            ],
        };
        assert_eq!(
            quota_lines(&quota, |at| at.to_owned()),
            vec![
                "  5-hour:                                   40% used · resets T1",
                "  5-hour alice.longer.address@example.com:  64% used · resets T2",
                "  5-hour bot@example.com:                   18% used · resets T3",
                "  7-day bot@example.com:                    9% used · resets T4",
            ]
        );
    }

    /// A reported metric shows one row per account, the unknown account included; a metric
    /// that is not reported shows one row with its reason.
    #[test]
    fn metric_rows_show_each_account() {
        let tokens = |account: Option<&str>, total: f64| AccountSeries {
            account: account.map(|name| Account {
                id: format!("id-{name}"),
                name: name.to_owned(),
            }),
            series: vec![MetricBin {
                start: "T0".to_owned(),
                end: None,
                value: json!({"total": total}),
                session: None,
            }],
        };
        let response = RuntimeMetricsResponse {
            runtime: "claude-code".to_owned(),
            start: None,
            end: None,
            bucket: None,
            metrics: vec![
                RuntimeMetricProjection {
                    metric: "token_usage".to_owned(),
                    status: MetricStatus::Reported,
                    source: "Claude Code Runtime telemetry".to_owned(),
                    sessions: SessionCounts::default(),
                    accounts: vec![tokens(None, 3.0), tokens(Some("me@example.com"), 150.0)],
                },
                RuntimeMetricProjection {
                    metric: "active_time".to_owned(),
                    status: MetricStatus::Unavailable,
                    source: "No correlated Runtime usage record".to_owned(),
                    sessions: SessionCounts::default(),
                    accounts: vec![],
                },
            ],
            quota: None,
        };
        assert_eq!(
            metric_rows(&response),
            vec![
                vec!["token usage", "unknown", "reported", "3 total"],
                vec!["token usage", "me@example.com", "reported", "150 total"],
                vec![
                    "active time",
                    "-",
                    "unavailable",
                    "No correlated Runtime usage record"
                ],
            ]
        );
    }
}
