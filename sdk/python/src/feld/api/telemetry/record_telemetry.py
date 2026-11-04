from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.record_telemetry_body import RecordTelemetryBody
from ...models.record_telemetry_response_200 import RecordTelemetryResponse200
from ...types import Response


def _get_kwargs(
    *,
    body: RecordTelemetryBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/self/telemetry",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | RecordTelemetryResponse200 | None:
    if response.status_code == 200:
        response_200 = RecordTelemetryResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = Error.from_dict(response.json())

        return response_400

    if response.status_code == 401:
        response_401 = Error.from_dict(response.json())

        return response_401

    if response.status_code == 409:
        response_409 = Error.from_dict(response.json())

        return response_409

    if response.status_code == 413:
        response_413 = Error.from_dict(response.json())

        return response_413

    if response.status_code == 422:
        response_422 = Error.from_dict(response.json())

        return response_422

    if response.status_code == 500:
        response_500 = Error.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Error | RecordTelemetryResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: RecordTelemetryBody,
) -> Response[Error | RecordTelemetryResponse200]:
    """Attributes telemetry received by the Collector to a Session and stores it as Telemetry

     The request body is limited to 8 MiB, and the base64-decoded payload to 4 MiB.

    Args:
        body (RecordTelemetryBody):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | RecordTelemetryResponse200]
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
    body: RecordTelemetryBody,
) -> RecordTelemetryResponse200:
    """Attributes telemetry received by the Collector to a Session and stores it as Telemetry

     The request body is limited to 8 MiB, and the base64-decoded payload to 4 MiB.

    Args:
        body (RecordTelemetryBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        RecordTelemetryResponse200
    """

    with operational():
        response = sync_detailed(
            client=client,
            body=body,
        )
    return cast("RecordTelemetryResponse200", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: RecordTelemetryBody,
) -> Response[Error | RecordTelemetryResponse200]:
    """Attributes telemetry received by the Collector to a Session and stores it as Telemetry

     The request body is limited to 8 MiB, and the base64-decoded payload to 4 MiB.

    Args:
        body (RecordTelemetryBody):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | RecordTelemetryResponse200]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: RecordTelemetryBody,
) -> RecordTelemetryResponse200:
    """Attributes telemetry received by the Collector to a Session and stores it as Telemetry

     The request body is limited to 8 MiB, and the base64-decoded payload to 4 MiB.

    Args:
        body (RecordTelemetryBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        RecordTelemetryResponse200
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
            body=body,
        )
    return cast("RecordTelemetryResponse200", expect(response))
