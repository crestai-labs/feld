"""The daemon: `GET /v1/daemon/status`, `GET /v1/runtimes`, and `GET /v1/providers`."""

from __future__ import annotations

from .._api.daemon import get_daemon_status, list_providers, list_runtimes
from ..models import DaemonStatus, ProviderEntry, RuntimeEntry
from ._resource import Resource


class Daemon(Resource):
    def status(self) -> DaemonStatus:
        return get_daemon_status.sync(client=self._api)

    def runtimes(self) -> list[RuntimeEntry]:
        """The installed Runtime Adapters."""
        return list_runtimes.sync(client=self._api).runtimes

    def providers(self) -> list[ProviderEntry]:
        """The installed Execution Providers."""
        return list_providers.sync(client=self._api).providers
