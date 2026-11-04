"""A type checker finds the misspelled keys and the unchecked nulls of the answers."""

from __future__ import annotations

from mypy import api

MISTAKES = """
import feld
from feld.api.actor import list_actors

client = feld.connect()
for actor in list_actors.sync(client=client, workspace="feld-dev").actors:
    print(actor.currentSession)
    print(actor.current_session.id)
"""


def test_mypy_finds_the_mistakes() -> None:
    stdout, _, status = api.run(
        ["--config-file=", "--strict", "--no-incremental", "-c", MISTAKES]
    )
    assert status == 1
    assert '"Actor" has no attribute "currentSession"' in stdout
    assert 'Item "None" of "Session | None" has no attribute "id"' in stdout
