use std::collections::BTreeMap;

use super::QUOTA;
use super::response::{Account, MetricStatus, QuotaPoint, QuotaSeries, ReportedBy, RuntimeQuota};
use super::runtime::Runtime;
use super::scan::{Readings, account_named};
use super::window::{Window, format_nanos_u64, format_seconds};
use feld_core::model::Session;

/// The account quota one Runtime's Sessions reported in the range.
pub(super) fn quota_of(
    runtime: Runtime,
    sessions: &[(&Session, Readings)],
    window: &Window,
) -> RuntimeQuota {
    let rules = match runtime {
        Runtime::Rules(rules) => rules,
        Runtime::NotApplicable(reason) => {
            return RuntimeQuota {
                status: MetricStatus::NotApplicable,
                source: reason.to_owned(),
                series: vec![],
            };
        }
    };
    let mut reports: BTreeMap<(Option<String>, String, i64), Vec<QuotaReport>> = BTreeMap::new();
    for (session, readings) in sessions {
        // A Session with an invalid value or undecodable Telemetry keeps its reports out
        // of the quota.
        let mut invalid = readings.decode_failed;
        let mut snapshots: BTreeMap<(String, u64), QuotaSnapshot> = BTreeMap::new();
        for (destination, point) in &readings.points {
            let Some(part) = destination.part.filter(|_| destination.metric == QUOTA) else {
                continue;
            };
            let used_percent = part == "used_percent";
            let Some(value) = point.numeric_value else {
                invalid = true;
                continue;
            };
            if !value.is_finite() || value < 0.0 || (used_percent && value > 100.0) {
                invalid = true;
                continue;
            }
            let Some(window_name) = point.attribute("window") else {
                invalid = true;
                continue;
            };
            if !matches!(window_name, "five_hour" | "seven_day") {
                continue;
            }
            let snapshot = snapshots
                .entry((window_name.to_owned(), point.timestamp_unix_nano))
                .or_default();
            if snapshot.account.is_none() {
                snapshot.account = account_named(point, "account.id", "account.name");
            }
            if used_percent {
                snapshot.used_percent = Some(value);
            } else {
                snapshot.resets_at = Some(value);
            }
        }
        if invalid {
            continue;
        }
        for ((window_name, timestamp), snapshot) in snapshots {
            let (Some(used_percent), Some(resets_at)) = (snapshot.used_percent, snapshot.resets_at)
            else {
                continue;
            };
            if !window.contains(timestamp) || resets_at > i64::MAX as f64 {
                continue;
            }
            let resets_at = resets_at as i64;
            if resets_at < 0
                || (resets_at as u128).saturating_mul(1_000_000_000) <= timestamp as u128
            {
                continue;
            }
            // The Runtime's own OTel names the Session's account before the gauge names
            // the account of its own report.
            let account = readings.account.clone().or(snapshot.account);
            reports
                .entry((
                    account.as_ref().map(|account| account.id.clone()),
                    window_name,
                    resets_at,
                ))
                .or_default()
                .push(QuotaReport {
                    timestamp,
                    used_percent,
                    account,
                    session: (*session).clone(),
                });
        }
    }

    let mut series = Vec::new();
    for ((_, window_name, resets_at), mut points) in reports {
        points.sort_by_key(|point| point.timestamp);
        let latest = points.last().expect("quota series is non-empty");
        let latest_point = QuotaPoint {
            at: format_nanos_u64(latest.timestamp),
            used_percent: latest.used_percent,
        };
        series.push(QuotaSeries {
            account: latest.account.clone(),
            window: window_name,
            resets_at: format_seconds(resets_at),
            end: format_seconds(resets_at),
            points: points
                .iter()
                .map(|point| QuotaPoint {
                    at: format_nanos_u64(point.timestamp),
                    used_percent: point.used_percent,
                })
                .collect(),
            latest: latest_point,
            reported_by: ReportedBy {
                session: latest.session.id.clone(),
                workspace: latest.session.workspace.clone(),
                actor: latest.session.actor.clone(),
            },
        });
    }

    let (status, source) = if series.is_empty() {
        (MetricStatus::Unavailable, "No rate limit report".to_owned())
    } else {
        (MetricStatus::Reported, rules.gauge_source.to_owned())
    };
    RuntimeQuota {
        status,
        source,
        series,
    }
}

#[derive(Default)]
struct QuotaSnapshot {
    used_percent: Option<f64>,
    resets_at: Option<f64>,
    account: Option<Account>,
}

struct QuotaReport {
    timestamp: u64,
    used_percent: f64,
    account: Option<Account>,
    session: Session,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::fixture::{
        T120, T130, claude_instruments, claude_maps, codex_instruments, codex_session,
        metrics_telemetry, point, quota_read, session, telemetry,
    };
    use crate::metrics::instruments::InstrumentMaps;
    use crate::metrics::runtime::{ROLLOUT_SOURCE, STATUS_SOURCE};
    use crate::store::Telemetry;
    use feld_core::telemetry::proto;

    #[test]
    fn groups_quota_by_window_and_reset_and_keeps_latest_reporter() {
        let first = session("agent-runtime", "Claude Code");
        let mut second = session("agent-runtime", "Claude Code");
        second.id = "s2".to_owned();
        second.actor = "reviewer".to_owned();
        let reset = 18_000.0;
        let first_telemetry = metrics_telemetry(
            "q1",
            vec![
                proto::Metric::gauge(
                    "feld.claude_code.rate_limit.used_percent",
                    vec![point(T120, 30.0).with("window", "five_hour")],
                ),
                proto::Metric::gauge(
                    "feld.claude_code.rate_limit.resets_at",
                    vec![point(T120, reset).with("window", "five_hour")],
                ),
            ],
        );
        let second_telemetry = metrics_telemetry(
            "q2",
            vec![
                proto::Metric::gauge(
                    "feld.claude_code.rate_limit.used_percent",
                    vec![point(T130, 40.0).with("window", "five_hour")],
                ),
                proto::Metric::gauge(
                    "feld.claude_code.rate_limit.resets_at",
                    vec![point(T130, reset).with("window", "five_hour")],
                ),
            ],
        );
        let response = quota_read(
            "claude-code",
            &[(&first, &[first_telemetry]), (&second, &[second_telemetry])],
            &claude_maps(),
        );
        assert_eq!(response.status, MetricStatus::Reported);
        let series = &response.series[0];
        assert_eq!(series.points.len(), 2);
        assert_eq!(series.latest.used_percent, 40.0);
        assert_eq!(series.reported_by.session, "s2");
        assert_eq!(series.end, "1970-01-01T05:00:00Z");
    }

    #[test]
    fn a_window_that_reset_in_the_range_returns_each_reset_as_its_own_series() {
        let first = session("agent-runtime", "Claude Code");
        let mut second = session("agent-runtime", "Claude Code");
        second.id = "s2".to_owned();
        const AFTER_RESET: u64 = 18_060_000_000_000;
        let before = metrics_telemetry(
            "q1",
            vec![
                proto::Metric::gauge(
                    "feld.claude_code.rate_limit.used_percent",
                    vec![point(T120, 30.0).with("window", "five_hour")],
                ),
                proto::Metric::gauge(
                    "feld.claude_code.rate_limit.resets_at",
                    vec![point(T120, 18_000.0).with("window", "five_hour")],
                ),
            ],
        );
        let after = metrics_telemetry(
            "q2",
            vec![
                proto::Metric::gauge(
                    "feld.claude_code.rate_limit.used_percent",
                    vec![point(AFTER_RESET, 5.0).with("window", "five_hour")],
                ),
                proto::Metric::gauge(
                    "feld.claude_code.rate_limit.resets_at",
                    vec![point(AFTER_RESET, 36_000.0).with("window", "five_hour")],
                ),
            ],
        );
        let response = quota_read(
            "claude-code",
            &[(&first, &[before]), (&second, &[after])],
            &claude_maps(),
        );
        let runtime = &response;
        assert_eq!(runtime.status, MetricStatus::Reported);
        let windows: Vec<(&str, &str, &str, f64)> = runtime
            .series
            .iter()
            .map(|series| {
                (
                    series.window.as_str(),
                    series.end.as_str(),
                    series.reported_by.session.as_str(),
                    series.latest.used_percent,
                )
            })
            .collect();
        assert_eq!(
            windows,
            vec![
                ("five_hour", "1970-01-01T05:00:00Z", "s1", 30.0),
                ("five_hour", "1970-01-01T10:00:00Z", "s2", 5.0),
            ]
        );
    }

    #[test]
    fn drops_quota_reports_whose_window_already_reset() {
        let claude = session("agent-runtime", "Claude Code");
        let quota = metrics_telemetry(
            "quota",
            vec![
                proto::Metric::gauge(
                    "feld.claude_code.rate_limit.used_percent",
                    vec![
                        point(T120, 20.0).with("window", "five_hour"),
                        point(T130, 30.0).with("window", "seven_day"),
                    ],
                ),
                proto::Metric::gauge(
                    "feld.claude_code.rate_limit.resets_at",
                    vec![
                        point(T120, 100.0).with("window", "five_hour"),
                        point(T130, 500.0).with("window", "seven_day"),
                    ],
                ),
            ],
        );
        let response = quota_read("claude-code", &[(&claude, &[quota])], &claude_maps());
        assert_eq!(response.status, MetricStatus::Reported);
        assert_eq!(response.series.len(), 1);
        assert_eq!(response.series[0].window, "seven_day");
        assert!(
            response.series[0]
                .points
                .iter()
                .all(|point| point.at < response.series[0].end)
        );
    }

    /// Sessions under different accounts report separate series, each carrying its
    /// account: the Runtime's own OTel names it before the Adapter's gauge does, and a
    /// Session whose Telemetry names none reports as before, with no account.
    #[test]
    fn reports_of_different_accounts_make_their_own_series() {
        let report = |id: &str, account: &[(&str, &str)], used: f64| {
            let with_account = |point: proto::NumberDataPoint| {
                account
                    .iter()
                    .fold(point.with("window", "five_hour"), |point, (key, value)| {
                        point.with(*key, *value)
                    })
            };
            metrics_telemetry(
                id,
                vec![
                    proto::Metric::gauge(
                        "feld.claude_code.rate_limit.used_percent",
                        vec![with_account(point(T120, used))],
                    ),
                    proto::Metric::gauge(
                        "feld.claude_code.rate_limit.resets_at",
                        vec![with_account(point(T120, 18_000.0))],
                    ),
                ],
            )
        };
        let session_of = |id: &str| {
            let mut session = session("agent-runtime", "Claude Code");
            session.id = id.to_owned();
            session
        };
        let (team, bot, oauth, unknown) = (
            session_of("s1"),
            session_of("s2"),
            session_of("s3"),
            session_of("s4"),
        );
        let team_reports = [report(
            "q1",
            &[("account.id", "team"), ("account.name", "team@example.com")],
            30.0,
        )];
        let bot_reports = [report("q2", &[("account.id", "bot")], 10.0)];
        let oauth_reports = [
            report("q3", &[("account.id", "team")], 50.0),
            metrics_telemetry(
                "otel",
                vec![proto::Metric::sum(
                    "claude_code.token.usage",
                    proto::AggregationTemporality::Delta,
                    vec![
                        point(T120, 1.0)
                            .with("type", "input")
                            .with("user.account_uuid", "uuid-3")
                            .with("user.email", "me@example.com"),
                    ],
                )],
            ),
        ];
        let unknown_reports = [report("q4", &[], 70.0)];
        let response = quota_read(
            "claude-code",
            &[
                (&team, &team_reports),
                (&bot, &bot_reports),
                (&oauth, &oauth_reports),
                (&unknown, &unknown_reports),
            ],
            &claude_maps(),
        );
        assert_eq!(response.status, MetricStatus::Reported);
        let series: Vec<(Option<String>, &str, f64)> = response
            .series
            .iter()
            .map(|series| {
                (
                    series
                        .account
                        .as_ref()
                        .map(|account| format!("{} {}", account.id, account.name)),
                    series.reported_by.session.as_str(),
                    series.latest.used_percent,
                )
            })
            .collect();
        let account = |text: &str| Some(text.to_owned());
        assert_eq!(
            series,
            vec![
                (None, "s4", 70.0),
                (account("bot bot"), "s2", 10.0),
                (account("team team@example.com"), "s1", 30.0),
                (account("uuid-3 me@example.com"), "s3", 50.0),
            ]
        );
        let unknown = serde_json::to_value(&response.series[0]).unwrap();
        assert!(unknown.get("account").is_none());
        let known = serde_json::to_value(&response.series[1]).unwrap();
        assert_eq!(
            known["account"],
            serde_json::json!({"id": "bot", "name": "bot"})
        );
    }

    /// The gauge names the account of its own report: a Session that signs in to another
    /// account midway reports each account's quota to that account, and reports under one
    /// ID make one series whatever name each carried, named by the latest.
    #[test]
    fn each_gauge_report_goes_to_the_account_it_names() {
        let report = |id: &str, time: u64, used: f64, account: &[(&str, &str)]| {
            let named = |point: proto::NumberDataPoint| {
                account
                    .iter()
                    .fold(point.with("window", "five_hour"), |point, (key, value)| {
                        point.with(*key, *value)
                    })
            };
            metrics_telemetry(
                id,
                vec![
                    proto::Metric::gauge(
                        "feld.claude_code.rate_limit.used_percent",
                        vec![named(point(time, used))],
                    ),
                    proto::Metric::gauge(
                        "feld.claude_code.rate_limit.resets_at",
                        vec![named(point(time, 18_000.0))],
                    ),
                ],
            )
        };
        let switching = session("agent-runtime", "Claude Code");
        let mut other = session("agent-runtime", "Claude Code");
        other.id = "s2".to_owned();
        let switching_reports = [
            report("q1", T120, 30.0, &[("account.id", "team")]),
            report("q2", T130, 5.0, &[("account.id", "bot")]),
        ];
        let other_reports = [report(
            "q3",
            T130,
            40.0,
            &[("account.id", "team"), ("account.name", "team@example.com")],
        )];
        let response = quota_read(
            "claude-code",
            &[(&switching, &switching_reports), (&other, &other_reports)],
            &claude_maps(),
        );
        let series: Vec<(String, usize, f64)> = response
            .series
            .iter()
            .map(|series| {
                let account = series.account.as_ref().unwrap();
                (
                    format!("{} {}", account.id, account.name),
                    series.points.len(),
                    series.latest.used_percent,
                )
            })
            .collect();
        assert_eq!(
            series,
            vec![
                ("bot bot".to_owned(), 1, 5.0),
                ("team team@example.com".to_owned(), 2, 40.0),
            ]
        );
    }

    /// A Session with an invalid quota value or undecodable Telemetry keeps all its reports
    /// out of the quota, and the other Sessions' reports still make the series.
    #[test]
    fn leaves_out_only_the_reports_of_a_session_with_an_invalid_value() {
        let report = |id: &str, time: u64, used: f64| {
            metrics_telemetry(
                id,
                vec![
                    proto::Metric::gauge(
                        "feld.claude_code.rate_limit.used_percent",
                        vec![point(time, used).with("window", "five_hour")],
                    ),
                    proto::Metric::gauge(
                        "feld.claude_code.rate_limit.resets_at",
                        vec![point(time, 18_000.0).with("window", "five_hour")],
                    ),
                ],
            )
        };
        let session_of = |id: &str| {
            let mut session = session("agent-runtime", "Claude Code");
            session.id = id.to_owned();
            session
        };
        let (valid, over, undecodable) = (session_of("s1"), session_of("s2"), session_of("s3"));
        let valid_reports = [report("q1", T120, 30.0)];
        let over_reports = [report("q2", T120, 50.0), report("q3", T130, 150.0)];
        let undecodable_reports = [
            report("q4", T130, 60.0),
            telemetry("broken", "metrics", vec![0xff]),
        ];
        let response = quota_read(
            "claude-code",
            &[
                (&valid, &valid_reports),
                (&over, &over_reports),
                (&undecodable, &undecodable_reports),
            ],
            &claude_maps(),
        );
        assert_eq!(response.status, MetricStatus::Reported);
        assert_eq!(response.series.len(), 1);
        let series = &response.series[0];
        assert_eq!(series.points.len(), 1);
        assert_eq!(series.latest.used_percent, 30.0);
        assert_eq!(series.reported_by.session, "s1");

        let response = quota_read(
            "claude-code",
            &[(&over, &over_reports), (&undecodable, &undecodable_reports)],
            &claude_maps(),
        );
        assert_eq!(response.status, MetricStatus::Unavailable);
        assert_eq!(response.source, "No rate limit report");
        assert!(response.series.is_empty());
    }

    /// The quota read answers per Runtime: each one's reports make its own series, with the
    /// source of the part that sent them.
    #[test]
    fn each_runtime_answers_the_quota_read_with_its_own_reports() {
        let claude = session("agent-runtime", "Claude Code");
        let codex = codex_session();
        let report = |name: &str, window: &str, used: f64, resets_at: f64| {
            metrics_telemetry(
                window,
                vec![
                    proto::Metric::gauge(
                        format!("feld.{name}.rate_limit.used_percent"),
                        vec![point(T120, used).with("window", window)],
                    ),
                    proto::Metric::gauge(
                        format!("feld.{name}.rate_limit.resets_at"),
                        vec![point(T120, resets_at).with("window", window)],
                    ),
                ],
            )
        };
        let claude_telemetries = [report("claude_code", "five_hour", 17.0, 4_000.0)];
        let codex_telemetries = [report("codex", "seven_day", 1.0, 900_000.0)];
        let maps = InstrumentMaps {
            by_runtime: [
                ("claude-code".to_owned(), claude_instruments()),
                ("codex".to_owned(), codex_instruments()),
            ]
            .into_iter()
            .collect(),
        };
        let sessions: [(&Session, &[Telemetry]); 2] =
            [(&claude, &claude_telemetries), (&codex, &codex_telemetries)];
        let of = |name: &str| quota_read(name, &sessions, &maps);
        let claude_quota = of("claude-code");
        assert_eq!(claude_quota.status, MetricStatus::Reported);
        assert_eq!(claude_quota.source, STATUS_SOURCE);
        assert_eq!(claude_quota.series.len(), 1);
        assert_eq!(claude_quota.series[0].window, "five_hour");
        let codex_quota = of("codex");
        assert_eq!(codex_quota.status, MetricStatus::Reported);
        assert_eq!(codex_quota.source, ROLLOUT_SOURCE);
        assert_eq!(codex_quota.series.len(), 1);
        assert_eq!(codex_quota.series[0].window, "seven_day");
        assert_eq!(codex_quota.series[0].latest.used_percent, 1.0);
        assert_eq!(codex_quota.series[0].resets_at, format_seconds(900_000));
    }
}
