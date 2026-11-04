"""The client of the daemon of a Feld home, with the token the caller speaks with."""

from __future__ import annotations

import json
import os
from pathlib import Path

import httpx

from .client import AuthenticatedClient
from .exceptions import FeldOperational

FELD_API_ENV = "FELD_API"
FELD_HOME_ENV = "FELD_HOME"
FELD_SESSION_TOKEN_ENV = "FELD_SESSION_TOKEN"


def feld_home(home: str | os.PathLike[str] | None = None) -> Path:
    """The Feld home: `home`, else `FELD_HOME`, else `~/.feld`."""
    if home is not None:
        return Path(home)
    if value := os.environ.get(FELD_HOME_ENV):
        return Path(value)
    return Path.home() / ".feld"


def connect(
    *,
    api: str | None = None,
    token: str | None = None,
    home: str | os.PathLike[str] | None = None,
    timeout: httpx.Timeout | float | None = None,
) -> AuthenticatedClient:
    """A client of the daemon API.

    The API is `api`, else `FELD_API`, else the `api` the daemon recorded in
    `<home>/daemon.json`. The token is `token`, else the Session token in
    `FELD_SESSION_TOKEN`, else the local token in `<home>/admin/api-token`. Without
    `timeout`, a request waits for its answer as long as it takes.
    """
    root = feld_home(home)
    base_url = api or os.environ.get(FELD_API_ENV) or _recorded_api(root)
    bearer = token or os.environ.get(FELD_SESSION_TOKEN_ENV) or _local_token(root)
    return AuthenticatedClient(
        base_url=base_url,
        token=bearer,
        timeout=httpx.Timeout(timeout)
        if isinstance(timeout, (int, float))
        else timeout,
    )


def _recorded_api(home: Path) -> str:
    path = home / "daemon.json"
    try:
        api = json.loads(path.read_text())["api"]
    except FileNotFoundError:
        raise FeldOperational(
            f"no daemon has started in the Feld home {home} ({path} is missing); "
            "start one with `feld daemon start`"
        ) from None
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise FeldOperational(f"read the daemon record {path}: {error}") from error
    if not isinstance(api, str):
        raise FeldOperational(f"the daemon record {path} has no API")
    return api


def _local_token(home: Path) -> str:
    path = home / "admin" / "api-token"
    try:
        return path.read_text().strip()
    except OSError as error:
        raise FeldOperational(
            f"read the local token {path}: {error}; outside a Session, run as the user "
            "of the Feld home"
        ) from error
