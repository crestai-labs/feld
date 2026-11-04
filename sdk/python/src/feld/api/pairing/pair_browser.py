from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.pair_browser_body import PairBrowserBody
from ...models.pair_browser_response_200 import PairBrowserResponse200
from ...types import Response


def _get_kwargs(
    *,
    body: PairBrowserBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/ui/pair",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | PairBrowserResponse200 | None:
    if response.status_code == 200:
        response_200 = PairBrowserResponse200.from_dict(response.json())

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
) -> Response[Error | PairBrowserResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: PairBrowserBody,
) -> Response[Error | PairBrowserResponse200]:
    """Redeems a pairing code for a browser token

     `code` is matched ignoring case and hyphens, and is consumed. Issues a browser token and returns the
    cookie value in `Set-Cookie` and the key in the body's `key`. If the request carries a valid browser
    token cookie and key, revokes that token before issuing.
    `403` if `Origin` does not match the request's destination. `401` if the code does not exist, has
    expired, or is already used. After 5 consecutive `401`s, invalidates all codes valid at that moment.

    Args:
        body (PairBrowserBody):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | PairBrowserResponse200]
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
    client: AuthenticatedClient | Client,
    body: PairBrowserBody,
) -> PairBrowserResponse200:
    """Redeems a pairing code for a browser token

     `code` is matched ignoring case and hyphens, and is consumed. Issues a browser token and returns the
    cookie value in `Set-Cookie` and the key in the body's `key`. If the request carries a valid browser
    token cookie and key, revokes that token before issuing.
    `403` if `Origin` does not match the request's destination. `401` if the code does not exist, has
    expired, or is already used. After 5 consecutive `401`s, invalidates all codes valid at that moment.

    Args:
        body (PairBrowserBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        PairBrowserResponse200
    """

    with operational():
        response = sync_detailed(
            client=client,
            body=body,
        )
    return cast("PairBrowserResponse200", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: PairBrowserBody,
) -> Response[Error | PairBrowserResponse200]:
    """Redeems a pairing code for a browser token

     `code` is matched ignoring case and hyphens, and is consumed. Issues a browser token and returns the
    cookie value in `Set-Cookie` and the key in the body's `key`. If the request carries a valid browser
    token cookie and key, revokes that token before issuing.
    `403` if `Origin` does not match the request's destination. `401` if the code does not exist, has
    expired, or is already used. After 5 consecutive `401`s, invalidates all codes valid at that moment.

    Args:
        body (PairBrowserBody):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | PairBrowserResponse200]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: PairBrowserBody,
) -> PairBrowserResponse200:
    """Redeems a pairing code for a browser token

     `code` is matched ignoring case and hyphens, and is consumed. Issues a browser token and returns the
    cookie value in `Set-Cookie` and the key in the body's `key`. If the request carries a valid browser
    token cookie and key, revokes that token before issuing.
    `403` if `Origin` does not match the request's destination. `401` if the code does not exist, has
    expired, or is already used. After 5 consecutive `401`s, invalidates all codes valid at that moment.

    Args:
        body (PairBrowserBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        PairBrowserResponse200
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
            body=body,
        )
    return cast("PairBrowserResponse200", expect(response))
