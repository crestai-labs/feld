from __future__ import annotations

import json
from pathlib import Path

import pytest

import feld
from feld.connect import FELD_API_ENV, FELD_HOME_ENV, FELD_SESSION_TOKEN_ENV


@pytest.fixture
def home(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    for name in (FELD_API_ENV, FELD_HOME_ENV, FELD_SESSION_TOKEN_ENV):
        monkeypatch.delenv(name, raising=False)
    (tmp_path / "admin").mkdir()
    (tmp_path / "admin" / "api-token").write_text("local-token\n")
    (tmp_path / "daemon.json").write_text(json.dumps({"api": "http://127.0.0.1:7381"}))
    monkeypatch.setenv(FELD_HOME_ENV, str(tmp_path))
    return tmp_path


def test_outside_a_session_speaks_with_the_local_token(home: Path) -> None:
    client = feld.connect()
    assert client.token == "local-token"
    assert client.base_url == "http://127.0.0.1:7381"


def test_inside_a_session_speaks_with_the_session_token(
    home: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv(FELD_SESSION_TOKEN_ENV, "session-token")
    monkeypatch.setenv(FELD_API_ENV, "http://127.0.0.1:9999")
    client = feld.connect()
    assert client.token == "session-token"
    assert client.base_url == "http://127.0.0.1:9999"


def test_arguments_come_first(home: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(FELD_SESSION_TOKEN_ENV, "session-token")
    client = feld.connect(api="http://127.0.0.1:1", token="given")
    assert (client.base_url, client.token) == ("http://127.0.0.1:1", "given")


def test_a_home_without_a_daemon_is_operational(home: Path) -> None:
    (home / "daemon.json").unlink()
    with pytest.raises(feld.FeldOperational, match="feld daemon start"):
        feld.connect()
