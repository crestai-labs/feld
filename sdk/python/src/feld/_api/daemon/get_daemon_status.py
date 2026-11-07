from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import _errors as errors
from ..._client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ..._models.daemon_status import DaemonStatus
from ..._models.error import Error
from ..._types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/daemon/status",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> DaemonStatus | Error | None:
    if response.status_code == 200:
        response_200 = DaemonStatus.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = Error.from_dict(response.json())

        return response_401

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[DaemonStatus | Error]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[DaemonStatus | Error]:
    """Returns the daemon status

     `version` is the same value as `feld --version`, `otlp` is the current Collector endpoint,
    `startedAt` is the daemon start time, and `serverTime` is the response time.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[DaemonStatus | Error]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
) -> DaemonStatus:
    """Returns the daemon status

     `version` is the same value as `feld --version`, `otlp` is the current Collector endpoint,
    `startedAt` is the daemon start time, and `serverTime` is the response time.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        DaemonStatus
    """

    with operational():
        response = sync_detailed(
            client=client,
        )
    return cast("DaemonStatus", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[DaemonStatus | Error]:
    """Returns the daemon status

     `version` is the same value as `feld --version`, `otlp` is the current Collector endpoint,
    `startedAt` is the daemon start time, and `serverTime` is the response time.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[DaemonStatus | Error]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
) -> DaemonStatus:
    """Returns the daemon status

     `version` is the same value as `feld --version`, `otlp` is the current Collector endpoint,
    `startedAt` is the daemon start time, and `serverTime` is the response time.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        DaemonStatus
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
        )
    return cast("DaemonStatus", expect(response))
