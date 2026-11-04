"""The Python SDK of the Feld daemon API.

The operations are the modules of `feld.api.<tag>` (`sync` / `asyncio`), generated from the
OpenAPI definition along with the models of `feld.models`. `connect` builds the client they
take, and `feld.records` reads records page by page or as they are appended.
"""

from .client import AuthenticatedClient, Client
from .connect import connect
from .exceptions import FeldError, FeldOperational, FeldRejected

__all__ = [
    "AuthenticatedClient",
    "Client",
    "FeldError",
    "FeldOperational",
    "FeldRejected",
    "connect",
]
