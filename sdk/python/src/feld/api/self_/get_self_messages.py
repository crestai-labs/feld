from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.message_list import MessageList
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    limit: int | Unset = UNSET,
    wait: bool | Unset = False,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["limit"] = limit

    params["wait"] = wait

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/self/messages",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | MessageList | None:
    if response.status_code == 200:
        response_200 = MessageList.from_dict(response.json())

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

    if response.status_code == 503:
        response_503 = Error.from_dict(response.json())

        return response_503

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Error | MessageList]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    wait: bool | Unset = False,
) -> Response[Error | MessageList]:
    """Takes unread Messages from the mailbox of the caller's Actor

     Returns Messages without `read` in FIFO order and records nothing.
    With `wait=true`, holds the response until an unread Message arrives. Returns `503` when the daemon
    starts to stop.

    Args:
        limit (int | Unset):
        wait (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | MessageList]
    """

    kwargs = _get_kwargs(
        limit=limit,
        wait=wait,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    wait: bool | Unset = False,
) -> MessageList:
    """Takes unread Messages from the mailbox of the caller's Actor

     Returns Messages without `read` in FIFO order and records nothing.
    With `wait=true`, holds the response until an unread Message arrives. Returns `503` when the daemon
    starts to stop.

    Args:
        limit (int | Unset):
        wait (bool | Unset):  Default: False.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        MessageList
    """

    with operational():
        response = sync_detailed(
            client=client,
            limit=limit,
            wait=wait,
        )
    return cast("MessageList", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    wait: bool | Unset = False,
) -> Response[Error | MessageList]:
    """Takes unread Messages from the mailbox of the caller's Actor

     Returns Messages without `read` in FIFO order and records nothing.
    With `wait=true`, holds the response until an unread Message arrives. Returns `503` when the daemon
    starts to stop.

    Args:
        limit (int | Unset):
        wait (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | MessageList]
    """

    kwargs = _get_kwargs(
        limit=limit,
        wait=wait,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    wait: bool | Unset = False,
) -> MessageList:
    """Takes unread Messages from the mailbox of the caller's Actor

     Returns Messages without `read` in FIFO order and records nothing.
    With `wait=true`, holds the response until an unread Message arrives. Returns `503` when the daemon
    starts to stop.

    Args:
        limit (int | Unset):
        wait (bool | Unset):  Default: False.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        MessageList
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
            limit=limit,
            wait=wait,
        )
    return cast("MessageList", expect(response))
