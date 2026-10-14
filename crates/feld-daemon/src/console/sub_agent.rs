use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use feld_core::model::{Execution, Outcome, Session};

use super::transcript::Reading;
use super::turn::turn_of;
use super::{ConsoleState, EntryState, TranscriptSource, TurnRead, Window};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentSession {
    pub id: String,
    pub workspace: String,
    pub actor: String,
    pub sub_agent: String,
    pub parent_session: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub start_kind: String,
    pub ownership: Option<String>,
    pub end_kind: Option<String>,
    pub exit_code: Option<i32>,
    pub execution: Execution,
    pub model: Option<Value>,
    pub placement: Option<Value>,
    pub observation: Option<Value>,
    pub outcome: Outcome,
    pub console_state: ConsoleState,
    pub console_detail: Option<String>,
}

/// The part of a Session's id that names the Sub Agent inside it.
const SUB_AGENT_MARK: &str = "_subagent_";

/// The id a Sub Agent Session is read by: its parent's id and the Sub Agent's own.
pub fn sub_agent_session_id(parent: &str, sub_agent: &str) -> String {
    format!("{parent}{SUB_AGENT_MARK}{sub_agent}")
}

/// The parent Session and the Sub Agent a Sub Agent Session's id names.
pub fn split_sub_agent_session_id(id: &str) -> Option<(&str, &str)> {
    id.split_once(SUB_AGENT_MARK)
        .filter(|(parent, sub_agent)| !parent.is_empty() && !sub_agent.is_empty())
}

/// The Sub Agent Sessions one Session's entries show: one per Sub Agent that wrote an
/// entry, starting at its first instant and ending where the Session's own entries say it
/// finished.
pub fn project_sub_agents(
    parent: &Session,
    source: &TranscriptSource,
) -> Result<Vec<SubAgentSession>> {
    Ok(read_sub_agents(parent, source)?.unwrap_or_default())
}

/// The Sub Agent Sessions [`project_sub_agents`] gives, or none when the Session's entries
/// cannot be read yet.
pub fn read_sub_agents(
    parent: &Session,
    source: &TranscriptSource,
) -> Result<Option<Vec<SubAgentSession>>> {
    let reading = source.read(&Window::Tail { limit: usize::MAX })?;
    Ok(sub_agents_of(parent, &reading))
}

/// A current Session's turn and its Sub Agent Sessions, from one read of its
/// entries.
pub fn read_turn_and_sub_agents(
    parent: &Session,
    source: &TranscriptSource,
) -> Result<(TurnRead, Option<Vec<SubAgentSession>>)> {
    let reading = source.read(&Window::Tail { limit: usize::MAX })?;
    Ok((turn_of(&reading), sub_agents_of(parent, &reading)))
}

fn sub_agents_of(parent: &Session, reading: &Reading) -> Option<Vec<SubAgentSession>> {
    if reading.state != ConsoleState::Readable {
        return None;
    }
    let mut projected: Vec<SubAgentSession> = Vec::new();
    let mut ended: Vec<(String, Option<String>)> = Vec::new();
    for record in &reading.records {
        if let Some(agent) = &record.sub_agent_ended {
            ended.push((agent.clone(), record.at.clone()));
        }
        let Some(agent) = &record.sub_agent else {
            continue;
        };
        let lost = record.state == EntryState::Lost;
        match projected.iter_mut().find(|held| &held.sub_agent == agent) {
            Some(held) => {
                if held.start.is_none() {
                    held.start = record.at.clone();
                }
                if lost {
                    held.console_state = ConsoleState::Lost;
                    held.console_detail = Some(record.text.clone());
                }
            }
            None => projected.push(SubAgentSession {
                id: sub_agent_session_id(&parent.id, agent),
                workspace: parent.workspace.clone(),
                actor: parent.actor.clone(),
                sub_agent: agent.clone(),
                parent_session: parent.id.clone(),
                start: record.at.clone(),
                end: None,
                start_kind: "sub-agent".to_owned(),
                ownership: None,
                end_kind: None,
                exit_code: None,
                execution: parent.execution.clone(),
                model: parent.model.clone(),
                placement: None,
                observation: None,
                outcome: Outcome::Running,
                console_state: if lost {
                    ConsoleState::Lost
                } else {
                    ConsoleState::Readable
                },
                console_detail: lost.then(|| record.text.clone()),
            }),
        }
    }
    for session in &mut projected {
        // A Sub Agent whose parent recorded no completion ends when its parent does.
        session.end = ended
            .iter()
            .find(|(agent, _)| agent == &session.sub_agent)
            .and_then(|(_, at)| at.clone())
            .or_else(|| parent.end.clone());
        session.outcome = match (&session.end, session.console_state) {
            (Some(_), _) => Outcome::Ended,
            // Its entries cannot be read, so whether it finished cannot be told.
            (None, ConsoleState::Lost) => Outcome::Unknown,
            (None, _) => Outcome::Running,
        };
    }
    projected.sort_by(|left, right| left.start.cmp(&right.start).then(left.id.cmp(&right.id)));
    Some(projected)
}
