"""The resources of `feld.Client`: one class per kind of resource, holding its operations."""

from .actors import Actors
from .daemon import Daemon
from .events import Events
from .messages import Messages
from .records import Records
from .sessions import Sessions
from .workspaces import Workspaces

__all__ = [
    "Actors",
    "Daemon",
    "Events",
    "Messages",
    "Records",
    "Sessions",
    "Workspaces",
]
