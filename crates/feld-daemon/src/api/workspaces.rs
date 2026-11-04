use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::store::{AcceptFrom, CreateWorkspace, WorkspaceDeleteError};
use feld_core::id::{valid_actor_pattern, valid_key};

use super::AppState;
use super::auth::{AdminCaller, Caller};
use super::error::{ApiError, error, internal, workspace_not_found};
use super::query::{ApiJson, ApiQuery, WorkspaceQuery};
use super::types::{CreateWorkspaceRequest, WorkspaceList, WorkspaceResponse};

pub(super) async fn list_workspaces(
    State(state): State<AppState>,
    Caller(auth): Caller,
    ApiQuery(query): ApiQuery<WorkspaceQuery>,
) -> Result<Json<WorkspaceList>, ApiError> {
    let mut workspaces = state
        .workspaces
        .list(query.include_deleted)
        .map_err(internal)?;
    workspaces.retain(|workspace| auth.reaches(&workspace.key));
    Ok(Json(WorkspaceList { workspaces }))
}

pub(super) async fn create_workspace(
    State(state): State<AppState>,
    _caller: AdminCaller,
    ApiJson(request): ApiJson<CreateWorkspaceRequest>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    if !valid_key(&request.key) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Workspace keys must match ^[a-z0-9][a-z0-9-]*$",
        ));
    }
    match state.workspaces.create(&request.key).map_err(internal)? {
        CreateWorkspace::Created(workspace) => Ok(Json(WorkspaceResponse { workspace })),
        CreateWorkspace::Exists(workspace) => Err(error(
            StatusCode::CONFLICT,
            &format!("Workspace key '{}' already exists", workspace.key),
        )),
    }
}

pub(super) async fn get_workspace(
    State(state): State<AppState>,
    Caller(auth): Caller,
    Path(workspace): Path<String>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    auth.require_workspace(&workspace)?;
    let workspace = state
        .workspaces
        .get(&workspace)
        .map_err(internal)?
        .ok_or_else(|| workspace_not_found(&workspace))?;
    Ok(Json(WorkspaceResponse { workspace }))
}

pub(super) async fn delete_workspace(
    State(state): State<AppState>,
    _caller: AdminCaller,
    Path(workspace): Path<String>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let workspace = state.workspaces.delete(&workspace).map_err(|error_value| {
        let status = match error_value {
            WorkspaceDeleteError::NotFound(_) => StatusCode::NOT_FOUND,
            WorkspaceDeleteError::Default(_) | WorkspaceDeleteError::LiveActors { .. } => {
                StatusCode::CONFLICT
            }
            WorkspaceDeleteError::Store(cause) => return internal(cause),
        };
        error(status, &error_value.to_string())
    })?;
    Ok(Json(WorkspaceResponse { workspace }))
}

pub(super) async fn allow_from(
    State(state): State<AppState>,
    _caller: AdminCaller,
    Path((workspace, from_workspace, actor)): Path<(String, String, String)>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let from = accept_from(&workspace, from_workspace, actor)?;
    let workspace = state
        .workspaces
        .allow_from(&workspace, &from)
        .map_err(internal)?
        .ok_or_else(|| workspace_not_found(&workspace))?;
    Ok(Json(WorkspaceResponse { workspace }))
}

pub(super) async fn deny_from(
    State(state): State<AppState>,
    _caller: AdminCaller,
    Path((workspace, from_workspace, actor)): Path<(String, String, String)>,
) -> Result<Json<WorkspaceResponse>, ApiError> {
    let from = accept_from(&workspace, from_workspace, actor)?;
    let workspace = state
        .workspaces
        .deny_from(&workspace, &from)
        .map_err(internal)?
        .ok_or_else(|| workspace_not_found(&workspace))?;
    Ok(Json(WorkspaceResponse { workspace }))
}

/// The Actors of another Workspace that `workspace` accepts Messages from.
fn accept_from(
    workspace: &str,
    from_workspace: String,
    actor: String,
) -> Result<AcceptFrom, ApiError> {
    if !valid_key(&from_workspace) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Workspace keys must match ^[a-z0-9][a-z0-9-]*$",
        ));
    }
    if from_workspace == workspace {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "a Workspace accepts Messages from its own Actors; acceptFrom names another Workspace",
        ));
    }
    if !valid_actor_pattern(&actor) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Actor patterns are Actor names, ^[a-z0-9][a-z0-9-]*$, in which `*` stands for any run of characters",
        ));
    }
    Ok(AcceptFrom {
        workspace: from_workspace,
        actor,
    })
}
