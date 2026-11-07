"""A type checker finds the misspelled keys and the unchecked nulls of the answers."""

from __future__ import annotations

from mypy import api

MISTAKES = """
import feld

client = feld.connect()
for actor in client.actors.list("feld-dev"):
    print(actor.currentSession)
    print(actor.current_session.id)
client.messages.send("feld-dev", "dev", text="hello")
"""


def test_mypy_finds_the_mistakes() -> None:
    stdout, _, status = api.run(
        ["--config-file=", "--strict", "--no-incremental", "-c", MISTAKES]
    )
    assert status == 1
    assert '"Actor" has no attribute "currentSession"' in stdout
    assert 'Item "None" of "Session | None" has no attribute "id"' in stdout
    assert 'Unexpected keyword argument "text"' in stdout
