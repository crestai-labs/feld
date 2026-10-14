use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::service::{ActorDeclaration, ScheduleRequest};
use feld_core::id::valid_key;
use feld_core::model::{Actor, ActorKind, Grant, SchedulePatch};
use feld_core::plugin::{Plugins, provider_not_installed, runtime_not_installed};

use super::AppState;
use super::auth::{AdminCaller, AuthContext, Caller, RequesterCaller};
use super::error::{ApiError, actor_error, actor_write_error, control_error, error, internal};
use super::query::{ActorQuery, ApiJson, ApiQuery};
use super::types::{
    ActorList, ActorResponse, AttachActorRequest, AttachResponse, CreateActorRequest,
    RestartResponse, StartResponse, UpdateActorRequest,
};

pub(super) async fn list_actors(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path(workspace): Path<String>,
    ApiQuery(query): ApiQuery<ActorQuery>,
) -> Result<Json<ActorList>, ApiError> {
    auth.require_workspace(&workspace)?;
    let actors = state
        .actors
        .list(&workspace, query.kind.as_deref(), query.include_deleted)
        .map_err(actor_error)?;
    let mut actors = derive_turns(&state, actors).await?;
    actors
        .iter_mut()
        .for_each(|actor| auth.conceal_actor(actor));
    Ok(Json(ActorList { workspace, actors }))
}

pub(super) async fn create_actor(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path(workspace): Path<String>,
    ApiJson(request): ApiJson<CreateActorRequest>,
) -> Result<Json<ActorResponse>, ApiError> {
    if request.grants.is_some() {
        auth.require_admin("`grants`")?;
    }
    if request.launch_spec.trusts_workdir() {
        auth.require_admin("`trustWorkdir`")?;
    }
    authorize_manage(&state, &auth, &workspace, &request.name, Some(request.kind))?;
    // The Execution Provider resolves the launchSpec, which blocks until its process answers.
    let actors = state.actors.clone();
    let writer = auth.writer();
    let mut actor = tokio::task::spawn_blocking(move || {
        actors.create(
            &workspace,
            &request.name,
            ActorDeclaration {
                kind: request.kind,
                launch: &request.launch_spec,
                schedule: request.schedule.map(|schedule| ScheduleRequest {
                    cron: schedule.cron,
                    timezone: schedule.timezone,
                }),
                grants: request.grants.as_ref(),
                accept_from: request.accept_from.as_ref(),
                writer,
            },
            request.caller.as_ref(),
        )
    })
    .await
    .map_err(|error| internal(anyhow::anyhow!(error)))?
    .map_err(actor_error)?;
    auth.conceal_actor(&mut actor);
    Ok(Json(ActorResponse { actor }))
}

pub(super) async fn get_actor(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path((workspace, actor)): Path<(String, String)>,
) -> Result<Json<ActorResponse>, ApiError> {
    auth.require_workspace(&workspace)?;
    let actor = state
        .actors
        .get(&workspace, &actor)
        .map_err(internal)?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                &format!("Actor '{actor}' was not found in Workspace '{workspace}'"),
            )
        })?;
    let mut actor = derive_turns(&state, vec![actor]).await?.remove(0);
    auth.conceal_actor(&mut actor);
    Ok(Json(ActorResponse { actor }))
}

pub(super) async fn get_actor_by_id(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path(actor): Path<String>,
) -> Result<Json<ActorResponse>, ApiError> {
    let actor = state
        .actors
        .get_by_id(&actor)
        .map_err(internal)?
        .ok_or_else(|| {
            error(
                StatusCode::NOT_FOUND,
                &format!("Actor '{actor}' was not found"),
            )
        })?;
    auth.require_workspace(&actor.workspace)?;
    let mut actor = derive_turns(&state, vec![actor]).await?.remove(0);
    auth.conceal_actor(&mut actor);
    Ok(Json(ActorResponse { actor }))
}

/// The Actors with the turn of each current Session filled in.
async fn derive_turns(state: &AppState, mut actors: Vec<Actor>) -> Result<Vec<Actor>, ApiError> {
    let service = state.sessions.clone();
    tokio::task::spawn_blocking(move || {
        for actor in &mut actors {
            service.derive_actor_turns(actor)?;
        }
        anyhow::Ok(actors)
    })
    .await
    .map_err(|error| internal(anyhow::anyhow!(error)))?
    .map_err(internal)
}

/// A create, an update or a delete of the Actor `actor` of a Session token. `kind` is the
/// kind a create declares; an update and a delete take the kind of the Actor, and leave one
/// that is not found to answer 404.
fn authorize_manage(
    state: &AppState,
    auth: &AuthContext,
    workspace: &str,
    actor: &str,
    kind: Option<ActorKind>,
) -> Result<(), ApiError> {
    if auth.is_admin() {
        return Ok(());
    }
    auth.require_workspace(workspace)?;
    let target = state.actors.get(workspace, actor).map_err(internal)?;
    match kind.or(target.as_ref().map(|target| target.kind)) {
        Some(kind) => auth.require_manage(workspace, kind, target.as_ref()),
        None => Ok(()),
    }
}

pub(super) async fn update_actor(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path((workspace, actor)): Path<(String, String)>,
    ApiJson(request): ApiJson<UpdateActorRequest>,
) -> Result<Json<ActorResponse>, ApiError> {
    if request.grants.is_some() {
        auth.require_admin("`grants`")?;
    }
    if request
        .launch_spec
        .as_ref()
        .is_some_and(|patch| patch.trusts_workdir())
    {
        auth.require_admin("`trustWorkdir`")?;
    }
    authorize_manage(&state, &auth, &workspace, &actor, None)?;
    // The Execution Provider resolves the merged launchSpec, which blocks until its process
    // answers.
    let actors = state.actors.clone();
    let writer = auth.writer();
    let mut actor = tokio::task::spawn_blocking(move || {
        actors.update(
            &workspace,
            &actor,
            request.name.as_deref(),
            request.launch_spec,
            request.schedule.map(|patch| SchedulePatch {
                cron: patch.cron,
                timezone: patch.timezone,
                enabled: patch.enabled,
            }),
            request.grants.as_ref(),
            request.accept_from.as_ref().map(Option::as_ref),
            request.caller.as_ref(),
            writer,
        )
    })
    .await
    .map_err(|error| internal(anyhow::anyhow!(error)))?
    .map_err(actor_error)?;
    auth.conceal_actor(&mut actor);
    Ok(Json(ActorResponse { actor }))
}

pub(super) async fn delete_actor(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path((workspace, actor)): Path<(String, String)>,
) -> Result<Json<ActorResponse>, ApiError> {
    authorize_manage(&state, &auth, &workspace, &actor, None)?;
    let mut actor = state
        .actors
        .delete(&workspace, &actor, auth.writer())
        .map_err(actor_write_error)?;
    auth.conceal_actor(&mut actor);
    Ok(Json(ActorResponse { actor }))
}

pub(super) async fn start_actor(
    State(state): State<AppState>,
    RequesterCaller(auth): RequesterCaller,
    Path((workspace, actor)): Path<(String, String)>,
) -> Result<Json<StartResponse>, ApiError> {
    auth.require_grant(&workspace, Grant::Control)?;
    let started = state
        .controller
        .start(
            &workspace,
            &actor,
            &auth.requester,
            auth.session_id.as_deref(),
        )
        .await
        .map_err(control_error)?;
    let mut session = started.session;
    auth.conceal_session(&mut session);
    Ok(Json(StartResponse {
        replaced_session: started.replaced,
        session,
    }))
}

pub(super) async fn restart_actor(
    State(state): State<AppState>,
    RequesterCaller(auth): RequesterCaller,
    Path((workspace, actor)): Path<(String, String)>,
) -> Result<Json<RestartResponse>, ApiError> {
    auth.require_grant(&workspace, Grant::Control)?;
    let restarted = state
        .controller
        .restart(
            &workspace,
            &actor,
            &auth.requester,
            auth.session_id.as_deref(),
        )
        .await
        .map_err(control_error)?;
    let mut stopped = restarted.stopped;
    auth.conceal_session(&mut stopped);
    let mut session = restarted.started.map(|started| started.session);
    session
        .iter_mut()
        .for_each(|session| auth.conceal_session(session));
    Ok(Json(RestartResponse { stopped, session }))
}

pub(super) async fn attach_actor(
    State(state): State<AppState>,
    _caller: AdminCaller,
    Path((workspace, actor)): Path<(String, String)>,
    ApiJson(request): ApiJson<AttachActorRequest>,
) -> Result<Json<AttachResponse>, ApiError> {
    if !valid_key(&workspace) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Workspace keys must match ^[a-z0-9][a-z0-9-]*$",
        ));
    }
    if !valid_key(&actor) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Actor names must match ^[a-z0-9][a-z0-9-]*$",
        ));
    }
    if state.registry.runtime(&request.runtime).is_none() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            &runtime_not_installed(state.registry.as_ref(), &request.runtime),
        ));
    }
    if state
        .registry
        .provider(&request.placement.provider)
        .is_none()
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            &provider_not_installed(state.registry.as_ref(), &request.placement.provider),
        ));
    }
    let result = state
        .controller
        .attach(
            &workspace,
            &actor,
            request.placement,
            &request.runtime,
            request.runtime_telemetry_prepared,
        )
        .await
        .map_err(control_error)?;
    Ok(Json(AttachResponse {
        session: result.session,
        token: result.token,
        reattached: result.reattached,
    }))
}
