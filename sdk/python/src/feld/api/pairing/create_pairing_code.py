from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.create_pairing_code_body import CreatePairingCodeBody
from ...models.error import Error
from ...models.pairing_code import PairingCode
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: CreatePairingCodeBody | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/ui/pairing-codes",
    }

    if not isinstance(body, Unset):
        _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | PairingCode | None:
    if response.status_code == 200:
        response_200 = PairingCode.from_dict(response.json())

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

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Error | PairingCode]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient,
    body: CreatePairingCodeBody | Unset = UNSET,
) -> Response[Error | PairingCode]:
    """Issues a pairing code

     The code is 8 characters drawn from 31 characters, the digits and uppercase letters excluding `0`,
    `1`, `I`, `L`, and `O`, and is returned in the form `XXXX-XXXX`. It is valid for 5 minutes after
    issue and can be used once. The daemon keeps the code only in memory, as a hash.
    `name` becomes the name of the browser token issued on redemption. `browserToken` and `sessionToken`
    get `403`.

    Args:
        body (CreatePairingCodeBody | Unset):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | PairingCode]
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
    body: CreatePairingCodeBody | Unset = UNSET,
) -> PairingCode:
    """Issues a pairing code

     The code is 8 characters drawn from 31 characters, the digits and uppercase letters excluding `0`,
    `1`, `I`, `L`, and `O`, and is returned in the form `XXXX-XXXX`. It is valid for 5 minutes after
    issue and can be used once. The daemon keeps the code only in memory, as a hash.
    `name` becomes the name of the browser token issued on redemption. `browserToken` and `sessionToken`
    get `403`.

    Args:
        body (CreatePairingCodeBody | Unset):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        PairingCode
    """

    with operational():
        response = sync_detailed(
            client=client,
            body=body,
        )
    return cast("PairingCode", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient,
    body: CreatePairingCodeBody | Unset = UNSET,
) -> Response[Error | PairingCode]:
    """Issues a pairing code

     The code is 8 characters drawn from 31 characters, the digits and uppercase letters excluding `0`,
    `1`, `I`, `L`, and `O`, and is returned in the form `XXXX-XXXX`. It is valid for 5 minutes after
    issue and can be used once. The daemon keeps the code only in memory, as a hash.
    `name` becomes the name of the browser token issued on redemption. `browserToken` and `sessionToken`
    get `403`.

    Args:
        body (CreatePairingCodeBody | Unset):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | PairingCode]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient,
    body: CreatePairingCodeBody | Unset = UNSET,
) -> PairingCode:
    """Issues a pairing code

     The code is 8 characters drawn from 31 characters, the digits and uppercase letters excluding `0`,
    `1`, `I`, `L`, and `O`, and is returned in the form `XXXX-XXXX`. It is valid for 5 minutes after
    issue and can be used once. The daemon keeps the code only in memory, as a hash.
    `name` becomes the name of the browser token issued on redemption. `browserToken` and `sessionToken`
    get `403`.

    Args:
        body (CreatePairingCodeBody | Unset):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        PairingCode
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
            body=body,
        )
    return cast("PairingCode", expect(response))
