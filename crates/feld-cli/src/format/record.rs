//! The timeline's records as a person reads them.

use feld_core::model::{Record, RecordKind};
use serde_json::Value;

use crate::Context;
use crate::output::kinded;

pub const RECORD_HEADERS: [&str; 5] = ["SEQ", "AT", "ACTOR", "KIND", "DETAIL"];

/// The human-readable description of one record.
pub fn record_description(ctx: &Context, record: &Record) -> String {
    let data = &record.data;
    let text = |key: &str| data.get(key).and_then(Value::as_str);
    match record.kind {
        RecordKind::Spawned => format!(
            "by {}",
            actor_label(data.get("spawner"), text("spawnerSessionId"))
        ),
        RecordKind::SessionStarted => {
            let mut description = format!("start_kind={}", text("startKind").unwrap_or("-"));
            if let Some(planned) = text("planned") {
                description.push_str(&format!(" planned={}", ctx.local_time(planned)));
            }
            description
        }
        RecordKind::SessionEnded => {
            let mut description = format!("end_kind={}", text("endKind").unwrap_or("-"));
            if let Some(exit_code) = data.get("exitCode").and_then(Value::as_i64) {
                description.push_str(&format!(" exit_code={exit_code}"));
            }
            description
        }
        RecordKind::Observation => {
            let mut description = text("kind").unwrap_or("-").to_owned();
            if let Some(detail) = text("detail") {
                description.push_str(&format!(" · {detail}"));
            }
            description
        }
        RecordKind::MessageAccepted => {
            let mut description = format!(
                "{} from {}",
                text("messageId").unwrap_or("-"),
                actor_label(data.get("from"), text("fromSessionId"))
            );
            if data.get("waiting").and_then(Value::as_bool) == Some(true) {
                description.push_str(" (waiting)");
            }
            description
        }
        RecordKind::MessageSent => format!(
            "{} to {}",
            text("messageId").unwrap_or("-"),
            data.get("to")
                .and_then(|to| to.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("-")
        ),
        RecordKind::MessageRead => text("messageId").unwrap_or("-").to_owned(),
        RecordKind::StopRequested | RecordKind::KillRequested => {
            format!("by {}", text("requesterSessionId").unwrap_or("Admin"))
        }
        RecordKind::ScheduleSkipped => format!(
            "planned {}, current {} was still running",
            text("planned").map_or_else(|| "-".to_owned(), |planned| ctx.local_time(planned)),
            text("currentSessionId").unwrap_or("-")
        ),
        RecordKind::ScheduleMissed => format!(
            "planned {}, the Scheduler stopped: {}",
            text("planned").map_or_else(|| "-".to_owned(), |planned| ctx.local_time(planned)),
            text("reason").unwrap_or("-")
        ),
    }
}

/// The name a Requester record carries, with its Session when it has one.
fn actor_label(requester: Option<&Value>, session_id: Option<&str>) -> String {
    let name = requester
        .and_then(|requester| requester.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("Admin");
    session_id.map_or_else(|| name.to_owned(), |session| kinded(name, session))
}
