/// The environment that makes a Runtime export its telemetry to the Collector at
/// `collector`: the exporters, protocol, temporality and export interval of `metrics.md`.
pub fn exporter_environment(collector: &str) -> Vec<(String, String)> {
    [
        ("CLAUDE_CODE_ENABLE_TELEMETRY", "1"),
        ("OTEL_METRICS_EXPORTER", "otlp"),
        ("OTEL_LOGS_EXPORTER", "otlp"),
        ("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf"),
        ("OTEL_EXPORTER_OTLP_ENDPOINT", collector),
        ("OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE", "delta"),
        ("OTEL_METRIC_EXPORT_INTERVAL", "10000"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

/// What `feld env` puts in a shell: [`exporter_environment`], and how long Claude Code
/// keeps an `otelHeadersHelper` answer, so that an attach and a token issued again reach the
/// header early. A Runtime started from that shell exports to this Collector.
pub fn shell_exporter_environment(collector: &str) -> Vec<(String, String)> {
    let mut environment = exporter_environment(collector);
    environment.push((
        "CLAUDE_CODE_OTEL_HEADERS_HELPER_DEBOUNCE_MS".to_owned(),
        "60000".to_owned(),
    ));
    environment
}
