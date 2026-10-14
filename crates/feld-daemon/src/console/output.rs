use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use feld_core::home::absent_as_none;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputLine {
    pub sequence: u64,
    pub stream: String,
    pub timestamp: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputPage {
    pub session: String,
    pub after: u64,
    pub next_after: u64,
    pub previous_before: Option<u64>,
    pub has_older: bool,
    pub lines: Vec<OutputLine>,
}

pub fn read_output(session: &str, path: &Path, after: u64) -> Result<OutputPage> {
    read_output_limited(session, path, after, usize::MAX)
}

pub fn read_output_limited(
    session: &str,
    path: &Path,
    after: u64,
    limit: usize,
) -> Result<OutputPage> {
    read_output_page(session, path, after, limit, false, None)
}

pub fn read_output_tail(session: &str, path: &Path, limit: usize) -> Result<OutputPage> {
    read_output_page(session, path, 0, limit, true, None)
}

pub fn read_output_before(
    session: &str,
    path: &Path,
    before: u64,
    limit: usize,
) -> Result<OutputPage> {
    read_output_page(session, path, 0, limit, false, Some(before))
}

fn read_output_page(
    session: &str,
    path: &Path,
    after: u64,
    limit: usize,
    tail: bool,
    before: Option<u64>,
) -> Result<OutputPage> {
    let mut lines = VecDeque::new();
    let mut eligible = 0_usize;
    let mut next_after = after;
    let Some(file) = absent_as_none(File::open(path)).context("open program output record")? else {
        return Ok(OutputPage {
            session: session.to_owned(),
            after,
            next_after,
            previous_before: None,
            has_older: false,
            lines: Vec::new(),
        });
    };
    let mut reader = BufReader::new(file);
    loop {
        let mut line = Vec::new();
        let read = reader
            .read_until(b'\n', &mut line)
            .context("read program output record")?;
        if read == 0 || !line.ends_with(b"\n") {
            break;
        }
        line.pop();
        let record: OutputLine =
            serde_json::from_slice(&line).context("decode program output record")?;
        validate_output_line(&record)?;
        let include = before.map_or(record.sequence > after, |before| record.sequence < before);
        // A forward page that is already full leaves this line to the next read.
        if include && !tail && before.is_none() && lines.len() >= limit {
            break;
        }
        next_after = next_after.max(record.sequence);
        if include {
            eligible += 1;
            lines.push_back(record);
            if (tail || before.is_some()) && lines.len() > limit {
                lines.pop_front();
            }
        }
    }
    let backward = tail || before.is_some();
    let has_older = backward && eligible > lines.len();
    let previous_before = backward
        .then(|| lines.front().map(|line| line.sequence))
        .flatten();
    Ok(OutputPage {
        session: session.to_owned(),
        after,
        next_after: if before.is_some() { after } else { next_after },
        previous_before,
        has_older,
        lines: lines.into(),
    })
}

pub fn validate_output_line(line: &OutputLine) -> Result<()> {
    if !matches!(line.stream.as_str(), "stdout" | "stderr") {
        bail!(
            "invalid program output stream '{}'; expected stdout or stderr",
            line.stream
        );
    }
    Ok(())
}
