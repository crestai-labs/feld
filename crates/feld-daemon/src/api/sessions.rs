use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::console::{
    OutputPage, Stream, SubAgentSession, TranscriptPage, Window, read_output_before,
    read_output_limited, read_output_tail, read_transcript_view, split_sub_agent_session_id,
};
use crate::metrics::{
    ContextWindowBefore, InstrumentMaps, MetricsQuery, SessionMetricsResponse, session_metrics,
    sub_agent_metrics,
};
use crate::store::{LifecycleActionKind, RecordFilter, SessionFilter};
use feld_core::model::{ActorKind, Grant, RecordKind, Session};

use super::AppState;
use super::auth::{AuthContext, Caller, RequesterCaller, SelfCaller};
use super::error::{ApiError, control_error, error, internal};
use super::query::{
    ApiQuery, CursorQuery, RecordQuery, SessionMetricsQuery, SessionQuery, TranscriptQuery,
    TranscriptView, normalize_timestamp, time_range,
};
use super::types::{
    LifecycleResponse, RecordList, SessionList, SessionResponse, SubAgentSessionResponse,
};

pub(super) async fn stop_session(
    State(state): State<AppState>,
    RequesterCaller(auth): RequesterCaller,
    Path(session): Path<String>,
) -> Result<Json<LifecycleResponse>, ApiError> {
    lifecycle_action(state, auth, session, LifecycleActionKind::Stop).await
}

pub(super) async fn kill_session(
    State(state): State<AppState>,
    RequesterCaller(auth): RequesterCaller,
    Path(session): Path<String>,
) -> Result<Json<LifecycleResponse>, ApiError> {
    lifecycle_action(state, auth, session, LifecycleActionKind::Kill).await
}

async fn lifecycle_action(
    state: AppState,
    auth: AuthContext,
    session: String,
    action: LifecycleActionKind,
) -> Result<Json<LifecycleResponse>, ApiError> {
    // A Session that is not found is left to the controller to answer 404.
    if let Some(found) = state.sessions.get(&session).map_err(internal)? {
        auth.require_stop(&found)?;
    }
    let response = state
        .controller
        .lifecycle_action(&session, action, auth.session_id.as_deref())
        .await
        .map_err(control_error)?;
    let mut session = response.session;
    auth.conceal_session(&mut session);
    Ok(Json(LifecycleResponse {
        session,
        already_requested: response.already_requested,
    }))
}

pub(super) async fn observe_session(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path(session): Path<String>,
) -> Result<Json<SessionResponse>, ApiError> {
    if let Some(found) = state.sessions.get(&session).map_err(internal)? {
        auth.require_workspace(&found.workspace)?;
    }
    let mut session = state
        .controller
        .observe(&session)
        .await
        .map_err(control_error)?;
    auth.conceal_session(&mut session);
    Ok(Json(SessionResponse {
        session,
        sub_agents: Vec::new(),
    }))
}

pub(super) async fn list_sessions(
    State(state): State<AppState>,
    Caller(auth): Caller,
    ApiQuery(query): ApiQuery<SessionQuery>,
) -> Result<Json<SessionList>, ApiError> {
    let workspace = auth.scope(query.workspace.as_deref())?;
    let kind = query
        .kind
        .map(|kind| kind.parse::<ActorKind>())
        .transpose()
        .map_err(|message| error(StatusCode::BAD_REQUEST, &message))?;
    let (start, end) = time_range(query.start.as_deref(), query.end.as_deref(), "Session")?;
    let sessions = state
        .sessions
        .query(&SessionFilter {
            workspace,
            actor: query.actor.as_deref(),
            kind,
            current: query.current,
            start: start.as_deref(),
            end: end.as_deref(),
        })
        .map_err(internal)?;
    let mut sessions = derive_turns(&state, sessions).await?;
    let mut sub_agents = if query.sub_agents {
        Some(listed_sub_agents(&state, sessions.clone()).await?)
    } else {
        None
    };
    sessions
        .iter_mut()
        .for_each(|session| auth.conceal_session(session));
    sub_agents
        .iter_mut()
        .flatten()
        .for_each(|sub_agent| auth.conceal_sub_agent(sub_agent));
    Ok(Json(SessionList {
        sessions,
        sub_agents,
    }))
}

/// The Sessions with the turn of each current `agent-runtime` Session filled in.
async fn derive_turns(
    state: &AppState,
    mut sessions: Vec<Session>,
) -> Result<Vec<Session>, ApiError> {
    let service = state.sessions.clone();
    tokio::task::spawn_blocking(move || {
        service.derive_turns(&mut sessions)?;
        anyhow::Ok(sessions)
    })
    .await
    .map_err(|error| internal(anyhow::anyhow!(error)))?
    .map_err(internal)
}

/// The Sub Agent Sessions the transcript entries of the listed Sessions show, in the
/// listing's order.
async fn listed_sub_agents(
    state: &AppState,
    sessions: Vec<Session>,
) -> Result<Vec<SubAgentSession>, ApiError> {
    let service = state.sessions.clone();
    tokio::task::spawn_blocking(move || {
        let mut sub_agents = Vec::new();
        for session in &sessions {
            sub_agents.extend(service.sub_agents(session)?);
        }
        anyhow::Ok(sub_agents)
    })
    .await
    .map_err(|error| internal(anyhow::anyhow!(error)))?
    .map_err(internal)
}

pub(super) async fn get_session(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path(session): Path<String>,
) -> Result<Response, ApiError> {
    // A Sub Agent Session is read from the entries of its parent Session.
    let Some((parent, _)) = split_sub_agent_session_id(&session) else {
        let mut response = session_response(&state, &session).await?;
        auth.require_workspace(&response.session.workspace)?;
        auth.conceal_session(&mut response.session);
        response
            .sub_agents
            .iter_mut()
            .for_each(|sub_agent| auth.conceal_sub_agent(sub_agent));
        return Ok(Json(response).into_response());
    };
    let not_found = || {
        error(
            StatusCode::NOT_FOUND,
            &format!("Sub Agent Session '{session}' was not found"),
        )
    };
    let mut sub_agent_session = session_response(&state, parent)
        .await
        .map_err(|error| match error.status {
            StatusCode::NOT_FOUND => not_found(),
            _ => error,
        })?
        .sub_agents
        .into_iter()
        .find(|sub_agent| sub_agent.id == session)
        .ok_or_else(not_found)?;
    auth.require_workspace(&sub_agent_session.workspace)?;
    auth.conceal_sub_agent(&mut sub_agent_session);
    Ok(Json(SubAgentSessionResponse { sub_agent_session }).into_response())
}

pub(super) async fn get_self(
    State(state): State<AppState>,
    caller: SelfCaller,
) -> Result<Json<SessionResponse>, ApiError> {
    session_response(&state, &caller.session_id).await.map(Json)
}

/// One Session as `get` reads it, with its turn and the Sub Agent Sessions its transcript
/// entries show.
async fn session_response(state: &AppState, session: &str) -> Result<SessionResponse, ApiError> {
    let mut session = state
        .sessions
        .get(session)
        .map_err(internal)?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                &format!("Session '{session}' was not found"),
            )
        })?;
    let service = state.sessions.clone();
    let (session, sub_agents) = tokio::task::spawn_blocking(move || {
        let sub_agents = service.session_read(&mut session)?;
        anyhow::Ok((session, sub_agents))
    })
    .await
    .map_err(|error| internal(anyhow::anyhow!(error)))?
    .map_err(internal)?;
    Ok(SessionResponse {
        session,
        sub_agents,
    })
}

pub(super) async fn get_transcript(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path(session_id): Path<String>,
    ApiQuery(query): ApiQuery<TranscriptQuery>,
) -> Result<Json<TranscriptPage>, ApiError> {
    if query.before.is_some() && (query.after.is_some() || query.tail) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "before cannot be combined with after or tail",
        ));
    }
    let limit = query
        .limit
        .map(|limit| limit.min(1_000))
        .unwrap_or(usize::MAX);
    let window = match (query.before, query.tail, query.after) {
        (Some(before), _, _) => Window::Before { before, limit },
        (None, true, None) => Window::Tail { limit },
        (None, _, after) => Window::After { after, limit },
    };
    // A Sub Agent Session is read by the id its parent's records give it, and its records
    // come from the same Runtime Adapter read as its parent's.
    let (parent_id, stream) = match split_sub_agent_session_id(&session_id) {
        Some((parent, sub_agent)) => (parent.to_owned(), Stream::SubAgent(sub_agent.to_owned())),
        None => (session_id.clone(), Stream::Parent),
    };
    let session = state
        .sessions
        .get(&parent_id)
        .map_err(internal)?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                &format!("Session '{session_id}' was not found"),
            )
        })?;
    auth.require_read(&session)?;
    if session.execution.execution_type != "agent-runtime" {
        return Err(error(
            StatusCode::CONFLICT,
            "program Sessions have output, not a transcript",
        ));
    }
    let source = state
        .sessions
        .transcript_source(&session)
        .map_err(internal)?;
    let id = session_id.clone();
    let conversation = query.view == TranscriptView::Conversation;
    let page = tokio::task::spawn_blocking(move || {
        read_transcript_view(&source, &id, &stream, window, conversation)
    })
    .await
    .map_err(|error| internal(anyhow::anyhow!(error)))?
    .map_err(|unknown| error(StatusCode::BAD_REQUEST, &unknown.to_string()))?;
    Ok(Json(page))
}

pub(super) async fn get_output(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path(session_id): Path<String>,
    ApiQuery(query): ApiQuery<CursorQuery>,
) -> Result<Json<OutputPage>, ApiError> {
    let session = state
        .sessions
        .get(&session_id)
        .map_err(internal)?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                &format!("Session '{session_id}' was not found"),
            )
        })?;
    auth.require_read(&session)?;
    if session.execution.execution_type != "program" {
        return Err(error(
            StatusCode::CONFLICT,
            "agent Runtime Sessions have a transcript, not program output",
        ));
    }
    let path = state.home.session_output_path(&session_id);
    let id = session_id.clone();
    let after = query.after.unwrap_or(0);
    let before = query.before;
    if before.is_some() && (query.after.is_some() || query.tail) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "before cannot be combined with after or tail",
        ));
    }
    let limit = query
        .limit
        .map(|limit| limit.min(1_000))
        .unwrap_or(usize::MAX);
    let page = tokio::task::spawn_blocking(move || {
        if let Some(before) = before {
            read_output_before(&id, &path, before, limit)
        } else if query.tail && after == 0 {
            read_output_tail(&id, &path, limit)
        } else {
            read_output_limited(&id, &path, after, limit)
        }
    })
    .await
    .map_err(|error| internal(anyhow::anyhow!(error)))?
    .map_err(internal)?;
    Ok(Json(page))
}

pub(super) async fn get_session_metrics(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path(session_id): Path<String>,
    ApiQuery(query): ApiQuery<SessionMetricsQuery>,
) -> Result<Json<SessionMetricsResponse>, ApiError> {
    let start = normalize_timestamp(query.start.as_deref(), "start")?;
    let end = normalize_timestamp(query.end.as_deref(), "end")?;
    // A Sub Agent Session is read by the id its parent's records give it, and its metrics
    // are its parent's.
    let parent = split_sub_agent_session_id(&session_id).map(|(parent, _)| parent);
    let session = state
        .sessions
        .get(parent.unwrap_or(&session_id))
        .map_err(internal)?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                &format!("Session '{session_id}' was not found"),
            )
        })?;
    auth.require_read(&session)?;
    let sub_agent = parent.is_some();
    let response = tokio::task::spawn_blocking(move || {
        let metrics = MetricsQuery {
            start: start.as_deref(),
            end: end.as_deref(),
            bucket: query.bucket,
            metric: query.metric.as_deref(),
        };
        if sub_agent {
            sub_agent_metrics(&session_id, &metrics)
        } else {
            let instruments = InstrumentMaps::read(&state.registry).of(&session);
            let telemetries = state
                .sessions
                .telemetries(&session_id, metrics.start, {
                    let mut before = ContextWindowBefore::new(instruments.clone());
                    move |page| before.holds(page)
                })
                .map_err(internal)?;
            session_metrics(&session, &instruments, &telemetries, &metrics)
        }
        .map_err(|cause| error(StatusCode::BAD_REQUEST, &cause.to_string()))
    })
    .await
    .map_err(|error| internal(anyhow::anyhow!(error)))??;
    Ok(Json(response))
}

pub(super) async fn list_records(
    State(state): State<AppState>,
    Caller(auth): Caller,
    ApiQuery(query): ApiQuery<RecordQuery>,
) -> Result<Json<RecordList>, ApiError> {
    let workspace = auth.scope(query.workspace.as_deref())?;
    // Without `read`, a Session token reads the records of its own Actor.
    let actor = match (query.actor.as_deref(), auth.own_name()) {
        (Some(actor), Some(own)) if actor != own => {
            auth.require_grant(workspace.unwrap_or_default(), Grant::Read)?;
            Some(actor)
        }
        (None, Some(own)) if !auth.has(Grant::Read) => Some(own),
        (actor, _) => actor,
    };
    let kind = query
        .kind
        .map(|kind| kind.parse::<RecordKind>())
        .transpose()
        .map_err(|message| error(StatusCode::BAD_REQUEST, &message))?;
    let (start, end) = time_range(query.start.as_deref(), query.end.as_deref(), "Record")?;
    let after = query.after.unwrap_or(0);
    let records = state
        .sessions
        .records(&RecordFilter {
            workspace,
            actor,
            session: query.session.as_deref(),
            kind,
            start: start.as_deref(),
            end: end.as_deref(),
            after,
            live_actors: false,
        })
        .map_err(internal)?;
    let next_after = records.last().map_or(after, |record| record.sequence);
    Ok(Json(RecordList {
        records,
        next_after,
    }))
}
