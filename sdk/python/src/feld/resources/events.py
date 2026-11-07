"""Change notifications: `GET /v1/events`, read as they arrive."""

from __future__ import annotations

import json
from collections.abc import Iterator

from .._api.events import stream_events
from .._resource import Resource, sent
from ..exceptions import FeldOperational, operational, rejection
from ..models import Event


class Events(Resource):
    def stream(self, *, workspace: str | None = None) -> Iterator[Event]:
        """Each event as the daemon sends it, until the daemon closes the stream. The first
        one is `daemon` with the daemon's instance ID."""
        kwargs = stream_events._get_kwargs(workspace=sent(workspace))
        with (
            operational(),
            self._api.get_httpx_client().stream(**kwargs) as response,
        ):
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
