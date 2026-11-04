from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.message_response import MessageResponse
from ...types import Response


def _get_kwargs(
    message: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "delete",
        "url": "/v1/self/messages/{message}/queued".format(
            message=quote(str(message), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | MessageResponse | None:
    if response.status_code == 200:
        response_200 = MessageResponse.from_dict(response.json())

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
) -> Response[Error | MessageResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    message: str,
    *,
    client: AuthenticatedClient,
) -> Response[Error | MessageResponse]:
    """Withdraws `queued` from Messages the Runtime did not put into its queue

     Removes the `queued` recorded by the caller's Session and returns the Messages. Returns as-is a
    Message that has `read` or whose `queued` another Session recorded.

    Args:
        message (str):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | MessageResponse]
    """

    kwargs = _get_kwargs(
        message=message,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    message: str,
    *,
    client: AuthenticatedClient,
) -> MessageResponse:
    """Withdraws `queued` from Messages the Runtime did not put into its queue

     Removes the `queued` recorded by the caller's Session and returns the Messages. Returns as-is a
    Message that has `read` or whose `queued` another Session recorded.

    Args:
        message (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        MessageResponse
    """

    with operational():
        response = sync_detailed(
            message=message,
            client=client,
        )
    return cast("MessageResponse", expect(response))


async def asyncio_detailed(
    message: str,
    *,
    client: AuthenticatedClient,
) -> Response[Error | MessageResponse]:
    """Withdraws `queued` from Messages the Runtime did not put into its queue

     Removes the `queued` recorded by the caller's Session and returns the Messages. Returns as-is a
    Message that has `read` or whose `queued` another Session recorded.

    Args:
        message (str):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | MessageResponse]
    """

    kwargs = _get_kwargs(
        message=message,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    message: str,
    *,
    client: AuthenticatedClient,
) -> MessageResponse:
    """Withdraws `queued` from Messages the Runtime did not put into its queue

     Removes the `queued` recorded by the caller's Session and returns the Messages. Returns as-is a
    Message that has `read` or whose `queued` another Session recorded.

    Args:
        message (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        MessageResponse
    """

    with operational():
        response = await asyncio_detailed(
            message=message,
            client=client,
        )
    return cast("MessageResponse", expect(response))
