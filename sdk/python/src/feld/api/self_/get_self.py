from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.session_response import SessionResponse
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/self",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | SessionResponse | None:
    if response.status_code == 200:
        response_200 = SessionResponse.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = Error.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = Error.from_dict(response.json())

        return response_403

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
) -> Response[Error | SessionResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
) -> Response[Error | SessionResponse]:
    """Returns the caller's Session

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | SessionResponse]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
) -> SessionResponse:
    """Returns the caller's Session

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        SessionResponse
    """

    with operational():
        response = sync_detailed(
            client=client,
        )
    return cast("SessionResponse", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
) -> Response[Error | SessionResponse]:
    """Returns the caller's Session

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | SessionResponse]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
) -> SessionResponse:
    """Returns the caller's Session

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        SessionResponse
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
        )
    return cast("SessionResponse", expect(response))
