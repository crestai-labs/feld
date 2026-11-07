from __future__ import annotations

from collections.abc import Iterator

import httpx
import pytest

import feld
from feld.models import Event
from feld.testing import FakeDaemon

STREAM = (
    'event: resource\ndata: {"resource":"daemon","id":"daemon_1"}\n\n'
    ": keep-alive\n\n"
    'event: resource\ndata: {"resource":"session","id":"session_1"}\n\n'
)


def test_stream_yields_each_event_and_skips_keep_alives() -> None:
    fake = FakeDaemon().on(
        "GET",
        "/v1/events",
        lambda request: httpx.Response(
            200, text=STREAM, headers={"content-type": "text/event-stream"}
        ),
    )
    seen = list(fake.client().events.stream(workspace="feld-dev"))
    assert seen == [Event("daemon", "daemon_1"), Event("session", "session_1")]
    assert fake.requests[0].url.params["workspace"] == "feld-dev"


class FirstEventOnly(httpx.SyncByteStream):
    """A stream that stays open after its first event, as `/v1/events` does."""

    def __iter__(self) -> Iterator[bytes]:
        yield b'event: resource\ndata: {"resource":"actor","id":"dev"}\n\n'
        raise AssertionError("read past the first event")


def test_stream_yields_an_event_before_the_stream_ends() -> None:
    fake = FakeDaemon().on(
        "GET",
        "/v1/events",
        lambda request: httpx.Response(200, stream=FirstEventOnly()),
    )
    assert next(fake.client().events.stream()) == Event("actor", "dev")


def test_a_refused_stream_is_rejected() -> None:
    fake = FakeDaemon().reject("GET", "/v1/events", 403, "other Workspace")
    with pytest.raises(feld.FeldRejected) as rejected:
        next(fake.client().events.stream(workspace="other"))
    assert rejected.value.status == 403
