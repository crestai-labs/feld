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
from .._resource import Resource, sent
from ..models import (
    NULL,
    Actor,
    ActorKind,
    AttachResponse,
    Grant,
    LaunchSpec,
    LaunchSpecPatch,
    Null,
    Placement,
    RestartResponse,
    SchedulePatch,
    ScheduleSpec,
    StartResponse,
)


class Actors(Resource):
    def list(
        self,
        workspace: str,
        *,
        kind: ActorKind | None = None,
        include_deleted: bool | None = None,
    ) -> list[Actor]:
        """The Actors of `workspace`, of `kind` when given, with the deleted ones when
        `include_deleted`."""
        return list_actors.sync(
            workspace,
            client=self._api,
            kind=sent(kind),
            include_deleted=sent(include_deleted),
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
        grants: builtins.list[Grant] | None = None,
        accept_from: builtins.list[str] | None = None,
        caller: dict[str, Any] | None = None,
        schedule: ScheduleSpec | None = None,
    ) -> Actor:
        body = CreateActorBody(
            name=name,
            kind=kind,
            launch_spec=launch_spec,
            grants=sent(grants),
            accept_from=sent(accept_from),
            caller=sent(caller),
            schedule=sent(schedule),
        )
        return create_actor.sync(workspace, client=self._api, body=body).actor

    def update(
        self,
        workspace: str,
        actor: str,
        *,
        name: str | None = None,
        launch_spec: LaunchSpecPatch | None = None,
        schedule: SchedulePatch | None = None,
        grants: builtins.list[Grant] | None = None,
        accept_from: builtins.list[str] | Null | None = None,
        caller: dict[str, Any] | None = None,
    ) -> Actor:
        """Changes the fields given; `accept_from=NULL` removes the Actor's `accept_from`."""
        body = UpdateActorBody(
            name=sent(name),
            launch_spec=sent(launch_spec),
            schedule=sent(schedule),
            grants=sent(grants),
            accept_from=None if accept_from is NULL else sent(accept_from),
            caller=sent(caller),
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
        runtime_telemetry_prepared: bool | None = None,
    ) -> AttachResponse:
        """Attaches a running Runtime as a Session of the Actor, and issues its Session
        token."""
        body = AttachActorBody(
            placement=placement,
            runtime=runtime,
            runtime_telemetry_prepared=sent(runtime_telemetry_prepared),
        )
        return attach_actor.sync(workspace, actor, client=self._api, body=body)

    def restart(self, workspace: str, actor: str) -> RestartResponse:
        """Stops the current Session and launches the next generation."""
        return restart_actor.sync(workspace, actor, client=self._api)
