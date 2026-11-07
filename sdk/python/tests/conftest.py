from __future__ import annotations

import datetime
import json
from pathlib import Path

import pytest

from feld.models import (
    ActorReference,
    AdminRequester,
    MessageSummary,
    Record,
    RecordKind,
    Session,
)

FIXTURES = Path(__file__).parent / "fixtures"
AT = datetime.datetime(2026, 11, 4, 9, 0, tzinfo=datetime.UTC)
DEV = ActorReference(id="actor_dev", workspace="feld-dev", name="dev")


@pytest.fixture
def session() -> Session:
    """A Session as a daemon answered it."""
    return Session.from_dict(json.loads((FIXTURES / "session.json").read_text()))


def record(sequence: int, kind: RecordKind = RecordKind.MESSAGE_ACCEPTED) -> Record:
    return Record(
        sequence=sequence,
        at=AT,
        kind=kind,
        actor=DEV,
        session_id=None,
        data={},
    )


def message(id: str, read: bool = False) -> MessageSummary:
    return MessageSummary(
        id=id,
        fifo=1,
        from_=AdminRequester(type_="admin"),
        from_session_id=None,
        to=DEV,
        to_session_id=None,
        accepted=AT,
        label=None,
        queued=None,
        read=None,
        preview="hello",
        anchor=f"feld:{id}",
        waiting=not read,
        deleted_at=None,
    )
