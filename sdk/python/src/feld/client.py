"""The client of the daemon API."""

from __future__ import annotations

from types import TracebackType
from typing import Self

import httpx

from ._client import AuthenticatedClient
from .resources import Actors, Daemon, Events, Messages, Records, Sessions, Workspaces


class Client:
    """Calls the daemon API at `base_url` with `token`. Its operations are grouped by
    resource: `workspaces`, `actors`, `sessions`, `messages`, `records`, `events`, and
    `daemon`.

    Without `timeout` (seconds, or an `httpx.Timeout`), a request waits for its answer as
    long as it takes. `transport` replaces the HTTP transport, as `feld.testing` does.
    """

    def __init__(
        self,
        base_url: str,
        token: str,
        *,
        timeout: httpx.Timeout | float | None = None,
        transport: httpx.BaseTransport | None = None,
    ) -> None:
        self._api = AuthenticatedClient(
            base_url=base_url,
            token=token,
            timeout=timeout
            if timeout is None or isinstance(timeout, httpx.Timeout)
            else httpx.Timeout(timeout),
            httpx_args={} if transport is None else {"transport": transport},
        )
        self.workspaces = Workspaces(self._api)
        self.actors = Actors(self._api)
        self.sessions = Sessions(self._api)
        self.messages = Messages(self._api)
        self.records = Records(self._api)
        self.events = Events(self._api)
        self.daemon = Daemon(self._api)

    @property
    def base_url(self) -> str:
        return self._api._base_url

    @property
    def token(self) -> str:
        return self._api.token

    def close(self) -> None:
        """Closes the connections the client keeps open."""
        self._api.get_httpx_client().close()

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        type: type[BaseException] | None,
        value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()
