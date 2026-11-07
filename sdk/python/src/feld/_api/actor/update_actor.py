from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import _errors as errors
from ..._client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ..._models.actor_response import ActorResponse
from ..._models.error import Error
from ..._models.update_actor_body import UpdateActorBody
from ..._types import Response


def _get_kwargs(
    workspace: str,
    actor: str,
    *,
    body: UpdateActorBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "patch",
        "url": "/v1/workspaces/{workspace}/actors/{actor}".format(
            workspace=quote(str(workspace), safe=""),
            actor=quote(str(actor), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ActorResponse | Error | None:
    if response.status_code == 200:
        response_200 = ActorResponse.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = Error.from_dict(response.json())

        return response_400

    if response.status_code == 401:
        response_401 = Error.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = Error.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = Error.from_dict(response.json())

        return response_404

    if response.status_code == 409:
        response_409 = Error.from_dict(response.json())

        return response_409

    if response.status_code == 500:
        response_500 = Error.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ActorResponse | Error]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient | Client,
    body: UpdateActorBody,
) -> Response[ActorResponse | Error]:
    """Partially updates the name, launchSpec, Schedule, grants, and acceptFrom of an Actor

     `name` changes the Actor's name, keeping its `id`, Session history, and mailbox. Renaming to a name
    held by another live Actor in the same Workspace, and renaming an Actor whose current Session is
    running, get `409`.
    `launchSpec` is handled per top-level field: replaced when present, kept when absent, and removed
    when `null`. The merged `launchSpec` is validated with the same rules as creation.
    `schedule` replaces only those of `cron`, `timezone`, and `enabled` that are present, and only a Job
    accepts it. For a Job without a Schedule, `cron` is required and a Schedule is created. A `schedule`
    for an Agent, and a `schedule` without `cron` for a Job without a Schedule, get `400`.
    Toggling `schedule.enabled` is pause / resume. A Session token with the `control` grant can send a
    request that changes only `schedule.enabled` to any Job in its Workspace, including one with grants;
    any other change needs `manage-agent` / `manage-job` and an Actor without grants or `trustWorkdir`.
    `grants` replaces the Actor's grants entirely. Only an Admin token can give it; a Session token
    request that includes `grants` gets `403`.
    `acceptFrom` replaces the Actor's `acceptFrom` entirely; `null` reverts it to omitted (accepting
    from all Actors in the same Workspace).
    A request with no `name`, no fields in `launchSpec` or `schedule`, and neither `grants` nor
    `acceptFrom` gets `400`.
    Changes to launchSpec and Schedule leave the current Session untouched and take effect from the next
    launch.

    Args:
        workspace (str):
        actor (str):
        body (UpdateActorBody):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[ActorResponse | Error]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient | Client,
    body: UpdateActorBody,
) -> ActorResponse:
    """Partially updates the name, launchSpec, Schedule, grants, and acceptFrom of an Actor

     `name` changes the Actor's name, keeping its `id`, Session history, and mailbox. Renaming to a name
    held by another live Actor in the same Workspace, and renaming an Actor whose current Session is
    running, get `409`.
    `launchSpec` is handled per top-level field: replaced when present, kept when absent, and removed
    when `null`. The merged `launchSpec` is validated with the same rules as creation.
    `schedule` replaces only those of `cron`, `timezone`, and `enabled` that are present, and only a Job
    accepts it. For a Job without a Schedule, `cron` is required and a Schedule is created. A `schedule`
    for an Agent, and a `schedule` without `cron` for a Job without a Schedule, get `400`.
    Toggling `schedule.enabled` is pause / resume. A Session token with the `control` grant can send a
    request that changes only `schedule.enabled` to any Job in its Workspace, including one with grants;
    any other change needs `manage-agent` / `manage-job` and an Actor without grants or `trustWorkdir`.
    `grants` replaces the Actor's grants entirely. Only an Admin token can give it; a Session token
    request that includes `grants` gets `403`.
    `acceptFrom` replaces the Actor's `acceptFrom` entirely; `null` reverts it to omitted (accepting
    from all Actors in the same Workspace).
    A request with no `name`, no fields in `launchSpec` or `schedule`, and neither `grants` nor
    `acceptFrom` gets `400`.
    Changes to launchSpec and Schedule leave the current Session untouched and take effect from the next
    launch.

    Args:
        workspace (str):
        actor (str):
        body (UpdateActorBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ActorResponse
    """

    with operational():
        response = sync_detailed(
            workspace=workspace,
            actor=actor,
            client=client,
            body=body,
        )
    return cast("ActorResponse", expect(response))


async def asyncio_detailed(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient | Client,
    body: UpdateActorBody,
) -> Response[ActorResponse | Error]:
    """Partially updates the name, launchSpec, Schedule, grants, and acceptFrom of an Actor

     `name` changes the Actor's name, keeping its `id`, Session history, and mailbox. Renaming to a name
    held by another live Actor in the same Workspace, and renaming an Actor whose current Session is
    running, get `409`.
    `launchSpec` is handled per top-level field: replaced when present, kept when absent, and removed
    when `null`. The merged `launchSpec` is validated with the same rules as creation.
    `schedule` replaces only those of `cron`, `timezone`, and `enabled` that are present, and only a Job
    accepts it. For a Job without a Schedule, `cron` is required and a Schedule is created. A `schedule`
    for an Agent, and a `schedule` without `cron` for a Job without a Schedule, get `400`.
    Toggling `schedule.enabled` is pause / resume. A Session token with the `control` grant can send a
    request that changes only `schedule.enabled` to any Job in its Workspace, including one with grants;
    any other change needs `manage-agent` / `manage-job` and an Actor without grants or `trustWorkdir`.
    `grants` replaces the Actor's grants entirely. Only an Admin token can give it; a Session token
    request that includes `grants` gets `403`.
    `acceptFrom` replaces the Actor's `acceptFrom` entirely; `null` reverts it to omitted (accepting
    from all Actors in the same Workspace).
    A request with no `name`, no fields in `launchSpec` or `schedule`, and neither `grants` nor
    `acceptFrom` gets `400`.
    Changes to launchSpec and Schedule leave the current Session untouched and take effect from the next
    launch.

    Args:
        workspace (str):
        actor (str):
        body (UpdateActorBody):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[ActorResponse | Error]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient | Client,
    body: UpdateActorBody,
) -> ActorResponse:
    """Partially updates the name, launchSpec, Schedule, grants, and acceptFrom of an Actor

     `name` changes the Actor's name, keeping its `id`, Session history, and mailbox. Renaming to a name
    held by another live Actor in the same Workspace, and renaming an Actor whose current Session is
    running, get `409`.
    `launchSpec` is handled per top-level field: replaced when present, kept when absent, and removed
    when `null`. The merged `launchSpec` is validated with the same rules as creation.
    `schedule` replaces only those of `cron`, `timezone`, and `enabled` that are present, and only a Job
    accepts it. For a Job without a Schedule, `cron` is required and a Schedule is created. A `schedule`
    for an Agent, and a `schedule` without `cron` for a Job without a Schedule, get `400`.
    Toggling `schedule.enabled` is pause / resume. A Session token with the `control` grant can send a
    request that changes only `schedule.enabled` to any Job in its Workspace, including one with grants;
    any other change needs `manage-agent` / `manage-job` and an Actor without grants or `trustWorkdir`.
    `grants` replaces the Actor's grants entirely. Only an Admin token can give it; a Session token
    request that includes `grants` gets `403`.
    `acceptFrom` replaces the Actor's `acceptFrom` entirely; `null` reverts it to omitted (accepting
    from all Actors in the same Workspace).
    A request with no `name`, no fields in `launchSpec` or `schedule`, and neither `grants` nor
    `acceptFrom` gets `400`.
    Changes to launchSpec and Schedule leave the current Session untouched and take effect from the next
    launch.

    Args:
        workspace (str):
        actor (str):
        body (UpdateActorBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ActorResponse
    """

    with operational():
        response = await asyncio_detailed(
            workspace=workspace,
            actor=actor,
            client=client,
            body=body,
        )
    return cast("ActorResponse", expect(response))
