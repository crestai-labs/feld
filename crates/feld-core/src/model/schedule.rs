use std::collections::BTreeSet;

use chrono::{DateTime, Duration, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::time;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub cron: String,
    pub timezone: String,
    pub enabled: bool,
    pub created_at: String,
    pub paused_at: Option<String>,
    pub enabled_at: Option<String>,
    pub next_run: Option<String>,
    /// How many ticks admission refused, from the `schedule.skipped` records of the Actor.
    #[serde(default)]
    pub skipped_ticks: u64,
    /// The planned time of a tick that should have fired by now and did not.
    #[serde(default)]
    pub overdue_tick: Option<String>,
    #[serde(skip)]
    pub revision: u64,
}

/// The `cron` and `timezone` an Actor declaration gives its Schedule.
#[derive(Clone, Debug)]
pub struct ScheduleDeclaration {
    pub cron: String,
    pub timezone: String,
}

/// The Schedule fields an Actor update changes; each `None` keeps the field.
#[derive(Clone, Debug, Default)]
pub struct SchedulePatch {
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub enabled: Option<bool>,
}

impl Schedule {
    pub(super) fn new(cron: String, timezone: String, enabled: bool, at: &str) -> Self {
        Self {
            cron,
            timezone,
            enabled,
            created_at: at.to_owned(),
            paused_at: (!enabled).then(|| at.to_owned()),
            enabled_at: enabled.then(|| at.to_owned()),
            next_run: None,
            skipped_ticks: 0,
            overdue_tick: None,
            revision: 0,
        }
    }

    /// The Schedule an Actor declaration gives: the declared `cron` and `timezone` over the
    /// `existing` Schedule, which keeps `enabled`, or a new enabled one.
    pub fn declare(declaration: &ScheduleDeclaration, existing: Option<Self>, at: &str) -> Self {
        let cron = declaration.cron.clone();
        let timezone = declaration.timezone.clone();
        match existing {
            Some(existing) => Self {
                cron,
                timezone,
                ..existing
            },
            None => Self::new(cron, timezone, true, at),
        }
    }

    /// The Schedule a patch creates on an Actor without one, which needs its `cron`.
    pub fn from_patch(patch: &SchedulePatch, at: &str, default_timezone: &str) -> Option<Self> {
        Some(Self::new(
            patch.cron.clone()?,
            patch
                .timezone
                .clone()
                .unwrap_or_else(|| default_timezone.to_owned()),
            patch.enabled.unwrap_or(true),
            at,
        ))
    }

    /// The Schedule after a patch at `at`: turning `enabled` off stamps `pausedAt`, and
    /// turning it on stamps `enabledAt`.
    pub fn apply_patch(self, patch: &SchedulePatch, at: &str) -> Self {
        let enabled = patch.enabled.unwrap_or(self.enabled);
        Self {
            cron: patch.cron.clone().unwrap_or(self.cron),
            timezone: patch.timezone.clone().unwrap_or(self.timezone),
            enabled,
            paused_at: if self.enabled && !enabled {
                Some(at.to_owned())
            } else {
                self.paused_at
            },
            enabled_at: if !self.enabled && enabled {
                Some(at.to_owned())
            } else {
                self.enabled_at
            },
            ..self
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_cron(&self.cron)?;
        validate_timezone(&self.timezone)
    }

    /// Whether this Schedule still fires the tick planned from `expected`: it is enabled and
    /// nothing changed since.
    pub fn fires_as(&self, expected: &Schedule) -> bool {
        self.enabled
            && self.cron == expected.cron
            && self.timezone == expected.timezone
            && self.created_at == expected.created_at
            && self.enabled_at == expected.enabled_at
            && self.revision == expected.revision
    }

    pub fn derive_next_run(&mut self) {
        self.next_run = if self.enabled {
            next_run_after(&self.cron, &self.timezone, Utc::now()).ok()
        } else {
            None
        };
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleSkip {
    pub planned: String,
    pub current_session_id: String,
    pub at: String,
}

pub fn validate_cron(expression: &str) -> Result<(), String> {
    const GREGORIAN_CYCLE_DAYS: i64 = 146_097;

    let schedule = cron_schedule(expression)?;
    let now = Utc::now();
    let horizon = now + Duration::days(GREGORIAN_CYCLE_DAYS);
    if schedule
        .after(&now)
        .next()
        .is_some_and(|next| next <= horizon)
    {
        Ok(())
    } else {
        Err("cron has no occurrence within the next 400 years".to_owned())
    }
}

pub fn validate_timezone(timezone: &str) -> Result<(), String> {
    timezone
        .parse::<Tz>()
        .map(|_| ())
        .map_err(|_| format!("'{timezone}' is not an IANA timezone"))
}

pub fn next_run_after(
    expression: &str,
    timezone: &str,
    after: chrono::DateTime<Utc>,
) -> Result<String, String> {
    let timezone = timezone
        .parse::<Tz>()
        .map_err(|_| format!("'{timezone}' is not an IANA timezone"))?;
    let schedule = cron_schedule(expression)?;
    let local_after = after.with_timezone(&timezone);
    // An ambiguous wall time resolves to its first instant, so when `after`
    // lies in the second copy the iterator's first answers precede it.
    let ordinary = schedule
        .after(&local_after)
        .find(|tick| tick.with_timezone(&Utc) > after)
        .ok_or_else(|| "cron expression has no future run".to_owned())?;

    // The cron iterator advances through wall time and omits the second copy
    // of an ambiguous time after a fall-back transition. Scan the short UTC
    // window before its answer so both real instants remain eligible ticks.
    let mut candidate = after
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .expect("zero is a valid second and nanosecond");
    if candidate <= after {
        candidate += Duration::minutes(1);
    }
    let scan_end = ordinary.with_timezone(&Utc).min(after + Duration::hours(4));
    while candidate < scan_end {
        let wall = candidate.with_timezone(&timezone);
        let wall_as_utc = Utc.from_utc_datetime(&wall.naive_local());
        if schedule
            .after(&(wall_as_utc - Duration::seconds(1)))
            .next()
            .is_some_and(|tick| tick == wall_as_utc)
        {
            return Ok(time::format(candidate));
        }
        candidate += Duration::minutes(1);
    }
    Ok(time::format(ordinary.with_timezone(&Utc)))
}

/// The latest tick at or before `at`, written as [`next_run_after`] plans it.
pub fn last_run_at_or_before(
    expression: &str,
    timezone: &str,
    at: chrono::DateTime<Utc>,
) -> Result<Option<String>, String> {
    let zone = timezone
        .parse::<Tz>()
        .map_err(|_| format!("'{timezone}' is not an IANA timezone"))?;
    let schedule = cron_schedule(expression)?;
    let Some(tick) = schedule
        .before(&(at + Duration::seconds(1)).with_timezone(&zone))
        .map(|tick| tick.with_timezone(&Utc))
        .find(|tick| *tick <= at)
    else {
        return Ok(None);
    };
    next_run_after(expression, timezone, tick - Duration::seconds(1)).map(Some)
}

/// Rewrites a standard day of week field, 0-7 with both 0 and 7 for Sunday, as the
/// list of days the `cron` crate reads, 1-7 with 1 for Sunday.
fn crate_day_of_week(field: &str) -> Option<String> {
    const NAMES: [&[&str]; 7] = [
        &["sun", "sunday"],
        &["mon", "monday"],
        &["tue", "tues", "tuesday"],
        &["wed", "wednesday"],
        &["thu", "thurs", "thursday"],
        &["fri", "friday"],
        &["sat", "saturday"],
    ];
    if field == "*" || field == "?" {
        return Some(field.to_owned());
    }
    let day = |value: &str| match value.parse::<usize>() {
        Ok(number) => (number <= 7).then_some(number),
        Err(_) => NAMES
            .iter()
            .position(|names| names.contains(&value.to_lowercase().as_str())),
    };
    let mut days = BTreeSet::new();
    for item in field.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => (range, step.parse::<usize>().ok().filter(|&step| step > 0)?),
            None => (item, 1),
        };
        let (low, high) = match range.split_once('-') {
            Some((low, high)) => (day(low)?, day(high)?),
            None if range == "*" => (0, 7),
            None if item.contains('/') => (day(range)?, 7),
            None => (day(range)?, day(range)?),
        };
        if low > high {
            return None;
        }
        days.extend((low..=high).step_by(step).map(|day| day % 7 + 1));
    }
    Some(
        days.iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(","),
    )
}

/// A cron expression as one `cron::Schedule` per alternative. The crate fires on days both
/// day fields allow, while standard cron fires on days either allows when neither field
/// starts with `*` or `?`, so such an expression takes one Schedule per day field.
struct CronSchedule(Vec<cron::Schedule>);

impl CronSchedule {
    /// The ticks after `at`, earliest first.
    fn after<Z: TimeZone>(&self, at: &DateTime<Z>) -> impl Iterator<Item = DateTime<Z>> {
        merge(self.0.iter().map(|schedule| schedule.after(at)), |a, b| {
            a < b
        })
    }

    /// The ticks before `at`, latest first.
    fn before<Z: TimeZone>(&self, at: &DateTime<Z>) -> impl Iterator<Item = DateTime<Z>> {
        merge(
            self.0.iter().map(|schedule| schedule.after(at).rev()),
            |a, b| a > b,
        )
    }
}

/// Merges iterators that each yield in the order `precedes` gives, yielding a tick that
/// several share once.
fn merge<T: PartialEq, I: Iterator<Item = T>>(
    iterators: impl Iterator<Item = I>,
    precedes: fn(&T, &T) -> bool,
) -> impl Iterator<Item = T> {
    let mut iterators: Vec<I> = iterators.collect();
    let mut heads: Vec<Option<T>> = iterators.iter_mut().map(Iterator::next).collect();
    std::iter::from_fn(move || {
        let first = (0..heads.len())
            .filter(|&index| heads[index].is_some())
            .reduce(|first, index| match (&heads[index], &heads[first]) {
                (Some(tick), Some(earliest)) if precedes(tick, earliest) => index,
                _ => first,
            })?;
        let tick = heads[first].take();
        for (index, (head, iterator)) in heads.iter_mut().zip(&mut iterators).enumerate() {
            if index == first || *head == tick {
                *head = iterator.next();
            }
        }
        tick
    })
}

fn cron_schedule(expression: &str) -> Result<CronSchedule, String> {
    const FIELDS: [(&str, &str); 5] = [
        ("minute", "0-59"),
        ("hour", "0-23"),
        ("day of month", "1-31"),
        ("month", "1-12 or JAN-DEC"),
        ("day of week", "0-7 or SUN-SAT"),
    ];
    let fields: Vec<&str> = expression.split_whitespace().collect();
    if fields.len() != FIELDS.len() {
        return Err(format!(
            "invalid cron expression '{expression}': it takes five fields, minute, hour, day of month, month, and day of week"
        ));
    }
    let parse = |fields: &[&str]| {
        let day_of_week = crate_day_of_week(fields[4])?;
        let schedule = |day_of_month: &str, day_of_week: &str| {
            format!(
                "0 {} {} {day_of_month} {} {day_of_week}",
                fields[0], fields[1], fields[3]
            )
            .parse::<cron::Schedule>()
            .ok()
        };
        let restricted = |field: &str| !field.starts_with(['*', '?']);
        Some(CronSchedule(
            if restricted(fields[2]) && restricted(fields[4]) {
                vec![schedule(fields[2], "*")?, schedule("*", &day_of_week)?]
            } else {
                vec![schedule(fields[2], &day_of_week)?]
            },
        ))
    };
    parse(&fields).ok_or_else(|| {
        // Parsing each field alone among wildcards names the one the expression breaks.
        let invalid = (0..fields.len()).find(|&index| {
            let mut probe = ["*"; 5];
            probe[index] = fields[index];
            parse(&probe).is_none()
        });
        match invalid {
            Some(index) => {
                let (name, range) = FIELDS[index];
                format!(
                    "invalid cron expression '{expression}': the {name} field '{}' takes {range}",
                    fields[index]
                )
            }
            None => format!("invalid cron expression '{expression}'"),
        }
    })
}
