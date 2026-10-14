use feld_core::model::Session;

pub(super) const CLAUDE_SOURCE: &str = "Claude Code Runtime telemetry";
pub(super) const STATUS_SOURCE: &str = "Claude Code status line";
pub(super) const CODEX_SOURCE: &str = "Codex Runtime telemetry";
pub(super) const ROLLOUT_SOURCE: &str = "Codex rollout";
pub(super) const PROGRAM: &str = "Program Sessions do not have Runtime telemetry";
pub(super) const NO_RULES: &str = "Feld has no metric rules for this Runtime";
const RUNTIME_UNPREPARED: &str = "Runtime did not start with this daemon's telemetry environment; start it again from a shell that ran `eval \"$(feld env)\"`";
/// Codex builds its OTLP header once, from the environment it started in, so only a launch
/// this Adapter made puts a Session token on its exports.
const CODEX_ATTACHED: &str = "Codex telemetry is attributed only in owned Sessions";

/// An attribute and the value it must hold.
pub(super) type Attribute = (&'static str, &'static str);

/// How the points of a counting metric add up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Count {
    /// Every point is one occurrence.
    Occurrences,
    /// A log record is one occurrence and every other point is its value, for the points
    /// that carry `only` and do not carry `except`.
    Values {
        only: Option<Attribute>,
        except: Option<Attribute>,
    },
}

/// Which of a Runtime's token types is each of the four kinds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TokenKinds {
    /// The attribute the type is in.
    pub(super) attribute: &'static str,
    pub(super) input: &'static str,
    pub(super) output: &'static str,
    pub(super) cache_read: &'static str,
    pub(super) cache_creation: &'static str,
    /// Whether the Runtime's input counts the cache reads, which the four kinds keep apart.
    pub(super) input_includes_cache_read: bool,
}

/// How the metrics of one Runtime are read from its telemetry: the values and choices
/// the Runtime's own file in `docs/spec/design/adapter/` lays down.
#[derive(Debug, PartialEq)]
pub(super) struct RuntimeRules {
    pub(super) name: &'static str,
    /// The `source` of a metric read from the Runtime's OTel.
    pub(super) source: &'static str,
    /// The `source` of a metric read from the gauges the part beside the Runtime sends.
    pub(super) gauge_source: &'static str,
    /// Why the metrics are `unavailable` when the Runtime started without the telemetry
    /// environment.
    pub(super) unprepared: &'static str,
    /// The metrics the Runtime does not report, with the reason.
    pub(super) not_applicable: &'static [(&'static str, &'static str)],
    pub(super) tokens: TokenKinds,
    /// How many units of the reported cost make one USD.
    pub(super) cost_per_usd: f64,
    pub(super) api_errors: Count,
    pub(super) tool_calls: Count,
}

pub(super) const CLAUDE_CODE: RuntimeRules = RuntimeRules {
    name: "claude-code",
    source: CLAUDE_SOURCE,
    gauge_source: STATUS_SOURCE,
    unprepared: RUNTIME_UNPREPARED,
    not_applicable: &[],
    tokens: TokenKinds {
        attribute: "type",
        input: "input",
        output: "output",
        cache_read: "cacheRead",
        cache_creation: "cacheCreation",
        input_includes_cache_read: false,
    },
    cost_per_usd: 1.0,
    api_errors: Count::Occurrences,
    tool_calls: Count::Occurrences,
};

pub(super) const CODEX: RuntimeRules = RuntimeRules {
    name: "codex",
    source: CODEX_SOURCE,
    gauge_source: ROLLOUT_SOURCE,
    unprepared: CODEX_ATTACHED,
    not_applicable: &[("active_time", "Not provided by Codex")],
    tokens: TokenKinds {
        attribute: "token_type",
        input: "input",
        output: "output",
        cache_read: "cached_input",
        cache_creation: "cache_write_input",
        input_includes_cache_read: true,
    },
    cost_per_usd: 1_000_000.0,
    api_errors: Count::Values {
        only: Some(("success", "false")),
        except: None,
    },
    // One shell run of Codex reports `exec_command` and `exec`; the pair is one call.
    tool_calls: Count::Values {
        only: None,
        except: Some(("tool", "exec")),
    },
};

const RULES: [&RuntimeRules; 2] = [&CLAUDE_CODE, &CODEX];

/// What the metrics of a Session's Runtime are read by: the rules of the Runtime, or the
/// reason every metric is `n/a`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Runtime {
    Rules(&'static RuntimeRules),
    NotApplicable(&'static str),
}

impl Runtime {
    pub(super) fn of(session: &Session) -> Self {
        session
            .execution
            .runtime
            .as_deref()
            .map_or(Self::NotApplicable(PROGRAM), Self::named)
    }

    pub(super) fn named(runtime: &str) -> Self {
        RULES
            .into_iter()
            .find(|rules| rules.name == runtime)
            .map_or(Self::NotApplicable(NO_RULES), Self::Rules)
    }
}

impl RuntimeRules {
    pub(super) fn source_of(&self, metric: &str) -> &'static str {
        if metric == "context_window_utilization" {
            self.gauge_source
        } else {
            self.source
        }
    }

    pub(super) fn not_applicable(&self, metric: &str) -> Option<&'static str> {
        self.not_applicable
            .iter()
            .find(|(name, _)| *name == metric)
            .map(|(_, reason)| *reason)
    }
}
