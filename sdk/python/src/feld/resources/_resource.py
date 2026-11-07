from __future__ import annotations

from .._client import AuthenticatedClient


class Resource:
    """The operations of one kind of resource, called through the client they share."""

    def __init__(self, api: AuthenticatedClient) -> None:
        self._api = api
