//! What the status line tells that Claude Code's OTel does not: the context window and the
//! account rate limits.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use feld_core::plugin::sdk::gauges::{Account, GaugePoint, finite, percentage};

/// The instrumentation scope these gauges carry into the Collector.
pub const SCOPE: &str = "feld.status-line";

/// The account Claude Code signed in with, from the global config file of its
/// configuration root: `CLAUDE_CONFIG_DIR/.claude.json`, otherwise `$HOME/.claude.json`.
pub fn account() -> Option<Account> {
    let file = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(root) if !root.is_empty() => PathBuf::from(root).join(".claude.json"),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".claude.json"),
    };
    let config = serde_json::from_slice::<Value>(&fs::read(file).ok()?).ok()?;
    account_of(&config)
}

fn account_of(config: &Value) -> Option<Account> {
    let account = config.get("oauthAccount")?;
    Account::of(account.get("accountUuid"), account.get("emailAddress"))
}

/// The gauges one status line JSON is worth, the rate limits under `account`. Values
/// outside their range are left out rather than reported wrongly.
pub fn gauges(status: &Value, account: Option<&Account>) -> Vec<GaugePoint> {
    let mut points = Vec::new();
    if let Some(value) = finite(status.pointer("/context_window/context_window_size")) {
        points.push(GaugePoint::new(
            "feld.claude_code.context_window.size",
            value,
        ));
    }
    if let Some(value) = percentage(status.pointer("/context_window/used_percentage")) {
        points.push(GaugePoint::new(
            "feld.claude_code.context_window.used_percent",
            value,
        ));
    }
    for window in ["five_hour", "seven_day"] {
        let Some(report) = status.pointer(&format!("/rate_limits/{window}")) else {
            continue;
        };
        if let Some(value) = percentage(report.get("used_percentage")) {
            points.push(
                GaugePoint::new("feld.claude_code.rate_limit.used_percent", value)
                    .with("window", window)
                    .with_account(account),
            );
        }
        if let Some(value) = finite(report.get("resets_at")) {
            points.push(
                GaugePoint::new("feld.claude_code.rate_limit.resets_at", value)
                    .with("window", window)
                    .with_account(account),
            );
        }
    }
    points
}

/// The line the status line shows: the Actor and what of its budgets the JSON tells.
pub fn line(actor: &str, status: &Value) -> String {
    let percent = |pointer: &str, label: &str| {
        status
            .pointer(pointer)
            .and_then(Value::as_f64)
            .map(|value| format!(" · {label}{value:.0}%"))
            .unwrap_or_default()
    };
    format!(
        "{actor}{}{}{}",
        percent("/context_window/used_percentage", "context "),
        percent("/rate_limits/five_hour/used_percentage", "5h "),
        percent("/rate_limits/seven_day/used_percentage", "7d "),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use feld_core::plugin::sdk::gauges::Account;

    use super::{account_of, gauges, line};

    fn names(status: &serde_json::Value) -> Vec<String> {
        gauges(status, None)
            .into_iter()
            .map(|point| point.name)
            .collect()
    }

    #[test]
    fn rate_limit_gauges_carry_the_account_of_the_config_root() {
        let account = account_of(&json!({
            "oauthAccount": {"accountUuid": "uuid-1", "emailAddress": "team@example.com"}
        }));
        assert_eq!(
            account,
            Some(Account {
                id: "uuid-1".to_owned(),
                name: Some("team@example.com".to_owned()),
            })
        );
        let status = json!({
            "context_window": {"context_window_size": 200000},
            "rate_limits": {"five_hour": {"used_percentage": 20, "resets_at": 18000}}
        });
        for point in gauges(&status, account.as_ref()) {
            let has_account = point
                .attributes
                .contains(&("account.id".to_owned(), "uuid-1".to_owned()));
            assert_eq!(
                has_account,
                point.name.contains("rate_limit"),
                "{}",
                point.name
            );
        }
        assert_eq!(account_of(&json!({"oauthAccount": {}})), None);
        assert_eq!(account_of(&json!({})), None);
    }

    #[test]
    fn status_gauges_omit_out_of_range_percentages() {
        let names = names(&json!({
            "context_window": {"context_window_size": 200000, "used_percentage": 150},
            "rate_limits": {"five_hour": {"used_percentage": 250, "resets_at": 18000}}
        }));
        assert!(names.contains(&"feld.claude_code.context_window.size".to_owned()));
        assert!(names.contains(&"feld.claude_code.rate_limit.resets_at".to_owned()));
        assert!(!names.contains(&"feld.claude_code.context_window.used_percent".to_owned()));
        assert!(!names.contains(&"feld.claude_code.rate_limit.used_percent".to_owned()));
    }

    #[test]
    fn the_status_line_shows_what_the_json_tells() {
        assert_eq!(line("agent", &json!(null)), "agent");
        assert_eq!(
            line(
                "agent",
                &json!({
                    "context_window": {"used_percentage": 42.0},
                    "rate_limits": {"seven_day": {"used_percentage": 3.4}}
                })
            ),
            "agent · context 42% · 7d 3%"
        );
    }
}
