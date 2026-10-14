use feld_core::model::Turn;

use super::transcript::Reading;
use super::{ConsoleState, EntryKind, TranscriptSource, Window};

/// A [turn] and the `at` of the entry it is read from.
///
/// [turn]: https://github.com/crestai-labs/feld/blob/main/docs/spec/concepts/glossary.md#turn
pub type TurnRead = (Turn, Option<String>);

/// The turn of a Session's Runtime: the last of the Session's own entries whose kind
/// carries a turn gives it. A transcript that cannot be read, or holds no such entry,
/// leaves the turn `unavailable`.
pub fn read_turn(source: &TranscriptSource) -> TurnRead {
    match source.read(&Window::Tail { limit: usize::MAX }) {
        Ok(reading) => turn_of(&reading),
        Err(_) => (Turn::Unavailable, None),
    }
}

pub(super) fn turn_of(reading: &Reading) -> TurnRead {
    if reading.state != ConsoleState::Readable {
        return (Turn::Unavailable, None);
    }
    let last = reading
        .records
        .iter()
        .rev()
        .find(|record| record.sub_agent.is_none() && record.kind.carries_turn());
    match last {
        Some(record) if record.kind == EntryKind::Response => {
            (Turn::WaitingInput, record.at.clone())
        }
        Some(record) => (Turn::Responding, record.at.clone()),
        None => (Turn::Unavailable, None),
    }
}
