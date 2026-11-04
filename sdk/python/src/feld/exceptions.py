"""The errors of a call, by the `kind` the CLI gives them: the daemon rejected the request
(`FeldRejected`), or the request did not reach an answer (`FeldOperational`)."""

from __future__ import annotations

import json
from collections.abc import Iterator
from contextlib import contextmanager
from typing import Any

import httpx

from .errors import UnexpectedStatus
from .types import Response


class FeldError(Exception):
    """An error of a call to the daemon API."""

    kind: str


class FeldRejected(FeldError):
    """The daemon answered with an error status: `message` is its reason, and `next` the next
    step a person takes when the daemon gives one."""

    kind = "rejected"

    def __init__(self, status: int, message: str, next: str | None = None) -> None:
        super().__init__(f"daemon rejected the request ({status}): {message}")
        self.status = status
        self.message = message
        self.next = next


class FeldOperational(FeldError):
    """The daemon could not be reached or found, or its answer could not be read."""

    kind = "operational"


@contextmanager
def operational() -> Iterator[None]:
    """Raises an error status as `FeldRejected`, and the failures of the transport and of
    reading an answer as `FeldOperational`."""
    try:
        yield
    except FeldError:
        raise
    except UnexpectedStatus as error:
        raise rejection(error.status_code, error.content) from error
    except (httpx.HTTPError, ValueError, KeyError, TypeError) as error:
        raise FeldOperational(str(error) or type(error).__name__) from error


def expect(response: Response[Any]) -> Any:
    """The parsed body of a success response, or `FeldRejected` for an error status."""
    status = int(response.status_code)
    if status < 400:
        return response.parsed
    raise rejection(status, response.content)


def rejection(status: int, content: bytes) -> FeldRejected:
    """The daemon's error answer: its `Error` body, or the text of any other body."""
    message = content.decode(errors="replace")
    next_step = None
    try:
        body = json.loads(content)
    except ValueError:
        body = None
    if isinstance(body, dict):
        if isinstance(body.get("message"), str):
            message = body["message"]
        if isinstance(body.get("next"), str):
            next_step = body["next"]
    return FeldRejected(status, message, next_step)
