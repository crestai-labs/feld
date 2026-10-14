use anyhow::{Result, bail};
use serde_json::{Value, json};

use super::decode::HistogramBuckets;
use super::instruments::Instruments;
use super::response::{MetricBin, MetricProjection, MetricStatus, SessionMetricsResponse};
use super::runtime::{Runtime, RuntimeRules};
use super::scan::{BinAccumulator, Scan, read, scan};
use super::window::{Window, format_nanos};
use super::{METRICS, MetricsQuery};
use crate::store::Telemetry;
use feld_core::model::Session;

pub(super) const NO_USAGE: &str = "No correlated Runtime usage record";
const COLLECTOR_UNAVAILABLE: &str =
    "Runtime telemetry Collector was unavailable when the Session started";
/// A Sub Agent runs inside its parent's Runtime process with the parent's exporter and
/// token, so its telemetry is in the parent Session's metrics.
const SUB_AGENT: &str = "Included in the parent Session";

pub fn session_metrics(
    session: &Session,
    instruments: &Instruments,
    telemetries: &[Telemetry],
    query: &MetricsQuery<'_>,
) -> Result<SessionMetricsResponse> {
    let window = checked(query)?;
    let runtime = Runtime::of(session);
    let scan = match runtime {
        Runtime::Rules(rules) => scan(rules, &read(instruments, telemetries), &window, session),
        Runtime::NotApplicable(_) => Scan::default(),
    };
    Ok(response(&session.id, query, |metric| {
        project_metric(metric, runtime, session, &window, &scan)
    }))
}

/// The metrics of a Sub Agent Session, which are its parent Session's.
pub fn sub_agent_metrics(
    session: &str,
    query: &MetricsQuery<'_>,
) -> Result<SessionMetricsResponse> {
    checked(query)?;
    Ok(response(session, query, |metric| {
        projection(metric, MetricStatus::NotApplicable, SUB_AGENT, vec![])
    }))
}

/// The range a Session metrics read asks for, once the metric it names is one Feld has.
fn checked(query: &MetricsQuery<'_>) -> Result<Window> {
    if let Some(metric) = query.metric
        && !METRICS.contains(&metric)
    {
        bail!("unknown metric '{metric}'");
    }
    Window::new(query.start, query.end, query.bucket)
}

fn response(
    session: &str,
    query: &MetricsQuery<'_>,
    project: impl Fn(&'static str) -> MetricProjection,
) -> SessionMetricsResponse {
    SessionMetricsResponse {
        session: session.to_owned(),
        start: query.start.map(str::to_owned),
        end: query.end.map(str::to_owned),
        bucket: query.bucket,
        metrics: METRICS
            .into_iter()
            .filter(|metric| query.metric.is_none_or(|wanted| wanted == *metric))
            .map(project)
            .collect(),
    }
}

pub(super) fn project_metric(
    metric: &'static str,
    runtime: Runtime,
    session: &Session,
    window: &Window,
    scan: &Scan,
) -> MetricProjection {
    let rules = match runtime {
        Runtime::Rules(rules) => rules,
        Runtime::NotApplicable(reason) => {
            return projection(metric, MetricStatus::NotApplicable, reason, vec![]);
        }
    };
    if let Some(reason) = rules.not_applicable(metric) {
        return projection(metric, MetricStatus::NotApplicable, reason, vec![]);
    }
    if session.execution.telemetry_collector_available == Some(false) {
        return projection(
            metric,
            MetricStatus::Unavailable,
            COLLECTOR_UNAVAILABLE,
            vec![],
        );
    }
    // The part beside the Runtime keeps reporting for a Runtime that started without the
    // telemetry environment, so the context window stays projected from its gauges.
    if session.execution.telemetry_runtime_prepared == Some(false)
        && metric != "context_window_utilization"
    {
        return projection(metric, MetricStatus::Unavailable, rules.unprepared, vec![]);
    }
    if let Some(reason) = scan.unavailable_reason(metric) {
        return projection(metric, MetricStatus::Unavailable, reason, vec![]);
    }

    let token_reported = scan.bins.values().any(|bin| bin.tokens.is_some())
        && scan.unavailable_reason("token_usage").is_none();
    let series = scan
        .bins
        .iter()
        .filter_map(|(key, bin)| {
            let value = bin_value(metric, rules, bin, token_reported)?;
            Some(metric_bin(
                *key,
                value,
                window,
                &session.start,
                session.end.as_deref(),
            ))
        })
        .collect::<Vec<_>>();

    let source = rules.source_of(metric);
    if metric == "prompt_cache_hit_rate" && token_reported {
        projection(metric, MetricStatus::Reported, source, series)
    } else if series.is_empty() {
        projection(metric, MetricStatus::Unavailable, NO_USAGE, series)
    } else {
        projection(metric, MetricStatus::Reported, source, series)
    }
}

/// The value of one metric in one bin, when the bin has one.
pub(super) fn bin_value(
    metric: &str,
    rules: &RuntimeRules,
    bin: &BinAccumulator,
    token_reported: bool,
) -> Option<Value> {
    // A counting metric with no record of its own reads zero where tokens were reported.
    let or_zero =
        |count: Option<f64>| count.or((token_reported && bin.tokens.is_some()).then_some(0.0));
    match metric {
        "token_usage" => bin.tokens?.value(&rules.tokens),
        "estimated_cost" => bin.cost.map(|cost| json!(cost)),
        "api_errors" => or_zero(bin.api_errors).map(|count| json!(count)),
        "prompt_cache_hit_rate" => bin
            .tokens?
            .cache_hit_rate(&rules.tokens)
            .map(|value| json!(value)),
        "api_request_latency_p95" if !bin.latency.is_empty() => {
            Some(json!(percentile_95(&bin.latency)))
        }
        "api_request_latency_p95" if !bin.latency_buckets.is_empty() => {
            percentile_95_of_buckets(&bin.latency_buckets).map(|value| json!(value))
        }
        "tool_calls" => or_zero(bin.tool_calls).map(|count| json!(count)),
        "context_window_utilization"
            if bin.context_size.is_some() || bin.context_used.is_some() =>
        {
            let mut value = serde_json::Map::new();
            if let Some((_, size)) = bin.context_size {
                value.insert("size".to_owned(), json!(size));
            }
            if let Some((_, used)) = bin.context_used {
                value.insert("used_percent".to_owned(), json!(used));
            }
            Some(Value::Object(value))
        }
        "active_time" => bin.active_time.map(|seconds| json!(seconds)),
        _ => None,
    }
}

fn projection(
    metric: &str,
    status: MetricStatus,
    source: &str,
    series: Vec<MetricBin>,
) -> MetricProjection {
    MetricProjection {
        metric: metric.to_owned(),
        status,
        source: source.to_owned(),
        series,
    }
}

/// One bin of a series. Without a bucket the bin is the range, and an open end of the range
/// falls back to the span of the Sessions it covers.
pub(super) fn metric_bin(
    key: i64,
    value: Value,
    window: &Window,
    start: &str,
    end: Option<&str>,
) -> MetricBin {
    match window.bucket_nanos {
        Some(bucket) => MetricBin {
            start: format_nanos(key),
            end: Some(format_nanos(key.saturating_add(bucket))),
            value,
            session: None,
        },
        None => MetricBin {
            start: window
                .start_nanos
                .map(format_nanos)
                .unwrap_or_else(|| start.to_owned()),
            end: window
                .end_nanos
                .map(format_nanos)
                .or_else(|| end.map(str::to_owned)),
            value,
            session: None,
        },
    }
}

/// The p95 of histogram buckets, with the rank interpolated linearly inside the bucket it
/// falls in. Observations are spread evenly over each bucket; the open bucket above the
/// highest bound has no width, so a rank that falls in it reads as that bound.
fn percentile_95_of_buckets(histograms: &[HistogramBuckets]) -> Option<f64> {
    // Each histogram's counts fit in `u64`, but the Sessions' together may not.
    let total: f64 = histograms
        .iter()
        .flat_map(|histogram| &histogram.counts)
        .map(|count| *count as f64)
        .sum();
    if total == 0.0 {
        return None;
    }
    let rank = total * 0.95;
    let cumulative = |at: f64| -> f64 {
        histograms
            .iter()
            .flat_map(|histogram| {
                let mut lower = 0.0_f64;
                histogram
                    .bounds
                    .iter()
                    .zip(&histogram.counts)
                    .map(move |(bound, count)| {
                        let below = *bound;
                        let width = below - lower;
                        let part = if at >= below {
                            *count as f64
                        } else if at > lower && width > 0.0 {
                            *count as f64 * (at - lower) / width
                        } else {
                            0.0
                        };
                        lower = below;
                        part
                    })
            })
            .sum()
    };

    let mut cuts: Vec<f64> = histograms
        .iter()
        .flat_map(|histogram| histogram.bounds.iter().copied())
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let (mut previous, mut reached) = (0.0_f64, 0.0_f64);
    for cut in cuts {
        let at = cumulative(cut);
        if at >= rank {
            let span = at - reached;
            return Some(if span > 0.0 {
                previous + (rank - reached) / span * (cut - previous)
            } else {
                cut
            });
        }
        (previous, reached) = (cut, at);
    }
    // The rank falls above every bound, where the bucket is open.
    Some(previous)
}

fn percentile_95(values: &[f64]) -> f64 {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let index = ((values.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    values[index]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::fixture::{
        T120, T130, claude_instruments, codex_instruments, codex_session, logs_telemetry, metric,
        metrics_telemetry, point, session,
    };
    use crate::metrics::runtime::{CLAUDE_CODE, CODEX, NO_RULES, ROLLOUT_SOURCE, STATUS_SOURCE};
    use crate::metrics::scan::NOT_DELTA;
    use feld_core::telemetry::proto::{
        self,
        AggregationTemporality::{Cumulative, Delta},
    };

    #[test]
    fn projects_claude_metrics_into_epoch_aligned_bins() {
        let claude = session("agent-runtime", "Claude Code");
        let metrics = metrics_telemetry(
            "m1",
            vec![
                proto::Metric::sum(
                    "claude_code.token.usage",
                    Delta,
                    vec![
                        point(T120, 100.0).with("type", "input"),
                        point(T120, 25.0).with("type", "cacheRead"),
                        point(T120, 10.0).with("type", "output"),
                    ],
                ),
                proto::Metric::sum("claude_code.cost.usage", Delta, vec![point(T120, 0.75)]),
                proto::Metric::gauge(
                    "feld.claude_code.context_window.size",
                    vec![point(T120, 200_000.0)],
                ),
                proto::Metric::gauge(
                    "feld.claude_code.context_window.used_percent",
                    vec![point(T120, 20.0), point(T130, 30.0)],
                ),
            ],
        );
        let logs = logs_telemetry(
            "l1",
            vec![
                proto::LogRecord::new(T120)
                    .with_event_name("claude_code.api_request")
                    .with("duration_ms", "10"),
                proto::LogRecord::new(T130)
                    .with_event_name("claude_code.api_request")
                    .with("duration_ms", "50"),
            ],
        );
        let response = session_metrics(
            &claude,
            &claude_instruments(),
            &[metrics, logs],
            &MetricsQuery {
                start: Some("1970-01-01T00:01:00Z"),
                end: Some("1970-01-01T00:04:00Z"),
                bucket: Some(60),
                metric: None,
            },
        )
        .unwrap();

        let token = metric(&response, "token_usage");
        assert_eq!(token.status, MetricStatus::Reported);
        assert_eq!(token.series[0].start, "1970-01-01T00:02:00.000000000Z");
        assert_eq!(token.series[0].value["total"], 135.0);
        let cache = metric(&response, "prompt_cache_hit_rate");
        assert_eq!(cache.series[0].value, 20.0);
        let latency = metric(&response, "api_request_latency_p95");
        assert_eq!(latency.series[0].value, 50.0);
        let context = metric(&response, "context_window_utilization");
        assert_eq!(context.series[0].value["used_percent"], 30.0);
        assert_eq!(context.source, STATUS_SOURCE);
        assert_eq!(metric(&response, "api_errors").series[0].value, 0.0);
        assert_eq!(metric(&response, "tool_calls").series[0].value, 0.0);
    }

    #[test]
    fn non_delta_precedes_invalid_and_program_is_not_applicable() {
        let claude = session("agent-runtime", "Claude Code");
        let telemetry = metrics_telemetry(
            "m1",
            vec![proto::Metric::sum(
                "claude_code.token.usage",
                Cumulative,
                vec![point(T120, -1.0).with("type", "input")],
            )],
        );
        let response = session_metrics(
            &claude,
            &claude_instruments(),
            &[telemetry],
            &MetricsQuery {
                metric: Some("token_usage"),
                ..MetricsQuery::default()
            },
        )
        .unwrap();
        assert_eq!(response.metrics[0].status, MetricStatus::Unavailable);
        assert_eq!(response.metrics[0].source, NOT_DELTA);

        let program = session("program", "printf");
        let response = session_metrics(
            &program,
            &claude_instruments(),
            &[],
            &MetricsQuery::default(),
        )
        .unwrap();
        assert!(
            response
                .metrics
                .iter()
                .all(|metric| metric.status == MetricStatus::NotApplicable)
        );
        assert!(
            session_metrics(
                &claude,
                &claude_instruments(),
                &[],
                &MetricsQuery {
                    bucket: Some(30),
                    ..MetricsQuery::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn collector_unavailability_at_start_is_a_persistent_metric_source() {
        let mut claude = session("agent-runtime", "Claude Code");
        claude.execution.telemetry_collector_available = Some(false);
        let response = session_metrics(
            &claude,
            &claude_instruments(),
            &[],
            &MetricsQuery::default(),
        )
        .unwrap();
        assert!(response.metrics.iter().all(|metric| {
            metric.status == MetricStatus::Unavailable && metric.source == COLLECTOR_UNAVAILABLE
        }));
    }

    #[test]
    fn a_runtime_started_before_telemetry_preparation_keeps_only_status_line_metrics() {
        let mut claude = session("agent-runtime", "Claude Code");
        claude.execution.telemetry_runtime_prepared = Some(false);
        let context = metrics_telemetry(
            "context",
            vec![proto::Metric::gauge(
                "feld.claude_code.context_window.used_percent",
                vec![point(T120, 42.0)],
            )],
        );
        let response = session_metrics(
            &claude,
            &claude_instruments(),
            &[context],
            &MetricsQuery::default(),
        )
        .unwrap();
        for projection in &response.metrics {
            if projection.metric == "context_window_utilization" {
                assert_eq!(projection.status, MetricStatus::Reported);
                assert_eq!(projection.source, STATUS_SOURCE);
                assert_eq!(projection.series[0].value["used_percent"], 42.0);
            } else {
                assert_eq!(
                    projection.status,
                    MetricStatus::Unavailable,
                    "{}",
                    projection.metric
                );
                assert_eq!(projection.source, CLAUDE_CODE.unprepared);
            }
        }
    }

    /// A Codex a person started exports without a token, so only what the hook sends
    /// from the rollout file reaches its Session.
    #[test]
    fn an_attached_codex_keeps_only_the_rollout_gauges() {
        let mut codex = codex_session();
        codex.execution.telemetry_runtime_prepared = Some(false);
        let telemetries = [metrics_telemetry(
            "context",
            vec![proto::Metric::gauge(
                "feld.codex.context_window.used_percent",
                vec![point(T120, 25.0)],
            )],
        )];
        let response = session_metrics(
            &codex,
            &codex_instruments(),
            &telemetries,
            &MetricsQuery::default(),
        )
        .unwrap();
        for projection in &response.metrics {
            match projection.metric.as_str() {
                "context_window_utilization" => {
                    assert_eq!(projection.status, MetricStatus::Reported);
                    assert_eq!(projection.source, ROLLOUT_SOURCE);
                }
                "active_time" => assert_eq!(projection.status, MetricStatus::NotApplicable),
                other => {
                    assert_eq!(projection.status, MetricStatus::Unavailable, "{other}");
                    assert_eq!(projection.source, CODEX.unprepared, "{other}");
                }
            }
        }
    }

    /// A Runtime whose rules Feld does not have reports nothing, rather than being read
    /// by another Runtime's rules.
    #[test]
    fn a_runtime_without_rules_is_not_applicable() {
        let mut other = session("agent-runtime", "Other");
        other.execution.runtime = Some("other".to_owned());
        let usage = metrics_telemetry(
            "m1",
            vec![proto::Metric::sum(
                "claude_code.token.usage",
                Delta,
                vec![point(T120, 100.0).with("type", "input")],
            )],
        );
        let response = session_metrics(
            &other,
            &claude_instruments(),
            &[usage],
            &MetricsQuery::default(),
        )
        .unwrap();
        assert!(response.metrics.iter().all(|metric| {
            metric.status == MetricStatus::NotApplicable && metric.source == NO_RULES
        }));
    }

    /// A Sub Agent Session answers every metric as `n/a`, over the range it was asked for.
    #[test]
    fn a_sub_agent_session_is_included_in_its_parent() {
        let query = MetricsQuery {
            start: Some("1970-01-01T00:01:00Z"),
            bucket: Some(60),
            ..MetricsQuery::default()
        };
        let response = sub_agent_metrics("s1_subagent_a1", &query).unwrap();
        assert_eq!(response.session, "s1_subagent_a1");
        assert_eq!(response.bucket, Some(60));
        assert_eq!(response.metrics.len(), METRICS.len());
        assert!(response.metrics.iter().all(|metric| {
            metric.status == MetricStatus::NotApplicable && metric.source == SUB_AGENT
        }));
        let one = MetricsQuery {
            metric: Some("tool_calls"),
            ..MetricsQuery::default()
        };
        assert_eq!(
            sub_agent_metrics("s1_subagent_a1", &one)
                .unwrap()
                .metrics
                .len(),
            1
        );
        let unknown = MetricsQuery {
            metric: Some("quota"),
            ..MetricsQuery::default()
        };
        assert!(sub_agent_metrics("s1_subagent_a1", &unknown).is_err());
    }

    /// A histogram places its observations inside the bucket the rank falls in, and reads
    /// as the highest bound when the rank falls in the open bucket above it.
    #[test]
    fn a_histogram_p95_is_interpolated_inside_its_bucket() {
        let buckets = |bounds: &[f64], counts: &[u64]| HistogramBuckets {
            bounds: bounds.to_vec(),
            counts: counts.to_vec(),
        };
        assert_eq!(
            percentile_95_of_buckets(&[buckets(&[100.0, 200.0], &[0, 10, 0])]),
            Some(195.0)
        );
        // Every observation is above the highest bound, which is as far as the file says.
        assert_eq!(
            percentile_95_of_buckets(&[buckets(&[100.0], &[0, 10])]),
            Some(100.0)
        );
        // Two exports of the same interval are one distribution.
        assert_eq!(
            percentile_95_of_buckets(&[
                buckets(&[100.0, 200.0], &[10, 0, 0]),
                buckets(&[100.0, 200.0], &[0, 10, 0]),
            ]),
            Some(190.0)
        );
        assert_eq!(
            percentile_95_of_buckets(&[buckets(&[100.0], &[0, 0])]),
            None
        );
        // Sessions whose counts together exceed `u64` still read as one distribution.
        assert_eq!(
            percentile_95_of_buckets(&[
                buckets(&[100.0], &[u64::MAX, 0]),
                buckets(&[100.0], &[u64::MAX, 0]),
            ]),
            Some(95.0)
        );
    }
}
