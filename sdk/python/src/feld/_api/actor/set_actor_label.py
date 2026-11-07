from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import _errors as errors
from ..._client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ..._models.actor_response import ActorResponse
from ..._models.error import Error
from ..._models.set_actor_label_body import SetActorLabelBody
from ..._types import Response


def _get_kwargs(
    workspace: str,
    actor: str,
    key: str,
    *,
    body: SetActorLabelBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "put",
        "url": "/v1/workspaces/{workspace}/actors/{actor}/labels/{key}".format(
            workspace=quote(str(workspace), safe=""),
            actor=quote(str(actor), safe=""),
            key=quote(str(key), safe=""),
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
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetActorLabelBody,
) -> Response[ActorResponse | Error]:
    """Sets a label on an Actor

     Replaces the label of the same key, and records `label.set`. `by` is `admin` for a `localToken` or
    browser token, and `actor` for a `sessionToken`.
    A `sessionToken` sets labels only on the Actor of its Session; another Actor gets `403`.
    A `key` that does not match the `Key` grammar, and an `until` that is not RFC 3339 or is not after
    now, get `400`.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):
        key (str):
        body (SetActorLabelBody):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[ActorResponse | Error]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
        key=key,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workspace: str,
    actor: str,
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetActorLabelBody,
) -> ActorResponse:
    """Sets a label on an Actor

     Replaces the label of the same key, and records `label.set`. `by` is `admin` for a `localToken` or
    browser token, and `actor` for a `sessionToken`.
    A `sessionToken` sets labels only on the Actor of its Session; another Actor gets `403`.
    A `key` that does not match the `Key` grammar, and an `until` that is not RFC 3339 or is not after
    now, get `400`.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):
        key (str):
        body (SetActorLabelBody):

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
            key=key,
            client=client,
            body=body,
        )
    return cast("ActorResponse", expect(response))


async def asyncio_detailed(
    workspace: str,
    actor: str,
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetActorLabelBody,
) -> Response[ActorResponse | Error]:
    """Sets a label on an Actor

     Replaces the label of the same key, and records `label.set`. `by` is `admin` for a `localToken` or
    browser token, and `actor` for a `sessionToken`.
    A `sessionToken` sets labels only on the Actor of its Session; another Actor gets `403`.
    A `key` that does not match the `Key` grammar, and an `until` that is not RFC 3339 or is not after
    now, get `400`.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):
        key (str):
        body (SetActorLabelBody):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[ActorResponse | Error]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
        key=key,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workspace: str,
    actor: str,
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetActorLabelBody,
) -> ActorResponse:
    """Sets a label on an Actor

     Replaces the label of the same key, and records `label.set`. `by` is `admin` for a `localToken` or
    browser token, and `actor` for a `sessionToken`.
    A `sessionToken` sets labels only on the Actor of its Session; another Actor gets `403`.
    A `key` that does not match the `Key` grammar, and an `until` that is not RFC 3339 or is not after
    now, get `400`.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        workspace (str):
        actor (str):
        key (str):
        body (SetActorLabelBody):

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
            key=key,
            client=client,
            body=body,
        )
    return cast("ActorResponse", expect(response))
