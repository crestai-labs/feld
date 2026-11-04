from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/ui/",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | str | None:
    if response.status_code == 200:
        response_200 = response.text
        return response_200

    if response.status_code == 304:
        response_304 = cast(Any, None)
        return response_304

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | str]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | str]:
    """Returns the index.html of the Web UI

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
) -> Any | str:
    """Returns the index.html of the Web UI

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        Any | str
    """

    with operational():
        response = sync_detailed(
            client=client,
        )
    return cast("Any | str", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | str]:
    """Returns the index.html of the Web UI

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
) -> Any | str:
    """Returns the index.html of the Web UI

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        Any | str
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
        )
    return cast("Any | str", expect(response))
