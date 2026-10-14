use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::service::MailboxError;
use crate::store::{MessageFilter, MessageWriteError, RequestIdentity};

use super::AppState;
use super::auth::{AdminCaller, Caller, RequesterCaller, SelfCaller, refused_token};
use super::error::{ApiError, error, internal, mailbox_error, message_error, message_write_error};
use super::query::{ApiJson, ApiQuery, MailboxQuery, MessageQuery, time_range};
use super::types::{
    MessageList, MessageQueuedList, MessageReadList, MessageResponse, MessageSummaryList,
    SendMessageRequest,
};

pub(super) async fn send_message(
    State(state): State<AppState>,
    RequesterCaller(auth): RequesterCaller,
    Path((workspace, actor)): Path<(String, String)>,
    ApiJson(request): ApiJson<SendMessageRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    let message = state
        .messages
        .send(
            RequestIdentity {
                requester: &auth.requester,
                session_id: auth.session_id.as_deref(),
            },
            auth.token.as_deref(),
            &workspace,
            &actor,
            request.label.as_deref(),
            &request.body,
        )
        .map_err(message_error)?;
    Ok(Json(MessageResponse { message }))
}

pub(super) async fn get_message(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path(message): Path<String>,
) -> Result<Json<MessageResponse>, ApiError> {
    let message = state
        .messages
        .get(&message)
        .map_err(internal)?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                &format!("Message '{message}' was not found"),
            )
        })?;
    if !auth.reads_message(&message.from, &message.to) {
        return Err(error(
            StatusCode::FORBIDDEN,
            "a Session token reads a Message from or to its own Actor, and with the grant \
             `read` one to an Actor of its Workspace",
        ));
    }
    Ok(Json(MessageResponse { message }))
}

pub(super) async fn delete_message(
    State(state): State<AppState>,
    _caller: AdminCaller,
    Path(message): Path<String>,
) -> Result<Json<MessageResponse>, ApiError> {
    let message = state
        .messages
        .delete(&message)
        .map_err(message_write_error)?;
    Ok(Json(MessageResponse { message }))
}

pub(super) async fn list_messages(
    State(state): State<AppState>,
    Caller(auth): Caller,
    ApiQuery(query): ApiQuery<MessageQuery>,
) -> Result<Json<MessageSummaryList>, ApiError> {
    let (start, end) = time_range(query.start.as_deref(), query.end.as_deref(), "Message")?;
    let mut messages = state
        .messages
        .summaries(
            &MessageFilter {
                workspace: match query.workspace.as_deref() {
                    Some(workspace) => auth
                        .require_workspace(workspace)
                        .map(|()| Some(workspace))?,
                    None => None,
                },
                from: query.from.as_deref(),
                to: query.to.as_deref(),
                label: query.label.as_deref(),
                waiting: query.waiting,
                start: start.as_deref(),
                end: end.as_deref(),
            },
            &state.tokens.secrets(),
        )
        .map_err(internal)?;
    messages.retain(|message| auth.reads_message(&message.from, &message.to));
    Ok(Json(MessageSummaryList { messages }))
}

pub(super) async fn get_mailbox(
    State(state): State<AppState>,
    caller: SelfCaller,
    ApiQuery(query): ApiQuery<MailboxQuery>,
) -> Result<Json<MessageList>, ApiError> {
    let messages = state
        .messages
        .mailbox(&caller.session_id, &caller.token, query.limit, query.wait)
        .await
        .map_err(|error_value| match error_value {
            // The Session closed while the read waited.
            MailboxError::Write(MessageWriteError::InvalidToken) => {
                refused_token(&state, &caller.token)
            }
            error_value => mailbox_error(error_value),
        })?;
    Ok(Json(MessageList { messages }))
}

pub(super) async fn queue_mailbox(
    State(state): State<AppState>,
    caller: SelfCaller,
    ApiJson(message_ids): ApiJson<Vec<String>>,
) -> Result<Json<MessageQueuedList>, ApiError> {
    let queued = state
        .messages
        .queue_mailbox(&caller.session_id, &caller.token, &message_ids)
        .map_err(message_write_error)?;
    Ok(Json(MessageQueuedList { queued }))
}

pub(super) async fn unqueue_mailbox(
    State(state): State<AppState>,
    caller: SelfCaller,
    Path(message): Path<String>,
) -> Result<Json<MessageResponse>, ApiError> {
    let message = state
        .messages
        .unqueue_mailbox(&caller.session_id, &caller.token, &message)
        .map_err(message_write_error)?;
    Ok(Json(MessageResponse { message }))
}

pub(super) async fn read_mailbox(
    State(state): State<AppState>,
    caller: SelfCaller,
    ApiJson(message_ids): ApiJson<Vec<String>>,
) -> Result<Json<MessageReadList>, ApiError> {
    let reads = state
        .messages
        .read_mailbox(&caller.session_id, &caller.token, &message_ids)
        .map_err(message_write_error)?;
    Ok(Json(MessageReadList { reads }))
}
