use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Mutex;

use axum::Json;
use serde_json::{Value, json};

use super::super::queue::TOKEN_CAPACITY;
use super::*;

/// A daemon whose open Sessions hold `session-secret` and `other-secret`. Its telemetry
/// intake fails the first request it sees.
#[derive(Clone, Default)]
struct Daemon {
    requests: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
}

async fn check_token(headers: HeaderMap, Json(body): Json<Value>) -> StatusCode {
    if headers[header::AUTHORIZATION] != "Bearer internal-secret" {
        return StatusCode::UNAUTHORIZED;
    }
    match body["token"].as_str().unwrap() {
        "session-secret" | "other-secret" => StatusCode::NO_CONTENT,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

async fn ingest(
    State(daemon): State<Daemon>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let mut requests = daemon.requests.lock().unwrap();
    requests.push((headers, body));
    if requests.len() == 1 {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "try again"})),
        );
    }
    (
        StatusCode::OK,
        Json(json!({"accepted": true, "duplicate": true, "session": "s1"})),
    )
}

async fn start_daemon(daemon: Daemon) -> SocketAddr {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/v1/self/telemetry/token", post(check_token))
                .route("/v1/self/telemetry", post(ingest))
                .with_state(daemon),
        )
        .into_future(),
    );
    address
}

fn receiver_state(daemon_address: SocketAddr) -> ReceiverState {
    ReceiverState {
        queue: Arc::default(),
        tokens: Arc::new(TokenCheck::new(
            reqwest::Client::new(),
            &format!("http://{daemon_address}"),
            "internal-secret",
        )),
    }
}

fn metrics_payload() -> Vec<u8> {
    proto::ExportMetricsServiceRequest::new(
        None,
        None,
        vec![proto::Metric::gauge(
            "feld.claude_code.context_window.size",
            vec![proto::NumberDataPoint::double(0, 0, 200_000.0)],
        )],
    )
    .encode_to_vec()
}

fn export_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "application/x-protobuf".parse().unwrap(),
    );
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {token}").parse().unwrap(),
    );
    headers
}

#[tokio::test]
async fn forwards_protobuf_with_one_request_id_across_retry() {
    let daemon = Daemon::default();
    let requests = Arc::clone(&daemon.requests);
    let daemon_address = start_daemon(daemon).await;

    let receiver_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let receiver_address = receiver_listener.local_addr().unwrap();
    let state = receiver_state(daemon_address);
    let queue = Arc::clone(&state.queue);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let receiver_task = tokio::spawn(
        axum::serve(
            receiver_listener,
            Router::new()
                .route("/v1/metrics", post(receive_metrics))
                .with_state(state),
        )
        .into_future(),
    );
    let worker_task = tokio::spawn(forward_requests(
        queue,
        shutdown_rx,
        reqwest::Client::new(),
        format!("http://{daemon_address}/v1/self/telemetry"),
        "internal-secret".to_owned(),
    ));

    let payload = metrics_payload();
    let response = reqwest::Client::new()
        .post(format!("http://{receiver_address}/v1/metrics"))
        .bearer_auth("session-secret")
        .header(header::CONTENT_TYPE, "application/x-protobuf")
        .body(payload.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if requests.lock().unwrap().len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    {
        let requests = requests.lock().unwrap();
        assert_eq!(requests[0].1["requestId"], requests[1].1["requestId"]);
        assert_eq!(requests[0].1["token"], "session-secret");
        assert_eq!(requests[0].1["signal"], "metrics");
        assert_eq!(requests[0].1["contentType"], "application/x-protobuf");
        assert_eq!(requests[0].1["payload"], BASE64.encode(payload));
        assert_eq!(
            requests[0].0[header::AUTHORIZATION],
            "Bearer internal-secret"
        );
    }

    let _ = shutdown_tx.send(true);
    receiver_task.abort();
    worker_task.await.unwrap();
}

#[tokio::test]
async fn receiver_rejects_missing_or_unknown_token_and_invalid_protobuf() {
    let state = receiver_state(start_daemon(Daemon::default()).await);
    let mut headers = export_headers("session-secret");
    headers.remove(header::AUTHORIZATION);
    assert_eq!(
        receive(state.clone(), headers, Body::empty(), "metrics").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        receive(
            state.clone(),
            export_headers("forged"),
            Body::from(metrics_payload()),
            "metrics"
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        receive(
            state,
            export_headers("session-secret"),
            Body::from(vec![0xff]),
            "metrics"
        )
        .await,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn one_full_token_leaves_room_for_another() {
    let state = receiver_state(start_daemon(Daemon::default()).await);
    let export = |token: &str| {
        receive(
            state.clone(),
            export_headers(token),
            Body::from(metrics_payload()),
            "metrics",
        )
    };
    for _ in 0..TOKEN_CAPACITY {
        assert_eq!(export("session-secret").await, StatusCode::OK);
    }
    assert_eq!(
        export("session-secret").await,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(export("other-secret").await, StatusCode::OK);
}

#[tokio::test]
async fn receiver_answers_503_when_the_daemon_cannot_judge_a_token() {
    let unreachable = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = unreachable.local_addr().unwrap();
    drop(unreachable);
    assert_eq!(
        receive(
            receiver_state(address),
            export_headers("session-secret"),
            Body::from(metrics_payload()),
            "metrics"
        )
        .await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn receiver_refuses_an_unknown_token_without_reading_the_body() {
    let state = receiver_state(start_daemon(Daemon::default()).await);
    let endless = Body::from_stream(async_stream::stream! {
        std::future::pending::<()>().await;
        yield Ok::<_, std::io::Error>(Bytes::new());
    });
    let status = tokio::time::timeout(
        Duration::from_secs(2),
        receive(state, export_headers("forged"), endless, "metrics"),
    )
    .await
    .expect("the receiver answers before the body ends");
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn receiver_refuses_a_body_over_the_limit() {
    let state = receiver_state(start_daemon(Daemon::default()).await);
    assert_eq!(
        receive(
            state,
            export_headers("session-secret"),
            Body::from(vec![0; MAX_REQUEST_BYTES + 1]),
            "metrics"
        )
        .await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn read_body_answers_408_when_the_body_misses_the_deadline() {
    let endless = Body::from_stream(async_stream::stream! {
        std::future::pending::<()>().await;
        yield Ok::<_, std::io::Error>(Bytes::new());
    });
    assert_eq!(
        read_body(endless, Duration::from_millis(50)).await,
        Err(StatusCode::REQUEST_TIMEOUT)
    );
}
