use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use anyhow::Result;

use feld_core::model::RuntimeIdentity;
use feld_core::plugin::protocol::{TranscriptRequest, TranscriptState};
use feld_core::plugin::{PluginKind, RuntimeAdapter, not_installed};

use super::{ConsoleState, EntryKind, EntryState, TranscriptEntry, TranscriptPage, UnknownCursor};

/// Which entries of a Session's Runtime a read is about: the Session's own, or those of
/// one Sub Agent inside it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Stream {
    Parent,
    SubAgent(String),
}

impl Stream {
    fn holds(&self, record: &TranscriptEntry) -> bool {
        match self {
            Self::Parent => record.sub_agent.is_none(),
            Self::SubAgent(id) => record.sub_agent.as_deref() == Some(id.as_str()),
        }
    }
}

/// Which part of the entry column a read asks for.
#[derive(Clone, Debug)]
pub enum Window {
    /// The entries after a cursor, oldest first. The live tail asks for this.
    After { after: Option<String>, limit: usize },
    /// The newest entries, which is what an opening Console shows.
    Tail { limit: usize },
    /// The entries before a cursor, which is the page above the one held.
    Before { before: String, limit: usize },
}

/// Where one Session's transcript entries come from: the Runtime Adapter that projects them,
/// and the identity that tells the Adapter which Runtime to read.
#[derive(Clone)]
pub struct TranscriptSource {
    adapter: Option<Arc<dyn RuntimeAdapter>>,
    identity: Option<RuntimeIdentity>,
    /// Why no Adapter answers, when none does.
    missing_adapter: Option<String>,
}

impl TranscriptSource {
    pub fn new(
        adapter: Option<Arc<dyn RuntimeAdapter>>,
        identity: Option<RuntimeIdentity>,
    ) -> Self {
        Self {
            adapter,
            identity,
            missing_adapter: None,
        }
    }

    /// The source of a Session whose Runtime Adapter is not installed, so that the Console
    /// says why rather than showing an empty entry column.
    pub fn without_adapter(runtime: Option<&str>, identity: Option<RuntimeIdentity>) -> Self {
        Self {
            adapter: None,
            identity,
            missing_adapter: Some(match runtime {
                Some(runtime) => not_installed(PluginKind::Runtime, runtime),
                None => "this Session names no Runtime Adapter".to_owned(),
            }),
        }
    }

    /// Every entry of this Runtime, the Sub Agents' among the Session's own.
    pub(super) fn read(&self, window: &Window) -> Result<Reading, UnknownCursor> {
        let Some(identity) = self.identity.clone() else {
            return Ok(Reading::unreadable(
                ConsoleState::Pending,
                "Runtime identity has not supplied a transcript location",
            ));
        };
        let Some(adapter) = self.adapter.as_ref() else {
            return Ok(Reading::unreadable(
                ConsoleState::Lost,
                self.missing_adapter
                    .as_deref()
                    .unwrap_or("no Runtime Adapter answers for this Session"),
            ));
        };
        // Only a forward read can be asked of the Adapter by cursor; the newest entries and
        // the page above one both need the column from its start.
        let (after, limit) = match window {
            Window::After { after, limit } => (after.clone(), *limit),
            Window::Tail { .. } | Window::Before { .. } => (None, usize::MAX),
        };
        let answer = adapter.transcript(&TranscriptRequest {
            identity,
            after: after.clone(),
            limit: limit as u64,
        });
        let answer = match answer {
            Ok(answer) => answer,
            Err(error) => {
                return Ok(Reading::unreadable(
                    ConsoleState::Lost,
                    &format!("{error:#}"),
                ));
            }
        };
        let state = match answer.state {
            TranscriptState::Missing => ConsoleState::Missing,
            TranscriptState::Lost => ConsoleState::Lost,
            TranscriptState::Readable => ConsoleState::Readable,
            TranscriptState::InvalidCursor => {
                return Err(UnknownCursor(after.unwrap_or_default()));
            }
        };
        if state != ConsoleState::Readable {
            return Ok(Reading {
                state,
                detail: answer.reason.or_else(|| Some(default_reason(state))),
                records: Vec::new(),
                next: None,
            });
        }
        let records = answer
            .records
            .into_iter()
            .map(serde_json::from_value::<TranscriptEntry>)
            .collect::<Result<Vec<_>, _>>();
        Ok(match records {
            Ok(records) => Reading {
                state,
                detail: answer.reason,
                records,
                next: answer.next.or(after),
            },
            Err(error) => Reading::unreadable(
                ConsoleState::Lost,
                &format!(
                    "the Runtime Adapter answered a transcript entry Feld cannot read: {error}"
                ),
            ),
        })
    }
}

/// What one read of a Runtime's entries answered.
pub(super) struct Reading {
    pub(super) state: ConsoleState,
    detail: Option<String>,
    pub(super) records: Vec<TranscriptEntry>,
    next: Option<String>,
}

impl Reading {
    fn unreadable(state: ConsoleState, detail: &str) -> Self {
        Self {
            state,
            detail: Some(detail.to_owned()),
            records: Vec::new(),
            next: None,
        }
    }
}

fn default_reason(state: ConsoleState) -> String {
    match state {
        ConsoleState::Missing => "transcript file does not exist".to_owned(),
        _ => "the transcript file's location can no longer be reached".to_owned(),
    }
}

/// One page of a Session's transcript entries, in the common shape. In the Conversation view,
/// when `conversation` is set, the entries of kind `other` leave the column before the window
/// is cut, so a tail of n holds n entries of the view, and the forward cursor still passes
/// the entries the view left out. A cursor that names no entry is refused.
pub fn read_transcript_view(
    source: &TranscriptSource,
    session: &str,
    stream: &Stream,
    window: Window,
    conversation: bool,
) -> Result<TranscriptPage, UnknownCursor> {
    // The Adapter counts every kind, so a view that leaves some out cuts the limit itself.
    let reading = match &window {
        Window::After { after, .. } if conversation => source.read(&Window::After {
            after: after.clone(),
            limit: usize::MAX,
        }),
        _ => source.read(&window),
    }?;
    let asked = match &window {
        Window::After { after, .. } => after.clone(),
        Window::Tail { .. } | Window::Before { .. } => None,
    };
    if reading.state != ConsoleState::Readable {
        return Ok(TranscriptPage {
            session: session.to_owned(),
            state: reading.state,
            detail: reading.detail,
            after: asked.clone(),
            next_after: asked,
            previous_before: None,
            has_older: false,
            records: Vec::new(),
        });
    }
    // The page above a cursor holds what the whole column has before it, whichever stream
    // or view the cursor came from.
    let earlier = match &window {
        Window::Before { before, .. } => {
            let at = reading
                .records
                .iter()
                .position(|record| &record.cursor == before)
                .ok_or_else(|| UnknownCursor(before.clone()))?;
            reading.records[..at]
                .iter()
                .map(|record| record.cursor.clone())
                .collect::<HashSet<_>>()
        }
        Window::After { .. } | Window::Tail { .. } => HashSet::new(),
    };
    let held = reading
        .records
        .into_iter()
        .filter(|record| stream.holds(record))
        .collect::<Vec<_>>();
    // The Adapter answers one column for the whole Runtime, so a Sub Agent whose own
    // entries it cannot read says so with an entry of its own rather than with the state
    // of the column.
    if let Some(lost) = held.iter().find(|record| record.state == EntryState::Lost) {
        return Ok(TranscriptPage {
            session: session.to_owned(),
            state: ConsoleState::Lost,
            detail: Some(lost.text.clone()),
            after: asked.clone(),
            next_after: asked,
            previous_before: None,
            has_older: false,
            records: Vec::new(),
        });
    }
    let last_held = held.last().map(|record| record.cursor.clone());
    let held = if conversation {
        held.into_iter()
            .filter(|record| record.kind != EntryKind::Other)
            .collect()
    } else {
        held
    };
    let mut truncated = false;
    let (records, has_older, previous_before) = match &window {
        Window::After { limit, .. } => {
            let mut records = held;
            truncated = records.len() > *limit;
            records.truncate(*limit);
            (records, false, None)
        }
        Window::Tail { limit } => {
            let eligible = held.len();
            let records = tail_of(held, *limit);
            let previous = records.first().map(|record| record.cursor.clone());
            (records, eligible > *limit, previous)
        }
        Window::Before { limit, .. } => {
            let eligible = held
                .iter()
                .take_while(|record| earlier.contains(&record.cursor))
                .count();
            let mut earlier = held;
            earlier.truncate(eligible);
            let records = tail_of(earlier, *limit);
            let previous = records.first().map(|record| record.cursor.clone());
            let has_older = eligible > records.len();
            (records, has_older, previous)
        }
    };
    // What a forward read continues from. The Adapter answers one column for the whole
    // Runtime, so its own `next` can name another Sub Agent's entry; the last entry this
    // stream holds is the cursor that never skips one of this stream's own. A page that
    // held none of them takes the Adapter's `next` instead, so the read still advances.
    let next_after = match &window {
        Window::Before { .. } => None,
        _ if truncated => records
            .last()
            .map(|record| record.cursor.clone())
            .or_else(|| asked.clone()),
        _ => last_held.or(reading.next).or_else(|| asked.clone()),
    };
    Ok(TranscriptPage {
        session: session.to_owned(),
        state: ConsoleState::Readable,
        detail: reading.detail,
        after: asked,
        next_after,
        previous_before,
        has_older,
        records,
    })
}

fn tail_of(records: Vec<TranscriptEntry>, limit: usize) -> Vec<TranscriptEntry> {
    let mut held = VecDeque::from(records);
    while held.len() > limit {
        held.pop_front();
    }
    held.into()
}
