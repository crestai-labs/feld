from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.kill_session_response_200 import KillSessionResponse200
from ...types import Response


def _get_kwargs(
    session: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/sessions/{session}/kill".format(
            session=quote(str(session), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | KillSessionResponse200 | None:
    if response.status_code == 200:
        response_200 = KillSessionResponse200.from_dict(response.json())

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
) -> Response[Error | KillSessionResponse200]:
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
) -> Response[Error | KillSessionResponse200]:
    """Requests a Session to be killed

     Records `lifecycle.kill` and `kill.requested` in the same form as stop. Accepted after a stop is
    accepted, as long as there is no `end`.
    A kill after a kill on a Session that is not closed returns the existing record and sets
    `alreadyRequested` to `true`. A kill whose `observed` is `unknown` returns a retried record.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        session (str):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | KillSessionResponse200]
    """

    kwargs = _get_kwargs(
        session=session,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    session: str,
    *,
    client: AuthenticatedClient | Client,
) -> KillSessionResponse200:
    """Requests a Session to be killed

     Records `lifecycle.kill` and `kill.requested` in the same form as stop. Accepted after a stop is
    accepted, as long as there is no `end`.
    A kill after a kill on a Session that is not closed returns the existing record and sets
    `alreadyRequested` to `true`. A kill whose `observed` is `unknown` returns a retried record.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        session (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        KillSessionResponse200
    """

    with operational():
        response = sync_detailed(
            session=session,
            client=client,
        )
    return cast("KillSessionResponse200", expect(response))


async def asyncio_detailed(
    session: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Error | KillSessionResponse200]:
    """Requests a Session to be killed

     Records `lifecycle.kill` and `kill.requested` in the same form as stop. Accepted after a stop is
    accepted, as long as there is no `end`.
    A kill after a kill on a Session that is not closed returns the existing record and sets
    `alreadyRequested` to `true`. A kill whose `observed` is `unknown` returns a retried record.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        session (str):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | KillSessionResponse200]
    """

    kwargs = _get_kwargs(
        session=session,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    session: str,
    *,
    client: AuthenticatedClient | Client,
) -> KillSessionResponse200:
    """Requests a Session to be killed

     Records `lifecycle.kill` and `kill.requested` in the same form as stop. Accepted after a stop is
    accepted, as long as there is no `end`.
    A kill after a kill on a Session that is not closed returns the existing record and sets
    `alreadyRequested` to `true`. A kill whose `observed` is `unknown` returns a retried record.
    The `sessionToken` of a closed Session gets `409` as session closed.

    Args:
        session (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        KillSessionResponse200
    """

    with operational():
        response = await asyncio_detailed(
            session=session,
            client=client,
        )
    return cast("KillSessionResponse200", expect(response))
