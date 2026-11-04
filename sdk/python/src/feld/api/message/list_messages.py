import datetime
from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.message_summary_list import MessageSummaryList
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workspace: str | Unset = UNSET,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    label: str | Unset = UNSET,
    waiting: bool | Unset = False,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["workspace"] = workspace

    params["from"] = from_

    params["to"] = to

    params["label"] = label

    params["waiting"] = waiting

    json_start: str | Unset = UNSET
    if not isinstance(start, Unset):
        json_start = start.isoformat()
    params["start"] = json_start

    json_end: str | Unset = UNSET
    if not isinstance(end, Unset):
        json_end = end.isoformat()
    params["end"] = json_end

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/messages",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | MessageSummaryList | None:
    if response.status_code == 200:
        response_200 = MessageSummaryList.from_dict(response.json())

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
) -> Response[Error | MessageSummaryList]:
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
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    label: str | Unset = UNSET,
    waiting: bool | Unset = False,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
) -> Response[Error | MessageSummaryList]:
    """Returns Messages that are not deleted

     Returns Messages oldest `accepted` first. The time range filters on `accepted`. An Actor's mailbox
    is the result of filtering by `to`. The body is shown as `preview`; read the full text with `GET
    /v1/messages/{message}`.

    Args:
        workspace (str | Unset):
        from_ (str | Unset):
        to (str | Unset):
        label (str | Unset):
        waiting (bool | Unset):  Default: False.
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | MessageSummaryList]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        from_=from_,
        to=to,
        label=label,
        waiting=waiting,
        start=start,
        end=end,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    label: str | Unset = UNSET,
    waiting: bool | Unset = False,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
) -> MessageSummaryList:
    """Returns Messages that are not deleted

     Returns Messages oldest `accepted` first. The time range filters on `accepted`. An Actor's mailbox
    is the result of filtering by `to`. The body is shown as `preview`; read the full text with `GET
    /v1/messages/{message}`.

    Args:
        workspace (str | Unset):
        from_ (str | Unset):
        to (str | Unset):
        label (str | Unset):
        waiting (bool | Unset):  Default: False.
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        MessageSummaryList
    """

    with operational():
        response = sync_detailed(
            client=client,
            workspace=workspace,
            from_=from_,
            to=to,
            label=label,
            waiting=waiting,
            start=start,
            end=end,
        )
    return cast("MessageSummaryList", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    label: str | Unset = UNSET,
    waiting: bool | Unset = False,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
) -> Response[Error | MessageSummaryList]:
    """Returns Messages that are not deleted

     Returns Messages oldest `accepted` first. The time range filters on `accepted`. An Actor's mailbox
    is the result of filtering by `to`. The body is shown as `preview`; read the full text with `GET
    /v1/messages/{message}`.

    Args:
        workspace (str | Unset):
        from_ (str | Unset):
        to (str | Unset):
        label (str | Unset):
        waiting (bool | Unset):  Default: False.
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | MessageSummaryList]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        from_=from_,
        to=to,
        label=label,
        waiting=waiting,
        start=start,
        end=end,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    from_: str | Unset = UNSET,
    to: str | Unset = UNSET,
    label: str | Unset = UNSET,
    waiting: bool | Unset = False,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
) -> MessageSummaryList:
    """Returns Messages that are not deleted

     Returns Messages oldest `accepted` first. The time range filters on `accepted`. An Actor's mailbox
    is the result of filtering by `to`. The body is shown as `preview`; read the full text with `GET
    /v1/messages/{message}`.

    Args:
        workspace (str | Unset):
        from_ (str | Unset):
        to (str | Unset):
        label (str | Unset):
        waiting (bool | Unset):  Default: False.
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        MessageSummaryList
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
            workspace=workspace,
            from_=from_,
            to=to,
            label=label,
            waiting=waiting,
            start=start,
            end=end,
        )
    return cast("MessageSummaryList", expect(response))
