from __future__ import annotations

from typing import TypeVar

from ._client import AuthenticatedClient
from ._types import UNSET, Unset

T = TypeVar("T")


class Resource:
    """The operations of one kind of resource, called through the client they share."""

    def __init__(self, api: AuthenticatedClient) -> None:
        self._api = api


def sent(value: T | None) -> T | Unset:
    """An argument as the generated layer takes it: `None`, an argument left out, is not
    sent."""
    return UNSET if value is None else value
