use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::watch;

use super::*;

#[tokio::test]
async fn keeps_the_daemon_answer_and_skips_failures() {
    async fn check_token(
        State(calls): State<Arc<AtomicUsize>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> StatusCode {
        calls.fetch_add(1, Ordering::SeqCst);
        if headers[header::AUTHORIZATION] != "Bearer internal" {
            return StatusCode::UNAUTHORIZED;
        }
        match body["token"].as_str().unwrap() {
            "open" => StatusCode::NO_CONTENT,
            "busy" => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let daemon = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/v1/self/telemetry/token", post(check_token))
                .with_state(Arc::clone(&calls)),
        )
        .into_future(),
    );
    let check = TokenCheck::new(
        reqwest::Client::new(),
        &format!("http://{address}"),
        "internal",
    );

    assert_eq!(check.check("open").await, Verdict::Valid);
    assert_eq!(check.check("open").await, Verdict::Valid);
    assert_eq!(check.check("closed").await, Verdict::Invalid);
    assert_eq!(check.check("unknown").await, Verdict::Invalid);
    assert_eq!(check.check("unknown").await, Verdict::Invalid);
    assert_eq!(calls.load(Ordering::SeqCst), 3);

    assert_eq!(check.check("busy").await, Verdict::Unknown);
    assert_eq!(check.check("busy").await, Verdict::Unknown);
    assert_eq!(calls.load(Ordering::SeqCst), 5);

    daemon.abort();
    assert_eq!(check.check("gone").await, Verdict::Unknown);
}

#[test]
fn a_full_cache_drops_expired_answers() {
    let check = TokenCheck::new(reqwest::Client::new(), "http://127.0.0.1:1", "internal");
    let expired = Instant::now() - Duration::from_secs(1);
    check
        .answers
        .lock()
        .unwrap()
        .extend((0..CACHE_CAPACITY).map(|index| (index.to_string(), (false, expired))));
    check.remember("fresh", true);
    assert_eq!(check.answers.lock().unwrap().len(), 1);
    assert_eq!(check.cached("fresh"), Some(true));
}

/// A daemon that holds every question until `open` turns true, and counts them.
#[derive(Clone)]
struct HeldDaemon {
    open: watch::Receiver<bool>,
    calls: Arc<AtomicUsize>,
    held: Arc<AtomicUsize>,
}

async fn held_check_token(State(daemon): State<HeldDaemon>) -> StatusCode {
    daemon.calls.fetch_add(1, Ordering::SeqCst);
    daemon.held.fetch_add(1, Ordering::SeqCst);
    let mut open = daemon.open.clone();
    let _ = open.wait_for(|open| *open).await;
    daemon.held.fetch_sub(1, Ordering::SeqCst);
    StatusCode::UNPROCESSABLE_ENTITY
}

async fn start_held_daemon() -> (Arc<TokenCheck>, HeldDaemon, watch::Sender<bool>) {
    let (open_tx, open) = watch::channel(false);
    let daemon = HeldDaemon {
        open,
        calls: Arc::default(),
        held: Arc::default(),
    };
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/v1/self/telemetry/token", post(held_check_token))
                .with_state(daemon.clone()),
        )
        .into_future(),
    );
    let check = TokenCheck::new(
        reqwest::Client::new(),
        &format!("http://{address}"),
        "internal",
    );
    (Arc::new(check), daemon, open_tx)
}

async fn eventually(condition: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

fn spawn_checks(
    check: &Arc<TokenCheck>,
    tokens: impl Iterator<Item = String>,
) -> Vec<tokio::task::JoinHandle<Verdict>> {
    tokens
        .map(|token| {
            let check = Arc::clone(check);
            tokio::spawn(async move { check.check(&token).await })
        })
        .collect()
}

/// Awaits every task and counts the verdicts as (invalid, unknown).
async fn verdicts(tasks: Vec<tokio::task::JoinHandle<Verdict>>) -> (usize, usize) {
    let mut counts = (0, 0);
    for task in tasks {
        match task.await.unwrap() {
            Verdict::Invalid => counts.0 += 1,
            Verdict::Unknown => counts.1 += 1,
            Verdict::Valid => panic!("a forged token passed"),
        }
    }
    counts
}

#[tokio::test]
async fn a_flood_of_one_unknown_token_shares_one_question_and_bounds_the_waiters() {
    let (check, daemon, open) = start_held_daemon().await;
    check.remember("open", true);

    let flood = spawn_checks(
        &check,
        std::iter::repeat_n("forged".to_owned(), MAX_WAITERS + 100),
    );
    eventually(|| daemon.held.load(Ordering::SeqCst) == 1).await;
    eventually(|| flood.iter().filter(|task| task.is_finished()).count() == 100).await;
    assert_eq!(check.checking.lock().unwrap().waiters, MAX_WAITERS);
    assert_eq!(check.check("open").await, Verdict::Valid);

    open.send(true).unwrap();
    assert_eq!(verdicts(flood).await, (MAX_WAITERS, 100));
    assert_eq!(daemon.calls.load(Ordering::SeqCst), 1);
    let checking = check.checking.lock().unwrap();
    assert_eq!(checking.waiters, 0);
    assert!(checking.answers.is_empty());
}

#[tokio::test]
async fn a_flood_of_distinct_unknown_tokens_bounds_the_questions() {
    let (check, daemon, open) = start_held_daemon().await;

    let flood = spawn_checks(
        &check,
        (0..MAX_WAITERS + 20).map(|index| format!("forged-{index}")),
    );
    eventually(|| daemon.held.load(Ordering::SeqCst) == MAX_LOOKUPS).await;
    eventually(|| flood.iter().filter(|task| task.is_finished()).count() == 20).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(daemon.held.load(Ordering::SeqCst), MAX_LOOKUPS);
    assert_eq!(check.checking.lock().unwrap().answers.len(), MAX_WAITERS);

    open.send(true).unwrap();
    assert_eq!(verdicts(flood).await, (MAX_WAITERS, 20));
    assert_eq!(daemon.calls.load(Ordering::SeqCst), MAX_WAITERS);
    let checking = check.checking.lock().unwrap();
    assert_eq!(checking.waiters, 0);
    assert!(checking.answers.is_empty());
}

#[tokio::test]
async fn a_cancelled_request_frees_the_room_of_its_token() {
    let (check, daemon, _open) = start_held_daemon().await;
    let task = {
        let check = Arc::clone(&check);
        tokio::spawn(async move { check.check("forged").await })
    };
    eventually(|| daemon.held.load(Ordering::SeqCst) == 1).await;
    task.abort();
    let _ = task.await;
    let checking = check.checking.lock().unwrap();
    assert_eq!(checking.waiters, 0);
    assert!(checking.answers.is_empty());
}
