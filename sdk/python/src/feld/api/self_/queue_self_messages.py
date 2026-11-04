from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.queue_self_messages_response_200 import QueueSelfMessagesResponse200
from ...types import Response


def _get_kwargs(
    *,
    body: list[str],
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/self/messages/queued",
    }

    _kwargs["json"] = body

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | QueueSelfMessagesResponse200 | None:
    if response.status_code == 200:
        response_200 = QueueSelfMessagesResponse200.from_dict(response.json())

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
) -> Response[Error | QueueSelfMessagesResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: list[str],
) -> Response[Error | QueueSelfMessagesResponse200]:
    """Records `queued` for Messages put into the Runtime's queue

     Records and returns `queued` for each Message. For a Message that already has `queued`, returns that
    `queued`.

    Args:
        body (list[str]): The list of Message IDs

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | QueueSelfMessagesResponse200]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
    body: list[str],
) -> QueueSelfMessagesResponse200:
    """Records `queued` for Messages put into the Runtime's queue

     Records and returns `queued` for each Message. For a Message that already has `queued`, returns that
    `queued`.

    Args:
        body (list[str]): The list of Message IDs

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        QueueSelfMessagesResponse200
    """

    with operational():
        response = sync_detailed(
            client=client,
            body=body,
        )
    return cast("QueueSelfMessagesResponse200", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: list[str],
) -> Response[Error | QueueSelfMessagesResponse200]:
    """Records `queued` for Messages put into the Runtime's queue

     Records and returns `queued` for each Message. For a Message that already has `queued`, returns that
    `queued`.

    Args:
        body (list[str]): The list of Message IDs

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | QueueSelfMessagesResponse200]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: list[str],
) -> QueueSelfMessagesResponse200:
    """Records `queued` for Messages put into the Runtime's queue

     Records and returns `queued` for each Message. For a Message that already has `queued`, returns that
    `queued`.

    Args:
        body (list[str]): The list of Message IDs

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        QueueSelfMessagesResponse200
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
            body=body,
        )
    return cast("QueueSelfMessagesResponse200", expect(response))
