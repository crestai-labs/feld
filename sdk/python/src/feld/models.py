"""The types that `feld.Client` returns and accepts.

The models are attrs classes with `to_dict()` and `from_dict()`. Their attributes are the
fields of the daemon API in snake case; a field an answer may leave out holds a falsy
placeholder when it is absent, so `if session.turn:` tells whether it is there.
"""

from __future__ import annotations

import enum
from dataclasses import dataclass
from typing import Final

from ._models import (
    Actor,
    ActorKind,
    ActorReference,
    ActorRequester,
    AdminRequester,
    AttachResponse,
    ConsoleState,
    DaemonStatus,
    DaemonStatusRestored,
    DaemonStatusScheduler,
    Execution,
    ExecutionType,
    Grant,
    LaunchSpec,
    LaunchSpecNode,
    LaunchSpecPatch,
    LifecycleAction,
    LifecycleResponse,
    Message,
    MessageQueued,
    MessageRead,
    MessageSummary,
    Observation,
    ObservationKind,
    Outcome,
    OutputLine,
    OutputPage,
    OutputStream,
    Placement,
    PlannedTick,
    Plugin,
    ProviderEntry,
    Record,
    RecordKind,
    RestartResponse,
    RuntimeEntry,
    Schedule,
    SchedulePatch,
    SchedulerRestart,
    ScheduleSpec,
    Session,
    SessionEndKind,
    SessionLifecycle,
    SessionOwnership,
    SessionResponse,
    SessionSpawnedBy,
    StartKind,
    StartResponse,
    SubAgentSession,
    TranscriptEntry,
    TranscriptEntryKind,
    TranscriptEntryState,
    TranscriptPage,
    TranscriptView,
    Turn,
    Workspace,
)


@dataclass(frozen=True)
class Event:
    """A resource that changed: its kind (`resource`) and ID. Redo the read it names."""

    resource: str
    id: str


class Null(enum.Enum):
    """The JSON `null`, for an argument where `null` has a meaning of its own: in
    `actors.update(accept_from=NULL)`, it removes the field, while `None` leaves it as it
    is."""

    NULL = "null"


NULL: Final = Null.NULL

__all__ = [
    "Actor",
    "ActorKind",
    "ActorReference",
    "ActorRequester",
    "AdminRequester",
    "AttachResponse",
    "ConsoleState",
    "DaemonStatus",
    "DaemonStatusRestored",
    "DaemonStatusScheduler",
    "Event",
    "Execution",
    "ExecutionType",
    "Grant",
    "LaunchSpec",
    "LaunchSpecNode",
    "LaunchSpecPatch",
    "LifecycleAction",
    "LifecycleResponse",
    "Message",
    "MessageQueued",
    "MessageRead",
    "MessageSummary",
    "NULL",
    "Null",
    "Observation",
    "ObservationKind",
    "Outcome",
    "OutputLine",
    "OutputPage",
    "OutputStream",
    "Placement",
    "PlannedTick",
    "Plugin",
    "ProviderEntry",
    "Record",
    "RecordKind",
    "RestartResponse",
    "RuntimeEntry",
    "Schedule",
    "SchedulePatch",
    "ScheduleSpec",
    "SchedulerRestart",
    "Session",
    "SessionEndKind",
    "SessionLifecycle",
    "SessionOwnership",
    "SessionResponse",
    "SessionSpawnedBy",
    "StartKind",
    "StartResponse",
    "SubAgentSession",
    "TranscriptEntry",
    "TranscriptEntryKind",
    "TranscriptEntryState",
    "TranscriptPage",
    "TranscriptView",
    "Turn",
    "Workspace",
]
