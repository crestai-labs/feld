use std::collections::BTreeMap;

use serde_json::Value;

use super::{METRICS, QUOTA};
use feld_core::model::Session;
use feld_core::plugin::{Plugins, Registry};

/// The numbers a metric made of more than one value is built from.
const PARTS: [&str; 3] = ["size", "used_percent", "resets_at"];

/// What a Runtime Adapter's manifest says one instrument means: the Feld metric it feeds
/// and, when that metric is made of more than one number, which of them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Destination {
    pub(super) metric: &'static str,
    pub(super) part: Option<&'static str>,
}

impl Destination {
    /// Reads one `"<metric>"` or `"<metric>.<part>"` entry. Names core does not know are
    /// left out rather than guessed at.
    fn parse(value: &str) -> Option<Self> {
        let (metric, part) = match value.split_once('.') {
            Some((metric, part)) => (
                metric,
                Some(PARTS.into_iter().find(|known| *known == part)?),
            ),
            None => (value, None),
        };
        let metric = METRICS
            .into_iter()
            .chain([QUOTA])
            .find(|known| *known == metric)?;
        Some(Self { metric, part })
    }
}

/// The `metrics` of one Runtime Adapter's manifest: which of the instruments the Runtime
/// exports Feld reads, and what each one is worth. The meaning of the attributes and the
/// aggregation are core's.
#[derive(Clone, Debug, Default)]
pub struct Instruments {
    destinations: BTreeMap<String, Destination>,
}

impl Instruments {
    pub(super) fn read(manifest: Option<&Value>) -> Self {
        let destinations = manifest
            .and_then(Value::as_object)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|(instrument, destination)| {
                        Some((
                            instrument.clone(),
                            Destination::parse(destination.as_str()?)?,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { destinations }
    }

    pub(super) fn of(&self, instrument: &str) -> Option<Destination> {
        self.destinations.get(instrument).copied()
    }

    pub(super) fn destinations(&self) -> impl Iterator<Item = &Destination> {
        self.destinations.values()
    }

    /// Whether this Runtime gives the name a meaning. The telemetry decoder asks so that a
    /// log record whose body names an instrument is read as that instrument.
    pub fn knows(&self, instrument: &str) -> bool {
        self.destinations.contains_key(instrument)
    }
}

/// The `metrics` of every installed Runtime Adapter, by Runtime name.
#[derive(Clone, Debug, Default)]
pub struct InstrumentMaps {
    pub(super) by_runtime: BTreeMap<String, Instruments>,
}

impl InstrumentMaps {
    pub fn read(registry: &Registry) -> Self {
        let by_runtime = registry
            .runtimes()
            .into_iter()
            .filter_map(|summary| {
                let runtime = registry.runtime(&summary.name)?;
                Some((
                    summary.name.clone(),
                    Instruments::read(runtime.manifest().extra.get("metrics")),
                ))
            })
            .collect();
        Self { by_runtime }
    }

    /// What the Runtime of one Session declared. A Session whose Runtime is no longer
    /// installed reports nothing rather than another Runtime's instruments.
    pub fn of(&self, session: &Session) -> Instruments {
        session
            .execution
            .runtime
            .as_deref()
            .and_then(|runtime| self.by_runtime.get(runtime))
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::metrics::fixture::claude_instruments;

    /// The manifest form: an instrument name maps to a Feld metric, and to the part of it
    /// when the metric is made of more than one number.
    #[test]
    fn the_manifest_says_what_each_instrument_is_worth() {
        let claude = claude_instruments();
        assert_eq!(
            claude.of("claude_code.token.usage"),
            Some(Destination {
                metric: "token_usage",
                part: None
            })
        );
        assert_eq!(
            claude.of("feld.claude_code.context_window.size"),
            Some(Destination {
                metric: "context_window_utilization",
                part: Some("size")
            })
        );
        assert_eq!(
            claude.of("feld.claude_code.rate_limit.resets_at"),
            Some(Destination {
                metric: QUOTA,
                part: Some("resets_at")
            })
        );
        // Instruments the manifest leaves out have no meaning here, log records included.
        assert!(claude.knows("claude_code.api_request"));
        assert!(!claude.knows("claude_code.lines_of_code.count"));
        // Neither has an entry core cannot read.
        let unknown = Instruments::read(Some(&json!({
            "unknown-metric": "no_such_metric",
            "unknown-part": "token_usage.no_such_part",
            "not-a-string": 7,
            "known": "active_time",
        })));
        for instrument in ["unknown-metric", "unknown-part", "not-a-string"] {
            assert!(!unknown.knows(instrument), "{instrument}");
        }
        assert_eq!(
            unknown.of("known"),
            Some(Destination {
                metric: "active_time",
                part: None
            })
        );
    }
}
