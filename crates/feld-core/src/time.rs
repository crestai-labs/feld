//! The timestamps Feld commits and answers: RFC 3339 in UTC, to the millisecond.

use chrono::{DateTime, ParseError, SecondsFormat, Utc};

/// The current instant as a timestamp.
pub fn now() -> String {
    format(Utc::now())
}

/// `instant` as a timestamp.
pub fn format(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// The instant an RFC 3339 timestamp names, whatever its offset.
pub fn parse(value: &str) -> Result<DateTime<Utc>, ParseError> {
    DateTime::parse_from_rfc3339(value).map(|instant| instant.with_timezone(&Utc))
}

/// The IANA name of the local timezone: the one `TZ` names, as the C library reads it,
/// else the system's.
pub fn local_timezone() -> Option<String> {
    std::env::var("TZ")
        .ok()
        .map(|name| name.trim_start_matches(':').to_owned())
        .filter(|name| name.parse::<chrono_tz::Tz>().is_ok())
        .or_else(|| iana_time_zone::get_timezone().ok())
}
