use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use base64::Engine;
use serde_json::json;

use crate::metrics::{
    ContextWindowBefore, InstrumentMaps, MetricsQuery, RuntimeMetricsResponse, runtime_metrics,
};
use crate::store::TelemetryAdmission;
use feld_core::plugin::protocol::RuntimeIdentity;
use feld_core::plugin::{PluginKind, Plugins, not_installed};

use super::AppState;
use super::auth::{AdminCaller, Caller, InternalCaller, SelfCaller};
use super::daemon::current_sessions;
use super::error::{ApiError, error, internal, session_write_error};
use super::query::{ApiJson, ApiQuery, SessionMetricsQuery, normalize_timestamp};
use super::types::{
    ProviderEntry, ProviderList, RuntimeEntry, RuntimeList, SessionResponse, TelemetryRequest,
    TelemetryTokenRequest,
};

pub(super) async fn list_runtimes(
    State(state): State<AppState>,
    _caller: Caller,
) -> Result<Json<RuntimeList>, ApiError> {
    let sessions = current_sessions(&state)?;
    let runtimes = state
        .registry
        .runtimes()
        .into_iter()
        .map(|runtime| RuntimeEntry {
            running_sessions: sessions
                .iter()
                .filter(|session| session.execution.runtime.as_deref() == Some(&runtime.name))
                .count(),
            plugin: runtime,
        })
        .collect();
    Ok(Json(RuntimeList { runtimes }))
}

pub(super) async fn list_providers(
    State(state): State<AppState>,
    _caller: Caller,
) -> Result<Json<ProviderList>, ApiError> {
    let sessions = current_sessions(&state)?;
    let providers = state
        .registry
        .providers()
        .into_iter()
        .map(|provider| ProviderEntry {
            running_sessions: sessions
                .iter()
                .filter(|session| session.placement.provider == provider.name)
                .count(),
            nodes: state.status.nodes.clone(),
            plugin: provider,
        })
        .collect();
    Ok(Json(ProviderList { providers }))
}

pub(super) async fn record_runtime(
    State(state): State<AppState>,
    caller: SelfCaller,
    ApiJson(identity): ApiJson<RuntimeIdentity>,
) -> Result<Json<SessionResponse>, ApiError> {
    let session = state
        .sessions
        .record_runtime_identity(&caller.session_id, &caller.token, &identity)
        .map_err(session_write_error)?;
    Ok(Json(SessionResponse {
        session,
        sub_agents: Vec::new(),
    }))
}

pub(super) async fn record_telemetry(
    State(state): State<AppState>,
    _caller: InternalCaller,
    ApiJson(request): ApiJson<TelemetryRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if request.request_id.is_empty()
        || request.token.is_empty()
        || !matches!(request.signal.as_str(), "metrics" | "logs")
        || request.content_type != "application/x-protobuf"
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "telemetry envelope is invalid",
        ));
    }
    let body = base64::engine::general_purpose::STANDARD
        .decode(&request.payload)
        .map_err(|_| error(StatusCode::BAD_REQUEST, "telemetry payload is not base64"))?;
    if body.len() > crate::collector::MAX_REQUEST_BYTES {
        return Err(error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "telemetry payload exceeds the Collector request limit",
        ));
    }
    match state
        .sessions
        .record_telemetry(
            &request.request_id,
            &request.token,
            &request.signal,
            &request.content_type,
            &body,
        )
        .map_err(internal)?
    {
        TelemetryAdmission::Stored { session_id } => Ok(Json(json!({
            "accepted": true,
            "duplicate": false,
            "session": session_id,
        }))),
        TelemetryAdmission::Duplicate { session_id } => Ok(Json(json!({
            "accepted": true,
            "duplicate": true,
            "session": session_id,
        }))),
        TelemetryAdmission::Unattributed => Err(error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "telemetry is unattributed",
        )),
        TelemetryAdmission::Conflict => Err(error(
            StatusCode::CONFLICT,
            "telemetry request id was reused with different content",
        )),
    }
}

/// Answers the Collector whether a telemetry token names an open Session, before it reads
/// the body of an export that carries the token.
pub(super) async fn check_telemetry_token(
    State(state): State<AppState>,
    _caller: InternalCaller,
    ApiJson(request): ApiJson<TelemetryTokenRequest>,
) -> Result<StatusCode, ApiError> {
    if request.token.is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "telemetry token is empty"));
    }
    match state
        .sessions
        .telemetry_token_session(&request.token)
        .map_err(internal)?
    {
        Some(_) => Ok(StatusCode::NO_CONTENT),
        None => Err(error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "telemetry is unattributed",
        )),
    }
}

pub(super) async fn get_runtime_metrics(
    State(state): State<AppState>,
    _caller: AdminCaller,
    Path(runtime): Path<String>,
    ApiQuery(query): ApiQuery<SessionMetricsQuery>,
) -> Result<Json<RuntimeMetricsResponse>, ApiError> {
    if state.registry.runtime(&runtime).is_none() {
        return Err(error(
            StatusCode::NOT_FOUND,
            &not_installed(PluginKind::Runtime, &runtime),
        ));
    }
    let start = normalize_timestamp(query.start.as_deref(), "start")?;
    let end = normalize_timestamp(query.end.as_deref(), "end")?;
    let response = tokio::task::spawn_blocking(move || {
        let maps = InstrumentMaps::read(&state.registry);
        let stored = state
            .sessions
            .range_telemetries(start.as_deref(), end.as_deref(), |session| {
                let mut before = ContextWindowBefore::new(maps.of(session));
                move |page: &[_]| before.holds(page)
            })
            .map_err(internal)?;
        let sessions = stored
            .iter()
            .map(|(session, telemetry)| (session, telemetry.as_slice()))
            .collect::<Vec<_>>();
        runtime_metrics(
            &runtime,
            &sessions,
            &maps,
            &MetricsQuery {
                start: start.as_deref(),
                end: end.as_deref(),
                bucket: query.bucket,
                metric: query.metric.as_deref(),
            },
        )
        .map_err(|cause| error(StatusCode::BAD_REQUEST, &cause.to_string()))
    })
    .await
    .map_err(|error| internal(anyhow::anyhow!(error)))??;
    Ok(Json(response))
}
