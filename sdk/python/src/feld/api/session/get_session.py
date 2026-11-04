from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.session_response import SessionResponse
from ...models.sub_agent_session_response import SubAgentSessionResponse
from ...types import Response


def _get_kwargs(
    session: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/sessions/{session}".format(
            session=quote(str(session), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | SessionResponse | SubAgentSessionResponse | None:
    if response.status_code == 200:

        def _parse_response_200(
            data: object,
        ) -> SessionResponse | SubAgentSessionResponse:
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                response_200_type_0 = SessionResponse.from_dict(data)

                return response_200_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            response_200_type_1 = SubAgentSessionResponse.from_dict(data)

            return response_200_type_1

        response_200 = _parse_response_200(response.json())

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

    if response.status_code == 500:
        response_500 = Error.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Error | SessionResponse | SubAgentSessionResponse]:
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
) -> Response[Error | SessionResponse | SubAgentSessionResponse]:
    """Returns a Session and the Sub Agent Sessions projected from its transcript entries

     Given the ID of a Sub Agent Session, returns that Sub Agent Session projected from the transcript
    entries of the parent Session.

    Args:
        session (str):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | SessionResponse | SubAgentSessionResponse]
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
) -> SessionResponse | SubAgentSessionResponse:
    """Returns a Session and the Sub Agent Sessions projected from its transcript entries

     Given the ID of a Sub Agent Session, returns that Sub Agent Session projected from the transcript
    entries of the parent Session.

    Args:
        session (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        SessionResponse | SubAgentSessionResponse
    """

    with operational():
        response = sync_detailed(
            session=session,
            client=client,
        )
    return cast("SessionResponse | SubAgentSessionResponse", expect(response))


async def asyncio_detailed(
    session: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Error | SessionResponse | SubAgentSessionResponse]:
    """Returns a Session and the Sub Agent Sessions projected from its transcript entries

     Given the ID of a Sub Agent Session, returns that Sub Agent Session projected from the transcript
    entries of the parent Session.

    Args:
        session (str):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | SessionResponse | SubAgentSessionResponse]
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
) -> SessionResponse | SubAgentSessionResponse:
    """Returns a Session and the Sub Agent Sessions projected from its transcript entries

     Given the ID of a Sub Agent Session, returns that Sub Agent Session projected from the transcript
    entries of the parent Session.

    Args:
        session (str):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        SessionResponse | SubAgentSessionResponse
    """

    with operational():
        response = await asyncio_detailed(
            session=session,
            client=client,
        )
    return cast("SessionResponse | SubAgentSessionResponse", expect(response))
