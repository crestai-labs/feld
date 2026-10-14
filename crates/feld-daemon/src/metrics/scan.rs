use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::QUOTA;
use super::decode::{HistogramBuckets, TelemetryPoint, TelemetryPointKind, decode_telemetry};
use super::instruments::{Destination, Instruments};
use super::response::Account;
use super::runtime::{Count, RuntimeRules, TokenKinds};
use super::window::Window;
use crate::store::Telemetry;
use feld_core::model::Session;

pub(super) const INVALID_VALUE: &str = "Invalid Runtime telemetry value";
pub(super) const NOT_DELTA: &str = "Runtime telemetry is not delta";

/// The metrics derived from another, which a reason to leave that one out leaves out too.
const DERIVED: [(&str, &str); 1] = [("token_usage", "prompt_cache_hit_rate")];

/// A Session's Telemetry decoded once, each point with the metric its Runtime's manifest
/// says it feeds. The metrics and the quota read the same points.
#[derive(Default)]
pub(super) struct Readings {
    pub(super) points: Vec<(Destination, TelemetryPoint)>,
    pub(super) decode_failed: bool,
    /// The account the Runtime's own OTel names, the latest if it names several.
    pub(super) account: Option<Account>,
}

/// The attributes a Runtime's own OTel names the account by: Claude Code's ID, Codex's ID,
/// and the name both use.
const OTEL_ACCOUNT_IDS: [&str; 2] = ["user.account_uuid", "user.account_id"];
const OTEL_ACCOUNT_NAME: &str = "user.email";

impl Readings {
    /// The Session's account: the one the Runtime's own OTel names, or else the one the
    /// latest quota gauge names.
    pub(super) fn session_account(&self) -> Option<Account> {
        self.account.clone().or_else(|| {
            self.points
                .iter()
                .filter(|(destination, _)| destination.metric == QUOTA)
                .filter_map(|(_, point)| {
                    let account = account_named(point, "account.id", "account.name")?;
                    Some((point.timestamp_unix_nano, account))
                })
                .max_by_key(|(timestamp, _)| *timestamp)
                .map(|(_, account)| account)
        })
    }
}

/// The account a point's `id` and `name` attributes name; the ID stands in for a missing
/// name.
pub(super) fn account_named(point: &TelemetryPoint, id: &str, name: &str) -> Option<Account> {
    let id = point.attribute(id).filter(|id| !id.is_empty())?.to_owned();
    let name = point
        .attribute(name)
        .filter(|name| !name.is_empty())
        .map_or_else(|| id.clone(), str::to_owned);
    Some(Account { id, name })
}

/// Decodes a Session's Telemetry and names each point by its Runtime's instruments. A log
/// record goes by the first of its names the Runtime gave a meaning to.
pub(super) fn read(instruments: &Instruments, telemetries: &[Telemetry]) -> Readings {
    let mut readings = Readings::default();
    let mut decoded = Vec::new();
    for telemetry in telemetries {
        let Ok(points) = decode_telemetry(&telemetry.signal, &telemetry.body) else {
            readings.decode_failed = true;
            continue;
        };
        decoded.extend(points);
    }
    readings.account = decoded
        .iter()
        .filter_map(|point| {
            let account = OTEL_ACCOUNT_IDS
                .into_iter()
                .find_map(|id| account_named(point, id, OTEL_ACCOUNT_NAME))?;
            Some((point.timestamp_unix_nano, account))
        })
        .max_by_key(|(timestamp, _)| *timestamp)
        .map(|(_, account)| account);
    readings
        .points
        .extend(decoded.into_iter().filter_map(|point| {
            let destination = point.names().find_map(|name| instruments.of(name))?;
            Some((destination, point))
        }));
    readings
}

/// The context window readings from before a range that carry into it: the latest
/// utilization and the latest size. A range read goes back through the Telemetries before the
/// range a page at a time, newest first, until it has found each part the Runtime reports.
pub struct ContextWindowBefore {
    instruments: Instruments,
    used_percent: bool,
    size: bool,
}

impl ContextWindowBefore {
    pub fn new(instruments: Instruments) -> Self {
        let reports = |part: &str| {
            instruments.destinations().any(|destination| {
                destination.metric == "context_window_utilization" && destination.part == Some(part)
            })
        };
        Self {
            used_percent: !reports("used_percent"),
            size: !reports("size"),
            instruments,
        }
    }

    /// Takes the next page and says whether the pages so far hold both parts.
    pub fn holds(&mut self, page: &[Telemetry]) -> bool {
        if !self.found() {
            for (destination, _) in read(&self.instruments, page).points {
                if destination.metric == "context_window_utilization" {
                    match destination.part {
                        Some("used_percent") => self.used_percent = true,
                        Some("size") => self.size = true,
                        _ => {}
                    }
                }
            }
        }
        self.found()
    }

    fn found(&self) -> bool {
        self.used_percent && self.size
    }
}

#[derive(Default)]
pub(super) struct BinAccumulator {
    pub(super) tokens: Option<TokenTotals>,
    pub(super) cost: Option<f64>,
    pub(super) api_errors: Option<f64>,
    pub(super) tool_calls: Option<f64>,
    pub(super) latency: Vec<f64>,
    pub(super) latency_buckets: Vec<HistogramBuckets>,
    pub(super) active_time: Option<f64>,
    pub(super) context_size: Option<(u64, f64)>,
    pub(super) context_used: Option<(u64, f64)>,
    /// The Session a Runtime's bin took its context window from.
    pub(super) context_session: Option<String>,
}

impl BinAccumulator {
    /// Adds one Session's bin into a Runtime's: the counts add up, the latency samples join,
    /// and the context window keeps the Session with the highest utilization.
    pub(super) fn merge(&mut self, other: &Self, session: &Session) {
        if let Some(tokens) = other.tokens {
            self.tokens.get_or_insert_default().add(tokens);
        }
        add(&mut self.cost, other.cost);
        add(&mut self.api_errors, other.api_errors);
        add(&mut self.tool_calls, other.tool_calls);
        self.latency.extend_from_slice(&other.latency);
        self.latency_buckets
            .extend(other.latency_buckets.iter().cloned());
        add(&mut self.active_time, other.active_time);
        let used = |bin: &Self| bin.context_used.map(|(_, used)| used);
        if (other.context_used.is_some() || other.context_size.is_some())
            && (self.context_session.is_none() || used(other) > used(self))
        {
            self.context_size = other.context_size;
            self.context_used = other.context_used;
            self.context_session = Some(session.id.clone());
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct TokenTotals {
    raw_input: f64,
    output: f64,
    cache_read: f64,
    cache_creation: f64,
}

impl TokenTotals {
    fn add(&mut self, other: Self) {
        self.raw_input += other.raw_input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_creation += other.cache_creation;
    }

    fn input(self, kinds: &TokenKinds) -> Option<f64> {
        if !kinds.input_includes_cache_read {
            Some(self.raw_input)
        } else if self.raw_input < self.cache_read {
            None
        } else {
            Some(self.raw_input - self.cache_read)
        }
    }

    pub(super) fn value(self, kinds: &TokenKinds) -> Option<Value> {
        let input = self.input(kinds)?;
        Some(json!({
            "input": input,
            "output": self.output,
            "cache_read": self.cache_read,
            "cache_creation": self.cache_creation,
            "total": input + self.output + self.cache_read + self.cache_creation,
        }))
    }

    pub(super) fn cache_hit_rate(self, kinds: &TokenKinds) -> Option<f64> {
        let denominator = self.input(kinds)? + self.cache_read + self.cache_creation;
        (denominator > 0.0).then_some(self.cache_read / denominator * 100.0)
    }
}

#[derive(Default)]
pub(super) struct Scan {
    pub(super) bins: BTreeMap<i64, BinAccumulator>,
    /// The context window as reported before the range.
    context_before: BinAccumulator,
    invalid: BTreeSet<&'static str>,
    not_delta: BTreeSet<&'static str>,
    decode_failed: bool,
}

impl Scan {
    pub(super) fn unavailable_reason(&self, metric: &'static str) -> Option<&'static str> {
        if self.not_delta.contains(metric) {
            Some(NOT_DELTA)
        } else if self.decode_failed || self.invalid.contains(metric) {
            Some(INVALID_VALUE)
        } else {
            None
        }
    }

    fn mark_invalid(&mut self, metric: &'static str) {
        mark(&mut self.invalid, metric);
    }
}

/// Marks a metric, and the metrics derived from it.
fn mark(set: &mut BTreeSet<&'static str>, metric: &'static str) {
    set.insert(metric);
    for (from, derived) in DERIVED {
        if from == metric {
            set.insert(derived);
        }
    }
}

pub(super) fn scan(
    rules: &RuntimeRules,
    readings: &Readings,
    window: &Window,
    session: &Session,
) -> Scan {
    let mut scan = Scan {
        decode_failed: readings.decode_failed,
        ..Scan::default()
    };
    for (destination, point) in &readings.points {
        if destination.metric != QUOTA {
            scan_point(rules, *destination, point, window, &mut scan);
        }
    }
    if rules.tokens.input_includes_cache_read
        && scan
            .bins
            .values()
            .filter_map(|bin| bin.tokens)
            .any(|tokens| tokens.raw_input < tokens.cache_read)
    {
        scan.mark_invalid("token_usage");
    }
    // A context window reading is its utilization: a bin without one has no reading, and a
    // bin with one carries the latest size reported by then. The reading from before the
    // range holds into its first bin while the Session still runs there.
    let before = std::mem::take(&mut scan.context_before);
    if let Some(key) = window.start_key()
        && window.runs_at_start(session.end.as_deref())
        && before.context_used.is_some()
    {
        let first = scan.bins.entry(key).or_default();
        first.context_used = first.context_used.or(before.context_used);
    }
    let mut size = before.context_size;
    for bin in scan.bins.values_mut() {
        size = bin.context_size.or(size);
        bin.context_size = bin.context_used.and(size);
    }
    scan
}

fn scan_point(
    rules: &RuntimeRules,
    destination: Destination,
    point: &TelemetryPoint,
    window: &Window,
    scan: &mut Scan,
) {
    let metric = destination.metric;
    if matches!(
        point.kind,
        TelemetryPointKind::Sum | TelemetryPointKind::Histogram
    ) && !point.temporality_delta
    {
        mark(&mut scan.not_delta, metric);
    }
    let numeric = point.numeric_value;
    // A log record counts as an occurrence and carries whatever it measures in its
    // attributes; every other point is the number itself.
    let numeric_required = point.kind != TelemetryPointKind::Log;
    if numeric.is_some_and(|value| !value.is_finite() || value < 0.0)
        || (numeric_required && numeric.is_none())
    {
        scan.mark_invalid(metric);
    }
    // The context window holds its reading until the next one, so the latest from before
    // the range is kept aside for the range's first bin.
    let bin = match window.key(point.timestamp_unix_nano) {
        Some(key) => scan.bins.entry(key).or_default(),
        None if metric == "context_window_utilization"
            && window.before_start(point.timestamp_unix_nano) =>
        {
            &mut scan.context_before
        }
        None => return,
    };
    match metric {
        "token_usage" => add_tokens(&rules.tokens, point, bin, &mut scan.invalid),
        "estimated_cost" => add_numeric(
            numeric.map(|value| value / rules.cost_per_usd),
            &mut bin.cost,
        ),
        "api_errors" => count(rules.api_errors, point, &mut bin.api_errors),
        "api_request_latency_p95" if point.kind == TelemetryPointKind::Histogram => {
            let buckets = point.buckets.clone().filter(HistogramBuckets::is_usable);
            match buckets.as_ref().map(HistogramBuckets::total) {
                Some(Some(0)) => {}
                Some(Some(_)) => bin.latency_buckets.extend(buckets),
                Some(None) => mark(&mut scan.invalid, metric),
                None => {
                    if point.count.is_none_or(|count| count > 0) {
                        mark(&mut scan.invalid, metric);
                    }
                }
            }
        }
        "api_request_latency_p95" => {
            if let Some(duration) = point
                .attribute("duration_ms")
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value >= 0.0)
            {
                bin.latency.push(duration);
            } else {
                mark(&mut scan.invalid, metric);
            }
        }
        "tool_calls" => count(rules.tool_calls, point, &mut bin.tool_calls),
        "active_time"
            if point
                .attribute("type")
                .is_some_and(|value| matches!(value, "user" | "cli")) =>
        {
            add_numeric(numeric, &mut bin.active_time);
        }
        "context_window_utilization" => {
            let Some(value) = numeric else {
                mark(&mut scan.invalid, metric);
                return;
            };
            if destination.part == Some("used_percent") && value > 100.0 {
                mark(&mut scan.invalid, metric);
                return;
            }
            let latest = if destination.part == Some("size") {
                &mut bin.context_size
            } else {
                &mut bin.context_used
            };
            if latest.is_none_or(|(timestamp, _)| point.timestamp_unix_nano >= timestamp) {
                *latest = Some((point.timestamp_unix_nano, value));
            }
        }
        _ => {}
    }
}

fn add_tokens(
    kinds: &TokenKinds,
    point: &TelemetryPoint,
    bin: &mut BinAccumulator,
    invalid: &mut BTreeSet<&'static str>,
) {
    let Some(token_type) = point.attribute(kinds.attribute) else {
        return;
    };
    let Some(value) = point
        .numeric_value
        .filter(|value| value.is_finite() && *value >= 0.0)
    else {
        mark(invalid, "token_usage");
        return;
    };
    let target: fn(&mut TokenTotals) -> &mut f64 = match token_type {
        kind if kind == kinds.input => |tokens| &mut tokens.raw_input,
        kind if kind == kinds.output => |tokens| &mut tokens.output,
        kind if kind == kinds.cache_read => |tokens| &mut tokens.cache_read,
        kind if kind == kinds.cache_creation => |tokens| &mut tokens.cache_creation,
        _ => return,
    };
    *target(bin.tokens.get_or_insert_default()) += value;
}

fn count(rule: Count, point: &TelemetryPoint, total: &mut Option<f64>) {
    let holds = |(attribute, value): (&str, &str)| {
        point.attribute(attribute).is_some_and(|held| held == value)
    };
    match rule {
        Count::Occurrences => add(total, Some(1.0)),
        Count::Values { only, except } if only.is_none_or(holds) && !except.is_some_and(holds) => {
            match point.kind {
                TelemetryPointKind::Log => add(total, Some(1.0)),
                _ => add_numeric(point.numeric_value, total),
            }
        }
        Count::Values { .. } => {}
    }
}

fn add_numeric(value: Option<f64>, total: &mut Option<f64>) {
    add(
        total,
        value.filter(|value| value.is_finite() && *value >= 0.0),
    );
}

fn add(total: &mut Option<f64>, value: Option<f64>) {
    if let Some(value) = value {
        *total = Some(total.unwrap_or(0.0) + value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::decode::MAX_POINTS;
    use crate::metrics::fixture::{
        INTERVAL, T120, T130, claude_instruments, codex_instruments, codex_session, logs_telemetry,
        metric, metrics_telemetry, point, session, telemetry,
    };
    use crate::metrics::runtime::{CODEX_SOURCE, ROLLOUT_SOURCE};
    use crate::metrics::{MetricStatus, MetricsQuery, session_metrics};
    use feld_core::telemetry::proto::{self, AggregationTemporality::Delta};

    /// Every Codex metric at once, each by the rule its own Runtime asks for: the token
    /// kinds of a histogram's sums, cost in micro-USD, failures over both transports, tool
    /// calls without the second record of a shell run, the p95 of a latency histogram, and
    /// the context window the rollout gauges carry.
    #[test]
    fn projects_codex_telemetry_by_the_rules_of_its_runtime() {
        let codex = codex_session();
        let tokens = |kind: &str, sum: f64| {
            proto::HistogramDataPoint::new(T120 - INTERVAL, T120, sum, &[1_000.0], &[1, 0])
                .with("token_type", kind)
        };
        // One log record as codex-cli 0.150.0 writes it: the instrument is the `event.name`
        // attribute and the OTLP `event_name` is the source location it was written at.
        let request = |success: &str| {
            proto::LogRecord::new(T120)
                .with_event_name("event otel/src/events/session_telemetry.rs:721")
                .with("event.name", "codex.api_request")
                .with("success", success)
        };
        let metrics = metrics_telemetry(
            "m1",
            vec![
                proto::Metric::histogram(
                    "codex.turn.token_usage",
                    Delta,
                    vec![
                        tokens("input", 57_877.0),
                        tokens("cached_input", 54_016.0),
                        tokens("output", 212.0),
                        tokens("cache_write_input", 0.0),
                        // Codex counts these inside the four above.
                        tokens("total", 58_089.0),
                        tokens("reasoning_output", 24.0),
                    ],
                ),
                proto::Metric::sum(
                    "codex.turn.cost_microusd",
                    Delta,
                    vec![point(T120, 12_500.0)],
                ),
                proto::Metric::sum(
                    "codex.websocket.request",
                    Delta,
                    vec![
                        point(T120, 2.0).with("success", "false"),
                        point(T120, 5.0).with("success", "true"),
                    ],
                ),
                proto::Metric::sum(
                    "codex.tool.call",
                    Delta,
                    vec![
                        point(T120, 3.0).with("tool", "exec_command"),
                        // The second record of the same shell runs.
                        point(T120, 3.0).with("tool", "exec"),
                        point(T120, 2.0).with("tool", "read_file"),
                    ],
                ),
                proto::Metric::histogram(
                    "codex.turn.ttft.duration_ms",
                    Delta,
                    vec![proto::HistogramDataPoint::new(
                        T120 - INTERVAL,
                        T120,
                        1_700.0,
                        &[100.0, 200.0],
                        &[0, 10, 0],
                    )],
                ),
                proto::Metric::gauge(
                    "feld.codex.context_window.size",
                    vec![point(T120, 258_400.0)],
                ),
                proto::Metric::gauge(
                    "feld.codex.context_window.used_percent",
                    vec![point(T120, 25.0)],
                ),
            ],
        );
        let events = logs_telemetry("l1", vec![request("false"), request("true")]);
        let response = session_metrics(
            &codex,
            &codex_instruments(),
            &[metrics, events],
            &MetricsQuery::default(),
        )
        .unwrap();

        let tokens = metric(&response, "token_usage");
        assert_eq!(tokens.status, MetricStatus::Reported);
        assert_eq!(tokens.source, CODEX_SOURCE);
        assert_eq!(
            tokens.series[0].value,
            json!({
                "input": 3_861.0,
                "output": 212.0,
                "cache_read": 54_016.0,
                "cache_creation": 0.0,
                "total": 58_089.0,
            })
        );
        assert_eq!(metric(&response, "estimated_cost").series[0].value, 0.0125);
        // Two failed websocket requests and the one failed HTTP request.
        assert_eq!(metric(&response, "api_errors").series[0].value, 3.0);
        assert_eq!(metric(&response, "tool_calls").series[0].value, 5.0);
        // The rank falls nine tenths of the way through the bucket above 100 ms.
        let latency = metric(&response, "api_request_latency_p95");
        assert_eq!(latency.status, MetricStatus::Reported);
        assert_eq!(latency.series[0].value, 195.0);
        let context = metric(&response, "context_window_utilization");
        assert_eq!(context.status, MetricStatus::Reported);
        assert_eq!(context.source, ROLLOUT_SOURCE);
        assert_eq!(
            context.series[0].value,
            json!({"size": 258_400.0, "used_percent": 25.0})
        );
        let active = metric(&response, "active_time");
        assert_eq!(active.status, MetricStatus::NotApplicable);
        assert_eq!(active.source, "Not provided by Codex");
    }

    #[test]
    fn projects_real_claude_request_and_tool_events() {
        let claude = session("agent-runtime", "Claude Code");
        let captured = telemetry(
            "real-logs",
            "logs",
            include_bytes!("../../../feld-cli/tests/fixtures/claude-2.1.273-logs.pb").to_vec(),
        );
        let error = logs_telemetry(
            "error",
            vec![
                proto::LogRecord::new(T130)
                    .with_body("claude_code.api_error")
                    .with("event.name", "api_error"),
            ],
        );
        let response = session_metrics(
            &claude,
            &claude_instruments(),
            &[captured, error],
            &MetricsQuery::default(),
        )
        .unwrap();
        let tools = metric(&response, "tool_calls");
        assert_eq!(tools.status, MetricStatus::Reported);
        assert_eq!(tools.series[0].value, 1.0);
        assert_eq!(
            metric(&response, "api_request_latency_p95").status,
            MetricStatus::Reported
        );
        let errors = metric(&response, "api_errors");
        assert_eq!(errors.status, MetricStatus::Reported);
        assert_eq!(errors.series[0].value, 1.0);
    }

    /// A size reported without a utilization is no reading of its own: its bin stays empty,
    /// and the size rides along with the next utilization.
    #[test]
    fn context_window_bins_are_the_ones_with_a_utilization() {
        let claude = session("agent-runtime", "Claude Code");
        let context = metrics_telemetry(
            "context",
            vec![
                proto::Metric::gauge(
                    "feld.claude_code.context_window.size",
                    vec![
                        point(T120 - 60_000_000_000, 200_000.0),
                        point(T120 + 120_000_000_000, 1_000_000.0),
                    ],
                ),
                proto::Metric::gauge(
                    "feld.claude_code.context_window.used_percent",
                    vec![point(T120, 30.0)],
                ),
            ],
        );
        let response = session_metrics(
            &claude,
            &claude_instruments(),
            &[context],
            &MetricsQuery {
                start: Some("1970-01-01T00:00:00Z"),
                end: Some("1970-01-01T00:05:00Z"),
                bucket: Some(60),
                metric: Some("context_window_utilization"),
            },
        )
        .unwrap();
        let series = &response.metrics[0].series;
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].start, "1970-01-01T00:02:00.000000000Z");
        assert_eq!(
            series[0].value,
            json!({"size": 200_000.0, "used_percent": 30.0})
        );
    }

    /// A reading from before the range holds into its first bin while the Session runs
    /// there, with the size known by then; an ended Session holds nothing past its end.
    #[test]
    fn a_context_window_reading_from_before_the_range_holds_into_it() {
        let context = metrics_telemetry(
            "context",
            vec![
                proto::Metric::gauge(
                    "feld.claude_code.context_window.size",
                    vec![point(T120 - 60_000_000_000, 200_000.0)],
                ),
                proto::Metric::gauge(
                    "feld.claude_code.context_window.used_percent",
                    vec![point(T130, 40.0)],
                ),
            ],
        );
        let query = MetricsQuery {
            start: Some("1970-01-01T00:03:00Z"),
            end: Some("1970-01-01T00:05:00Z"),
            bucket: Some(60),
            metric: Some("context_window_utilization"),
        };
        let running = session("agent-runtime", "Claude Code");
        let response = session_metrics(
            &running,
            &claude_instruments(),
            std::slice::from_ref(&context),
            &query,
        )
        .unwrap();
        let series = &response.metrics[0].series;
        assert_eq!(response.metrics[0].status, MetricStatus::Reported);
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].start, "1970-01-01T00:03:00.000000000Z");
        assert_eq!(
            series[0].value,
            json!({"size": 200_000.0, "used_percent": 40.0})
        );

        let mut ended = running;
        ended.end = Some("1970-01-01T00:02:30Z".to_owned());
        let response = session_metrics(&ended, &claude_instruments(), &[context], &query).unwrap();
        assert_eq!(response.metrics[0].status, MetricStatus::Unavailable);
    }

    /// A range read goes back before the range until it has a utilization and a size, each
    /// possibly in its own page, unless the Runtime reports neither.
    #[test]
    fn the_pages_before_a_range_hold_the_context_window_once_both_parts_are_found() {
        let gauge = |name: &str, value: f64| {
            metrics_telemetry(
                name,
                vec![proto::Metric::gauge(name, vec![point(T120, value)])],
            )
        };
        let size = gauge("feld.claude_code.context_window.size", 200_000.0);
        let used = gauge("feld.claude_code.context_window.used_percent", 40.0);

        let mut before = ContextWindowBefore::new(claude_instruments());
        assert!(!before.holds(&[]));
        assert!(!before.holds(std::slice::from_ref(&used)));
        assert!(before.holds(std::slice::from_ref(&size)));

        let mut before = ContextWindowBefore::new(claude_instruments());
        assert!(before.holds(&[size.clone(), used]));

        assert!(ContextWindowBefore::new(Instruments::default()).holds(&[size]));
    }

    /// A Session whose Runtime reported a size but never a utilization has no reading.
    #[test]
    fn a_context_window_size_alone_is_no_usage() {
        let claude = session("agent-runtime", "Claude Code");
        let context = metrics_telemetry(
            "context",
            vec![proto::Metric::gauge(
                "feld.claude_code.context_window.size",
                vec![point(T120, 200_000.0)],
            )],
        );
        let response = session_metrics(
            &claude,
            &claude_instruments(),
            &[context],
            &MetricsQuery {
                metric: Some("context_window_utilization"),
                ..MetricsQuery::default()
            },
        )
        .unwrap();
        assert_eq!(response.metrics[0].status, MetricStatus::Unavailable);
        assert_eq!(
            response.metrics[0].source,
            "No correlated Runtime usage record"
        );
    }

    #[test]
    fn rejects_a_context_window_percentage_above_100() {
        let claude = session("agent-runtime", "Claude Code");
        let context = metrics_telemetry(
            "context",
            vec![proto::Metric::gauge(
                "feld.claude_code.context_window.used_percent",
                vec![point(T120, 150.0)],
            )],
        );
        let response = session_metrics(
            &claude,
            &claude_instruments(),
            &[context],
            &MetricsQuery {
                metric: Some("context_window_utilization"),
                ..MetricsQuery::default()
            },
        )
        .unwrap();
        assert_eq!(response.metrics[0].status, MetricStatus::Unavailable);
        assert_eq!(response.metrics[0].source, INVALID_VALUE);
    }

    #[test]
    fn rejects_a_latency_histogram_whose_counts_overflow() {
        let codex = codex_session();
        let latency = metrics_telemetry(
            "latency",
            vec![proto::Metric::histogram(
                "codex.turn.ttft.duration_ms",
                Delta,
                vec![proto::HistogramDataPoint {
                    bucket_counts: vec![u64::MAX, 2],
                    ..proto::HistogramDataPoint::new(
                        T120 - INTERVAL,
                        T120,
                        1_700.0,
                        &[100.0],
                        &[0, 1],
                    )
                }],
            )],
        );
        let response = session_metrics(
            &codex,
            &codex_instruments(),
            &[latency],
            &MetricsQuery {
                metric: Some("api_request_latency_p95"),
                ..MetricsQuery::default()
            },
        )
        .unwrap();
        assert_eq!(response.metrics[0].status, MetricStatus::Unavailable);
        assert_eq!(response.metrics[0].source, INVALID_VALUE);
    }

    /// An export over the point limit is invalid, as one that fails to decode.
    #[test]
    fn rejects_an_export_over_the_point_limit() {
        let codex = codex_session();
        let latency = metrics_telemetry(
            "latency",
            vec![proto::Metric::histogram(
                "codex.turn.ttft.duration_ms",
                Delta,
                vec![
                    proto::HistogramDataPoint::new(
                        T120 - INTERVAL,
                        T120,
                        1_700.0,
                        &[100.0],
                        &[0, 1],
                    );
                    MAX_POINTS + 1
                ],
            )],
        );
        let response = session_metrics(
            &codex,
            &codex_instruments(),
            &[latency],
            &MetricsQuery {
                metric: Some("api_request_latency_p95"),
                ..MetricsQuery::default()
            },
        )
        .unwrap();
        assert_eq!(response.metrics[0].status, MetricStatus::Unavailable);
        assert_eq!(response.metrics[0].source, INVALID_VALUE);
    }
}
