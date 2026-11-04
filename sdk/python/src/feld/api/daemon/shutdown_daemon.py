from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.shutdown_daemon_response_200 import ShutdownDaemonResponse200
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/daemon/shutdown",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | ShutdownDaemonResponse200 | None:
    if response.status_code == 200:
        response_200 = ShutdownDaemonResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = Error.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = Error.from_dict(response.json())

        return response_403

    if response.status_code == 500:
        response_500 = Error.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Error | ShutdownDaemonResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
) -> Response[Error | ShutdownDaemonResponse200]:
    """Requests the daemon to stop

     Leaves Sessions untouched.

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | ShutdownDaemonResponse200]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient,
) -> ShutdownDaemonResponse200:
    """Requests the daemon to stop

     Leaves Sessions untouched.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ShutdownDaemonResponse200
    """

    with operational():
        response = sync_detailed(
            client=client,
        )
    return cast("ShutdownDaemonResponse200", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
) -> Response[Error | ShutdownDaemonResponse200]:
    """Requests the daemon to stop

     Leaves Sessions untouched.

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | ShutdownDaemonResponse200]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
) -> ShutdownDaemonResponse200:
    """Requests the daemon to stop

     Leaves Sessions untouched.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ShutdownDaemonResponse200
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
        )
    return cast("ShutdownDaemonResponse200", expect(response))
