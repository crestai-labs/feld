from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...types import Response


def _get_kwargs(
    path: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/ui/{path}".format(
            path=quote(str(path), safe=""),
        ),
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

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

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
    path: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | str]:
    """Returns a file of the Web UI build, or index.html for a page route

     If `path` is a file of the build, returns that file. For a path without an extension (a page route
    such as `daemon` or `settings`),
    returns index.html, and the page renders the route. Every file is returned with `Cache-Control: no-
    cache` and an `ETag` of the SHA-256 of its content,
    and `304` is returned if `If-None-Match` matches the `ETag`.

    Args:
        path (str):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        path=path,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    path: str,
    *,
    client: AuthenticatedClient | Client,
) -> Any | str:
    """Returns a file of the Web UI build, or index.html for a page route

     If `path` is a file of the build, returns that file. For a path without an extension (a page route
    such as `daemon` or `settings`),
    returns index.html, and the page renders the route. Every file is returned with `Cache-Control: no-
    cache` and an `ETag` of the SHA-256 of its content,
    and `304` is returned if `If-None-Match` matches the `ETag`.

    Args:
        path (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        Any | str
    """

    with operational():
        response = sync_detailed(
            path=path,
            client=client,
        )
    return cast("Any | str", expect(response))


async def asyncio_detailed(
    path: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | str]:
    """Returns a file of the Web UI build, or index.html for a page route

     If `path` is a file of the build, returns that file. For a path without an extension (a page route
    such as `daemon` or `settings`),
    returns index.html, and the page renders the route. Every file is returned with `Cache-Control: no-
    cache` and an `ETag` of the SHA-256 of its content,
    and `304` is returned if `If-None-Match` matches the `ETag`.

    Args:
        path (str):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        path=path,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    path: str,
    *,
    client: AuthenticatedClient | Client,
) -> Any | str:
    """Returns a file of the Web UI build, or index.html for a page route

     If `path` is a file of the build, returns that file. For a path without an extension (a page route
    such as `daemon` or `settings`),
    returns index.html, and the page renders the route. Every file is returned with `Cache-Control: no-
    cache` and an `ETag` of the SHA-256 of its content,
    and `304` is returned if `If-None-Match` matches the `ETag`.

    Args:
        path (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        Any | str
    """

    with operational():
        response = await asyncio_detailed(
            path=path,
            client=client,
        )
    return cast("Any | str", expect(response))
