from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.actor_response import ActorResponse
from ...models.create_actor_body import CreateActorBody
from ...models.error import Error
from ...types import Response


def _get_kwargs(
    workspace: str,
    *,
    body: CreateActorBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/workspaces/{workspace}/actors".format(
            workspace=quote(str(workspace), safe=""),
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
    *,
    client: AuthenticatedClient | Client,
    body: CreateActorBody,
) -> Response[ActorResponse | Error]:
    """Creates an Actor

     If an Actor with the same name that is not deleted exists, replaces its `launchSpec` and Schedule
    with the request contents, keeping the same `id`.
    The Schedule is replaced, keeping `enabled`, when `schedule` is present, and removed when it is
    absent. Only a Job accepts `schedule`; an Agent gets `400`. A different kind gets `409`, and a
    `launchSpec` other than `runtime` for kind `agent` or other than `command` for `job` gets `400`,
    without writing anything.
    Omitting `schedule.timezone` uses the daemon's timezone.
    Only an Admin token can give `grants`; a Session token request that includes `grants` gets `403`. A
    replacement that omits `grants` keeps the Actor's grants.
    A replacement that omits `acceptFrom` keeps the Actor's `acceptFrom`.

    Args:
        workspace (str):
        body (CreateActorBody):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[ActorResponse | Error]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    body: CreateActorBody,
) -> ActorResponse:
    """Creates an Actor

     If an Actor with the same name that is not deleted exists, replaces its `launchSpec` and Schedule
    with the request contents, keeping the same `id`.
    The Schedule is replaced, keeping `enabled`, when `schedule` is present, and removed when it is
    absent. Only a Job accepts `schedule`; an Agent gets `400`. A different kind gets `409`, and a
    `launchSpec` other than `runtime` for kind `agent` or other than `command` for `job` gets `400`,
    without writing anything.
    Omitting `schedule.timezone` uses the daemon's timezone.
    Only an Admin token can give `grants`; a Session token request that includes `grants` gets `403`. A
    replacement that omits `grants` keeps the Actor's grants.
    A replacement that omits `acceptFrom` keeps the Actor's `acceptFrom`.

    Args:
        workspace (str):
        body (CreateActorBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ActorResponse
    """

    with operational():
        response = sync_detailed(
            workspace=workspace,
            client=client,
            body=body,
        )
    return cast("ActorResponse", expect(response))


async def asyncio_detailed(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    body: CreateActorBody,
) -> Response[ActorResponse | Error]:
    """Creates an Actor

     If an Actor with the same name that is not deleted exists, replaces its `launchSpec` and Schedule
    with the request contents, keeping the same `id`.
    The Schedule is replaced, keeping `enabled`, when `schedule` is present, and removed when it is
    absent. Only a Job accepts `schedule`; an Agent gets `400`. A different kind gets `409`, and a
    `launchSpec` other than `runtime` for kind `agent` or other than `command` for `job` gets `400`,
    without writing anything.
    Omitting `schedule.timezone` uses the daemon's timezone.
    Only an Admin token can give `grants`; a Session token request that includes `grants` gets `403`. A
    replacement that omits `grants` keeps the Actor's grants.
    A replacement that omits `acceptFrom` keeps the Actor's `acceptFrom`.

    Args:
        workspace (str):
        body (CreateActorBody):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[ActorResponse | Error]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    body: CreateActorBody,
) -> ActorResponse:
    """Creates an Actor

     If an Actor with the same name that is not deleted exists, replaces its `launchSpec` and Schedule
    with the request contents, keeping the same `id`.
    The Schedule is replaced, keeping `enabled`, when `schedule` is present, and removed when it is
    absent. Only a Job accepts `schedule`; an Agent gets `400`. A different kind gets `409`, and a
    `launchSpec` other than `runtime` for kind `agent` or other than `command` for `job` gets `400`,
    without writing anything.
    Omitting `schedule.timezone` uses the daemon's timezone.
    Only an Admin token can give `grants`; a Session token request that includes `grants` gets `403`. A
    replacement that omits `grants` keeps the Actor's grants.
    A replacement that omits `acceptFrom` keeps the Actor's `acceptFrom`.

    Args:
        workspace (str):
        body (CreateActorBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ActorResponse
    """

    with operational():
        response = await asyncio_detailed(
            workspace=workspace,
            client=client,
            body=body,
        )
    return cast("ActorResponse", expect(response))
