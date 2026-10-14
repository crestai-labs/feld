//! Whether a telemetry token names an open Session, as the Collector learns it from the daemon.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{OnceCell, Semaphore};

use super::receiver::retryable;

/// How long the Collector keeps the daemon's answer for one token.
const ANSWER_LIFETIME: Duration = Duration::from_secs(30);
/// How many answers the Collector keeps at once.
const CACHE_CAPACITY: usize = 1024;
/// How many requests may wait for an answer at once, over every token.
const MAX_WAITERS: usize = 64;
/// How many questions the Collector puts to the daemon at once.
const MAX_LOOKUPS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    Valid,
    Invalid,
    /// The daemon gave no answer, or too many requests wait for one.
    Unknown,
}

type Answer = Arc<OnceCell<Verdict>>;

/// Asks the daemon about each telemetry token through `POST /v1/self/telemetry/token` with
/// the internal token and keeps the answer for a while. The requests that carry one token
/// share one question.
pub(super) struct TokenCheck {
    client: reqwest::Client,
    check_url: String,
    internal_token: String,
    answers: Mutex<HashMap<String, (bool, Instant)>>,
    checking: Mutex<Checking>,
    lookups: Semaphore,
}

/// The tokens being asked about, each with the answer its requests wait for.
#[derive(Default)]
struct Checking {
    answers: HashMap<String, Answer>,
    waiters: usize,
}

impl TokenCheck {
    pub(super) fn new(client: reqwest::Client, daemon_api: &str, internal_token: &str) -> Self {
        Self {
            client,
            check_url: format!("{daemon_api}/v1/self/telemetry/token"),
            internal_token: internal_token.to_owned(),
            answers: Mutex::default(),
            checking: Mutex::default(),
            lookups: Semaphore::new(MAX_LOOKUPS),
        }
    }

    pub(super) async fn check(&self, token: &str) -> Verdict {
        if let Some(valid) = self.cached(token) {
            return verdict(valid);
        }
        let Some(waiter) = self.wait_for(token) else {
            return Verdict::Unknown;
        };
        let answer = waiter.answer.as_ref().expect("a waiter holds its answer");
        *answer.get_or_init(|| self.ask(token)).await
    }

    /// Joins the requests waiting for the answer about `token`, when there is room.
    fn wait_for<'a>(&'a self, token: &'a str) -> Option<Waiter<'a>> {
        let mut checking = self.checking.lock().unwrap();
        if checking.waiters >= MAX_WAITERS {
            return None;
        }
        checking.waiters += 1;
        let answer = Arc::clone(checking.answers.entry(token.to_owned()).or_default());
        Some(Waiter {
            check: self,
            token,
            answer: Some(answer),
        })
    }

    async fn ask(&self, token: &str) -> Verdict {
        let Ok(_lookup) = self.lookups.acquire().await else {
            return Verdict::Unknown;
        };
        let valid = match self
            .client
            .post(&self.check_url)
            .bearer_auth(&self.internal_token)
            .json(&serde_json::json!({ "token": token }))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => true,
            Ok(response) if !retryable(response.status()) => false,
            Ok(_) | Err(_) => return Verdict::Unknown,
        };
        self.remember(token, valid);
        verdict(valid)
    }

    fn cached(&self, token: &str) -> Option<bool> {
        let answers = self.answers.lock().unwrap();
        let (valid, expires) = answers.get(token)?;
        (Instant::now() < *expires).then_some(*valid)
    }

    fn remember(&self, token: &str, valid: bool) {
        let mut answers = self.answers.lock().unwrap();
        let now = Instant::now();
        if answers.len() >= CACHE_CAPACITY && !answers.contains_key(token) {
            answers.retain(|_, (_, expires)| now < *expires);
            if answers.len() >= CACHE_CAPACITY {
                return;
            }
        }
        answers.insert(token.to_owned(), (valid, now + ANSWER_LIFETIME));
    }
}

/// One request waiting for the answer about its token. The last one to leave, answered or
/// cancelled, frees the room the token took.
struct Waiter<'a> {
    check: &'a TokenCheck,
    token: &'a str,
    answer: Option<Answer>,
}

impl Drop for Waiter<'_> {
    fn drop(&mut self) {
        let mut checking = self.check.checking.lock().unwrap();
        checking.waiters -= 1;
        // Released under the lock, so the count below is exact.
        drop(self.answer.take());
        if checking
            .answers
            .get(self.token)
            .is_some_and(|answer| Arc::strong_count(answer) == 1)
        {
            checking.answers.remove(self.token);
        }
    }
}

fn verdict(valid: bool) -> Verdict {
    if valid {
        Verdict::Valid
    } else {
        Verdict::Invalid
    }
}

#[cfg(test)]
mod tests;
