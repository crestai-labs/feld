from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import _errors as errors
from ..._client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ..._models.error import Error
from ..._models.restart_response import RestartResponse
from ..._types import Response


def _get_kwargs(
    workspace: str,
    actor: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/workspaces/{workspace}/actors/{actor}/restart".format(
            workspace=quote(str(workspace), safe=""),
            actor=quote(str(actor), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | RestartResponse | None:
    if response.status_code == 200:
        response_200 = RestartResponse.from_dict(response.json())

        return response_200

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
) -> Response[Error | RestartResponse]:
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
) -> Response[Error | RestartResponse]:
    """Stops the current Session and launches the next generation

     Accepted only when the current Session is `owned` and running; otherwise returns `409`. Responds
    once stop has been requested for the current Session, and launches the next generation as
    `replacement` when that Session closes.
    Records spawned.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | RestartResponse]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
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
) -> RestartResponse:
    """Stops the current Session and launches the next generation

     Accepted only when the current Session is `owned` and running; otherwise returns `409`. Responds
    once stop has been requested for the current Session, and launches the next generation as
    `replacement` when that Session closes.
    Records spawned.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        RestartResponse
    """

    with operational():
        response = sync_detailed(
            workspace=workspace,
            actor=actor,
            client=client,
        )
    return cast("RestartResponse", expect(response))


async def asyncio_detailed(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Error | RestartResponse]:
    """Stops the current Session and launches the next generation

     Accepted only when the current Session is `owned` and running; otherwise returns `409`. Responds
    once stop has been requested for the current Session, and launches the next generation as
    `replacement` when that Session closes.
    Records spawned.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | RestartResponse]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient | Client,
) -> RestartResponse:
    """Stops the current Session and launches the next generation

     Accepted only when the current Session is `owned` and running; otherwise returns `409`. Responds
    once stop has been requested for the current Session, and launches the next generation as
    `replacement` when that Session closes.
    Records spawned.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        RestartResponse
    """

    with operational():
        response = await asyncio_detailed(
            workspace=workspace,
            actor=actor,
            client=client,
        )
    return cast("RestartResponse", expect(response))
