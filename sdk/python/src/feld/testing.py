"""A stand-in for the daemon in tests: it answers each request with the model or error
registered for its method and path, and keeps the requests it received.

    fake = FakeDaemon()
    fake.on("GET", "/v1/messages", MessageSummaryList(messages=[...]))
    list_messages.sync(client=fake.client(), to="dev")
    assert fake.requests[0].url.params["to"] == "dev"
"""

from __future__ import annotations

import json
from collections.abc import Callable
from typing import Any, Protocol

import httpx

from .client import AuthenticatedClient


class Model(Protocol):
    def to_dict(self) -> dict[str, Any]: ...


Handler = Callable[[httpx.Request], httpx.Response]


class FakeDaemon:
    """Routes requests by `(method, path)` to registered answers. An unregistered route gets
    `404` with the `Error` body of the daemon."""

    def __init__(self) -> None:
        self.routes: dict[tuple[str, str], list[Handler]] = {}
        self.requests: list[httpx.Request] = []

    def on(
        self, method: str, path: str, *answers: Model | list[Model] | Handler
    ) -> FakeDaemon:
        """Answers `method path` with `answers` in turn, repeating the last one. An answer is a
        model (or a list of them) sent as JSON with `200`, or a function of the request."""
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

    def client(self, token: str = "test-token") -> AuthenticatedClient:
        """A client whose requests this answers."""
        return AuthenticatedClient(
            base_url="http://feld.test",
            token=token,
            httpx_args={"transport": httpx.MockTransport(self._handle)},
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


def _handler(answer: Model | list[Model] | Handler) -> Handler:
    if isinstance(answer, list):
        items = [item.to_dict() for item in answer]
        return lambda _: httpx.Response(200, json=items)
    if callable(answer):
        return answer
    body = answer.to_dict()
    return lambda _: httpx.Response(200, json=body)
