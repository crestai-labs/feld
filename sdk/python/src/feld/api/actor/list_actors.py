from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.actor_kind import ActorKind
from ...models.error import Error
from ...models.list_actors_response_200 import ListActorsResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    workspace: str,
    *,
    kind: ActorKind | Unset = UNSET,
    include_deleted: bool | Unset = False,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    json_kind: str | Unset = UNSET
    if not isinstance(kind, Unset):
        json_kind = kind.value

    params["kind"] = json_kind

    params["includeDeleted"] = include_deleted

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/workspaces/{workspace}/actors".format(
            workspace=quote(str(workspace), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | ListActorsResponse200 | None:
    if response.status_code == 200:
        response_200 = ListActorsResponse200.from_dict(response.json())

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

    if response.status_code == 500:
        response_500 = Error.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Error | ListActorsResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    kind: ActorKind | Unset = UNSET,
    include_deleted: bool | Unset = False,
) -> Response[Error | ListActorsResponse200]:
    """Lists the Actors of a Workspace

     Orders Actors with a current Session first, then Actors in `attention`, then the rest; ties are
    ordered by `name`, then `createdAt`.

    Args:
        workspace (str):
        kind (ActorKind | Unset):
        include_deleted (bool | Unset):  Default: False.

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | ListActorsResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        kind=kind,
        include_deleted=include_deleted,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    kind: ActorKind | Unset = UNSET,
    include_deleted: bool | Unset = False,
) -> ListActorsResponse200:
    """Lists the Actors of a Workspace

     Orders Actors with a current Session first, then Actors in `attention`, then the rest; ties are
    ordered by `name`, then `createdAt`.

    Args:
        workspace (str):
        kind (ActorKind | Unset):
        include_deleted (bool | Unset):  Default: False.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ListActorsResponse200
    """

    with operational():
        response = sync_detailed(
            workspace=workspace,
            client=client,
            kind=kind,
            include_deleted=include_deleted,
        )
    return cast("ListActorsResponse200", expect(response))


async def asyncio_detailed(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    kind: ActorKind | Unset = UNSET,
    include_deleted: bool | Unset = False,
) -> Response[Error | ListActorsResponse200]:
    """Lists the Actors of a Workspace

     Orders Actors with a current Session first, then Actors in `attention`, then the rest; ties are
    ordered by `name`, then `createdAt`.

    Args:
        workspace (str):
        kind (ActorKind | Unset):
        include_deleted (bool | Unset):  Default: False.

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | ListActorsResponse200]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        kind=kind,
        include_deleted=include_deleted,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    kind: ActorKind | Unset = UNSET,
    include_deleted: bool | Unset = False,
) -> ListActorsResponse200:
    """Lists the Actors of a Workspace

     Orders Actors with a current Session first, then Actors in `attention`, then the rest; ties are
    ordered by `name`, then `createdAt`.

    Args:
        workspace (str):
        kind (ActorKind | Unset):
        include_deleted (bool | Unset):  Default: False.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ListActorsResponse200
    """

    with operational():
        response = await asyncio_detailed(
            workspace=workspace,
            client=client,
            kind=kind,
            include_deleted=include_deleted,
        )
    return cast("ListActorsResponse200", expect(response))
