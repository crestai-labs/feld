//! What the rollout file tells that Codex's OTel does not: the context window and the
//! account rate limits.
//!
//! Codex writes a `token_count` event into the rollout file after every turn, and the last
//! one holds the current state. The hook reads the file from its end, turns that event into
//! gauges, and sends them to the same Collector the Runtime exports to.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;

use feld_core::home::FeldHome;
use feld_core::plugin::sdk::gauges::{Account, GaugePoint, finite, percentage};

use crate::adapter::CONFIG_ROOT;

/// The instrumentation scope these gauges carry into the Collector.
pub const SCOPE: &str = "feld.codex-rollout";

/// What Codex holds in a context window before a conversation starts, and leaves out of
/// the percentage it shows.
const BASELINE_TOKENS: f64 = 12_000.0;

/// How much of the rollout file's end is read at a time while looking for the last
/// `token_count`. A turn's records are far smaller than this, so the first read finds it.
const TAIL_BYTES: u64 = 128 * 1024;

/// How far back the search goes before giving the file up as having no `token_count`.
const MAX_TAIL_BYTES: u64 = 8 * 1024 * 1024;

/// The two window lengths Codex reports rate limits over, in minutes.
const WINDOWS: [(u64, &str); 2] = [(300, "five_hour"), (10_080, "seven_day")];

/// Reports the gauges of the rollout file a hook event names, signed with the Session
/// token that hook resolved. A file with no `token_count` yet and a Collector that
/// cannot be reached both leave the report undone; the next hook that fires sends the
/// state as it is then.
pub fn report(home: &FeldHome, token: &str, event: &Value) -> Result<()> {
    let path = event
        .get("transcript_path")
        .and_then(Value::as_str)
        .context("hook input has no transcript_path")?;
    let Some(payload) = last_token_count(Path::new(path))? else {
        return Ok(());
    };
    let points = gauges(&payload, account().as_ref());
    feld_core::plugin::sdk::gauges::send_gauges(home, token, SCOPE, &points)
}

/// The ChatGPT account Codex signed in with, from `auth.json` of its configuration root.
/// An API key login names none.
pub fn account() -> Option<Account> {
    let file = CONFIG_ROOT.path()?.join("auth.json");
    let auth = serde_json::from_slice::<Value>(&std::fs::read(file).ok()?).ok()?;
    account_of(&auth)
}

fn account_of(auth: &Value) -> Option<Account> {
    let tokens = auth.get("tokens")?;
    let claims = tokens
        .get("id_token")
        .and_then(Value::as_str)
        .and_then(|token| token.split('.').nth(1))
        .and_then(|payload| URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok())
        .and_then(|payload| serde_json::from_slice::<Value>(&payload).ok());
    let claim = |pointer: &str| claims.as_ref().and_then(|claims| claims.pointer(pointer));
    let id = tokens
        .get("account_id")
        .filter(|id| id.as_str().is_some_and(|id| !id.is_empty()))
        .or_else(|| claim("/https:~1~1api.openai.com~1auth/chatgpt_account_id"));
    Account::of(id, claim("/email"))
}

/// The payload of the last `token_count` event of a rollout file, read from the end so
/// that the length of the Session does not decide the cost of a hook.
pub fn last_token_count(path: &Path) -> Result<Option<Value>> {
    let mut file =
        File::open(path).with_context(|| format!("open rollout file {}", path.display()))?;
    let length = file
        .metadata()
        .with_context(|| format!("read rollout file {}", path.display()))?
        .len();
    let mut tail = TAIL_BYTES;
    loop {
        let from = length.saturating_sub(tail);
        file.seek(SeekFrom::Start(from))
            .with_context(|| format!("read rollout file {}", path.display()))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .with_context(|| format!("read rollout file {}", path.display()))?;
        let text = String::from_utf8_lossy(&bytes);
        // A read that began inside the file may have begun inside a record.
        let mut lines: Vec<&str> = text.lines().collect();
        if from > 0 && !lines.is_empty() {
            lines.remove(0);
        }
        if let Some(payload) = lines.iter().rev().find_map(|line| token_count(line)) {
            return Ok(Some(payload));
        }
        if from == 0 || tail >= MAX_TAIL_BYTES {
            return Ok(None);
        }
        tail = (tail * 4).min(MAX_TAIL_BYTES);
    }
}

/// The payload of one record, when the record is a `token_count` event.
fn token_count(line: &str) -> Option<Value> {
    let record: Value = serde_json::from_str(line).ok()?;
    if record.get("type").and_then(Value::as_str) != Some("event_msg") {
        return None;
    }
    let payload = record.get("payload")?;
    (payload.get("type").and_then(Value::as_str) == Some("token_count")).then(|| payload.clone())
}

/// The gauges one `token_count` payload is worth. A value Codex left null, and a window it
/// does not name, make no data point rather than a wrong one. The rate limits are
/// `account`'s.
pub fn gauges(payload: &Value, account: Option<&Account>) -> Vec<GaugePoint> {
    let mut points = Vec::new();
    let size = finite(payload.pointer("/info/model_context_window")).filter(|size| *size > 0.0);
    if let Some(size) = size {
        points.push(GaugePoint::new("feld.codex.context_window.size", size));
    }
    if let Some(used) = used_percent(payload, size) {
        points.push(GaugePoint::new(
            "feld.codex.context_window.used_percent",
            used,
        ));
    }
    for limit in ["primary", "secondary"] {
        let Some(report) = payload
            .pointer(&format!("/rate_limits/{limit}"))
            .filter(|report| report.is_object())
        else {
            continue;
        };
        let Some(window) = window_of(report) else {
            continue;
        };
        if let Some(value) = percentage(report.get("used_percent")) {
            points.push(
                GaugePoint::new("feld.codex.rate_limit.used_percent", value)
                    .with("window", window)
                    .with_account(account),
            );
        }
        if let Some(value) = finite(report.get("resets_at")) {
            points.push(
                GaugePoint::new("feld.codex.rate_limit.resets_at", value)
                    .with("window", window)
                    .with_account(account),
            );
        }
    }
    points
}

/// How much of the window the conversation holds, as Codex itself shows it: both the usage
/// and the window are counted from the baseline Codex starts every conversation with.
fn used_percent(payload: &Value, size: Option<f64>) -> Option<f64> {
    let size = size?;
    let used = finite(payload.pointer("/info/last_token_usage/total_tokens"))?;
    let window = size - BASELINE_TOKENS;
    if window <= 0.0 {
        return None;
    }
    Some(((used - BASELINE_TOKENS) / window * 100.0).clamp(0.0, 100.0))
}

/// The name of the window one rate limit report covers, from the length Codex reports it
/// over.
fn window_of(report: &Value) -> Option<&'static str> {
    let minutes = report.get("window_minutes").and_then(Value::as_u64)?;
    WINDOWS
        .into_iter()
        .find(|(length, _)| *length == minutes)
        .map(|(_, name)| name)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use serde_json::json;

    use super::*;

    /// The gauges of a payload whose account is not known.
    fn gauges(payload: &Value) -> Vec<GaugePoint> {
        super::gauges(payload, None)
    }

    #[test]
    fn rate_limit_gauges_carry_the_account_auth_json_names() {
        let claims = URL_SAFE_NO_PAD.encode(
            json!({
                "email": "team@example.com",
                "https://api.openai.com/auth": {"chatgpt_account_id": "from-claim"}
            })
            .to_string(),
        );
        let id_token = format!("header.{claims}.signature");
        let account = account_of(&json!({
            "tokens": {"id_token": id_token, "account_id": "account-1"}
        }));
        assert_eq!(
            account,
            Some(Account {
                id: "account-1".to_owned(),
                name: Some("team@example.com".to_owned()),
            })
        );
        let from_claim = account_of(&json!({"tokens": {"id_token": id_token}}));
        assert_eq!(
            from_claim.map(|account| account.id).as_deref(),
            Some("from-claim")
        );
        assert_eq!(
            account_of(&json!({"auth_mode": "apikey", "tokens": null})),
            None
        );

        let points = super::gauges(
            &event(
                json!(null),
                json!({"primary": {"used_percent": 3.0, "window_minutes": 300, "resets_at": 100}}),
            ),
            account.as_ref(),
        );
        assert_eq!(points.len(), 2);
        for point in points {
            assert!(
                point
                    .attributes
                    .contains(&("account.id".to_owned(), "account-1".to_owned()))
            );
            assert!(
                point
                    .attributes
                    .contains(&("account.name".to_owned(), "team@example.com".to_owned()))
            );
        }
    }

    fn event(info: Value, rate_limits: Value) -> Value {
        json!({"type": "token_count", "info": info, "rate_limits": rate_limits})
    }

    fn named(points: &[GaugePoint], name: &str) -> Vec<(f64, Option<String>)> {
        points
            .iter()
            .filter(|point| point.name == name)
            .map(|point| {
                let window = point
                    .attributes
                    .iter()
                    .find(|(key, _)| key == "window")
                    .map(|(_, value)| value.clone());
                (point.value, window)
            })
            .collect()
    }

    /// The window Codex shows is what is left of it above the baseline every conversation
    /// starts with, on both sides of the ratio.
    #[test]
    fn used_percent_counts_the_window_from_the_baseline() {
        let percent = |used: f64, window: f64| {
            used_percent(
                &event(
                    json!({"model_context_window": window, "last_token_usage": {"total_tokens": used}}),
                    Value::Null,
                ),
                Some(window),
            )
        };
        // Half of the 200,000 tokens above the baseline are held.
        assert_eq!(percent(112_000.0, 212_000.0), Some(50.0));
        // A turn below the baseline reads as an empty window, a full one as a full window.
        assert_eq!(percent(3_000.0, 212_000.0), Some(0.0));
        assert_eq!(percent(300_000.0, 212_000.0), Some(100.0));
        // A window no larger than the baseline leaves nothing to take a ratio of.
        assert_eq!(percent(12_000.0, 12_000.0), None);
    }

    /// Codex reports one rate limit window on some plans and two on others, and names each
    /// one by the length it covers.
    #[test]
    fn rate_limit_gauges_follow_the_window_length() {
        let both = gauges(&event(
            json!({"model_context_window": 262_144, "last_token_usage": {"total_tokens": 62_000}}),
            json!({
                "primary": {"used_percent": 12.5, "window_minutes": 300, "resets_at": 1_788_272_140_u64},
                "secondary": {"used_percent": 3.0, "window_minutes": 10080, "resets_at": 1_788_872_140_u64},
            }),
        ));
        assert_eq!(
            named(&both, "feld.codex.rate_limit.used_percent"),
            vec![
                (12.5, Some("five_hour".to_owned())),
                (3.0, Some("seven_day".to_owned())),
            ]
        );
        assert_eq!(
            named(&both, "feld.codex.rate_limit.resets_at"),
            vec![
                (1_788_272_140.0, Some("five_hour".to_owned())),
                (1_788_872_140.0, Some("seven_day".to_owned())),
            ]
        );
        assert_eq!(
            named(&both, "feld.codex.context_window.size"),
            vec![(262_144.0, None)]
        );

        // The plan of the real capture reports one window, as its seven-day limit.
        let primary_only = gauges(&event(
            json!({"model_context_window": 258_400, "last_token_usage": {"total_tokens": 58_089}}),
            json!({
                "primary": {"used_percent": 1.0, "window_minutes": 10080, "resets_at": 1_788_272_140_u64},
                "secondary": Value::Null,
            }),
        ));
        assert_eq!(
            named(&primary_only, "feld.codex.rate_limit.used_percent"),
            vec![(1.0, Some("seven_day".to_owned()))]
        );
    }

    /// Nothing Codex left out becomes a data point: no rate limits at all, a window length
    /// Feld has no name for, and a context window Codex has not learnt yet.
    #[test]
    fn a_value_codex_left_out_makes_no_data_point() {
        let without_limits = gauges(&event(
            json!({"model_context_window": 262_144, "last_token_usage": {"total_tokens": 62_000}}),
            Value::Null,
        ));
        assert_eq!(
            without_limits
                .iter()
                .map(|point| point.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "feld.codex.context_window.size",
                "feld.codex.context_window.used_percent",
            ]
        );

        let unknown_window = gauges(&event(
            json!({"model_context_window": Value::Null, "last_token_usage": {"total_tokens": 62_000}}),
            json!({"primary": {"used_percent": 4.0, "window_minutes": 60, "resets_at": 1_788_272_140_u64}}),
        ));
        assert!(unknown_window.is_empty(), "{unknown_window:?}");

        let out_of_range = gauges(&event(
            json!({"model_context_window": 262_144, "last_token_usage": {"total_tokens": 62_000}}),
            json!({"primary": {"used_percent": 140.0, "window_minutes": 300, "resets_at": Value::Null}}),
        ));
        assert!(
            named(&out_of_range, "feld.codex.rate_limit.used_percent").is_empty(),
            "{out_of_range:?}"
        );
        assert!(
            named(&out_of_range, "feld.codex.rate_limit.resets_at").is_empty(),
            "{out_of_range:?}"
        );
    }

    /// The state is the last `token_count` of the file, found from its end however many
    /// records came before it.
    #[test]
    fn the_last_token_count_is_read_from_the_end_of_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(file, "{}", json!({"type": "session_meta", "payload": {}})).unwrap();
        for used in [20_000, 40_000, 62_000] {
            // Records of every other kind sit between the ones the gauges come from.
            for _ in 0..200 {
                writeln!(
                    file,
                    "{}",
                    json!({"type": "response_item", "payload": {"type": "message", "content": "x".repeat(400)}})
                )
                .unwrap();
            }
            writeln!(
                file,
                "{}",
                json!({"type": "event_msg", "payload": event(
                    json!({"model_context_window": 212_000, "last_token_usage": {"total_tokens": used}}),
                    Value::Null,
                )})
            )
            .unwrap();
        }
        // A record Codex was still writing when the hook read the file.
        write!(file, "{{\"type\": \"event_msg\", \"pay").unwrap();
        file.flush().unwrap();

        let payload = last_token_count(&path).unwrap().unwrap();
        assert_eq!(payload["info"]["last_token_usage"]["total_tokens"], 62_000);
        assert_eq!(
            named(&gauges(&payload), "feld.codex.context_window.used_percent"),
            vec![(25.0, None)]
        );
    }

    /// The gauges a rollout file codex-cli 0.150.0 wrote is worth: the state of its last
    /// turn, and the one rate limit window that plan reports.
    #[test]
    fn a_real_rollout_file_reports_the_state_of_its_last_turn() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            include_bytes!("../../feld-cli/tests/fixtures/codex-0.150.0-exec.jsonl"),
        )
        .unwrap();
        let points = gauges(&last_token_count(&path).unwrap().unwrap());
        assert_eq!(
            named(&points, "feld.codex.context_window.size"),
            vec![(258_400.0, None)]
        );
        // The 13,680 tokens of the last turn, above the baseline of the 258,400 window.
        let used = named(&points, "feld.codex.context_window.used_percent");
        assert!(
            (used[0].0 - 0.681_818_181_818_181_8).abs() < 1e-9,
            "{used:?}"
        );
        assert_eq!(
            named(&points, "feld.codex.rate_limit.used_percent"),
            vec![(60.0, Some("seven_day".to_owned()))]
        );
        assert_eq!(
            named(&points, "feld.codex.rate_limit.resets_at"),
            vec![(1_789_950_432.0, Some("seven_day".to_owned()))]
        );
    }

    /// A rollout file of a Session that has had no turn yet has no state to report.
    #[test]
    fn a_rollout_file_without_a_token_count_reports_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            format!("{}\n", json!({"type": "session_meta", "payload": {}})),
        )
        .unwrap();
        assert!(last_token_count(&path).unwrap().is_none());
    }
}
