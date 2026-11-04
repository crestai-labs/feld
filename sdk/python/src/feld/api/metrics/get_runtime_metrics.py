import datetime
from http import HTTPStatus
from typing import Any, Literal, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.metric_name import MetricName
from ...models.runtime_metrics import RuntimeMetrics
from ...types import UNSET, Response, Unset


def _get_kwargs(
    runtime: str,
    *,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    bucket: int | Unset = UNSET,
    metric: Literal["quota"] | MetricName | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    json_start: str | Unset = UNSET
    if not isinstance(start, Unset):
        json_start = start.isoformat()
    params["start"] = json_start

    json_end: str | Unset = UNSET
    if not isinstance(end, Unset):
        json_end = end.isoformat()
    params["end"] = json_end

    params["bucket"] = bucket

    json_metric: Literal["quota"] | str | Unset
    if isinstance(metric, Unset):
        json_metric = UNSET
    elif isinstance(metric, MetricName):
        json_metric = metric.value
    else:
        json_metric = metric
    params["metric"] = json_metric

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/runtimes/{runtime}/metrics".format(
            runtime=quote(str(runtime), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | RuntimeMetrics | None:
    if response.status_code == 200:
        response_200 = RuntimeMetrics.from_dict(response.json())

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
) -> Response[Error | RuntimeMetrics]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    runtime: str,
    *,
    client: AuthenticatedClient,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    bucket: int | Unset = UNSET,
    metric: Literal["quota"] | MetricName | Unset = UNSET,
) -> Response[Error | RuntimeMetrics]:
    """Returns Runtime metrics

     Returns series that aggregate, per Runtime and account, the metrics of Sessions in all Workspaces
    that overlap the range, along with the account quota.
    Omitting `metric` returns all metrics and `quota`. With `metric=quota`, `metrics` is empty and only
    `quota` is returned.

    Args:
        runtime (str):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        bucket (int | Unset):
        metric (Literal['quota'] | MetricName | Unset):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | RuntimeMetrics]
    """

    kwargs = _get_kwargs(
        runtime=runtime,
        start=start,
        end=end,
        bucket=bucket,
        metric=metric,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    runtime: str,
    *,
    client: AuthenticatedClient,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    bucket: int | Unset = UNSET,
    metric: Literal["quota"] | MetricName | Unset = UNSET,
) -> RuntimeMetrics:
    """Returns Runtime metrics

     Returns series that aggregate, per Runtime and account, the metrics of Sessions in all Workspaces
    that overlap the range, along with the account quota.
    Omitting `metric` returns all metrics and `quota`. With `metric=quota`, `metrics` is empty and only
    `quota` is returned.

    Args:
        runtime (str):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        bucket (int | Unset):
        metric (Literal['quota'] | MetricName | Unset):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        RuntimeMetrics
    """

    with operational():
        response = sync_detailed(
            runtime=runtime,
            client=client,
            start=start,
            end=end,
            bucket=bucket,
            metric=metric,
        )
    return cast("RuntimeMetrics", expect(response))


async def asyncio_detailed(
    runtime: str,
    *,
    client: AuthenticatedClient,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    bucket: int | Unset = UNSET,
    metric: Literal["quota"] | MetricName | Unset = UNSET,
) -> Response[Error | RuntimeMetrics]:
    """Returns Runtime metrics

     Returns series that aggregate, per Runtime and account, the metrics of Sessions in all Workspaces
    that overlap the range, along with the account quota.
    Omitting `metric` returns all metrics and `quota`. With `metric=quota`, `metrics` is empty and only
    `quota` is returned.

    Args:
        runtime (str):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        bucket (int | Unset):
        metric (Literal['quota'] | MetricName | Unset):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | RuntimeMetrics]
    """

    kwargs = _get_kwargs(
        runtime=runtime,
        start=start,
        end=end,
        bucket=bucket,
        metric=metric,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    runtime: str,
    *,
    client: AuthenticatedClient,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    bucket: int | Unset = UNSET,
    metric: Literal["quota"] | MetricName | Unset = UNSET,
) -> RuntimeMetrics:
    """Returns Runtime metrics

     Returns series that aggregate, per Runtime and account, the metrics of Sessions in all Workspaces
    that overlap the range, along with the account quota.
    Omitting `metric` returns all metrics and `quota`. With `metric=quota`, `metrics` is empty and only
    `quota` is returned.

    Args:
        runtime (str):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        bucket (int | Unset):
        metric (Literal['quota'] | MetricName | Unset):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        RuntimeMetrics
    """

    with operational():
        response = await asyncio_detailed(
            runtime=runtime,
            client=client,
            start=start,
            end=end,
            bucket=bucket,
            metric=metric,
        )
    return cast("RuntimeMetrics", expect(response))
