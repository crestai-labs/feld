//! The Sessions, manifests and exports the metrics tests share.

use prost::Message;
use serde_json::{Value, json};

use super::instruments::{InstrumentMaps, Instruments};
use super::response::{MetricProjection, RuntimeQuota, SessionMetricsResponse};
use super::{MetricsQuery, QUOTA, runtime_metrics};
use crate::store::Telemetry;
use feld_core::model::Session;
use feld_core::telemetry::proto;

pub(super) const T120: u64 = 120_000_000_000;
pub(super) const T130: u64 = 130_000_000_000;
/// The interval each exported point covers, up to its timestamp.
pub(super) const INTERVAL: u64 = 10_000_000_000;

/// One interval's value, ending at `time`.
pub(super) fn point(time: u64, value: f64) -> proto::NumberDataPoint {
    proto::NumberDataPoint::double(time - INTERVAL, time, value)
}

pub(super) fn quota_read(
    runtime: &str,
    sessions: &[(&Session, &[Telemetry])],
    maps: &InstrumentMaps,
) -> RuntimeQuota {
    runtime_metrics(
        runtime,
        sessions,
        maps,
        &MetricsQuery {
            metric: Some(QUOTA),
            ..MetricsQuery::default()
        },
    )
    .unwrap()
    .quota
    .unwrap()
}

pub(super) fn metric<'a>(response: &'a SessionMetricsResponse, name: &str) -> &'a MetricProjection {
    response
        .metrics
        .iter()
        .find(|metric| metric.metric == name)
        .unwrap()
}

/// The `metrics` of the Claude Code Plugin Feld ships, so that these tests read the
/// map a real manifest gives the daemon.
pub(super) fn claude_instruments() -> Instruments {
    let manifest: Value = serde_json::from_str(include_str!(
        "../../../../plugins/runtime/claude-code/plugin.json"
    ))
    .expect("the bundled manifest is JSON");
    Instruments::read(manifest.get("metrics"))
}

pub(super) fn claude_maps() -> InstrumentMaps {
    InstrumentMaps {
        by_runtime: [("claude-code".to_owned(), claude_instruments())]
            .into_iter()
            .collect(),
    }
}

/// The `metrics` of the Codex Plugin Feld ships.
pub(super) fn codex_instruments() -> Instruments {
    let manifest: Value = serde_json::from_str(include_str!(
        "../../../../plugins/runtime/codex/plugin.json"
    ))
    .expect("the bundled manifest is JSON");
    Instruments::read(manifest.get("metrics"))
}

pub(super) fn session(execution_type: &str, label: &str) -> Session {
    let runtime = (execution_type != "program").then_some("claude-code");
    serde_json::from_value(json!({
        "id": "s1",
        "workspace": "default",
        "actorId": "actor_developer",
        "actor": "developer",
        "actorKind": "agent",
        "generation": 1,
        "start": "1970-01-01T00:00:00Z",
        "end": null,
        "startKind": "start",
        "ownership": "owned",
        "endKind": "running",
        "exitCode": null,
        "planned": null,
        "execution": {
            "type": execution_type,
            "runtime": runtime,
            "label": label,
            "command": null,
            "ai": true
        },
        "placement": {"provider": "tmux", "node": "local", "workdir": "/tmp", "placement": {}},
        "lifecycle": {},
        "observation": null,
        "model": null,
        "outcome": "running"
    }))
    .unwrap()
}

pub(super) fn codex_session() -> Session {
    let mut codex = session("agent-runtime", "Codex");
    codex.execution.runtime = Some("codex".to_owned());
    codex
}

pub(super) fn metrics_telemetry(id: &str, metrics: Vec<proto::Metric>) -> Telemetry {
    let request = proto::ExportMetricsServiceRequest::new(None, None, metrics);
    telemetry(id, "metrics", request.encode_to_vec())
}

pub(super) fn logs_telemetry(id: &str, records: Vec<proto::LogRecord>) -> Telemetry {
    let request = proto::ExportLogsServiceRequest::new(None, None, records);
    telemetry(id, "logs", request.encode_to_vec())
}

pub(super) fn telemetry(id: &str, signal: &str, body: Vec<u8>) -> Telemetry {
    Telemetry {
        request_id: id.to_owned(),
        session_id: "s1".to_owned(),
        signal: signal.to_owned(),
        content_type: "application/x-protobuf".to_owned(),
        body,
        received_at: "1970-01-01T00:03:00Z".to_owned(),
    }
}
