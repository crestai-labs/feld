//! What every `transcript` operation shares: the common transcript entry shape `plugin.md`
//! defines, the JSONL reading of a product's transcript files, and the page a cursor asks
//! for.
//!
//! A Plugin knows how its product writes records and where it keeps Sub Agent files; it
//! projects each record onto a [`Block`], places it in the column as a [`Projected`], and
//! hands the whole column to [`page`].

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::{Value, json};

use crate::plugin::protocol::{TranscriptRequest, TranscriptResponse, TranscriptState};

/// The kinds of the common transcript entry shape.
pub const INPUT: &str = "input";
pub const NOTIFICATION: &str = "notification";
pub const RESPONSE: &str = "response";
pub const THINKING: &str = "thinking";
pub const TOOL_CALL: &str = "tool-call";
pub const TOOL_RESULT: &str = "tool-result";
pub const SYSTEM: &str = "system";
pub const OTHER: &str = "other";
/// The `state` of a record that stands for records Feld cannot read.
pub const LOST: &str = "lost";

pub const MISSING_REASON: &str = "transcript file does not exist";

/// The answer for a transcript that cannot be read at all.
pub fn unreadable(state: TranscriptState, reason: &str) -> TranscriptResponse {
    TranscriptResponse {
        state,
        reason: Some(reason.to_owned()),
        records: Vec::new(),
        next: None,
    }
}

/// The page of the whole column the request asks for: the records after its cursor, at
/// most `limit` of them. A cursor that names no record is refused rather than read as the
/// end.
pub fn page(mut projected: Vec<Projected>, request: &TranscriptRequest) -> TranscriptResponse {
    projected.sort_by(|left, right| left.cursor.cmp(&right.cursor));
    if let Some(after) = request.after.as_deref()
        && !projected.iter().any(|record| record.cursor == after)
    {
        return unreadable(
            TranscriptState::InvalidCursor,
            &format!("cursor '{after}' names no transcript entry"),
        );
    }

    let mut records = projected
        .into_iter()
        .skip_while(|record| {
            request
                .after
                .as_deref()
                .is_some_and(|after| record.cursor.as_str() <= after)
        })
        .collect::<Vec<_>>();
    if request.limit > 0 && (request.limit as usize) < records.len() {
        records.truncate(request.limit as usize);
    }
    let next = records
        .last()
        .map(|record| record.cursor.clone())
        .or_else(|| request.after.clone());
    TranscriptResponse {
        state: TranscriptState::Readable,
        reason: None,
        records: records.into_iter().map(Projected::into_value).collect(),
        next,
    }
}

/// One record of the common shape, with the sort key its cursor carries.
pub struct Projected {
    pub cursor: String,
    pub at: Option<String>,
    pub kind: &'static str,
    pub text: String,
    /// Whether a tool result reports a failure.
    pub error: bool,
    /// `lost` when the record stands for a Sub Agent whose transcript file cannot be read.
    pub state: Option<&'static str>,
    pub sub_agent: Option<String>,
    pub sub_agent_ended: Option<String>,
    pub raw: Value,
}

impl Projected {
    /// The one record of a Sub Agent whose transcript file cannot be read. The Sub Agent
    /// Session is still there, and core draws it as a Console that cannot be read.
    pub fn lost(cursor: String, at: Option<String>, sub_agent: &str, reason: &str) -> Self {
        Projected {
            cursor,
            at,
            kind: OTHER,
            text: reason.to_owned(),
            error: false,
            state: Some(LOST),
            sub_agent: Some(sub_agent.to_owned()),
            sub_agent_ended: None,
            raw: Value::Null,
        }
    }

    fn into_value(self) -> Value {
        let mut record = json!({
            "cursor": self.cursor,
            "at": self.at,
            "kind": self.kind,
            "text": self.text,
            "raw": self.raw,
        });
        if self.error {
            record["error"] = Value::Bool(true);
        }
        if let Some(state) = self.state {
            record["state"] = Value::String(state.to_owned());
        }
        if let Some(agent) = self.sub_agent {
            record["subAgent"] = Value::String(agent);
        }
        if let Some(agent) = self.sub_agent_ended {
            record["subAgentEnded"] = Value::String(agent);
        }
        record
    }
}

/// What one line of a transcript file becomes: one transcript entry of the common shape,
/// without the place in the column that the cursor carries.
pub struct Block {
    pub kind: &'static str,
    pub text: String,
    pub error: bool,
}

pub fn block(kind: &'static str, text: impl Into<String>) -> Block {
    Block {
        kind,
        text: text.into(),
        error: false,
    }
}

/// One record of a transcript file with the place the column orders it by.
pub struct Timed<'a> {
    pub line: usize,
    pub record: &'a Value,
    pub at: Option<String>,
    /// The instant the record carries, or for a record written without one the last
    /// instant of its own file.
    pub ordering: i64,
}

/// The records of one transcript file in the order the product wrote them, each with the
/// instant the column orders it by.
pub fn timed(records: &[Value]) -> impl Iterator<Item = Timed<'_>> {
    records
        .iter()
        .enumerate()
        .scan(0_i64, |ordering, (line, record)| {
            let at = record_timestamp(record);
            if let Some(instant) = at.as_deref().and_then(epoch_millis) {
                *ordering = instant;
            }
            Some(Timed {
                line,
                record,
                at,
                ordering: *ordering,
            })
        })
}

pub enum Reading {
    Missing,
    Lost(String),
    Records(Vec<Value>),
}

impl Reading {
    /// The records, or the answer that says why the transcript cannot be read.
    pub fn into_records(self) -> Result<Vec<Value>, TranscriptResponse> {
        match self {
            Reading::Missing => Err(unreadable(TranscriptState::Missing, MISSING_REASON)),
            Reading::Lost(reason) => Err(unreadable(TranscriptState::Lost, &reason)),
            Reading::Records(records) => Ok(records),
        }
    }
}

/// One transcript file read as whole JSONL lines. A line the product has not finished
/// writing is left for the next read, which is why the cursor never passes it; a complete
/// line that is not a JSON object means the file is not the transcript it claims to be.
///
/// `noun` is what the product calls the file (`transcript`, `rollout`), and every reason
/// names it so.
pub fn read_records(path: &Path, noun: &str) -> Reading {
    if !path.is_absolute() || path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
        return Reading::Lost(format!("{noun} location must be an absolute JSONL path"));
    }
    let Some(parent) = path.parent() else {
        return Reading::Lost(format!("{noun} location has no parent"));
    };
    // A product creates the directory together with the file at the first input, so a
    // parent directory that does not exist yet is the same normal state as a missing file.
    match fs::read_dir(parent) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Reading::Missing,
        Err(error) => {
            return Reading::Lost(format!("{noun} location is unavailable: {error}"));
        }
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Reading::Missing,
        Err(error) => {
            return Reading::Lost(format!("{noun} file cannot be inspected: {error}"));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Reading::Lost(format!("{noun} location is not a regular file"));
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) => return Reading::Lost(format!("{noun} file cannot be read: {error}")),
    };
    let mut records = Vec::new();
    let mut reader = BufReader::new(file);
    loop {
        let mut line = Vec::new();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => return Reading::Lost(format!("{noun} file cannot be read: {error}")),
        }
        if !line.ends_with(b"\n") {
            break;
        }
        line.pop();
        if line.ends_with(b"\r") {
            line.pop();
        }
        let number = records.len() + 1;
        match serde_json::from_slice::<Value>(&line) {
            Ok(value) if value.is_object() => records.push(value),
            Ok(_) => return Reading::Lost(format!("{noun} line {number} is not a JSON object")),
            Err(_) => return Reading::Lost(format!("{noun} line {number} is not valid JSON")),
        }
    }
    Reading::Records(records)
}

pub fn record_timestamp(record: &Value) -> Option<String> {
    record
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn epoch_millis(timestamp: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|instant| instant.timestamp_millis())
}
