use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::sync::broadcast;

use crate::console::{
    SubAgentSession, TranscriptSource, TurnRead, read_sub_agents, read_turn,
    read_turn_and_sub_agents,
};
use crate::store::{
    RecordFilter, ResourceEvent, SessionFilter, SessionWriteError, Store, Telemetry,
    TelemetryAdmission,
};
use feld_core::model::{Actor, Record, Session};
use feld_core::plugin::protocol::RuntimeIdentity;
use feld_core::plugin::{Plugins, Registry};

/// What the API reads of Sessions and their Records, and what a Session's own Runtime and
/// the Collector deliver about it. Starting and ending a Session is the
/// [`SessionController`](crate::control::SessionController)'s.
pub struct SessionService {
    store: Arc<Store>,
    registry: Arc<Registry>,
    /// The Sub Agent Sessions of ended Sessions, whose entries no longer grow.
    ended_sub_agents: Mutex<HashMap<String, Vec<SubAgentSession>>>,
}

impl SessionService {
    pub fn new(store: Arc<Store>, registry: Arc<Registry>) -> Self {
        Self {
            store,
            registry,
            ended_sub_agents: Mutex::default(),
        }
    }

    /// The Sub Agent Sessions a Session's entries show. An ended Session's are projected
    /// once and kept.
    pub fn sub_agents(&self, session: &Session) -> Result<Vec<SubAgentSession>> {
        if let Some(held) = self.held_sub_agents(&session.id) {
            return Ok(held);
        }
        let source = self.transcript_source(session)?;
        let Some(projected) = read_sub_agents(session, &source)? else {
            return Ok(Vec::new());
        };
        if session.end.is_some() {
            self.ended_sub_agents
                .lock()
                .unwrap()
                .insert(session.id.clone(), projected.clone());
        }
        Ok(projected)
    }

    /// Fills in the turn of each current `agent-runtime` Session, which its transcript
    /// tells.
    pub fn derive_turns<'a>(
        &self,
        sessions: impl IntoIterator<Item = &'a mut Session>,
    ) -> Result<()> {
        for session in sessions {
            if has_turn(session) {
                let read = read_turn(&self.transcript_source(session)?);
                set_turn(session, read);
            }
        }
        Ok(())
    }

    /// A Session as `get` reads it: its turn filled in, and the Sub Agent Sessions its
    /// entries show, from one read of its transcript.
    pub fn session_read(&self, session: &mut Session) -> Result<Vec<SubAgentSession>> {
        if !has_turn(session) {
            return self.sub_agents(session);
        }
        let source = self.transcript_source(session)?;
        let (read, sub_agents) = read_turn_and_sub_agents(session, &source)?;
        set_turn(session, read);
        Ok(sub_agents.unwrap_or_default())
    }

    /// Fills in the turn of an Actor's current Session wherever the Actor holds it.
    pub fn derive_actor_turns(&self, actor: &mut Actor) -> Result<()> {
        let Some(current) = actor.current_session.as_mut() else {
            return Ok(());
        };
        self.derive_turns([&mut *current])?;
        let current = current.clone();
        for session in actor
            .latest_session
            .iter_mut()
            .chain(actor.sessions.iter_mut())
        {
            if session.id == current.id {
                *session = current.clone();
            }
        }
        Ok(())
    }

    fn held_sub_agents(&self, session: &str) -> Option<Vec<SubAgentSession>> {
        self.ended_sub_agents.lock().unwrap().get(session).cloned()
    }

    pub fn get(&self, id: &str) -> Result<Option<Session>> {
        self.store.get_session(id)
    }

    pub fn query(&self, filter: &SessionFilter<'_>) -> Result<Vec<Session>> {
        self.store.query_sessions(filter)
    }

    /// The current Sessions of every Workspace.
    pub fn current(&self) -> Result<Vec<Session>> {
        self.query(&SessionFilter {
            current: true,
            ..SessionFilter::default()
        })
    }

    pub fn records(&self, filter: &RecordFilter<'_>) -> Result<Vec<Record>> {
        self.store.list_records(filter)
    }

    /// When the oldest Record Feld keeps was written.
    pub fn history_start(&self) -> Result<Option<String>> {
        self.store.history_start()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<ResourceEvent> {
        self.store.subscribe_events()
    }

    /// Where one Session's transcript entries are read from: the Runtime Adapter its
    /// execution names, and the identity the Runtime delivered.
    pub fn transcript_source(&self, session: &Session) -> Result<TranscriptSource> {
        let identity = self.store.runtime_identity(&session.id)?;
        let runtime = session.execution.runtime.as_deref();
        Ok(match runtime.and_then(|name| self.registry.runtime(name)) {
            Some(adapter) => TranscriptSource::new(Some(adapter), identity),
            None => TranscriptSource::without_adapter(runtime, identity),
        })
    }

    pub fn record_runtime_identity(
        &self,
        session_id: &str,
        token: &str,
        identity: &RuntimeIdentity,
    ) -> std::result::Result<Session, SessionWriteError> {
        self.store
            .record_runtime_identity(session_id, token, identity)
    }

    /// The Telemetry one Session delivered for a range from `start`.
    pub fn telemetries(
        &self,
        session_id: &str,
        start: Option<&str>,
        holds: impl FnMut(&[Telemetry]) -> bool,
    ) -> Result<Vec<Telemetry>> {
        self.store.session_telemetries(session_id, start, holds)
    }

    /// Each Session that ran within the range, with the Telemetry it delivered.
    pub fn range_telemetries<H: FnMut(&[Telemetry]) -> bool>(
        &self,
        start: Option<&str>,
        end: Option<&str>,
        before: impl Fn(&Session) -> H,
    ) -> Result<Vec<(Session, Vec<Telemetry>)>> {
        self.store.range_telemetries(start, end, before)
    }

    /// Keeps one Collector request for the Session its token names.
    pub fn record_telemetry(
        &self,
        request_id: &str,
        token: &str,
        signal: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<TelemetryAdmission> {
        self.store
            .record_telemetry(request_id, token, signal, content_type, body)
    }

    /// The open Session a telemetry token names, if any.
    pub fn telemetry_token_session(&self, token: &str) -> Result<Option<String>> {
        self.store.telemetry_token_session(token)
    }
}

/// Whether a Session has a turn: a current `agent-runtime` one does.
fn has_turn(session: &Session) -> bool {
    session.end.is_none() && session.execution.execution_type == "agent-runtime"
}

fn set_turn(session: &mut Session, (turn, last_entry_at): TurnRead) {
    session.turn = Some(turn);
    session.last_entry_at = Some(last_entry_at);
}
