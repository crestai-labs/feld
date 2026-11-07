from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import _errors as errors
from ..._client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ..._models.declare_actor_body import DeclareActorBody
from ..._models.declare_response import DeclareResponse
from ..._models.error import Error
from ..._types import UNSET, Response, Unset


def _get_kwargs(
    workspace: str,
    actor: str,
    *,
    body: DeclareActorBody,
    dry_run: bool | Unset = False,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    params: dict[str, Any] = {}

    params["dryRun"] = dry_run

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "put",
        "url": "/v1/workspaces/{workspace}/actors/{actor}".format(
            workspace=quote(str(workspace), safe=""),
            actor=quote(str(actor), safe=""),
        ),
        "params": params,
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> DeclareResponse | Error | None:
    if response.status_code == 200:
        response_200 = DeclareResponse.from_dict(response.json())

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
) -> Response[DeclareResponse | Error]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient | Client,
    body: DeclareActorBody,
    dry_run: bool | Unset = False,
) -> Response[DeclareResponse | Error]:
    """Declares an Actor once, or with `dryRun` answers what the declaration would change

     Makes the Actor match the declaration once: creates it when no live Actor holds the name (`action:
    create`), brings a differing live Actor to match while keeping its `id` (`replace`), or leaves a
    matching one as it is (`unchanged`). `fields` are the fields that differ, by dotted path
    (`env.EPIC`, `schedule.cron`, `grants`), as `before` and `after`; a create lists every field of the
    new Actor, and `unchanged` lists none.
    The `launchSpec` is admitted as `POST` admits one, so the `providerOptions` the Execution Provider
    fills in are compared too. A field the request leaves out is kept as it is on a live Actor and takes
    its default on a new one: `grants` (none), `acceptFrom` (every Actor of the Workspace; `null` states
    that again), `declaredIn` (none), `schedule.timezone` (the daemon's) and `schedule.enabled` (true).
    A Job without `schedule` has no Schedule.
    With `dryRun=true`, nothing is written, and `actor` is the live Actor, or `null` for a create.
    The same refusals as `POST` apply: a different kind gets `409`, a `launchSpec` that does not fit the
    kind or a `schedule` on an Agent gets `400`, and `grants` or `trustWorkdir` from a Session token get
    `403`. `feld diff` and `feld apply` declare each Actor of a declaration file this way.

    Args:
        workspace (str):
        actor (str):
        dry_run (bool | Unset):  Default: False.
        body (DeclareActorBody):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[DeclareResponse | Error]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
        body=body,
        dry_run=dry_run,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient | Client,
    body: DeclareActorBody,
    dry_run: bool | Unset = False,
) -> DeclareResponse:
    """Declares an Actor once, or with `dryRun` answers what the declaration would change

     Makes the Actor match the declaration once: creates it when no live Actor holds the name (`action:
    create`), brings a differing live Actor to match while keeping its `id` (`replace`), or leaves a
    matching one as it is (`unchanged`). `fields` are the fields that differ, by dotted path
    (`env.EPIC`, `schedule.cron`, `grants`), as `before` and `after`; a create lists every field of the
    new Actor, and `unchanged` lists none.
    The `launchSpec` is admitted as `POST` admits one, so the `providerOptions` the Execution Provider
    fills in are compared too. A field the request leaves out is kept as it is on a live Actor and takes
    its default on a new one: `grants` (none), `acceptFrom` (every Actor of the Workspace; `null` states
    that again), `declaredIn` (none), `schedule.timezone` (the daemon's) and `schedule.enabled` (true).
    A Job without `schedule` has no Schedule.
    With `dryRun=true`, nothing is written, and `actor` is the live Actor, or `null` for a create.
    The same refusals as `POST` apply: a different kind gets `409`, a `launchSpec` that does not fit the
    kind or a `schedule` on an Agent gets `400`, and `grants` or `trustWorkdir` from a Session token get
    `403`. `feld diff` and `feld apply` declare each Actor of a declaration file this way.

    Args:
        workspace (str):
        actor (str):
        dry_run (bool | Unset):  Default: False.
        body (DeclareActorBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        DeclareResponse
    """

    with operational():
        response = sync_detailed(
            workspace=workspace,
            actor=actor,
            client=client,
            body=body,
            dry_run=dry_run,
        )
    return cast("DeclareResponse", expect(response))


async def asyncio_detailed(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient | Client,
    body: DeclareActorBody,
    dry_run: bool | Unset = False,
) -> Response[DeclareResponse | Error]:
    """Declares an Actor once, or with `dryRun` answers what the declaration would change

     Makes the Actor match the declaration once: creates it when no live Actor holds the name (`action:
    create`), brings a differing live Actor to match while keeping its `id` (`replace`), or leaves a
    matching one as it is (`unchanged`). `fields` are the fields that differ, by dotted path
    (`env.EPIC`, `schedule.cron`, `grants`), as `before` and `after`; a create lists every field of the
    new Actor, and `unchanged` lists none.
    The `launchSpec` is admitted as `POST` admits one, so the `providerOptions` the Execution Provider
    fills in are compared too. A field the request leaves out is kept as it is on a live Actor and takes
    its default on a new one: `grants` (none), `acceptFrom` (every Actor of the Workspace; `null` states
    that again), `declaredIn` (none), `schedule.timezone` (the daemon's) and `schedule.enabled` (true).
    A Job without `schedule` has no Schedule.
    With `dryRun=true`, nothing is written, and `actor` is the live Actor, or `null` for a create.
    The same refusals as `POST` apply: a different kind gets `409`, a `launchSpec` that does not fit the
    kind or a `schedule` on an Agent gets `400`, and `grants` or `trustWorkdir` from a Session token get
    `403`. `feld diff` and `feld apply` declare each Actor of a declaration file this way.

    Args:
        workspace (str):
        actor (str):
        dry_run (bool | Unset):  Default: False.
        body (DeclareActorBody):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[DeclareResponse | Error]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        actor=actor,
        body=body,
        dry_run=dry_run,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workspace: str,
    actor: str,
    *,
    client: AuthenticatedClient | Client,
    body: DeclareActorBody,
    dry_run: bool | Unset = False,
) -> DeclareResponse:
    """Declares an Actor once, or with `dryRun` answers what the declaration would change

     Makes the Actor match the declaration once: creates it when no live Actor holds the name (`action:
    create`), brings a differing live Actor to match while keeping its `id` (`replace`), or leaves a
    matching one as it is (`unchanged`). `fields` are the fields that differ, by dotted path
    (`env.EPIC`, `schedule.cron`, `grants`), as `before` and `after`; a create lists every field of the
    new Actor, and `unchanged` lists none.
    The `launchSpec` is admitted as `POST` admits one, so the `providerOptions` the Execution Provider
    fills in are compared too. A field the request leaves out is kept as it is on a live Actor and takes
    its default on a new one: `grants` (none), `acceptFrom` (every Actor of the Workspace; `null` states
    that again), `declaredIn` (none), `schedule.timezone` (the daemon's) and `schedule.enabled` (true).
    A Job without `schedule` has no Schedule.
    With `dryRun=true`, nothing is written, and `actor` is the live Actor, or `null` for a create.
    The same refusals as `POST` apply: a different kind gets `409`, a `launchSpec` that does not fit the
    kind or a `schedule` on an Agent gets `400`, and `grants` or `trustWorkdir` from a Session token get
    `403`. `feld diff` and `feld apply` declare each Actor of a declaration file this way.

    Args:
        workspace (str):
        actor (str):
        dry_run (bool | Unset):  Default: False.
        body (DeclareActorBody):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        DeclareResponse
    """

    with operational():
        response = await asyncio_detailed(
            workspace=workspace,
            actor=actor,
            client=client,
            body=body,
            dry_run=dry_run,
        )
    return cast("DeclareResponse", expect(response))
