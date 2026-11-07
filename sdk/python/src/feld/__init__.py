"""The Python SDK of the Feld daemon API.

`connect()` finds the daemon and the token the way the CLI does and returns a `Client`, whose
operations are grouped by resource (`client.actors.list(...)`). `feld.models` holds the types
they return and accept, and `feld.testing` a stand-in daemon for tests.
"""

from .client import Client
from .connect import connect
from .exceptions import FeldError, FeldOperational, FeldRejected

__all__ = [
    "Client",
    "FeldError",
    "FeldOperational",
    "FeldRejected",
    "connect",
]
