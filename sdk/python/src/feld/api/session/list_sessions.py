import datetime
from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.actor_kind import ActorKind
from ...models.error import Error
from ...models.list_sessions_response_200 import ListSessionsResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workspace: str | Unset = UNSET,
    actor: str | Unset = UNSET,
    kind: ActorKind | Unset = UNSET,
    current: bool | Unset = False,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    sub_agents: bool | Unset = False,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["workspace"] = workspace

    params["actor"] = actor

    json_kind: str | Unset = UNSET
    if not isinstance(kind, Unset):
        json_kind = kind.value

    params["kind"] = json_kind

    params["current"] = current

    json_start: str | Unset = UNSET
    if not isinstance(start, Unset):
        json_start = start.isoformat()
    params["start"] = json_start

    json_end: str | Unset = UNSET
    if not isinstance(end, Unset):
        json_end = end.isoformat()
    params["end"] = json_end

    params["subAgents"] = sub_agents

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/sessions",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | ListSessionsResponse200 | None:
    if response.status_code == 200:
        response_200 = ListSessionsResponse200.from_dict(response.json())

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
) -> Response[Error | ListSessionsResponse200]:
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
    kind: ActorKind | Unset = UNSET,
    current: bool | Unset = False,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    sub_agents: bool | Unset = False,
) -> Response[Error | ListSessionsResponse200]:
    """Lists Sessions

     Returns newest first. A time range selects Sessions whose `[start, end)` overlaps it.

    Args:
        workspace (str | Unset):
        actor (str | Unset):
        kind (ActorKind | Unset):
        current (bool | Unset):  Default: False.
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        sub_agents (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | ListSessionsResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
        kind=kind,
        current=current,
        start=start,
        end=end,
        sub_agents=sub_agents,
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
    kind: ActorKind | Unset = UNSET,
    current: bool | Unset = False,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    sub_agents: bool | Unset = False,
) -> ListSessionsResponse200:
    """Lists Sessions

     Returns newest first. A time range selects Sessions whose `[start, end)` overlaps it.

    Args:
        workspace (str | Unset):
        actor (str | Unset):
        kind (ActorKind | Unset):
        current (bool | Unset):  Default: False.
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        sub_agents (bool | Unset):  Default: False.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ListSessionsResponse200
    """

    with operational():
        response = sync_detailed(
            client=client,
            workspace=workspace,
            actor=actor,
            kind=kind,
            current=current,
            start=start,
            end=end,
            sub_agents=sub_agents,
        )
    return cast("ListSessionsResponse200", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    actor: str | Unset = UNSET,
    kind: ActorKind | Unset = UNSET,
    current: bool | Unset = False,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    sub_agents: bool | Unset = False,
) -> Response[Error | ListSessionsResponse200]:
    """Lists Sessions

     Returns newest first. A time range selects Sessions whose `[start, end)` overlaps it.

    Args:
        workspace (str | Unset):
        actor (str | Unset):
        kind (ActorKind | Unset):
        current (bool | Unset):  Default: False.
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        sub_agents (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | ListSessionsResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
        kind=kind,
        current=current,
        start=start,
        end=end,
        sub_agents=sub_agents,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
    actor: str | Unset = UNSET,
    kind: ActorKind | Unset = UNSET,
    current: bool | Unset = False,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    sub_agents: bool | Unset = False,
) -> ListSessionsResponse200:
    """Lists Sessions

     Returns newest first. A time range selects Sessions whose `[start, end)` overlaps it.

    Args:
        workspace (str | Unset):
        actor (str | Unset):
        kind (ActorKind | Unset):
        current (bool | Unset):  Default: False.
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        sub_agents (bool | Unset):  Default: False.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ListSessionsResponse200
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
            workspace=workspace,
            actor=actor,
            kind=kind,
            current=current,
            start=start,
            end=end,
            sub_agents=sub_agents,
        )
    return cast("ListSessionsResponse200", expect(response))
