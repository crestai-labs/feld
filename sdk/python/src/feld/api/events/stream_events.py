from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workspace: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["workspace"] = workspace

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/events",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | str | None:
    if response.status_code == 200:
        response_200 = response.text
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

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Error | str]:
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
) -> Response[Error | str]:
    """Notifies resource changes with Server-Sent Events

     Every event has the event name `resource`, and its data is the JSON `{"resource": <kind>, "id":
    <ID>}`. It carries no values; the client redoes the corresponding read.
    The kinds are `workspace`, `actor`, `session`, `record`, `message`, `log`, `quota`, plus `daemon`
    and `resync`, which handle the connection.
    The ID of `actor` is the Actor's `name`; the ID of `record` is the ID of the Session the record was
    added to (the Actor's `name` for a record without a Session); the ID of `log` is the ID of the
    Session whose log cursor advanced. Workspace creation and deletion are notified as `workspace`,
    which belongs to no Workspace. Telemetry arrival is notified as `session` for a change in Session
    metrics, and as `quota` (whose ID is the request ID), which belongs to no Workspace, for a change in
    the quota of Runtime metrics.
    Right after connecting, sends `{"resource":"daemon","id":<instanceId>}`. When the daemon starts to
    stop, sends `{"resource":"daemon","id":"shutdown"}` and closes.
    When notifications fall behind, sends `{"resource":"resync","id":<workspace or "*">}`.
    Sends a keep-alive comment every 15 seconds.
    A `sessionToken` connection that omits `workspace` is treated as specifying the Session's Workspace,
    and any other `workspace` gets `403`. Of the `workspace` events, it receives only those of the
    Session's Workspace. `browserToken` and `sessionToken` connections close when the token becomes
    invalid.

    Args:
        workspace (str | Unset):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | str]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
) -> str:
    """Notifies resource changes with Server-Sent Events

     Every event has the event name `resource`, and its data is the JSON `{"resource": <kind>, "id":
    <ID>}`. It carries no values; the client redoes the corresponding read.
    The kinds are `workspace`, `actor`, `session`, `record`, `message`, `log`, `quota`, plus `daemon`
    and `resync`, which handle the connection.
    The ID of `actor` is the Actor's `name`; the ID of `record` is the ID of the Session the record was
    added to (the Actor's `name` for a record without a Session); the ID of `log` is the ID of the
    Session whose log cursor advanced. Workspace creation and deletion are notified as `workspace`,
    which belongs to no Workspace. Telemetry arrival is notified as `session` for a change in Session
    metrics, and as `quota` (whose ID is the request ID), which belongs to no Workspace, for a change in
    the quota of Runtime metrics.
    Right after connecting, sends `{"resource":"daemon","id":<instanceId>}`. When the daemon starts to
    stop, sends `{"resource":"daemon","id":"shutdown"}` and closes.
    When notifications fall behind, sends `{"resource":"resync","id":<workspace or "*">}`.
    Sends a keep-alive comment every 15 seconds.
    A `sessionToken` connection that omits `workspace` is treated as specifying the Session's Workspace,
    and any other `workspace` gets `403`. Of the `workspace` events, it receives only those of the
    Session's Workspace. `browserToken` and `sessionToken` connections close when the token becomes
    invalid.

    Args:
        workspace (str | Unset):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        str
    """

    with operational():
        response = sync_detailed(
            client=client,
            workspace=workspace,
        )
    return cast("str", expect(response))


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
) -> Response[Error | str]:
    """Notifies resource changes with Server-Sent Events

     Every event has the event name `resource`, and its data is the JSON `{"resource": <kind>, "id":
    <ID>}`. It carries no values; the client redoes the corresponding read.
    The kinds are `workspace`, `actor`, `session`, `record`, `message`, `log`, `quota`, plus `daemon`
    and `resync`, which handle the connection.
    The ID of `actor` is the Actor's `name`; the ID of `record` is the ID of the Session the record was
    added to (the Actor's `name` for a record without a Session); the ID of `log` is the ID of the
    Session whose log cursor advanced. Workspace creation and deletion are notified as `workspace`,
    which belongs to no Workspace. Telemetry arrival is notified as `session` for a change in Session
    metrics, and as `quota` (whose ID is the request ID), which belongs to no Workspace, for a change in
    the quota of Runtime metrics.
    Right after connecting, sends `{"resource":"daemon","id":<instanceId>}`. When the daemon starts to
    stop, sends `{"resource":"daemon","id":"shutdown"}` and closes.
    When notifications fall behind, sends `{"resource":"resync","id":<workspace or "*">}`.
    Sends a keep-alive comment every 15 seconds.
    A `sessionToken` connection that omits `workspace` is treated as specifying the Session's Workspace,
    and any other `workspace` gets `403`. Of the `workspace` events, it receives only those of the
    Session's Workspace. `browserToken` and `sessionToken` connections close when the token becomes
    invalid.

    Args:
        workspace (str | Unset):

    Raises:
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | str]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    workspace: str | Unset = UNSET,
) -> str:
    """Notifies resource changes with Server-Sent Events

     Every event has the event name `resource`, and its data is the JSON `{"resource": <kind>, "id":
    <ID>}`. It carries no values; the client redoes the corresponding read.
    The kinds are `workspace`, `actor`, `session`, `record`, `message`, `log`, `quota`, plus `daemon`
    and `resync`, which handle the connection.
    The ID of `actor` is the Actor's `name`; the ID of `record` is the ID of the Session the record was
    added to (the Actor's `name` for a record without a Session); the ID of `log` is the ID of the
    Session whose log cursor advanced. Workspace creation and deletion are notified as `workspace`,
    which belongs to no Workspace. Telemetry arrival is notified as `session` for a change in Session
    metrics, and as `quota` (whose ID is the request ID), which belongs to no Workspace, for a change in
    the quota of Runtime metrics.
    Right after connecting, sends `{"resource":"daemon","id":<instanceId>}`. When the daemon starts to
    stop, sends `{"resource":"daemon","id":"shutdown"}` and closes.
    When notifications fall behind, sends `{"resource":"resync","id":<workspace or "*">}`.
    Sends a keep-alive comment every 15 seconds.
    A `sessionToken` connection that omits `workspace` is treated as specifying the Session's Workspace,
    and any other `workspace` gets `403`. Of the `workspace` events, it receives only those of the
    Session's Workspace. `browserToken` and `sessionToken` connections close when the token becomes
    invalid.

    Args:
        workspace (str | Unset):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        str
    """

    with operational():
        response = await asyncio_detailed(
            client=client,
            workspace=workspace,
        )
    return cast("str", expect(response))
