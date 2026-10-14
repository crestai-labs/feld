use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use prost::Message;

use feld_core::telemetry::proto;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelemetryPointKind {
    Gauge,
    Sum,
    Histogram,
    Log,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TelemetryPoint {
    /// The metric's name, or a log record's OTLP `event_name`, which may be empty. Every
    /// point of one metric shares it.
    name: Arc<str>,
    /// A log record's body, when it reads as a string.
    body: Option<String>,
    pub kind: TelemetryPointKind,
    pub start_time_unix_nano: u64,
    pub timestamp_unix_nano: u64,
    pub numeric_value: Option<f64>,
    pub count: Option<u64>,
    pub buckets: Option<HistogramBuckets>,
    /// The point's own attributes; [`TelemetryPoint::attribute`] reads them over the
    /// resource's.
    attributes: BTreeMap<String, String>,
    /// The attributes of the resource the point came from, shared by all its points.
    resource: Arc<BTreeMap<String, String>>,
    pub temporality_delta: bool,
}

/// How one histogram data point spread its observations: the explicit upper bounds and
/// the count of each bucket, the last of which is the open bucket above the highest
/// bound. A projection that needs a quantile reads the value from these.
#[derive(Clone, Debug, PartialEq)]
pub struct HistogramBuckets {
    pub bounds: Vec<f64>,
    pub counts: Vec<u64>,
}

impl HistogramBuckets {
    /// Whether the bounds rise and the counts cover every bucket they cut, which is what
    /// makes the observations placeable.
    pub fn is_usable(&self) -> bool {
        self.counts.len() == self.bounds.len() + 1
            && !self.bounds.is_empty()
            && self.bounds.iter().all(|bound| bound.is_finite())
            && self.bounds.windows(2).all(|pair| pair[0] < pair[1])
    }

    /// The count of every observation, or none when the counts add up beyond `u64`.
    pub fn total(&self) -> Option<u64> {
        self.counts
            .iter()
            .try_fold(0_u64, |total, count| total.checked_add(*count))
    }
}

impl TelemetryPoint {
    /// The value of attribute `key`: the point's own, or else its resource's.
    pub fn attribute(&self, key: &str) -> Option<&str> {
        self.attributes
            .get(key)
            .or_else(|| self.resource.get(key))
            .map(String::as_str)
    }

    /// The point's name: a log record goes by its OTLP `event_name`, or else by its last
    /// alias, or else by `log`.
    pub fn name(&self) -> &str {
        if !self.name.is_empty() || self.kind != TelemetryPointKind::Log {
            return &self.name;
        }
        self.aliases().last().unwrap_or("log")
    }

    /// Every name the point carries for its instrument, the one to prefer first. Which of
    /// them means something is for the Runtime's instruments to say.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.aliases().chain([self.name()])
    }

    /// The other names a log record gives its instrument, in the order it is read by: the
    /// body, then the `event.name` attribute. A record names its instrument wherever its
    /// Runtime puts it: Claude Code in the body, Codex in the `event.name` attribute while
    /// the OTLP `event_name` holds the source location the record was written at.
    fn aliases(&self) -> impl Iterator<Item = &str> {
        let event_name = (self.kind == TelemetryPointKind::Log)
            .then(|| self.attribute("event.name"))
            .flatten();
        self.body.as_deref().into_iter().chain(event_name)
    }
}

/// The most points one export decodes into. An export over it fails to decode, which keeps
/// what the points hold in proportion to the export.
pub(super) const MAX_POINTS: usize = 10_000;

/// Decodes one export into the points the projections read.
pub fn decode_telemetry(signal: &str, bytes: &[u8]) -> Result<Vec<TelemetryPoint>> {
    match signal {
        "metrics" => decode_metrics(bytes),
        "logs" => decode_logs(bytes),
        _ => bail!("unsupported telemetry signal {signal}"),
    }
}

fn decode_metrics(bytes: &[u8]) -> Result<Vec<TelemetryPoint>> {
    let request = proto::ExportMetricsServiceRequest::decode(bytes)
        .context("decode OTLP metrics protobuf")?;
    let mut output = Vec::new();
    for resource_metrics in request.resource_metrics {
        let resource_attributes = resource_metrics
            .resource
            .as_ref()
            .map(|resource| attributes(&resource.attributes))
            .map(Arc::new)
            .unwrap_or_default();
        for scope in resource_metrics.scope_metrics {
            for metric in scope.metrics {
                let name: Arc<str> = metric.name.into();
                match metric.data {
                    Some(proto::metric::Data::Gauge(gauge)) => {
                        for point in gauge.data_points {
                            push(
                                &mut output,
                                number_telemetry_point(
                                    &name,
                                    TelemetryPointKind::Gauge,
                                    false,
                                    point,
                                    &resource_attributes,
                                ),
                            )?;
                        }
                    }
                    Some(proto::metric::Data::Sum(sum)) => {
                        let delta = sum.aggregation_temporality == 1;
                        for point in sum.data_points {
                            push(
                                &mut output,
                                number_telemetry_point(
                                    &name,
                                    TelemetryPointKind::Sum,
                                    delta,
                                    point,
                                    &resource_attributes,
                                ),
                            )?;
                        }
                    }
                    Some(proto::metric::Data::Histogram(histogram)) => {
                        let delta = histogram.aggregation_temporality == 1;
                        for point in histogram.data_points {
                            push(
                                &mut output,
                                TelemetryPoint {
                                    name: name.clone(),
                                    body: None,
                                    kind: TelemetryPointKind::Histogram,
                                    start_time_unix_nano: point.start_time_unix_nano,
                                    timestamp_unix_nano: point.time_unix_nano,
                                    numeric_value: point.sum,
                                    count: Some(point.count),
                                    buckets: Some(HistogramBuckets {
                                        bounds: point.explicit_bounds,
                                        counts: point.bucket_counts,
                                    }),
                                    attributes: attributes(&point.attributes),
                                    resource: resource_attributes.clone(),
                                    temporality_delta: delta,
                                },
                            )?;
                        }
                    }
                    Some(proto::metric::Data::ExponentialHistogram(_))
                    | Some(proto::metric::Data::Summary(_))
                    | None => {}
                }
            }
        }
    }
    Ok(output)
}

/// Adds a decoded point, failing once the export holds more than [`MAX_POINTS`].
fn push(output: &mut Vec<TelemetryPoint>, point: TelemetryPoint) -> Result<()> {
    if output.len() == MAX_POINTS {
        bail!("OTLP export holds more than {MAX_POINTS} points");
    }
    output.push(point);
    Ok(())
}

fn number_telemetry_point(
    name: &Arc<str>,
    kind: TelemetryPointKind,
    temporality_delta: bool,
    point: proto::NumberDataPoint,
    resource_attributes: &Arc<BTreeMap<String, String>>,
) -> TelemetryPoint {
    let numeric_value = point.value.map(|value| match value {
        proto::number_data_point::Value::AsDouble(value) => value,
        proto::number_data_point::Value::AsInt(value) => value as f64,
    });
    TelemetryPoint {
        name: name.clone(),
        body: None,
        kind,
        start_time_unix_nano: point.start_time_unix_nano,
        timestamp_unix_nano: point.time_unix_nano,
        numeric_value,
        count: None,
        buckets: None,
        attributes: attributes(&point.attributes),
        resource: resource_attributes.clone(),
        temporality_delta,
    }
}

fn decode_logs(bytes: &[u8]) -> Result<Vec<TelemetryPoint>> {
    let request =
        proto::ExportLogsServiceRequest::decode(bytes).context("decode OTLP logs protobuf")?;
    let mut output = Vec::new();
    for resource_logs in request.resource_logs {
        let resource_attributes = resource_logs
            .resource
            .as_ref()
            .map(|resource| attributes(&resource.attributes))
            .map(Arc::new)
            .unwrap_or_default();
        for scope in resource_logs.scope_logs {
            for record in scope.log_records {
                push(
                    &mut output,
                    TelemetryPoint {
                        name: record.event_name.into(),
                        body: record.body.as_ref().and_then(any_value_string),
                        kind: TelemetryPointKind::Log,
                        start_time_unix_nano: 0,
                        timestamp_unix_nano: if record.time_unix_nano == 0 {
                            record.observed_time_unix_nano
                        } else {
                            record.time_unix_nano
                        },
                        numeric_value: None,
                        count: None,
                        buckets: None,
                        attributes: attributes(&record.attributes),
                        resource: resource_attributes.clone(),
                        temporality_delta: false,
                    },
                )?;
            }
        }
    }
    Ok(output)
}

fn attributes(values: &[proto::KeyValue]) -> BTreeMap<String, String> {
    values
        .iter()
        .filter_map(|attribute| {
            Some((
                attribute.key.clone(),
                any_value_string(attribute.value.as_ref()?)?,
            ))
        })
        .collect()
}

fn any_value_string(value: &proto::AnyValue) -> Option<String> {
    Some(match value.value.as_ref()? {
        proto::any_value::Value::String(value) => value.clone(),
        proto::any_value::Value::Bool(value) => value.to_string(),
        proto::any_value::Value::Int(value) => value.to_string(),
        proto::any_value::Value::Double(value) => value.to_string(),
        proto::any_value::Value::Bytes(value) => BASE64.encode(value),
    })
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;

    fn names(point: &TelemetryPoint) -> Vec<&str> {
        point.names().collect()
    }

    #[test]
    fn decodes_metric_kinds_and_log_events() {
        let metrics = proto::ExportMetricsServiceRequest::new(
            None,
            None,
            vec![
                proto::Metric::sum(
                    "counter",
                    proto::AggregationTemporality::Delta,
                    vec![proto::NumberDataPoint::int(10, 20, 7).with("token_type", "input")],
                ),
                proto::Metric::histogram(
                    "histogram",
                    proto::AggregationTemporality::Cumulative,
                    vec![
                        proto::HistogramDataPoint::new(10, 20, 11.0, &[5.0, 10.0], &[1, 1, 0])
                            .with("token_type", "input"),
                    ],
                ),
            ],
        );
        let points = decode_telemetry("metrics", &metrics.encode_to_vec()).unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].kind, TelemetryPointKind::Sum);
        assert!(points[0].temporality_delta);
        assert_eq!(points[0].numeric_value, Some(7.0));
        assert_eq!(points[1].kind, TelemetryPointKind::Histogram);
        assert!(!points[1].temporality_delta);
        assert_eq!(points[1].count, Some(2));

        let logs = proto::ExportLogsServiceRequest::new(
            None,
            None,
            vec![proto::LogRecord::new(42).with_event_name("claude_code.api_request")],
        );
        let points = decode_telemetry("logs", &logs.encode_to_vec()).unwrap();
        assert_eq!(points[0].kind, TelemetryPointKind::Log);
        assert_eq!(points[0].name(), "claude_code.api_request");
        assert_eq!(points[0].timestamp_unix_nano, 42);
    }

    /// Claude Code names the instrument in the body and a short form in `event.name`.
    #[test]
    fn a_claude_log_event_names_its_instrument_in_the_body_first() {
        let points = decode_telemetry(
            "logs",
            include_bytes!("../../../feld-cli/tests/fixtures/claude-2.1.273-logs.pb"),
        )
        .unwrap();
        let request = points
            .iter()
            .find(|point| point.names().next() == Some("claude_code.api_request"))
            .unwrap();
        assert_eq!(request.attribute("event.name"), Some("api_request"));
        assert!(request.attribute("duration_ms").is_some());
        assert!(
            points
                .iter()
                .any(|point| names(point).starts_with(&["claude_code.tool_result", "tool_result"]))
        );

        let error = proto::ExportLogsServiceRequest::new(
            None,
            None,
            vec![
                proto::LogRecord::new(42)
                    .with_body("claude_code.api_error")
                    .with("event.name", "api_error"),
            ],
        );
        let points = decode_telemetry("logs", &error.encode_to_vec()).unwrap();
        assert_eq!(
            names(&points[0]),
            ["claude_code.api_error", "api_error", "api_error"]
        );
    }

    /// The shape codex-cli 0.150.0 writes: the instrument is the `event.name` attribute,
    /// the OTLP `event_name` is the source location the record was written at, and
    /// `success` is a boolean.
    #[test]
    fn a_codex_log_event_keeps_every_name_it_carries() {
        let logs = proto::ExportLogsServiceRequest::new(
            None,
            None,
            vec![
                proto::LogRecord::new(42)
                    .with_event_name("event model-provider/src/models_endpoint.rs:204")
                    .with("event.name", "codex.api_request")
                    .with("success", false)
                    .with("http.response.status_code", 500_i64),
            ],
        );
        let points = decode_telemetry("logs", &logs.encode_to_vec()).unwrap();
        assert_eq!(
            names(&points[0]),
            [
                "codex.api_request",
                "event model-provider/src/models_endpoint.rs:204"
            ]
        );
        assert_eq!(points[0].attribute("success"), Some("false"));
        assert_eq!(
            points[0].attribute("http.response.status_code"),
            Some("500")
        );
    }

    fn resource(attributes: &[(&str, &str)]) -> proto::Resource {
        proto::Resource::new(
            attributes
                .iter()
                .map(|(key, value)| proto::KeyValue::new(*key, *value))
                .collect(),
        )
    }

    /// Every point of an export shares its resource's attributes and its metric's name,
    /// so what the points hold grows with the export, not with attributes × points.
    #[test]
    fn points_share_the_resource_attributes_and_the_metric_name() {
        let large = "x".repeat(1 << 20);
        let name = format!("claude_code.{large}");
        let points = MAX_POINTS / 2;
        let metrics = proto::ExportMetricsServiceRequest::new(
            Some(resource(&[("service.name", &large)])),
            None,
            vec![
                proto::Metric::sum(
                    &name,
                    proto::AggregationTemporality::Delta,
                    vec![proto::NumberDataPoint::default(); points],
                ),
                proto::Metric::histogram(
                    &name,
                    proto::AggregationTemporality::Delta,
                    vec![proto::HistogramDataPoint::default(); points],
                ),
            ],
        );
        let decoded = decode_telemetry("metrics", &metrics.encode_to_vec()).unwrap();
        assert_eq!(decoded.len(), 2 * points);
        let first = &decoded[0];
        assert!(decoded.iter().all(|point| {
            Arc::ptr_eq(&point.resource, &first.resource) && point.attributes.is_empty()
        }));
        assert_eq!(Arc::strong_count(&first.resource), 2 * points);
        assert!(
            decoded[..points]
                .iter()
                .all(|point| Arc::ptr_eq(&point.name, &first.name))
        );
        assert!(
            decoded[points..]
                .iter()
                .all(|point| Arc::ptr_eq(&point.name, &decoded[points].name))
        );
        assert_eq!(first.name(), name);
        assert_eq!(first.attribute("service.name"), Some(large.as_str()));
    }

    /// A point's own attribute reads over its resource's, as one merged map would.
    #[test]
    fn a_point_attribute_reads_over_the_resource_attribute() {
        let metrics = proto::ExportMetricsServiceRequest::new(
            Some(resource(&[("type", "resource"), ("host", "laptop")])),
            None,
            vec![proto::Metric::gauge(
                "gauge",
                vec![proto::NumberDataPoint::int(10, 20, 1).with("type", "user")],
            )],
        );
        let points = decode_telemetry("metrics", &metrics.encode_to_vec()).unwrap();
        assert_eq!(points[0].attribute("type"), Some("user"));
        assert_eq!(points[0].attribute("host"), Some("laptop"));
        assert_eq!(points[0].attribute("missing"), None);
    }

    /// A log record's `event.name` is read from its attributes, not copied into its names.
    #[test]
    fn a_log_event_name_is_read_from_its_attribute() {
        let logs = proto::ExportLogsServiceRequest::new(
            Some(resource(&[("event.name", "codex.api_request")])),
            None,
            vec![proto::LogRecord::new(42)],
        );
        let points = decode_telemetry("logs", &logs.encode_to_vec()).unwrap();
        let attribute = points[0].attribute("event.name").unwrap();
        assert_eq!(
            names(&points[0]),
            ["codex.api_request", "codex.api_request"]
        );
        assert!(
            names(&points[0])
                .iter()
                .all(|name| std::ptr::eq(*name, attribute))
        );
    }

    /// An export decodes into at most [`MAX_POINTS`] points, its data points and log
    /// records alike.
    #[test]
    fn an_export_over_the_point_limit_fails_to_decode() {
        let metrics = |points| {
            proto::ExportMetricsServiceRequest::new(
                None,
                None,
                vec![
                    proto::Metric::gauge("gauge", vec![proto::NumberDataPoint::default(); 1]),
                    proto::Metric::sum(
                        "sum",
                        proto::AggregationTemporality::Delta,
                        vec![proto::NumberDataPoint::default(); points - 1],
                    ),
                ],
            )
            .encode_to_vec()
        };
        assert_eq!(
            decode_telemetry("metrics", &metrics(MAX_POINTS))
                .unwrap()
                .len(),
            MAX_POINTS
        );
        assert!(decode_telemetry("metrics", &metrics(MAX_POINTS + 1)).is_err());

        let logs = |records| {
            proto::ExportLogsServiceRequest::new(
                None,
                None,
                vec![proto::LogRecord::new(42); records],
            )
            .encode_to_vec()
        };
        assert_eq!(
            decode_telemetry("logs", &logs(MAX_POINTS)).unwrap().len(),
            MAX_POINTS
        );
        assert!(decode_telemetry("logs", &logs(MAX_POINTS + 1)).is_err());
    }
}
