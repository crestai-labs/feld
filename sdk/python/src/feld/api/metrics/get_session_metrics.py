import datetime
from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...exceptions import expect, operational
from ...models.error import Error
from ...models.metric_name import MetricName
from ...models.session_metrics import SessionMetrics
from ...types import UNSET, Response, Unset


def _get_kwargs(
    session: str,
    *,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    bucket: int | Unset = UNSET,
    metric: MetricName | Unset = UNSET,
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

    json_metric: str | Unset = UNSET
    if not isinstance(metric, Unset):
        json_metric = metric.value

    params["metric"] = json_metric

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/sessions/{session}/metrics".format(
            session=quote(str(session), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Error | SessionMetrics | None:
    if response.status_code == 200:
        response_200 = SessionMetrics.from_dict(response.json())

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
) -> Response[Error | SessionMetrics]:
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
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    bucket: int | Unset = UNSET,
    metric: MetricName | Unset = UNSET,
) -> Response[Error | SessionMetrics]:
    """Returns Session metrics

     Returns, from the Session's Runtime telemetry, the `status` and `source` of each metric, plus series
    when `reported`. Omitting the range aggregates the whole Session. Given the ID of a Sub Agent
    Session, returns every metric as `n/a` ("Included in the parent Session").

    Args:
        session (str):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        bucket (int | Unset):
        metric (MetricName | Unset):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | SessionMetrics]
    """

    kwargs = _get_kwargs(
        session=session,
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
    session: str,
    *,
    client: AuthenticatedClient | Client,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    bucket: int | Unset = UNSET,
    metric: MetricName | Unset = UNSET,
) -> SessionMetrics:
    """Returns Session metrics

     Returns, from the Session's Runtime telemetry, the `status` and `source` of each metric, plus series
    when `reported`. Omitting the range aggregates the whole Session. Given the ID of a Sub Agent
    Session, returns every metric as `n/a` ("Included in the parent Session").

    Args:
        session (str):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        bucket (int | Unset):
        metric (MetricName | Unset):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        SessionMetrics
    """

    with operational():
        response = sync_detailed(
            session=session,
            client=client,
            start=start,
            end=end,
            bucket=bucket,
            metric=metric,
        )
    return cast("SessionMetrics", expect(response))


async def asyncio_detailed(
    session: str,
    *,
    client: AuthenticatedClient | Client,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    bucket: int | Unset = UNSET,
    metric: MetricName | Unset = UNSET,
) -> Response[Error | SessionMetrics]:
    """Returns Session metrics

     Returns, from the Session's Runtime telemetry, the `status` and `source` of each metric, plus series
    when `reported`. Omitting the range aggregates the whole Session. Given the ID of a Sub Agent
    Session, returns every metric as `n/a` ("Included in the parent Session").

    Args:
        session (str):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        bucket (int | Unset):
        metric (MetricName | Unset):

    Raises:
        errors.UnexpectedStatus: If the daemon answers with an undocumented status and Client.raise_on_unexpected_status is True.
        httpx.HTTPError: If the request does not reach an answer.

    Returns:
        Response[Error | SessionMetrics]
    """

    kwargs = _get_kwargs(
        session=session,
        start=start,
        end=end,
        bucket=bucket,
        metric=metric,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    session: str,
    *,
    client: AuthenticatedClient | Client,
    start: datetime.datetime | Unset = UNSET,
    end: datetime.datetime | Unset = UNSET,
    bucket: int | Unset = UNSET,
    metric: MetricName | Unset = UNSET,
) -> SessionMetrics:
    """Returns Session metrics

     Returns, from the Session's Runtime telemetry, the `status` and `source` of each metric, plus series
    when `reported`. Omitting the range aggregates the whole Session. Given the ID of a Sub Agent
    Session, returns every metric as `n/a` ("Included in the parent Session").

    Args:
        session (str):
        start (datetime.datetime | Unset):
        end (datetime.datetime | Unset):
        bucket (int | Unset):
        metric (MetricName | Unset):

    Raises:
        FeldRejected: If the daemon answers with an error status.
        FeldOperational: If the request does not reach an answer, or the answer cannot be read.

    Returns:
        SessionMetrics
    """

    with operational():
        response = await asyncio_detailed(
            session=session,
            client=client,
            start=start,
            end=end,
            bucket=bucket,
            metric=metric,
        )
    return cast("SessionMetrics", expect(response))
