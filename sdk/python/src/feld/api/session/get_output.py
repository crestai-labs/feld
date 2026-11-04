from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.output_page import OutputPage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    session: str,
    *,
    after: int | Unset = 0,
    before: int | Unset = UNSET,
    limit: int | Unset = UNSET,
    tail: bool | Unset = False,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["after"] = after

    params["before"] = before

    params["limit"] = limit

    params["tail"] = tail

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/sessions/{session}/output".format(
            session=quote(str(session), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | OutputPage | None:
    if response.status_code == 200:
        response_200 = OutputPage.from_dict(response.json())

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
) -> Response[Error | OutputPage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    session: str,
    *,
    client: AuthenticatedClient | Client,
    after: int | Unset = 0,
    before: int | Unset = UNSET,
    limit: int | Unset = UNSET,
    tail: bool | Unset = False,
) -> Response[Error | OutputPage]:
    """Returns the log (stdout / stderr lines) of a program Session

    Args:
        session (str):
        after (int | Unset):  Default: 0.
        before (int | Unset):
        limit (int | Unset):
        tail (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | OutputPage]
    """

    kwargs = _get_kwargs(
        session=session,
        after=after,
        before=before,
        limit=limit,
        tail=tail,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    session: str,
    *,
    client: AuthenticatedClient | Client,
    after: int | Unset = 0,
    before: int | Unset = UNSET,
    limit: int | Unset = UNSET,
    tail: bool | Unset = False,
) -> OutputPage:
    """Returns the log (stdout / stderr lines) of a program Session

    Args:
        session (str):
        after (int | Unset):  Default: 0.
        before (int | Unset):
        limit (int | Unset):
        tail (bool | Unset):  Default: False.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        OutputPage
    """

    with operational():
        response = sync_detailed(
            session=session,
            client=client,
            after=after,
            before=before,
            limit=limit,
            tail=tail,
        )
    return cast("OutputPage", expect(response))


async def asyncio_detailed(
    session: str,
    *,
    client: AuthenticatedClient | Client,
    after: int | Unset = 0,
    before: int | Unset = UNSET,
    limit: int | Unset = UNSET,
    tail: bool | Unset = False,
) -> Response[Error | OutputPage]:
    """Returns the log (stdout / stderr lines) of a program Session

    Args:
        session (str):
        after (int | Unset):  Default: 0.
        before (int | Unset):
        limit (int | Unset):
        tail (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | OutputPage]
    """

    kwargs = _get_kwargs(
        session=session,
        after=after,
        before=before,
        limit=limit,
        tail=tail,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    session: str,
    *,
    client: AuthenticatedClient | Client,
    after: int | Unset = 0,
    before: int | Unset = UNSET,
    limit: int | Unset = UNSET,
    tail: bool | Unset = False,
) -> OutputPage:
    """Returns the log (stdout / stderr lines) of a program Session

    Args:
        session (str):
        after (int | Unset):  Default: 0.
        before (int | Unset):
        limit (int | Unset):
        tail (bool | Unset):  Default: False.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        OutputPage
    """

    with operational():
        response = await asyncio_detailed(
            session=session,
            client=client,
            after=after,
            before=before,
            limit=limit,
            tail=tail,
        )
    return cast("OutputPage", expect(response))
