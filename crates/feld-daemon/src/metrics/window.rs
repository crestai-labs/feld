use anyhow::{Context, Result, bail};
use chrono::{DateTime, SecondsFormat, Utc};

use feld_core::time;

pub(super) struct Window {
    pub(super) start_nanos: Option<i64>,
    pub(super) end_nanos: Option<i64>,
    pub(super) bucket_nanos: Option<i64>,
}

impl Window {
    pub(super) fn new(start: Option<&str>, end: Option<&str>, bucket: Option<u64>) -> Result<Self> {
        if bucket.is_some_and(|seconds| seconds < 60) {
            bail!("metric bucket must be at least 60 seconds");
        }
        let start_nanos = start.map(parse_nanos).transpose()?;
        let end_nanos = end.map(parse_nanos).transpose()?;
        if start_nanos
            .zip(end_nanos)
            .is_some_and(|(start, end)| start >= end)
        {
            bail!("metric range start must be before end");
        }
        let bucket_nanos = bucket
            .map(|seconds| {
                i64::try_from(seconds)
                    .ok()
                    .and_then(|seconds| seconds.checked_mul(1_000_000_000))
                    .context("metric bucket is too large")
            })
            .transpose()?;
        Ok(Self {
            start_nanos,
            end_nanos,
            bucket_nanos,
        })
    }

    pub(super) fn contains(&self, timestamp: u64) -> bool {
        let Ok(timestamp) = i64::try_from(timestamp) else {
            return false;
        };
        self.start_nanos.is_none_or(|start| timestamp >= start)
            && self.end_nanos.is_none_or(|end| timestamp < end)
    }

    pub(super) fn before_start(&self, timestamp: u64) -> bool {
        self.start_nanos
            .is_some_and(|start| i64::try_from(timestamp).is_ok_and(|timestamp| timestamp < start))
    }

    /// The bin a reading held from before the range falls in: the range's first.
    pub(super) fn start_key(&self) -> Option<i64> {
        self.key(u64::try_from(self.start_nanos?).ok()?)
    }

    /// Whether a Session with this end still runs at the range's start.
    pub(super) fn runs_at_start(&self, end: Option<&str>) -> bool {
        match (self.start_nanos, end) {
            (Some(start), Some(end)) => parse_nanos(end).is_ok_and(|end| end > start),
            _ => true,
        }
    }

    pub(super) fn key(&self, timestamp: u64) -> Option<i64> {
        if !self.contains(timestamp) {
            return None;
        }
        let timestamp = i64::try_from(timestamp).ok()?;
        Some(match self.bucket_nanos {
            Some(bucket) => timestamp.div_euclid(bucket) * bucket,
            None => 0,
        })
    }
}

fn parse_nanos(value: &str) -> Result<i64> {
    time::parse(value)
        .with_context(|| format!("parse RFC 3339 timestamp '{value}'"))?
        .timestamp_nanos_opt()
        .context("timestamp is outside the supported range")
}

pub(super) fn format_nanos(value: i64) -> String {
    let seconds = value.div_euclid(1_000_000_000);
    let nanos = value.rem_euclid(1_000_000_000) as u32;
    DateTime::<Utc>::from_timestamp(seconds, nanos)
        .expect("valid nanosecond timestamp")
        .to_rfc3339_opts(SecondsFormat::Nanos, true)
}

pub(super) fn format_nanos_u64(value: u64) -> String {
    i64::try_from(value)
        .map(format_nanos)
        .unwrap_or_else(|_| value.to_string())
}

pub(super) fn format_seconds(value: i64) -> String {
    DateTime::<Utc>::from_timestamp(value, 0)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|| value.to_string())
}
