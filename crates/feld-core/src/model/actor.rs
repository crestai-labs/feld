use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::{LaunchSpec, Outcome, Schedule, Session};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorKind {
    Agent,
    Job,
}

impl ActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Job => "job",
        }
    }
}

impl fmt::Display for ActorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ActorKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "agent" => Ok(Self::Agent),
            "job" => Ok(Self::Job),
            _ => Err(format!("unknown Actor kind '{value}'")),
        }
    }
}

/// An operation on the other Actors of its Workspace that the Admin lets an Actor's Sessions
/// call.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Grant {
    Read,
    Control,
    ManageAgent,
    ManageJob,
}

impl Grant {
    pub const ALL: [Self; 4] = [
        Self::Read,
        Self::Control,
        Self::ManageAgent,
        Self::ManageJob,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Control => "control",
            Self::ManageAgent => "manage-agent",
            Self::ManageJob => "manage-job",
        }
    }
}

impl fmt::Display for Grant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Grant {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|grant| grant.as_str() == value)
            .ok_or_else(|| {
                format!(
                    "unknown grant '{value}'; expected one of {}",
                    Self::ALL.map(Self::as_str).join(", ")
                )
            })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    /// The opaque ID Feld gives the Actor, which stays with it after a delete.
    pub id: String,
    pub name: String,
    pub workspace: String,
    pub kind: ActorKind,
    pub launch_spec: Option<LaunchSpec>,
    pub schedule: Option<Schedule>,
    /// What the Admin lets the Actor's Sessions do to the other Actors of its Workspace, each
    /// once and in order.
    pub grants: BTreeSet<Grant>,
    /// The names of the Actors of its Workspace whose Messages the Actor accepts, besides the
    /// Admin's. None accepts every Actor of the Workspace.
    pub accept_from: Option<BTreeSet<String>>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub current_session: Option<Session>,
    pub latest_session: Option<Session>,
    pub sessions: Vec<Session>,
    pub attention: bool,
    pub waiting_count: u64,
    pub unread_mail: bool,
    pub skip_mark: bool,
    pub unlinked_mark: bool,
}

impl Actor {
    /// Derives what the stored Actor implies. `last_skipped_at` is when admission last
    /// refused a tick of its Schedule.
    pub fn derive(&mut self, last_skipped_at: Option<&str>) {
        if let Some(schedule) = &mut self.schedule {
            schedule.derive_next_run();
            // A deleted Actor is no longer a start target, so its Schedule never fires again.
            if self.deleted_at.is_some() {
                schedule.next_run = None;
            }
        }
        self.sessions.iter_mut().for_each(Session::derive);
        self.current_session = self
            .sessions
            .iter()
            .find(|session| session.end.is_none())
            .cloned();
        self.latest_session = self.sessions.first().cloned();
        let evaluated = self
            .current_session
            .as_ref()
            .or(self.latest_session.as_ref());
        self.attention = evaluated.is_some_and(|session| {
            matches!(
                session.outcome,
                Outcome::Failed | Outcome::Exited | Outcome::Unknown
            ) || (session.end.is_none()
                && session
                    .observation
                    .as_ref()
                    .is_some_and(|observation| observation.kind.is_unavailable()))
        });
        let current_start = self.current_session.as_ref().map(|session| &session.start);
        self.unread_mail = current_start.is_some() && self.waiting_count > 0;
        self.unlinked_mark = self
            .current_session
            .as_ref()
            .is_some_and(Session::is_unlinked);
        // A refused tick marks the current Session it was refused for.
        self.skip_mark = current_start
            .is_some_and(|start| last_skipped_at.is_some_and(|skipped| skipped > start.as_str()));
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorReference {
    pub id: String,
    pub workspace: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Requester {
    Admin,
    Actor {
        id: String,
        workspace: String,
        name: String,
    },
}

impl Requester {
    /// The name a person reads the requester as.
    pub fn name(&self) -> &str {
        match self {
            Requester::Admin => "Admin",
            Requester::Actor { name, .. } => name,
        }
    }

    /// Which kind of requester this is, `admin` or `actor`, which a name alone leaves open
    /// because `admin` is also an Actor name.
    pub fn kind(&self) -> &'static str {
        match self {
            Requester::Admin => "admin",
            Requester::Actor { .. } => "actor",
        }
    }

    /// The `id` of the requesting Actor, which the Admin has none of.
    pub fn actor_id(&self) -> Option<&str> {
        match self {
            Requester::Admin => None,
            Requester::Actor { id, .. } => Some(id),
        }
    }
}
