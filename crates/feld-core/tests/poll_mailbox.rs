//! A mailbox reader whose daemon keeps answering what it cannot use, met by a fake daemon on
//! the socket of the Feld home.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::time::Instant;

use feld_core::home::{FELD_SESSION_ID_ENV, FELD_SESSION_TOKEN_ENV, FeldHome};
use feld_core::self_api::{RETRY, poll_mailbox};
use serde_json::json;

/// The answers the fake daemon gives in turn: a mailbox that does not decode, and a failure.
const ANSWERS: [(&str, &str); 2] = [
    ("200 OK", "not a mailbox"),
    ("500 Internal Server Error", ""),
];

/// How many requests the fake daemon answers before it takes the Feld home away.
const REQUESTS: usize = 6;

/// Every answer the reader cannot use is followed by a wait of [`RETRY`], and a home that is
/// gone ends the reader.
#[test]
fn a_reader_waits_retry_after_every_answer_it_cannot_use() {
    // The only test of this binary, so no other thread reads the environment.
    unsafe {
        std::env::set_var(FELD_SESSION_TOKEN_ENV, "fst_poll");
        std::env::set_var(FELD_SESSION_ID_ENV, "session_poll");
    }
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("home");
    let home = FeldHome::resolve(Some(root.clone())).unwrap();
    home.create().unwrap();
    std::fs::write(
        home.record_path(),
        json!({"instanceId": "daemon_0a1b", "pid": 1, "api": "http://127.0.0.1:7381"}).to_string(),
    )
    .unwrap();
    let listener = UnixListener::bind(home.socket_path()).unwrap();
    let served = std::thread::spawn(move || {
        let mut arrivals = Vec::new();
        for (index, stream) in listener.incoming().take(REQUESTS).enumerate() {
            let mut stream = stream.unwrap();
            let mut request = [0; 4096];
            let read = stream.read(&mut request).unwrap();
            arrivals.push(Instant::now());
            let request = String::from_utf8_lossy(&request[..read]).into_owned();
            assert!(
                request.starts_with("GET /v1/self/messages?limit=1&wait=true "),
                "{request}"
            );
            assert!(
                request.contains("authorization: Bearer fst_poll"),
                "{request}"
            );
            let (status, body) = ANSWERS[index % ANSWERS.len()];
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
        std::fs::remove_dir_all(&root).unwrap();
        arrivals
    });

    let mut held = Vec::new();
    poll_mailbox(
        &home,
        |session| held.push(session.to_owned()),
        |message| panic!("delivered {}", message.id),
    );
    let arrivals = served.join().unwrap();

    assert_eq!(held, ["session_poll"]);
    assert_eq!(arrivals.len(), REQUESTS);
    for (index, pair) in arrivals.windows(2).enumerate() {
        let interval = pair[1] - pair[0];
        assert!(
            interval >= RETRY,
            "request {} came {interval:?} after the one before",
            index + 1
        );
    }
}
