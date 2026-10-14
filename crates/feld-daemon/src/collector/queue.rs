//! The in-memory queue of exports waiting for the daemon, split by Session token so that one
//! token cannot take the room of another.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use serde::Serialize;
use tokio::sync::Notify;

/// How many exports one Session token may hold in the queue.
pub(super) const TOKEN_CAPACITY: usize = 16;

/// One export as the Collector forwards it to the daemon.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Envelope {
    pub(super) request_id: String,
    pub(super) token: String,
    pub(super) signal: &'static str,
    pub(super) content_type: &'static str,
    pub(super) payload: String,
}

/// The queue has no room left for the token of a pushed envelope.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Full;

/// A FIFO per Session token, popped round-robin across the tokens. One consumer pops.
#[derive(Default)]
pub(super) struct Queue {
    inner: Mutex<Inner>,
    ready: Notify,
}

#[derive(Default)]
struct Inner {
    pending: HashMap<String, VecDeque<Envelope>>,
    /// The tokens with pending envelopes, in the order they take their next turn.
    turns: VecDeque<String>,
}

impl Queue {
    pub(super) fn push(&self, envelope: Envelope) -> Result<(), Full> {
        let mut inner = self.inner.lock().unwrap();
        let Inner { pending, turns } = &mut *inner;
        let envelopes = match pending.get_mut(&envelope.token) {
            Some(envelopes) if envelopes.len() >= TOKEN_CAPACITY => return Err(Full),
            Some(envelopes) => envelopes,
            None => {
                turns.push_back(envelope.token.clone());
                pending.entry(envelope.token.clone()).or_default()
            }
        };
        envelopes.push_back(envelope);
        drop(inner);
        self.ready.notify_one();
        Ok(())
    }

    /// Waits for the next envelope. Cancel-safe: nothing is taken until it returns.
    pub(super) async fn pop(&self) -> Envelope {
        loop {
            if let Some(envelope) = self.try_pop() {
                return envelope;
            }
            self.ready.notified().await;
        }
    }

    fn try_pop(&self) -> Option<Envelope> {
        let mut inner = self.inner.lock().unwrap();
        let token = inner.turns.pop_front()?;
        let envelopes = inner.pending.get_mut(&token)?;
        let envelope = envelopes.pop_front()?;
        if envelopes.is_empty() {
            inner.pending.remove(&token);
        } else {
            inner.turns.push_back(token);
        }
        Some(envelope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(token: &str, request_id: &str) -> Envelope {
        Envelope {
            request_id: request_id.to_owned(),
            token: token.to_owned(),
            signal: "metrics",
            content_type: "application/x-protobuf",
            payload: String::new(),
        }
    }

    #[test]
    fn a_full_token_leaves_room_for_another() {
        let queue = Queue::default();
        for index in 0..TOKEN_CAPACITY {
            queue.push(envelope("a", &index.to_string())).unwrap();
        }
        assert_eq!(queue.push(envelope("a", "over")), Err(Full));
        assert_eq!(queue.push(envelope("b", "b0")), Ok(()));
    }

    #[test]
    fn pops_round_robin_across_tokens() {
        let queue = Queue::default();
        for request_id in ["a0", "a1", "a2"] {
            queue.push(envelope("a", request_id)).unwrap();
        }
        queue.push(envelope("b", "b0")).unwrap();
        let order: Vec<_> = std::iter::from_fn(|| queue.try_pop())
            .map(|envelope| envelope.request_id)
            .collect();
        assert_eq!(order, ["a0", "b0", "a1", "a2"]);
        assert!(queue.inner.lock().unwrap().pending.is_empty());

        for index in 0..TOKEN_CAPACITY {
            queue.push(envelope("a", &index.to_string())).unwrap();
        }
    }
}
