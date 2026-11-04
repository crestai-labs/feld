use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

/// How many connections the Collector holds open at once. Further connections wait in the
/// listen backlog until one closes.
pub(super) const MAX_CONNECTIONS: usize = 64;
/// How long a connection may take to send the headers of its next request, idle time
/// between requests included.
pub(super) const HEADER_READ_DEADLINE: Duration = Duration::from_secs(10);
const ACCEPT_RETRY_DELAY: Duration = Duration::from_secs(1);

/// The bounds on the connections the Collector serves.
#[derive(Clone, Copy)]
pub(super) struct ConnectionLimits {
    pub(super) max_connections: usize,
    pub(super) header_read_deadline: Duration,
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            header_read_deadline: HEADER_READ_DEADLINE,
        }
    }
}

/// Serves HTTP/1.1 on `listener` with `app`, holding at most `limits.max_connections`
/// connections and closing each one that misses `limits.header_read_deadline`.
pub(super) async fn serve_connections(
    listener: TcpListener,
    app: Router,
    limits: ConnectionLimits,
) {
    let permits = Arc::new(Semaphore::new(limits.max_connections));
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(limits.header_read_deadline);
    loop {
        let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
            return;
        };
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(error) if is_connection_error(&error) => continue,
            Err(_) => {
                tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                continue;
            }
        };
        let connection =
            builder.serve_connection(TokioIo::new(stream), TowerToHyperService::new(app.clone()));
        tokio::spawn(async move {
            let _ = connection.await;
            drop(permit);
        });
    }
}

fn is_connection_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::ConnectionRefused | ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset
    )
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use axum::routing::get;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    use super::*;

    async fn start(limits: ConnectionLimits) -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route("/", get(|| async { "ok" }));
        tokio::spawn(serve_connections(listener, app, limits));
        address
    }

    /// Reads until the server closes the connection, and returns what it sent.
    async fn read_to_close(stream: &mut TcpStream, within: Duration) -> Vec<u8> {
        let mut received = Vec::new();
        tokio::time::timeout(within, stream.read_to_end(&mut received))
            .await
            .expect("the server closes the connection")
            .unwrap();
        received
    }

    #[tokio::test]
    async fn closes_a_connection_that_does_not_finish_its_headers() {
        let address = start(ConnectionLimits {
            max_connections: 4,
            header_read_deadline: Duration::from_millis(200),
        })
        .await;

        let mut silent = TcpStream::connect(address).await.unwrap();
        let mut partial = TcpStream::connect(address).await.unwrap();
        partial
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n")
            .await
            .unwrap();

        read_to_close(&mut silent, Duration::from_secs(2)).await;
        read_to_close(&mut partial, Duration::from_secs(2)).await;
    }

    #[tokio::test]
    async fn serves_a_waiting_connection_once_a_held_one_closes() {
        let address = start(ConnectionLimits {
            max_connections: 1,
            header_read_deadline: Duration::from_millis(300),
        })
        .await;

        let mut held = TcpStream::connect(address).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut waiting = TcpStream::connect(address).await.unwrap();
        waiting
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();

        let mut first = [0u8; 1];
        let early =
            tokio::time::timeout(Duration::from_millis(150), waiting.read(&mut first)).await;
        assert!(
            early.is_err(),
            "the second connection waits while the first holds the only slot"
        );

        read_to_close(&mut held, Duration::from_secs(2)).await;
        let response = read_to_close(&mut waiting, Duration::from_secs(2)).await;
        assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200 OK"));
    }
}
