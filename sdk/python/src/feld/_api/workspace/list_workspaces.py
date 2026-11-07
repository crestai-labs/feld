from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import _errors as errors
from ..._client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ..._models.error import Error
from ..._models.list_workspaces_response_200 import ListWorkspacesResponse200
from ..._types import UNSET, Response, Unset


def _get_kwargs(
    *,
    include_deleted: bool | Unset = False,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["includeDeleted"] = include_deleted

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/workspaces",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | ListWorkspacesResponse200 | None:
    if response.status_code == 200:
        response_200 = ListWorkspacesResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = Error.from_dict(response.json())

        return response_401

    if response.status_code == 500:
        response_500 = Error.from_dict(response.json())

        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Error | ListWorkspacesResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    include_deleted: bool | Unset = False,
) -> Response[Error | ListWorkspacesResponse200]:
    """Lists Workspaces

    Args:
        include_deleted (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | ListWorkspacesResponse200]
    """

    kwargs = _get_kwargs(
        include_deleted=include_deleted,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    include_deleted: bool | Unset = False,
) -> ListWorkspacesResponse200:
    """Lists Workspaces

    Args:
        include_deleted (bool | Unset):  Default: False.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ListWorkspacesResponse200
    """

    with operational():
        response = sync_detailed(
            client=client,
            include_deleted=include_deleted,
        )
    return cast("ListWorkspacesResponse200", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    include_deleted: bool | Unset = False,
) -> Response[Error | ListWorkspacesResponse200]:
    """Lists Workspaces

    Args:
        include_deleted (bool | Unset):  Default: False.

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | ListWorkspacesResponse200]
    """

    kwargs = _get_kwargs(
        include_deleted=include_deleted,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    include_deleted: bool | Unset = False,
) -> ListWorkspacesResponse200:
    """Lists Workspaces

    Args:
        include_deleted (bool | Unset):  Default: False.

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        ListWorkspacesResponse200
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
            include_deleted=include_deleted,
        )
    return cast("ListWorkspacesResponse200", expect(response))
