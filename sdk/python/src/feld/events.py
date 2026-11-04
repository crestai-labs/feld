"""The notifications of `GET /v1/events`, read as they arrive."""

from __future__ import annotations

import json
from collections.abc import Iterator
from dataclasses import dataclass

from .api.events import stream_events
from .client import AuthenticatedClient, Client
from .exceptions import FeldOperational, operational, rejection
from .types import UNSET, Unset


@dataclass(frozen=True)
class Event:
    """A resource that changed: its kind (`resource`) and ID. Redo the read it names."""

    resource: str
    id: str


def stream(
    client: AuthenticatedClient | Client, *, workspace: str | Unset = UNSET
) -> Iterator[Event]:
    """Each event as the daemon sends it, until the daemon closes the stream. The first one
    is `daemon` with the daemon's instance ID."""
    kwargs = stream_events._get_kwargs(workspace=workspace)
    with operational():
        with client.get_httpx_client().stream(**kwargs) as response:
            if response.status_code >= 400:
                raise rejection(response.status_code, response.read())
            data: list[str] = []
            for line in response.iter_lines():
                if line:
                    field, _, value = line.partition(":")
                    if field == "data":
                        data.append(value.removeprefix(" "))
                    continue
                if data:
                    yield _event("\n".join(data))
                    data = []


def _event(data: str) -> Event:
    body = json.loads(data)
    if not isinstance(body, dict):
        raise FeldOperational(f"an event that is not an object: {data}")
    return Event(resource=str(body["resource"]), id=str(body["id"]))
