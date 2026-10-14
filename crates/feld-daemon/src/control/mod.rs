use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, RwLock};

use tokio::sync::Mutex;

use crate::store::{SessionWriteError, Store};
use feld_core::home::FeldHome;
use feld_core::model::{ScheduleSkip, Session};
use feld_core::plugin::{
    ExecutionProvider, PluginKind, Plugins, Registry, RuntimeAdapter, not_installed,
};

mod attach;
mod capture;
mod lifecycle;
mod observe;
mod start;

#[cfg(test)]
mod tests;

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error(transparent)]
    Store(#[from] SessionWriteError),
    #[error("Actor '{actor}' was not found in Workspace '{workspace}'")]
    ActorNotFound { workspace: String, actor: String },
    #[error("Actor '{0}' has no supported launchSpec")]
    NotProgramJob(String),
    #[error("{}", not_installed(*kind, name))]
    NotInstalled { kind: PluginKind, name: String },
    #[error("this milestone requires node 'local'")]
    NodeNotLocal,
    #[error("{provider} answered observe with {}", observe_answer(*answered))]
    ObserveMismatch { provider: String, answered: usize },
    /// The Provider could not place a launch, or an attached Placement is not one Feld can
    /// take over.
    #[error("{0}")]
    Placement(String),
    #[error("Actor declaration kept changing while launch was prepared")]
    LaunchKeptChanging,
    /// The product a shipped Plugin connects is not on the daemon's `PATH`, which a Session
    /// runs with.
    #[error("`{command}` is not on the PATH of the Feld daemon")]
    ProductMissing { command: String, next: String },
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

#[derive(Clone, Debug)]
pub struct LifecycleResult {
    pub session: Session,
    pub already_requested: bool,
}

#[derive(Clone, Debug)]
pub struct AttachResult {
    pub session: Session,
    pub token: String,
    pub reattached: bool,
}

#[derive(Clone, Debug)]
pub struct RestartResult {
    /// The current Session the restart asked to stop, as it reads after the stop.
    pub stopped: Session,
    /// The next generation, when the stopped Session closed within the request.
    pub started: Option<StartedSession>,
}

#[derive(Clone, Debug)]
pub struct StartedSession {
    pub session: Session,
    /// The previous generation this start replaced: every generation after the first
    /// replaces one.
    pub replaced: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ScheduleTickResult {
    Started(Box<StartedSession>),
    Skipped(ScheduleSkip),
    Already,
    Stale,
    Cancelled,
}

pub struct SessionController {
    store: Arc<Store>,
    /// The Runtime Adapters and Execution Providers this daemon calls.
    plugins: Arc<dyn Plugins>,
    home: FeldHome,
    api: String,
    collector: Arc<RwLock<Option<String>>>,
    operation: Arc<Mutex<()>>,
    /// How long each open `program` Session's captured output was at the previous
    /// poll, so that a poll only notifies readers when the cursor actually moved.
    console_lengths: StdMutex<HashMap<String, u64>>,
}

impl SessionController {
    /// A controller that reads the installed Plugins itself, for the parts of Feld that
    /// run without a daemon's Collector.
    pub fn new(store: Arc<Store>, home: FeldHome, api: String) -> Self {
        let registry = Arc::new(Registry::load(&home, Some(&api)));
        Self::with_collector_state(store, registry, home, api, Arc::new(RwLock::new(None)))
    }

    pub fn with_collector_state(
        store: Arc<Store>,
        plugins: Arc<dyn Plugins>,
        home: FeldHome,
        api: String,
        collector: Arc<RwLock<Option<String>>>,
    ) -> Self {
        Self {
            store,
            plugins,
            home,
            api,
            collector,
            operation: Arc::default(),
            console_lengths: StdMutex::new(HashMap::new()),
        }
    }

    /// Runs `operation` under the operation lock on a blocking thread, since the Plugins it
    /// calls block until their process answers. The lock moves with the operation, so a
    /// caller that stops waiting leaves it held until the operation ends.
    async fn operate<T, E>(
        self: &Arc<Self>,
        operation: impl FnOnce(&Self) -> std::result::Result<T, E> + Send + 'static,
    ) -> std::result::Result<T, E>
    where
        T: Send + 'static,
        E: From<anyhow::Error> + Send + 'static,
    {
        let guard = Arc::clone(&self.operation).lock_owned().await;
        let controller = Arc::clone(self);
        let operation = tokio::task::spawn_blocking(move || {
            let _guard = guard;
            operation(&controller)
        });
        match operation.await {
            Ok(result) => result,
            Err(error) => match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(error) => Err(anyhow::Error::new(error).into()),
            },
        }
    }

    fn collector_endpoint(&self) -> Option<String> {
        self.collector
            .read()
            .expect("Collector status lock poisoned")
            .clone()
    }

    fn provider(
        &self,
        name: &str,
    ) -> std::result::Result<Arc<dyn ExecutionProvider>, ControlError> {
        self.plugins
            .provider(name)
            .ok_or_else(|| ControlError::NotInstalled {
                kind: PluginKind::Provider,
                name: name.to_owned(),
            })
    }

    fn runtime(&self, name: &str) -> std::result::Result<Arc<dyn RuntimeAdapter>, ControlError> {
        self.plugins
            .runtime(name)
            .ok_or_else(|| ControlError::NotInstalled {
                kind: PluginKind::Runtime,
                name: name.to_owned(),
            })
    }
}

/// What a Provider's observe answer held, when it was not one result per Placement.
fn observe_answer(answered: usize) -> &'static str {
    if answered == 0 {
        "nothing"
    } else {
        "a different number of results"
    }
}
