use std::collections::BTreeMap;

use anyhow::{Result, bail};

use super::instruments::InstrumentMaps;
use super::project::{NO_USAGE, bin_value, metric_bin, project_metric};
use super::quota::quota_of;
use super::response::{
    Account, AccountSeries, MetricStatus, RuntimeMetricProjection, RuntimeMetricsResponse,
    SessionCounts,
};
use super::runtime::{Runtime, RuntimeRules};
use super::scan::{BinAccumulator, Readings, Scan, read, scan};
use super::window::Window;
use super::{METRICS, MetricsQuery, QUOTA};
use crate::store::Telemetry;
use feld_core::model::Session;

/// The Runtime metrics of one Runtime: the metrics of its Sessions in every Workspace,
/// aggregated by each metric's rule, and the account quota its Sessions reported. Only the
/// `reported` Sessions of a metric enter its aggregate.
pub fn runtime_metrics(
    runtime_name: &str,
    sessions: &[(&Session, &[Telemetry])],
    maps: &InstrumentMaps,
    query: &MetricsQuery<'_>,
) -> Result<RuntimeMetricsResponse> {
    if let Some(metric) = query.metric
        && metric != QUOTA
        && !METRICS.contains(&metric)
    {
        bail!("unknown metric '{metric}'");
    }
    let window = Window::new(query.start, query.end, query.bucket)?;
    let runtime = Runtime::named(runtime_name);
    let sessions = sessions
        .iter()
        .filter(|(session, _)| session.execution.runtime.as_deref() == Some(runtime_name))
        .collect::<Vec<_>>();
    let names: Vec<&'static str> = METRICS
        .into_iter()
        .filter(|metric| query.metric.is_none_or(|wanted| wanted == *metric))
        .collect();

    // Each Session's Telemetry is decoded once, for its metrics and for the quota alike.
    // A Runtime without rules reads nothing from it.
    let readings: Vec<(&Session, Readings)> = sessions
        .iter()
        .map(|(session, telemetries)| {
            let readings = match runtime {
                Runtime::Rules(_) => read(&maps.of(session), telemetries),
                Runtime::NotApplicable(_) => Readings::default(),
            };
            (*session, readings)
        })
        .collect();
    let mut aggregates: BTreeMap<&'static str, Aggregate> = BTreeMap::new();
    if !names.is_empty() {
        for (session, readings) in &readings {
            let scan = match runtime {
                Runtime::Rules(rules) => scan(rules, readings, &window, session),
                Runtime::NotApplicable(_) => Scan::default(),
            };
            let account = readings.session_account();
            for metric in &names {
                let projected = project_metric(metric, runtime, session, &window, &scan);
                let aggregate = aggregates.entry(metric).or_default();
                match projected.status {
                    MetricStatus::Reported => {
                        aggregate.counts.reported += 1;
                        aggregate
                            .accounts
                            .entry(account.as_ref().map(|account| account.id.clone()))
                            .or_default()
                            .add(session, account.as_ref(), &scan);
                    }
                    MetricStatus::Unavailable => {
                        aggregate.counts.unavailable += 1;
                        aggregate.source.get_or_insert(projected.source);
                    }
                    MetricStatus::NotApplicable => {
                        aggregate.counts.not_applicable += 1;
                        aggregate.not_applicable.get_or_insert(projected.source);
                    }
                }
            }
        }
    }
    let metrics = names
        .into_iter()
        .map(|metric| {
            let aggregate = aggregates.remove(metric).unwrap_or_default();
            aggregate.project(metric, runtime, &window)
        })
        .collect();

    let quota = query
        .metric
        .is_none_or(|metric| metric == QUOTA)
        .then(|| quota_of(runtime, &readings, &window));
    Ok(RuntimeMetricsResponse {
        runtime: runtime_name.to_owned(),
        start: query.start.map(str::to_owned),
        end: query.end.map(str::to_owned),
        bucket: query.bucket,
        metrics,
        quota,
    })
}

/// The Sessions of one metric of a Runtime, their `reported` ones grouped by account.
#[derive(Default)]
struct Aggregate {
    /// Keyed by the account ID; `None` holds the Sessions whose account is unknown.
    accounts: BTreeMap<Option<String>, AccountAggregate>,
    counts: SessionCounts,
    /// The reason of the first `unavailable` Session, and of the first `n/a` one.
    source: Option<String>,
    not_applicable: Option<String>,
}

/// The `reported` Sessions of one account, merged bin by bin.
struct AccountAggregate {
    /// Named by the Session that started last.
    account: Option<Account>,
    bins: BTreeMap<i64, BinAccumulator>,
    start: Option<String>,
    latest_start: Option<String>,
    end: Option<String>,
    ended: bool,
}

impl Default for AccountAggregate {
    fn default() -> Self {
        Self {
            account: None,
            bins: BTreeMap::new(),
            start: None,
            latest_start: None,
            end: None,
            ended: true,
        }
    }
}

impl AccountAggregate {
    fn add(&mut self, session: &Session, account: Option<&Account>, scan: &Scan) {
        for (key, bin) in &scan.bins {
            self.bins.entry(*key).or_default().merge(bin, session);
        }
        if self
            .latest_start
            .as_ref()
            .is_none_or(|latest| *latest <= session.start)
        {
            self.latest_start = Some(session.start.clone());
            self.account = account.cloned();
        }
        if self
            .start
            .as_ref()
            .is_none_or(|start| session.start < *start)
        {
            self.start = Some(session.start.clone());
        }
        self.ended &= session.end.is_some();
        if self.end.as_deref() < session.end.as_deref() {
            self.end = session.end.clone();
        }
    }

    fn project(self, metric: &str, rules: &RuntimeRules, window: &Window) -> AccountSeries {
        let start = self.start.unwrap_or_default();
        let end = self.end.filter(|_| self.ended);
        let series = self
            .bins
            .iter()
            .filter_map(|(key, bin)| {
                let value = bin_value(metric, rules, bin, true)?;
                let mut bin_of = metric_bin(*key, value, window, &start, end.as_deref());
                if metric == "context_window_utilization" {
                    bin_of.session = bin.context_session.clone();
                }
                Some(bin_of)
            })
            .collect();
        AccountSeries {
            account: self.account,
            series,
        }
    }
}

impl Aggregate {
    fn project(
        self,
        metric: &'static str,
        runtime: Runtime,
        window: &Window,
    ) -> RuntimeMetricProjection {
        let rules = match runtime {
            Runtime::Rules(rules) => rules,
            Runtime::NotApplicable(reason) => {
                return RuntimeMetricProjection {
                    metric: metric.to_owned(),
                    status: MetricStatus::NotApplicable,
                    source: reason.to_owned(),
                    sessions: self.counts,
                    accounts: vec![],
                };
            }
        };
        let (status, source) = if self.counts.reported > 0 {
            (MetricStatus::Reported, rules.source_of(metric).to_owned())
        } else if let Some(source) = self.source {
            (MetricStatus::Unavailable, source)
        } else if let Some(source) = self.not_applicable {
            (MetricStatus::NotApplicable, source)
        } else {
            (MetricStatus::Unavailable, NO_USAGE.to_owned())
        };
        let accounts = self
            .accounts
            .into_values()
            .map(|account| account.project(metric, rules, window))
            .collect();
        RuntimeMetricProjection {
            metric: metric.to_owned(),
            status,
            source,
            sessions: self.counts,
            accounts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::fixture::{
        T120, T130, claude_instruments, claude_maps, codex_instruments, codex_session,
        metrics_telemetry, point, session,
    };
    use crate::metrics::runtime::NO_RULES;
    use feld_core::telemetry::proto::{self, AggregationTemporality::Delta};

    /// The Runtime metrics add up the `reported` Sessions of the Runtime, keep the Session
    /// with the fullest context window, and count the Sessions left out.
    #[test]
    fn aggregates_the_reported_sessions_of_one_runtime() {
        let first = session("agent-runtime", "Claude Code");
        let mut second = session("agent-runtime", "Claude Code");
        second.id = "s2".to_owned();
        let mut unprepared = session("agent-runtime", "Claude Code");
        unprepared.id = "s3".to_owned();
        unprepared.execution.telemetry_collector_available = Some(false);
        let codex = codex_session();
        let usage = |id: &str, input: f64, cache_read: f64, context: f64| {
            metrics_telemetry(
                id,
                vec![
                    proto::Metric::sum(
                        "claude_code.token.usage",
                        Delta,
                        vec![
                            point(T120, input).with("type", "input"),
                            point(T120, cache_read).with("type", "cacheRead"),
                        ],
                    ),
                    proto::Metric::gauge(
                        "feld.claude_code.context_window.used_percent",
                        vec![point(T120, context)],
                    ),
                ],
            )
        };
        let first_telemetries = [usage("a", 100.0, 0.0, 40.0)];
        let second_telemetries = [usage("b", 50.0, 150.0, 70.0)];
        let codex_telemetries = [usage("c", 1_000.0, 0.0, 99.0)];
        let sessions: [(&Session, &[Telemetry]); 4] = [
            (&first, &first_telemetries),
            (&second, &second_telemetries),
            (&unprepared, &[]),
            (&codex, &codex_telemetries),
        ];
        let response = runtime_metrics(
            "claude-code",
            &sessions,
            &claude_maps(),
            &MetricsQuery {
                bucket: Some(60),
                ..MetricsQuery::default()
            },
        )
        .unwrap();
        let of = |name: &str| {
            response
                .metrics
                .iter()
                .find(|metric| metric.metric == name)
                .unwrap()
        };

        let token = of("token_usage");
        assert_eq!(token.status, MetricStatus::Reported);
        assert_eq!(
            (token.sessions.reported, token.sessions.unavailable),
            (2, 1)
        );
        assert_eq!(token.accounts.len(), 1);
        assert!(token.accounts[0].account.is_none());
        assert_eq!(token.accounts[0].series[0].value["total"], 300.0);
        assert_eq!(
            of("prompt_cache_hit_rate").accounts[0].series[0].value,
            50.0
        );
        let context = &of("context_window_utilization").accounts[0];
        assert_eq!(context.series[0].value["used_percent"], 70.0);
        assert_eq!(context.series[0].session.as_deref(), Some("s2"));
        assert_eq!(response.quota.unwrap().status, MetricStatus::Unavailable);
    }

    /// An installed Runtime whose rules Feld does not have answers `n/a` for its metrics
    /// and its quota, with or without Sessions.
    #[test]
    fn a_runtime_without_rules_answers_not_applicable() {
        let mut other = session("agent-runtime", "Other");
        other.execution.runtime = Some("other".to_owned());
        let maps = InstrumentMaps {
            by_runtime: [("other".to_owned(), claude_instruments())]
                .into_iter()
                .collect(),
        };
        for sessions in [&[(&other, &[][..])][..], &[]] {
            let response =
                runtime_metrics("other", sessions, &maps, &MetricsQuery::default()).unwrap();
            for metric in &response.metrics {
                assert_eq!(
                    metric.status,
                    MetricStatus::NotApplicable,
                    "{}",
                    metric.metric
                );
                assert_eq!(metric.source, NO_RULES);
                assert_eq!(metric.sessions.not_applicable, sessions.len() as u64);
            }
            let quota = response.quota.unwrap();
            assert_eq!(quota.status, MetricStatus::NotApplicable);
            assert_eq!(quota.source, NO_RULES);
        }
    }

    /// The `reported` Sessions of each account make that account's series: the Runtime's
    /// own OTel names the account (Claude Code and Codex by their own ID attributes), the
    /// latest quota gauge names it for a Session whose OTel names none, and the Sessions
    /// whose account is unknown make one series without an account.
    #[test]
    fn each_account_makes_its_own_series() {
        let session_of = |id: &str, codex: bool| {
            let mut session = if codex {
                codex_session()
            } else {
                session("agent-runtime", "Claude Code")
            };
            session.id = id.to_owned();
            session
        };
        let claude_usage = |id: &str, input: f64, account: &[(&str, &str)]| {
            let point = account.iter().fold(
                point(T120, input).with("type", "input"),
                |point, (key, value)| point.with(*key, *value),
            );
            metrics_telemetry(
                id,
                vec![proto::Metric::sum(
                    "claude_code.token.usage",
                    Delta,
                    vec![point],
                )],
            )
        };
        let gauge = |id: &str, time: u64, account: &str| {
            metrics_telemetry(
                id,
                vec![proto::Metric::gauge(
                    "feld.claude_code.rate_limit.used_percent",
                    vec![
                        point(time, 10.0)
                            .with("window", "five_hour")
                            .with("account.id", account),
                    ],
                )],
            )
        };
        let (team, team_again, bot, unknown) = (
            session_of("s1", false),
            session_of("s2", false),
            session_of("s3", false),
            session_of("s4", false),
        );
        let team_telemetries = [claude_usage(
            "a",
            100.0,
            &[
                ("user.account_uuid", "team"),
                ("user.email", "team@example.com"),
            ],
        )];
        let team_again_telemetries = [claude_usage(
            "b",
            50.0,
            &[
                ("user.account_uuid", "team"),
                ("user.email", "team@example.com"),
            ],
        )];
        let bot_telemetries = [
            claude_usage("c", 7.0, &[]),
            gauge("d", T120, "team"),
            gauge("e", T130, "bot"),
        ];
        let unknown_telemetries = [claude_usage("f", 3.0, &[])];
        let sessions: [(&Session, &[Telemetry]); 4] = [
            (&team, &team_telemetries),
            (&team_again, &team_again_telemetries),
            (&bot, &bot_telemetries),
            (&unknown, &unknown_telemetries),
        ];
        let response = runtime_metrics(
            "claude-code",
            &sessions,
            &claude_maps(),
            &MetricsQuery {
                bucket: Some(60),
                metric: Some("token_usage"),
                ..MetricsQuery::default()
            },
        )
        .unwrap();
        let totals = |metric: &RuntimeMetricProjection| -> Vec<(Option<String>, f64)> {
            metric
                .accounts
                .iter()
                .map(|account| {
                    (
                        account
                            .account
                            .as_ref()
                            .map(|account| format!("{} {}", account.id, account.name)),
                        account.series[0].value["total"].as_f64().unwrap(),
                    )
                })
                .collect()
        };
        let token = &response.metrics[0];
        assert_eq!(token.sessions.reported, 4);
        assert_eq!(
            totals(token),
            vec![
                (None, 3.0),
                (Some("bot bot".to_owned()), 7.0),
                (Some("team team@example.com".to_owned()), 150.0),
            ]
        );
        let unknown = serde_json::to_value(&token.accounts[0]).unwrap();
        assert!(unknown.get("account").is_none());

        let codex_usage = |id: &str, account: &str| {
            metrics_telemetry(
                id,
                vec![proto::Metric::sum(
                    "codex.turn.token_usage",
                    Delta,
                    vec![
                        point(T120, 10.0)
                            .with("token_type", "input")
                            .with("user.account_id", account)
                            .with("user.email", "me@example.com"),
                    ],
                )],
            )
        };
        let (first, second) = (session_of("c1", true), session_of("c2", true));
        let first_telemetries = [codex_usage("g", "acct-1")];
        let second_telemetries = [codex_usage("h", "acct-2")];
        let sessions: [(&Session, &[Telemetry]); 2] =
            [(&first, &first_telemetries), (&second, &second_telemetries)];
        let maps = InstrumentMaps {
            by_runtime: [("codex".to_owned(), codex_instruments())]
                .into_iter()
                .collect(),
        };
        let response = runtime_metrics(
            "codex",
            &sessions,
            &maps,
            &MetricsQuery {
                bucket: Some(60),
                metric: Some("token_usage"),
                ..MetricsQuery::default()
            },
        )
        .unwrap();
        assert_eq!(
            totals(&response.metrics[0]),
            vec![
                (Some("acct-1 me@example.com".to_owned()), 10.0),
                (Some("acct-2 me@example.com".to_owned()), 10.0),
            ]
        );
    }
}
