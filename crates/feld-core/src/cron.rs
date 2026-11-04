//! The cron expressions of Schedules: their validation and the ticks they plan.

use std::collections::BTreeSet;

use chrono::{DateTime, Duration, TimeZone, Timelike, Utc};
use chrono_tz::Tz;

use crate::time;

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
    after: DateTime<Utc>,
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
    at: DateTime<Utc>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_planner_preserves_both_fall_back_instants_and_skips_spring_gap() {
        let before_fall = "2026-11-01T04:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let first = next_run_after("30 1 * * *", "America/New_York", before_fall).unwrap();
        assert_eq!(first, "2026-11-01T05:30:00.000Z");
        let first_instant = DateTime::parse_from_rfc3339(&first)
            .unwrap()
            .with_timezone(&Utc);
        let second = next_run_after("30 1 * * *", "America/New_York", first_instant).unwrap();
        assert_eq!(second, "2026-11-01T06:30:00.000Z");

        // An origin inside the second copy of the repeated hour.
        for (cron, timezone, after, expected) in [
            (
                "* * * * *",
                "America/New_York",
                "2026-11-01T06:30:00Z",
                "2026-11-01T06:31:00.000Z",
            ),
            (
                "30 1 * * *",
                "America/New_York",
                "2026-11-01T06:15:00Z",
                "2026-11-01T06:30:00.000Z",
            ),
            (
                "* * * * *",
                "Australia/Lord_Howe",
                "2026-04-04T15:00:00Z",
                "2026-04-04T15:01:00.000Z",
            ),
            (
                "30 1 * * *",
                "America/New_York",
                "2026-11-01T06:45:00Z",
                "2026-11-02T06:30:00.000Z",
            ),
        ] {
            assert_eq!(
                next_run_after(cron, timezone, after.parse().unwrap()).unwrap(),
                expected,
                "{cron} in {timezone} after {after}"
            );
        }

        let before_spring = "2026-03-08T05:00:00Z".parse::<DateTime<Utc>>().unwrap();
        assert_eq!(
            next_run_after("30 2 * * *", "America/New_York", before_spring).unwrap(),
            "2026-03-09T06:30:00.000Z"
        );
        assert_eq!(
            next_run_after("* * * * *", "UTC", "2026-09-16T00:05:30Z".parse().unwrap()).unwrap(),
            "2026-09-16T00:06:00.000Z"
        );
    }

    #[test]
    fn last_run_is_the_latest_tick_at_or_before_the_instant_as_the_planner_writes_it() {
        for (cron, timezone, at, expected) in [
            (
                "*/15 * * * *",
                "Asia/Tokyo",
                "2026-09-29T20:14:59Z",
                Some("2026-09-29T20:00:00.000Z"),
            ),
            (
                "*/15 * * * *",
                "Asia/Tokyo",
                "2026-09-29T20:15:00Z",
                Some("2026-09-29T20:15:00.000Z"),
            ),
            (
                "30 1 * * *",
                "America/New_York",
                "2026-11-01T06:45:00Z",
                Some("2026-11-01T06:30:00.000Z"),
            ),
            (
                "0 0 1 1 *",
                "UTC",
                "1970-01-01T00:00:00Z",
                Some("1970-01-01T00:00:00.000Z"),
            ),
        ] {
            assert_eq!(
                last_run_at_or_before(cron, timezone, at.parse().unwrap())
                    .unwrap()
                    .as_deref(),
                expected,
                "{cron} in {timezone} at {at}"
            );
        }
    }

    #[test]
    fn cron_rejection_names_the_feld_of_the_expression_given() {
        assert_eq!(
            validate_cron("61 * * * *").unwrap_err(),
            "invalid cron expression '61 * * * *': the minute field '61' takes 0-59"
        );
        assert_eq!(
            validate_cron("0 9 * 13 MON").unwrap_err(),
            "invalid cron expression '0 9 * 13 MON': the month field '13' takes 1-12 or JAN-DEC"
        );
        assert_eq!(
            validate_cron("0 9 * *").unwrap_err(),
            "invalid cron expression '0 9 * *': it takes five fields, minute, hour, day of month, month, and day of week"
        );
        assert!(validate_cron("59 23 31 12 SUN").is_ok());
        assert_eq!(
            validate_cron("0 9 * * 8").unwrap_err(),
            "invalid cron expression '0 9 * * 8': the day of week field '8' takes 0-7 or SUN-SAT"
        );
    }

    #[test]
    fn cron_day_of_week_numbers_follow_standard_cron() {
        // 2026-09-16 is a Wednesday.
        let after = "2026-09-16T00:05:00Z".parse().unwrap();
        let next = |cron| next_run_after(cron, "UTC", after).unwrap();
        assert_eq!(next("0 0 * * 1"), "2026-09-21T00:00:00.000Z");
        assert_eq!(next("0 0 * * MON"), "2026-09-21T00:00:00.000Z");
        assert_eq!(next("0 0 * * 0"), "2026-09-20T00:00:00.000Z");
        assert_eq!(next("0 0 * * 7"), "2026-09-20T00:00:00.000Z");
        assert_eq!(next("0 0 * * 6"), "2026-09-19T00:00:00.000Z");
        assert_eq!(next("0 0 * * 5-7"), "2026-09-18T00:00:00.000Z");
        assert_eq!(next("0 0 * * 1-5/2"), "2026-09-18T00:00:00.000Z");
        assert_eq!(next("0 0 * * 0,3"), "2026-09-20T00:00:00.000Z");
    }

    #[test]
    fn cron_with_both_day_fields_fires_on_days_either_allows() {
        let next = |cron, after: &str| next_run_after(cron, "UTC", after.parse().unwrap()).unwrap();
        // 2026-09-16 is a Wednesday, and 2027-02-01 a Monday.
        assert_eq!(
            next("0 9 1 * MON", "2026-09-16T00:00:00Z"),
            "2026-09-21T09:00:00.000Z"
        );
        assert_eq!(
            next("0 9 1 * MON", "2026-09-28T10:00:00Z"),
            "2026-10-01T09:00:00.000Z"
        );
        assert_eq!(
            next("0 9 1 * MON", "2027-01-25T10:00:00Z"),
            "2027-02-01T09:00:00.000Z"
        );
        assert_eq!(
            next("0 9 1 * MON", "2027-02-01T09:00:00Z"),
            "2027-02-08T09:00:00.000Z"
        );
        // A day field starting with `*` leaves both to be met, as in standard cron.
        assert_eq!(
            next("0 9 */10 * MON", "2026-09-22T00:00:00Z"),
            "2026-12-21T09:00:00.000Z"
        );

        let last = |at: &str| {
            last_run_at_or_before("0 9 1 * MON", "UTC", at.parse().unwrap())
                .unwrap()
                .unwrap()
        };
        assert_eq!(last("2026-10-02T00:00:00Z"), "2026-10-01T09:00:00.000Z");
        assert_eq!(last("2026-09-30T00:00:00Z"), "2026-09-28T09:00:00.000Z");
        assert!(validate_cron("0 9 31 2 MON").is_ok());
    }
}
