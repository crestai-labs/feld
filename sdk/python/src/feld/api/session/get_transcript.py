from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.get_transcript_view import GetTranscriptView
from ...models.transcript_page import TranscriptPage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    session: str,
    *,
    after: str | Unset = UNSET,
    before: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    tail: bool | Unset = False,
    view: GetTranscriptView | Unset = GetTranscriptView.ALL,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["after"] = after

    params["before"] = before

    params["limit"] = limit

    params["tail"] = tail

    json_view: str | Unset = UNSET
    if not isinstance(view, Unset):
        json_view = view.value

    params["view"] = json_view

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/sessions/{session}/transcript".format(
            session=quote(str(session), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | TranscriptPage | None:
    if response.status_code == 200:
        response_200 = TranscriptPage.from_dict(response.json())

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
) -> Response[Error | TranscriptPage]:
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
    after: str | Unset = UNSET,
    before: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    tail: bool | Unset = False,
    view: GetTranscriptView | Unset = GetTranscriptView.ALL,
) -> Response[Error | TranscriptPage]:
    """Returns the transcript of an agent-runtime Session

     A sequence of transcript entries that the Runtime Adapter projects from the transcript file into a
    common form. Given the ID of a Sub Agent Session, returns the transcript entries of that Sub Agent.

    Args:
        session (str):
        after (str | Unset):
        before (str | Unset):
        limit (int | Unset):
        tail (bool | Unset):  Default: False.
        view (GetTranscriptView | Unset):  Default: GetTranscriptView.ALL.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | TranscriptPage]
    """

    kwargs = _get_kwargs(
        session=session,
        after=after,
        before=before,
        limit=limit,
        tail=tail,
        view=view,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    session: str,
    *,
    client: AuthenticatedClient | Client,
    after: str | Unset = UNSET,
    before: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    tail: bool | Unset = False,
    view: GetTranscriptView | Unset = GetTranscriptView.ALL,
) -> TranscriptPage:
    """Returns the transcript of an agent-runtime Session

     A sequence of transcript entries that the Runtime Adapter projects from the transcript file into a
    common form. Given the ID of a Sub Agent Session, returns the transcript entries of that Sub Agent.

    Args:
        session (str):
        after (str | Unset):
        before (str | Unset):
        limit (int | Unset):
        tail (bool | Unset):  Default: False.
        view (GetTranscriptView | Unset):  Default: GetTranscriptView.ALL.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        TranscriptPage
    """

    with operational():
        response = sync_detailed(
            session=session,
            client=client,
            after=after,
            before=before,
            limit=limit,
            tail=tail,
            view=view,
        )
    return cast("TranscriptPage", expect(response))


async def asyncio_detailed(
    session: str,
    *,
    client: AuthenticatedClient | Client,
    after: str | Unset = UNSET,
    before: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    tail: bool | Unset = False,
    view: GetTranscriptView | Unset = GetTranscriptView.ALL,
) -> Response[Error | TranscriptPage]:
    """Returns the transcript of an agent-runtime Session

     A sequence of transcript entries that the Runtime Adapter projects from the transcript file into a
    common form. Given the ID of a Sub Agent Session, returns the transcript entries of that Sub Agent.

    Args:
        session (str):
        after (str | Unset):
        before (str | Unset):
        limit (int | Unset):
        tail (bool | Unset):  Default: False.
        view (GetTranscriptView | Unset):  Default: GetTranscriptView.ALL.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | TranscriptPage]
    """

    kwargs = _get_kwargs(
        session=session,
        after=after,
        before=before,
        limit=limit,
        tail=tail,
        view=view,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    session: str,
    *,
    client: AuthenticatedClient | Client,
    after: str | Unset = UNSET,
    before: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    tail: bool | Unset = False,
    view: GetTranscriptView | Unset = GetTranscriptView.ALL,
) -> TranscriptPage:
    """Returns the transcript of an agent-runtime Session

     A sequence of transcript entries that the Runtime Adapter projects from the transcript file into a
    common form. Given the ID of a Sub Agent Session, returns the transcript entries of that Sub Agent.

    Args:
        session (str):
        after (str | Unset):
        before (str | Unset):
        limit (int | Unset):
        tail (bool | Unset):  Default: False.
        view (GetTranscriptView | Unset):  Default: GetTranscriptView.ALL.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        TranscriptPage
    """

    with operational():
        response = await asyncio_detailed(
            session=session,
            client=client,
            after=after,
            before=before,
            limit=limit,
            tail=tail,
            view=view,
        )
    return cast("TranscriptPage", expect(response))
