from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.message_response import MessageResponse
from ...models.send_message_body import SendMessageBody
from ...types import Response


def _get_kwargs(
    workspace: str,
    actor: str,
    *,
    body: SendMessageBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/workspaces/{workspace}/actors/{actor}/messages".format(
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
) -> Error | MessageResponse | None:
    if response.status_code == 200:
        response_200 = MessageResponse.from_dict(response.json())

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
) -> Response[Error | MessageResponse]:
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
    body: SendMessageBody,
) -> Response[Error | MessageResponse]:
    """Sends a Message to an Actor

     `from` and `fromSessionId` are determined by the token: Admin for a `localToken`, and the Actor of
    that Session for a `sessionToken`.
    `toSessionId` is the recipient's current Session at the time of acceptance.
    A `body` that is empty after trimming, and a `label` that does not match the `Key` grammar, get
    `400`.
    With a `sessionToken`, sending to an Actor in another Workspace, or to an Actor whose `acceptFrom`
    does not accept that Session's Actor, gets `403`.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):
        body (SendMessageBody):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | MessageResponse]
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
    body: SendMessageBody,
) -> MessageResponse:
    """Sends a Message to an Actor

     `from` and `fromSessionId` are determined by the token: Admin for a `localToken`, and the Actor of
    that Session for a `sessionToken`.
    `toSessionId` is the recipient's current Session at the time of acceptance.
    A `body` that is empty after trimming, and a `label` that does not match the `Key` grammar, get
    `400`.
    With a `sessionToken`, sending to an Actor in another Workspace, or to an Actor whose `acceptFrom`
    does not accept that Session's Actor, gets `403`.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):
        body (SendMessageBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        MessageResponse
    """

    with operational():
        response = sync_detailed(
            workspace=workspace,
            actor=actor,
            client=client,
            body=body,
        )
    return cast("MessageResponse", expect(response))


async def asyncio_detailed(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient | Client,
    body: SendMessageBody,
) -> Response[Error | MessageResponse]:
    """Sends a Message to an Actor

     `from` and `fromSessionId` are determined by the token: Admin for a `localToken`, and the Actor of
    that Session for a `sessionToken`.
    `toSessionId` is the recipient's current Session at the time of acceptance.
    A `body` that is empty after trimming, and a `label` that does not match the `Key` grammar, get
    `400`.
    With a `sessionToken`, sending to an Actor in another Workspace, or to an Actor whose `acceptFrom`
    does not accept that Session's Actor, gets `403`.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):
        body (SendMessageBody):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | MessageResponse]
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
    body: SendMessageBody,
) -> MessageResponse:
    """Sends a Message to an Actor

     `from` and `fromSessionId` are determined by the token: Admin for a `localToken`, and the Actor of
    that Session for a `sessionToken`.
    `toSessionId` is the recipient's current Session at the time of acceptance.
    A `body` that is empty after trimming, and a `label` that does not match the `Key` grammar, get
    `400`.
    With a `sessionToken`, sending to an Actor in another Workspace, or to an Actor whose `acceptFrom`
    does not accept that Session's Actor, gets `403`.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):
        body (SendMessageBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        MessageResponse
    """

    with operational():
        response = await asyncio_detailed(
            workspace=workspace,
            actor=actor,
            client=client,
            body=body,
        )
    return cast("MessageResponse", expect(response))
