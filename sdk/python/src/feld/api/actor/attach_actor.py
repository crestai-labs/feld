from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.attach_actor_body import AttachActorBody
from ...models.attach_response import AttachResponse
from ...models.error import Error
from ...types import Response


def _get_kwargs(
    workspace: str,
    actor: str,
    *,
    body: AttachActorBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/workspaces/{workspace}/actors/{actor}/attach".format(
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
) -> AttachResponse | Error | None:
    if response.status_code == 200:
        response_200 = AttachResponse.from_dict(response.json())

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
) -> Response[AttachResponse | Error]:
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
    client: AuthenticatedClient,
    body: AttachActorBody,
) -> Response[AttachResponse | Error]:
    """Attaches a running Runtime as a Session of the Actor

     Creates an Actor of kind `agent` and a Session with `startKind` `attach` and `ownership` `attached`.
    Creates the Actor if it does not exist; for an existing Actor, keeps its `launchSpec`.
    If the Runtime process to attach is the same entity as an `attached` current Session, reissues the
    token without creating a Session, invalidates the old token, and sets `reattached` to `true`.
    A Workspace key or Actor name that does not match the grammar, and a `runtime` or
    `placement.provider` that is not installed, get `400`; a nonexistent Workspace gets `404`.
    An Actor of kind `job`, a Runtime process that is the current Session of another Actor, and an
    attach that admission rejects get `409`.

    Args:
        workspace (str):
        actor (str):
        body (AttachActorBody):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[AttachResponse | Error]
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
    client: AuthenticatedClient,
    body: AttachActorBody,
) -> AttachResponse:
    """Attaches a running Runtime as a Session of the Actor

     Creates an Actor of kind `agent` and a Session with `startKind` `attach` and `ownership` `attached`.
    Creates the Actor if it does not exist; for an existing Actor, keeps its `launchSpec`.
    If the Runtime process to attach is the same entity as an `attached` current Session, reissues the
    token without creating a Session, invalidates the old token, and sets `reattached` to `true`.
    A Workspace key or Actor name that does not match the grammar, and a `runtime` or
    `placement.provider` that is not installed, get `400`; a nonexistent Workspace gets `404`.
    An Actor of kind `job`, a Runtime process that is the current Session of another Actor, and an
    attach that admission rejects get `409`.

    Args:
        workspace (str):
        actor (str):
        body (AttachActorBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        AttachResponse
    """

    with operational():
        response = sync_detailed(
            workspace=workspace,
            actor=actor,
            client=client,
            body=body,
        )
    return cast("AttachResponse", expect(response))


async def asyncio_detailed(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient,
    body: AttachActorBody,
) -> Response[AttachResponse | Error]:
    """Attaches a running Runtime as a Session of the Actor

     Creates an Actor of kind `agent` and a Session with `startKind` `attach` and `ownership` `attached`.
    Creates the Actor if it does not exist; for an existing Actor, keeps its `launchSpec`.
    If the Runtime process to attach is the same entity as an `attached` current Session, reissues the
    token without creating a Session, invalidates the old token, and sets `reattached` to `true`.
    A Workspace key or Actor name that does not match the grammar, and a `runtime` or
    `placement.provider` that is not installed, get `400`; a nonexistent Workspace gets `404`.
    An Actor of kind `job`, a Runtime process that is the current Session of another Actor, and an
    attach that admission rejects get `409`.

    Args:
        workspace (str):
        actor (str):
        body (AttachActorBody):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[AttachResponse | Error]
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
    client: AuthenticatedClient,
    body: AttachActorBody,
) -> AttachResponse:
    """Attaches a running Runtime as a Session of the Actor

     Creates an Actor of kind `agent` and a Session with `startKind` `attach` and `ownership` `attached`.
    Creates the Actor if it does not exist; for an existing Actor, keeps its `launchSpec`.
    If the Runtime process to attach is the same entity as an `attached` current Session, reissues the
    token without creating a Session, invalidates the old token, and sets `reattached` to `true`.
    A Workspace key or Actor name that does not match the grammar, and a `runtime` or
    `placement.provider` that is not installed, get `400`; a nonexistent Workspace gets `404`.
    An Actor of kind `job`, a Runtime process that is the current Session of another Actor, and an
    attach that admission rejects get `409`.

    Args:
        workspace (str):
        actor (str):
        body (AttachActorBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        AttachResponse
    """

    with operational():
        response = await asyncio_detailed(
            workspace=workspace,
            actor=actor,
            client=client,
            body=body,
        )
    return cast("AttachResponse", expect(response))
