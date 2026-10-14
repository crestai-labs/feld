use std::path::Path;
use std::sync::{Mutex, RwLock};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::console::Secrets;

mod actor;
mod admission;
mod browser;
mod error;
mod lifecycle;
mod message;
mod record;
mod schedule;
pub mod schema;
mod session;
mod sql;
mod telemetry;
mod token;
mod workspace;

pub use actor::{ActorPatch, LaunchUpdate, Writer};
pub use admission::{
    AttachAdmission, AttachedExecution, PendingSession, RequestIdentity, ScheduledAdmission,
    ScheduledAdmissionRequest,
};
pub use browser::{BROWSER_IDLE, BROWSER_LIFETIME, NewBrowser};
pub use error::{ActorWriteError, MessageWriteError, SessionWriteError, WorkspaceDeleteError};
pub use lifecycle::{AcceptedLifecycleAction, ObservationUpdate, PendingRestart};
pub use message::MessageFilter;
pub use record::RecordFilter;
pub use schedule::ScheduledActor;
pub use session::{SessionFilter, session_overlaps};
pub use telemetry::{
    CLOSED_SESSION_TELEMETRY_RETENTION, SESSION_TELEMETRY_LIMIT, TELEMETRY_ROW_OVERHEAD, Telemetry,
    TelemetryAdmission,
};
pub use token::AuthenticatedSession;
pub use workspace::{CreateWorkspace, Workspace};

/// The inputs of the Store's writes that the model defines.
pub use feld_core::model::{LifecycleActionKind, ScheduleDeclaration, SchedulePatch};

use schema::migrate;

pub struct Store {
    /// The SQLite connection, until [`Store::close`] closes it.
    connection: Mutex<Option<Connection>>,
    events: broadcast::Sender<ResourceEvent>,
    /// Every token value the store holds, kept in memory for the redaction of responses.
    secrets: RwLock<Secrets>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceEvent {
    pub resource: String,
    pub id: String,
    #[serde(skip_serializing)]
    pub workspace: Option<String>,
}

/// The resource events a write sends once its transaction has committed.
#[derive(Default)]
struct Notices(Vec<ResourceEvent>);

impl Notices {
    fn push(&mut self, resource: &str, id: &str, workspace: Option<&str>) {
        self.0.push(ResourceEvent {
            resource: resource.to_owned(),
            id: id.to_owned(),
            workspace: workspace.map(str::to_owned),
        });
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StoreCounts {
    pub actors: u64,
    pub running_sessions: u64,
    pub schedules: u64,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut connection = Connection::open(path)
            .with_context(|| format!("open SQLite store {}", path.display()))?;
        connection
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .context("configure SQLite store")?;
        migrate(&mut connection)?;
        let (events, _) = broadcast::channel(256);
        let store = Self {
            connection: Mutex::new(Some(connection)),
            events,
            secrets: RwLock::default(),
        };
        *store.secrets.write().expect("secrets lock poisoned") =
            store.token_secrets()?.into_iter().collect();
        let _ = store.create_workspace(workspace::DEFAULT_WORKSPACE)?;
        Ok(store)
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<ResourceEvent> {
        self.events.subscribe()
    }

    /// Sends an event about a resource outside the store, such as a Session's output.
    pub fn notify(&self, resource: &str, id: &str, workspace: Option<&str>) {
        let mut notices = Notices::default();
        notices.push(resource, id, workspace);
        self.send(notices);
    }

    fn send(&self, notices: Notices) {
        for event in notices.0 {
            let _ = self.events.send(event);
        }
    }

    /// Closes the SQLite connection, which checkpoints the WAL and removes its files. A
    /// read or a write after it fails.
    pub fn close(&self) -> Result<()> {
        let connection = self.connection.lock().expect("store mutex poisoned").take();
        match connection {
            Some(connection) => connection
                .close()
                .map_err(|(_, error)| error)
                .context("close SQLite store"),
            None => Ok(()),
        }
    }

    fn read<T, E: From<anyhow::Error>>(
        &self,
        read: impl FnOnce(&Connection) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let connection = self.connection.lock().expect("store mutex poisoned");
        read(connection.as_ref().context("the store is closed")?)
    }

    /// Runs `write` in one immediate transaction, which commits when `write` succeeds, and
    /// sends the events `write` gathered only after the commit.
    fn write<T, E: From<anyhow::Error>>(
        &self,
        name: &str,
        write: impl FnOnce(&Transaction<'_>, &mut Notices) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let mut notices = Notices::default();
        let value = {
            let mut connection = self.connection.lock().expect("store mutex poisoned");
            let transaction = connection
                .as_mut()
                .context("the store is closed")?
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .with_context(|| format!("begin {name}"))?;
            let value = write(&transaction, &mut notices)?;
            transaction
                .commit()
                .with_context(|| format!("commit {name}"))?;
            value
        };
        self.send(notices);
        Ok(value)
    }

    pub fn counts(&self) -> Result<StoreCounts> {
        self.read(|connection| {
            let count = |statement: &str, context: &'static str| -> Result<u64> {
                connection
                    .query_row(statement, [], |row| row.get(0))
                    .context(context)
            };
            Ok(StoreCounts {
                actors: count(
                    "SELECT COUNT(*) FROM actors WHERE deleted_at IS NULL",
                    "count Actors",
                )?,
                running_sessions: count(
                    "SELECT COUNT(*) FROM sessions WHERE end IS NULL",
                    "count running Sessions",
                )?,
                schedules: count(
                    "SELECT COUNT(*) FROM actors
                     WHERE schedule_cron IS NOT NULL AND deleted_at IS NULL",
                    "count Schedules",
                )?,
            })
        })
    }

    /// The `at` of the oldest record, where the kept history begins.
    pub fn history_start(&self) -> Result<Option<String>> {
        self.read(|connection| {
            connection
                .query_row(
                    "SELECT at FROM records ORDER BY sequence LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .context("read the oldest record")
        })
    }
}
