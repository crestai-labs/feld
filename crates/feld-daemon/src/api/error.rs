use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::control::ControlError;
use crate::service::{ActorError, MailboxError, MessageError};
use crate::store::{ActorWriteError, MessageWriteError, SessionWriteError};

pub(super) struct ApiError {
    pub(super) status: StatusCode,
    pub(super) message: String,
    /// The step a person takes next, for the refusals that can tell one.
    pub(super) next: Option<String>,
}

impl ApiError {
    pub(super) fn with_next(self, next: impl Into<String>) -> Self {
        Self {
            next: Some(next.into()),
            ..self
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = match self.next {
            Some(next) => json!({ "message": self.message, "next": next }),
            None => json!({ "message": self.message }),
        };
        (self.status, Json(body)).into_response()
    }
}

pub(super) fn error(status: StatusCode, message: &str) -> ApiError {
    ApiError {
        status,
        message: message.to_owned(),
        next: None,
    }
}

/// The refusal of a Workspace that is not there.
pub(super) fn workspace_not_found(workspace: &str) -> ApiError {
    error(
        StatusCode::NOT_FOUND,
        &format!("Workspace '{workspace}' was not found"),
    )
    .with_next(format!(
        "feld workspace create {workspace}, or choose one of `feld workspace list`"
    ))
}

pub(super) fn actor_write_error(error_value: ActorWriteError) -> ApiError {
    let status = match error_value {
        ActorWriteError::WorkspaceNotFound(workspace) => return workspace_not_found(&workspace),
        ActorWriteError::ActorNotFound { .. } => StatusCode::NOT_FOUND,
        ActorWriteError::Granted(_)
        | ActorWriteError::Trusted(_)
        | ActorWriteError::Unmanaged { .. } => StatusCode::FORBIDDEN,
        ActorWriteError::KindMismatch { .. }
        | ActorWriteError::Running { .. }
        | ActorWriteError::NameTaken { .. }
        | ActorWriteError::LaunchChanged(_) => StatusCode::CONFLICT,
        ActorWriteError::NotJob(_)
        | ActorWriteError::NoSchedule(_)
        | ActorWriteError::InvalidLaunch(_)
        | ActorWriteError::InvalidSchedule(_) => StatusCode::BAD_REQUEST,
        ActorWriteError::Database(cause) => return internal(cause),
    };
    error(status, &error_value.to_string())
}

pub(super) fn actor_error(error_value: ActorError) -> ApiError {
    match error_value {
        ActorError::Invalid(message) => error(StatusCode::BAD_REQUEST, &message),
        ActorError::WorkspaceNotFound(workspace) => workspace_not_found(&workspace),
        ActorError::Write(write) => actor_write_error(write),
        ActorError::Internal(cause) => internal(cause),
    }
}

pub(super) fn control_error(error_value: ControlError) -> ApiError {
    let status = match error_value {
        ControlError::Store(store) => return session_write_error(store),
        ControlError::ProductMissing { ref next, .. } => {
            return error(StatusCode::CONFLICT, &error_value.to_string()).with_next(next.clone());
        }
        ControlError::ActorNotFound { .. } => StatusCode::NOT_FOUND,
        ControlError::NotProgramJob(_)
        | ControlError::NotInstalled { .. }
        | ControlError::NodeNotLocal
        | ControlError::ObserveMismatch { .. }
        | ControlError::Placement(_)
        | ControlError::LaunchKeptChanging => StatusCode::CONFLICT,
        ControlError::Internal(cause) => return internal(cause),
    };
    error(status, &error_value.to_string())
}

pub(super) fn session_write_error(error_value: SessionWriteError) -> ApiError {
    match error_value {
        SessionWriteError::WorkspaceNotFound(workspace) => workspace_not_found(&workspace),
        SessionWriteError::Database(cause) => internal(cause),
        error_value => error(session_write_status(&error_value), &error_value.to_string()),
    }
}

fn session_write_status(error_value: &SessionWriteError) -> StatusCode {
    match error_value {
        SessionWriteError::WorkspaceNotFound(_)
        | SessionWriteError::SessionNotFound(_)
        | SessionWriteError::ActorNotFound { .. } => StatusCode::NOT_FOUND,
        SessionWriteError::SessionClosed(_)
        | SessionWriteError::Admission { .. }
        | SessionWriteError::RuntimeAlreadyAttached { .. }
        | SessionWriteError::NotOwned(_)
        | SessionWriteError::NotRunning(_)
        | SessionWriteError::NotProgramJob(_)
        | SessionWriteError::AttachToJob(_)
        | SessionWriteError::RuntimeIdentityConflict { .. }
        | SessionWriteError::LaunchChanged(_) => StatusCode::CONFLICT,
        SessionWriteError::InvalidToken => StatusCode::UNAUTHORIZED,
        SessionWriteError::NotAgentRuntime(_) => StatusCode::CONFLICT,
        SessionWriteError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(super) fn message_write_error(error_value: MessageWriteError) -> ApiError {
    let status = match error_value {
        MessageWriteError::ActorNotFound { .. } | MessageWriteError::MessageNotFound(_) => {
            StatusCode::NOT_FOUND
        }
        MessageWriteError::WrongMailbox(_)
        | MessageWriteError::OtherWorkspace(_)
        | MessageWriteError::NotAccepted { .. } => StatusCode::FORBIDDEN,
        MessageWriteError::Read(_) | MessageWriteError::Queued(_) => StatusCode::CONFLICT,
        MessageWriteError::InvalidToken => StatusCode::UNAUTHORIZED,
        MessageWriteError::Database(cause) => return internal(cause),
    };
    error(status, &error_value.to_string())
}

pub(super) fn message_error(error_value: MessageError) -> ApiError {
    match error_value {
        MessageError::EmptyBody | MessageError::InvalidLabel => {
            error(StatusCode::BAD_REQUEST, &error_value.to_string())
        }
        MessageError::Write(write) => message_write_error(write),
    }
}

pub(super) fn mailbox_error(error_value: MailboxError) -> ApiError {
    match error_value {
        MailboxError::Write(write) => message_write_error(write),
        MailboxError::ShuttingDown => {
            error(StatusCode::SERVICE_UNAVAILABLE, &error_value.to_string())
        }
    }
}

/// A failure of the daemon itself: the whole cause chain goes into the response and the
/// daemon log.
pub(super) fn internal(error_value: anyhow::Error) -> ApiError {
    let message = format!("the daemon failed: {error_value:#}");
    eprintln!("Feld API: {message}");
    error(StatusCode::INTERNAL_SERVER_ERROR, &message)
}

#[cfg(test)]
mod tests {
    use anyhow::Context;

    use super::*;

    #[test]
    fn internal_keeps_the_cause_chain() {
        let error_value = Err::<(), _>(anyhow::anyhow!("database is locked"))
            .context("read workspaces")
            .unwrap_err();
        let api_error = internal(error_value);
        assert_eq!(api_error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            api_error.message,
            "the daemon failed: read workspaces: database is locked"
        );
    }
}
