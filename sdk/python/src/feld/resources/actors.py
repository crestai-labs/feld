"""Actors: `/v1/workspaces/{workspace}/actors` and `/v1/actors/{actor}`."""

from __future__ import annotations

import builtins
from typing import Any

from .._api.actor import (
    attach_actor,
    create_actor,
    delete_actor,
    get_actor,
    get_actor_by_id,
    list_actors,
    restart_actor,
    start_actor,
    update_actor,
)
from .._models import AttachActorBody, CreateActorBody, UpdateActorBody
from .._types import UNSET, Unset
from ..models import (
    Actor,
    ActorKind,
    AttachResponse,
    Grant,
    LaunchSpec,
    LaunchSpecPatch,
    Placement,
    RestartResponse,
    SchedulePatch,
    ScheduleSpec,
    StartResponse,
)
from ._resource import Resource


class Actors(Resource):
    def list(
        self,
        workspace: str,
        *,
        kind: ActorKind | Unset = UNSET,
        include_deleted: bool | Unset = UNSET,
    ) -> list[Actor]:
        """The Actors of `workspace`, of `kind` when given, with the deleted ones when
        `include_deleted`."""
        return list_actors.sync(
            workspace, client=self._api, kind=kind, include_deleted=include_deleted
        ).actors

    def get(self, workspace: str, actor: str) -> Actor:
        """The Actor named `actor` in `workspace`."""
        return get_actor.sync(workspace, actor, client=self._api).actor

    def get_by_id(self, actor: str) -> Actor:
        """The Actor whose `id` is `actor`."""
        return get_actor_by_id.sync(actor, client=self._api).actor

    def create(
        self,
        workspace: str,
        *,
        name: str,
        kind: ActorKind,
        launch_spec: LaunchSpec,
        grants: builtins.list[Grant] | Unset = UNSET,
        accept_from: builtins.list[str] | Unset = UNSET,
        caller: dict[str, Any] | Unset = UNSET,
        schedule: ScheduleSpec | None | Unset = UNSET,
    ) -> Actor:
        body = CreateActorBody(
            name=name,
            kind=kind,
            launch_spec=launch_spec,
            grants=grants,
            accept_from=accept_from,
            caller=caller,
            schedule=schedule,
        )
        return create_actor.sync(workspace, client=self._api, body=body).actor

    def update(
        self,
        workspace: str,
        actor: str,
        *,
        name: str | Unset = UNSET,
        launch_spec: LaunchSpecPatch | Unset = UNSET,
        schedule: SchedulePatch | Unset = UNSET,
        grants: builtins.list[Grant] | Unset = UNSET,
        accept_from: builtins.list[str] | None | Unset = UNSET,
        caller: dict[str, Any] | Unset = UNSET,
    ) -> Actor:
        """Changes the fields given; `accept_from=None` removes it."""
        body = UpdateActorBody(
            name=name,
            launch_spec=launch_spec,
            schedule=schedule,
            grants=grants,
            accept_from=accept_from,
            caller=caller,
        )
        return update_actor.sync(workspace, actor, client=self._api, body=body).actor

    def delete(self, workspace: str, actor: str) -> Actor:
        """Soft-deletes the Actor."""
        return delete_actor.sync(workspace, actor, client=self._api).actor

    def start(self, workspace: str, actor: str) -> StartResponse:
        """Launches the next-generation Session with the Actor's `launch_spec`."""
        return start_actor.sync(workspace, actor, client=self._api)

    def attach(
        self,
        workspace: str,
        actor: str,
        *,
        placement: Placement,
        runtime: str,
        runtime_telemetry_prepared: bool | Unset = UNSET,
    ) -> AttachResponse:
        """Attaches a running Runtime as a Session of the Actor, and issues its Session
        token."""
        body = AttachActorBody(
            placement=placement,
            runtime=runtime,
            runtime_telemetry_prepared=runtime_telemetry_prepared,
        )
        return attach_actor.sync(workspace, actor, client=self._api, body=body)

    def restart(self, workspace: str, actor: str) -> RestartResponse:
        """Stops the current Session and launches the next generation."""
        return restart_actor.sync(workspace, actor, client=self._api)
