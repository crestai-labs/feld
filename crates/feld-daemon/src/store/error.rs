use feld_core::model::ActorKind;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceDeleteError {
    #[error("Workspace '{0}' was not found")]
    NotFound(String),
    #[error("Workspace '{0}' is the default Workspace and cannot be deleted")]
    Default(String),
    #[error("Workspace '{workspace}' has live Actors ({actors})")]
    LiveActors { workspace: String, actors: u64 },
    #[error(transparent)]
    Store(#[from] anyhow::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum ActorWriteError {
    #[error("Workspace '{0}' was not found")]
    WorkspaceNotFound(String),
    #[error("Actor '{actor}' was not found in Workspace '{workspace}'")]
    ActorNotFound { workspace: String, actor: String },
    #[error("Actor '{actor}' is already kind {existing}; kind cannot change to {requested}")]
    KindMismatch {
        actor: String,
        existing: ActorKind,
        requested: ActorKind,
    },
    #[error("Actor '{0}' is not a Job; only Jobs can have a Schedule")]
    NotJob(String),
    #[error("Actor '{0}' has no Schedule; creating one needs `cron` in `schedule`")]
    NoSchedule(String),
    #[error(
        "Actor '{actor}' has a running current Session '{session}'; stop it before the {change}"
    )]
    Running {
        actor: String,
        session: String,
        /// The change the running Session refuses: `delete` or `rename`.
        change: &'static str,
    },
    #[error("Actor '{name}' already exists in Workspace '{workspace}'")]
    NameTaken { workspace: String, name: String },
    #[error("Actor '{0}' changed while its launchSpec was being resolved; retry the update")]
    LaunchChanged(String),
    #[error("Actor '{0}' has grants; only the local token or a browser token changes it")]
    Granted(String),
    #[error("Actor '{0}' trusts its workdir; only the local token or a browser token changes it")]
    Trusted(String),
    #[error("Actor '{actor}' is kind {kind}, which the grants of this Session do not manage")]
    Unmanaged { actor: String, kind: ActorKind },
    #[error("invalid launchSpec: {0}")]
    InvalidLaunch(String),
    #[error("invalid Schedule: {0}")]
    InvalidSchedule(String),
    #[error(transparent)]
    Database(#[from] anyhow::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum SessionWriteError {
    #[error("Workspace '{0}' was not found")]
    WorkspaceNotFound(String),
    #[error("Actor '{actor}' was not found in Workspace '{workspace}'")]
    ActorNotFound { workspace: String, actor: String },
    #[error("Session '{0}' was not found")]
    SessionNotFound(String),
    #[error("Actor '{0}' is not a program Job")]
    NotProgramJob(String),
    #[error("Actor '{0}' is a Job; attach takes an agent Actor")]
    AttachToJob(String),
    #[error("Actor '{0}' changed while its launch was being prepared; retry the start")]
    LaunchChanged(String),
    #[error(
        "Actor '{actor}' already has current Session '{session}' ({observation}); refresh its observation before retrying when needed"
    )]
    Admission {
        actor: String,
        session: String,
        observation: String,
    },
    #[error("Session '{0}' is already closed")]
    SessionClosed(String),
    #[error("Session '{0}' is not owned by Feld")]
    NotOwned(String),
    #[error("Actor '{0}' has no running current Session to restart")]
    NotRunning(String),
    #[error("Runtime is already attached to Actor '{actor}' in Session '{session}'")]
    RuntimeAlreadyAttached { actor: String, session: String },
    #[error("Session token is no longer valid")]
    InvalidToken,
    #[error("Session '{0}' is not an agent-runtime Session")]
    NotAgentRuntime(String),
    #[error(
        "Session '{session}' has recorded Runtime session id '{recorded_session_id}' with transcript '{recorded_transcript_path}'; the request names session id '{requested_session_id}' with transcript '{requested_transcript_path}'"
    )]
    RuntimeIdentityConflict {
        session: String,
        recorded_session_id: String,
        recorded_transcript_path: String,
        requested_session_id: String,
        requested_transcript_path: String,
    },
    #[error(transparent)]
    Database(#[from] anyhow::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum MessageWriteError {
    #[error("Actor '{actor}' was not found in Workspace '{workspace}'")]
    ActorNotFound { workspace: String, actor: String },
    #[error("Message '{0}' was not found")]
    MessageNotFound(String),
    #[error("Message '{0}' does not belong to this Actor mailbox")]
    WrongMailbox(String),
    #[error("Message '{0}' has a read, so it cannot be deleted")]
    Read(String),
    #[error("Message '{0}' is queued in its Runtime, so it cannot be deleted")]
    Queued(String),
    #[error("Session token is no longer valid")]
    InvalidToken,
    #[error(
        "a Session token sends only within its Workspace; sending to Workspace '{0}' requires the local token or a browser token"
    )]
    OtherWorkspace(String),
    #[error("Actor '{actor}' does not accept Messages from Actor '{from}'")]
    NotAccepted { actor: String, from: String },
    #[error(transparent)]
    Database(#[from] anyhow::Error),
}
