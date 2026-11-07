"""Workspaces: `GET|POST /v1/workspaces` and `GET|DELETE /v1/workspaces/{workspace}`."""

from __future__ import annotations

from .._api.workspace import (
    create_workspace,
    delete_workspace,
    get_workspace,
    list_workspaces,
)
from .._models import CreateWorkspaceBody
from .._types import UNSET, Unset
from ..models import Workspace
from ._resource import Resource


class Workspaces(Resource):
    def list(self, *, include_deleted: bool | Unset = UNSET) -> list[Workspace]:
        """The Workspaces, with the deleted ones when `include_deleted`."""
        return list_workspaces.sync(
            client=self._api, include_deleted=include_deleted
        ).workspaces

    def get(self, workspace: str) -> Workspace:
        return get_workspace.sync(workspace, client=self._api).workspace

    def create(self, *, key: str) -> Workspace:
        return create_workspace.sync(
            client=self._api, body=CreateWorkspaceBody(key=key)
        ).workspace

    def delete(self, workspace: str) -> Workspace:
        """Soft-deletes the Workspace."""
        return delete_workspace.sync(workspace, client=self._api).workspace
