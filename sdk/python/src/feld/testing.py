"""A stand-in for the daemon in tests: it answers each request with what is registered for its
method and path, and keeps the requests it received.

    fake = FakeDaemon()
    fake.on("GET", "/v1/messages", {"messages": [summary]})
    fake.client().messages.list(to="dev")
    assert fake.requests[0].url.params["to"] == "dev"
"""

from __future__ import annotations

import json
from collections.abc import Callable
from typing import Any, Protocol

import httpx

from .client import Client


class Model(Protocol):
    def to_dict(self) -> dict[str, Any]: ...


Handler = Callable[[httpx.Request], httpx.Response]
Answer = Model | list[Any] | dict[str, Any] | Handler


class FakeDaemon:
    """Routes requests by `(method, path)` to registered answers. An unregistered route gets
    `404` with the `Error` body of the daemon."""

    def __init__(self) -> None:
        self.routes: dict[tuple[str, str], list[Handler]] = {}
        self.requests: list[httpx.Request] = []

    def on(self, method: str, path: str, *answers: Answer) -> FakeDaemon:
        """Answers `method path` with `answers` in turn, repeating the last one. An answer is
        sent as JSON with `200` when it is a model of `feld.models`, a list of them, or a
        `dict` (the body, whose values may hold models); a function of the request answers
        with what it returns."""
        self.routes.setdefault((method.upper(), path), []).extend(
            map(_handler, answers)
        )
        return self

    def reject(
        self, method: str, path: str, status: int, message: str, next: str | None = None
    ) -> FakeDaemon:
        """Answers `method path` with the daemon's error."""
        body: dict[str, Any] = {"message": message}
        if next is not None:
            body["next"] = next
        return self.on(method, path, lambda _: httpx.Response(status, json=body))

    def client(self, token: str = "test-token") -> Client:
        """A client whose requests this answers."""
        return Client(
            "http://feld.test", token, transport=httpx.MockTransport(self._handle)
        )

    def json(self, index: int = -1) -> Any:
        """The JSON body of a received request, the last one by default."""
        return json.loads(self.requests[index].content)

    def _handle(self, request: httpx.Request) -> httpx.Response:
        self.requests.append(request)
        handlers = self.routes.get((request.method, request.url.path))
        if not handlers:
            return httpx.Response(
                404, json={"message": f"no route {request.method} {request.url.path}"}
            )
        handler = handlers.pop(0) if len(handlers) > 1 else handlers[0]
        return handler(request)


def _handler(answer: Answer) -> Handler:
    if isinstance(answer, (list, dict)) or hasattr(answer, "to_dict"):
        body = _json(answer)
        return lambda _: httpx.Response(200, json=body)
    if callable(answer):
        return answer
    raise TypeError(f"not an answer: {answer!r}")


def _json(value: Any) -> Any:
    if hasattr(value, "to_dict"):
        return value.to_dict()
    if isinstance(value, dict):
        return {key: _json(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_json(item) for item in value]
    return value
