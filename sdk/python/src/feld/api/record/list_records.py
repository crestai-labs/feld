import datetime
from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.list_records_response_200 import ListRecordsResponse200
from ...models.record_kind import RecordKind
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workspace: str | Unset = UNSET,
    actor: str | Unset = UNSET,
    session: str | Unset = UNSET,
    kind: RecordKind | Unset = UNSET,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    after: int | Unset = 0,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["workspace"] = workspace

    params["actor"] = actor

    params["session"] = session

    json_kind: str | Unset = UNSET
    if not isinstance(kind, Unset):
        json_kind = kind.value

    params["kind"] = json_kind

    json_start: str | Unset = UNSET
    if not isinstance(start, Unset):
        json_start = start.isoformat()
    params["start"] = json_start

    json_end: str | Unset = UNSET
    if not isinstance(end, Unset):
        json_end = end.isoformat()
    params["end"] = json_end

    params["after"] = after

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/records",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | ListRecordsResponse200 | None:
    if response.status_code == 200:
        response_200 = ListRecordsResponse200.from_dict(response.json())

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

    if response.status_code == 500:
        response_500 = Error.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Error | ListRecordsResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    actor: str | Unset = UNSET,
    session: str | Unset = UNSET,
    kind: RecordKind | Unset = UNSET,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    after: int | Unset = 0,
) -> Response[Error | ListRecordsResponse200]:
    """Searches records

     Returns records in `sequence` order (the overall order along time). Pass `nextAfter` as `after` to
    read the rest.

    Args:
        workspace (str | Unset):
        actor (str | Unset):
        session (str | Unset):
        kind (RecordKind | Unset):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        after (int | Unset):  Default: 0.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | ListRecordsResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
        session=session,
        kind=kind,
        start=start,
        end=end,
        after=after,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    actor: str | Unset = UNSET,
    session: str | Unset = UNSET,
    kind: RecordKind | Unset = UNSET,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    after: int | Unset = 0,
) -> ListRecordsResponse200:
    """Searches records

     Returns records in `sequence` order (the overall order along time). Pass `nextAfter` as `after` to
    read the rest.

    Args:
        workspace (str | Unset):
        actor (str | Unset):
        session (str | Unset):
        kind (RecordKind | Unset):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        after (int | Unset):  Default: 0.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ListRecordsResponse200
    """

    with operational():
        response = sync_detailed(
            client=client,
            workspace=workspace,
            actor=actor,
            session=session,
            kind=kind,
            start=start,
            end=end,
            after=after,
        )
    return cast("ListRecordsResponse200", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    actor: str | Unset = UNSET,
    session: str | Unset = UNSET,
    kind: RecordKind | Unset = UNSET,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    after: int | Unset = 0,
) -> Response[Error | ListRecordsResponse200]:
    """Searches records

     Returns records in `sequence` order (the overall order along time). Pass `nextAfter` as `after` to
    read the rest.

    Args:
        workspace (str | Unset):
        actor (str | Unset):
        session (str | Unset):
        kind (RecordKind | Unset):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        after (int | Unset):  Default: 0.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | ListRecordsResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
        session=session,
        kind=kind,
        start=start,
        end=end,
        after=after,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    actor: str | Unset = UNSET,
    session: str | Unset = UNSET,
    kind: RecordKind | Unset = UNSET,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    after: int | Unset = 0,
) -> ListRecordsResponse200:
    """Searches records

     Returns records in `sequence` order (the overall order along time). Pass `nextAfter` as `after` to
    read the rest.

    Args:
        workspace (str | Unset):
        actor (str | Unset):
        session (str | Unset):
        kind (RecordKind | Unset):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        after (int | Unset):  Default: 0.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ListRecordsResponse200
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
            workspace=workspace,
            actor=actor,
            session=session,
            kind=kind,
            start=start,
            end=end,
            after=after,
        )
    return cast("ListRecordsResponse200", expect(response))
