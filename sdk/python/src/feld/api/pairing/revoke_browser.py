from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.revoke_browser_response_200 import RevokeBrowserResponse200
from ...types import Response


def _get_kwargs(
    browser_id: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "delete",
        "url": "/v1/ui/browsers/{browser_id}".format(
            browser_id=quote(str(browser_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | RevokeBrowserResponse200 | None:
    if response.status_code == 200:
        response_200 = RevokeBrowserResponse200.from_dict(response.json())

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

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Error | RevokeBrowserResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    browser_id: str,
    *,
    client: AuthenticatedClient,
) -> Response[Error | RevokeBrowserResponse200]:
    """Revokes a browser token

     The browser's next request gets `401`. An `id` that does not exist, has expired, or is already
    revoked gets `404`. `browserToken` and `sessionToken` get `403`.

    Args:
        browser_id (str):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | RevokeBrowserResponse200]
    """

    kwargs = _get_kwargs(
        browser_id=browser_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    browser_id: str,
    *,
    client: AuthenticatedClient,
) -> RevokeBrowserResponse200:
    """Revokes a browser token

     The browser's next request gets `401`. An `id` that does not exist, has expired, or is already
    revoked gets `404`. `browserToken` and `sessionToken` get `403`.

    Args:
        browser_id (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        RevokeBrowserResponse200
    """

    with operational():
        response = sync_detailed(
            browser_id=browser_id,
            client=client,
        )
    return cast("RevokeBrowserResponse200", expect(response))


async def asyncio_detailed(
    browser_id: str,
    *,
    client: AuthenticatedClient,
) -> Response[Error | RevokeBrowserResponse200]:
    """Revokes a browser token

     The browser's next request gets `401`. An `id` that does not exist, has expired, or is already
    revoked gets `404`. `browserToken` and `sessionToken` get `403`.

    Args:
        browser_id (str):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | RevokeBrowserResponse200]
    """

    kwargs = _get_kwargs(
        browser_id=browser_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    browser_id: str,
    *,
    client: AuthenticatedClient,
) -> RevokeBrowserResponse200:
    """Revokes a browser token

     The browser's next request gets `401`. An `id` that does not exist, has expired, or is already
    revoked gets `404`. `browserToken` and `sessionToken` get `403`.

    Args:
        browser_id (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        RevokeBrowserResponse200
    """

    with operational():
        response = await asyncio_detailed(
            browser_id=browser_id,
            client=client,
        )
    return cast("RevokeBrowserResponse200", expect(response))
