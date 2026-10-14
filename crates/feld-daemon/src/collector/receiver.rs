use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::post;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use http_body_util::LengthLimitError;
use prost::Message;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tower::limit::GlobalConcurrencyLimitLayer;
use uuid::Uuid;

use super::CollectorConfig;
use super::child::wait_for_parent_exit;
use super::queue::{Envelope, Queue};
use super::tokens::{TokenCheck, Verdict};
use feld_core::telemetry::proto;

pub(crate) const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_ENVELOPE_BYTES: usize = 8 * 1024 * 1024;
/// How many exports the Collector handles at once, over every route.
const MAX_CONCURRENT_REQUESTS: usize = 16;
/// How long the Collector waits for the body of an authorized export.
const BODY_READ_DEADLINE: Duration = Duration::from_secs(10);
const RETRY_DELAYS: [Duration; 6] = [
    Duration::from_millis(0),
    Duration::from_millis(100),
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];

/// Receives OTLP/HTTP on `listener` and forwards each export to the daemon until the daemon
/// that started this Collector exits.
pub(super) async fn serve(listener: TcpListener, config: CollectorConfig) -> Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(5))
        .build()
        .context("build collector forwarding client")?;
    let queue = Arc::new(Queue::default());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let app = Router::new()
        .route("/v1/metrics", post(receive_metrics))
        .route("/v1/logs", post(receive_logs))
        .layer(GlobalConcurrencyLimitLayer::new(MAX_CONCURRENT_REQUESTS))
        .with_state(ReceiverState {
            queue: Arc::clone(&queue),
            tokens: Arc::new(TokenCheck::new(
                client.clone(),
                &config.daemon_api,
                &config.internal_token,
            )),
        });
    let worker = tokio::spawn(forward_requests(
        queue,
        shutdown_rx.clone(),
        client,
        format!("{}/v1/self/telemetry", config.daemon_api),
        config.internal_token,
    ));
    let mut server_shutdown = shutdown_rx;
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        wait_for_shutdown(&mut server_shutdown).await;
    });
    tokio::select! {
        result = server => result.context("serve collector endpoint")?,
        signal = tokio::signal::ctrl_c() => signal.context("wait for collector shutdown")?,
        () = wait_for_parent_exit(config.parent_pid) => {},
    }
    let _ = shutdown_tx.send(true);
    let _ = worker.await;
    Ok(())
}

#[derive(Clone)]
struct ReceiverState {
    queue: Arc<Queue>,
    tokens: Arc<TokenCheck>,
}

async fn receive_metrics(
    State(state): State<ReceiverState>,
    headers: HeaderMap,
    body: Body,
) -> StatusCode {
    receive(state, headers, body, "metrics").await
}

async fn receive_logs(
    State(state): State<ReceiverState>,
    headers: HeaderMap,
    body: Body,
) -> StatusCode {
    receive(state, headers, body, "logs").await
}

/// Reads the body only after the token names an open Session, so a request without one
/// costs the Collector no more than its headers.
async fn receive(
    state: ReceiverState,
    headers: HeaderMap,
    body: Body,
    signal: &'static str,
) -> StatusCode {
    if !is_protobuf(&headers) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE;
    }
    let Some(token) = bearer_token(&headers) else {
        return StatusCode::UNAUTHORIZED;
    };
    match state.tokens.check(&token).await {
        Verdict::Valid => {}
        Verdict::Invalid => return StatusCode::UNAUTHORIZED,
        Verdict::Unknown => return StatusCode::SERVICE_UNAVAILABLE,
    }
    let body = match read_body(body, BODY_READ_DEADLINE).await {
        Ok(body) => body,
        Err(status) => return status,
    };
    let valid = match signal {
        "metrics" => proto::ExportMetricsServiceRequest::decode(body.as_ref()).is_ok(),
        "logs" => proto::ExportLogsServiceRequest::decode(body.as_ref()).is_ok(),
        _ => false,
    };
    if !valid {
        return StatusCode::BAD_REQUEST;
    }

    let envelope = Envelope {
        request_id: Uuid::new_v4().to_string(),
        token,
        signal,
        content_type: "application/x-protobuf",
        payload: BASE64.encode(body),
    };
    match state.queue.push(envelope) {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn read_body(body: Body, deadline: Duration) -> Result<Bytes, StatusCode> {
    match tokio::time::timeout(deadline, axum::body::to_bytes(body, MAX_REQUEST_BYTES)).await {
        Ok(Ok(body)) => Ok(body),
        Ok(Err(error)) if is_too_large(&error) => Err(StatusCode::PAYLOAD_TOO_LARGE),
        Ok(Err(_)) => Err(StatusCode::BAD_REQUEST),
        Err(_) => Err(StatusCode::REQUEST_TIMEOUT),
    }
}

fn is_too_large(error: &axum::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = source {
        if error.is::<LengthLimitError>() {
            return true;
        }
        source = error.source();
    }
    false
}

async fn forward_requests(
    queue: Arc<Queue>,
    mut shutdown_rx: watch::Receiver<bool>,
    client: reqwest::Client,
    daemon_url: String,
    internal_token: String,
) {
    loop {
        tokio::select! {
            biased;
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            envelope = queue.pop() => {
                forward_one(
                    &client,
                    &daemon_url,
                    &internal_token,
                    &envelope,
                    &mut shutdown_rx,
                ).await;
            }
        }
    }
}

async fn forward_one(
    client: &reqwest::Client,
    daemon_url: &str,
    internal_token: &str,
    envelope: &Envelope,
    shutdown_rx: &mut watch::Receiver<bool>,
) {
    for delay in RETRY_DELAYS {
        if !delay.is_zero() {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        return;
                    }
                }
                _ = tokio::time::sleep(delay) => {}
            }
        }
        if *shutdown_rx.borrow() {
            return;
        }

        let result = client
            .post(daemon_url)
            .bearer_auth(internal_token)
            .json(envelope)
            .send()
            .await;
        match result {
            Ok(response) if response.status().is_success() => return,
            Ok(response) if !retryable(response.status()) => return,
            Ok(_) | Err(_) => {}
        }
    }
}

pub(super) fn retryable(status: StatusCode) -> bool {
    status.is_server_error()
        || status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
}

async fn wait_for_shutdown(shutdown_rx: &mut watch::Receiver<bool>) {
    if *shutdown_rx.borrow() {
        return;
    }
    while shutdown_rx.changed().await.is_ok() {
        if *shutdown_rx.borrow() {
            return;
        }
    }
}

fn is_protobuf(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/x-protobuf"))
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() || token.trim() != token {
        return None;
    }
    Some(token.to_owned())
}

#[cfg(test)]
mod tests;
