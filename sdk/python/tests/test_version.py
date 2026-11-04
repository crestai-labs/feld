"""The SDK has the version of Feld and of the OpenAPI definition it is generated from."""

from __future__ import annotations

import re
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]


def test_the_version_is_the_version_of_feld() -> None:
    sdk = tomllib.loads((REPO / "sdk/python/pyproject.toml").read_text())["project"][
        "version"
    ]
    cargo = tomllib.loads((REPO / "Cargo.toml").read_text())["workspace"]["package"][
        "version"
    ]
    openapi = re.search(
        r"^  version: (\S+)$",
        (REPO / "docs/guide/daemon-api.openapi.yaml").read_text(),
        re.MULTILINE,
    )
    assert openapi is not None
    assert sdk == cargo == openapi.group(1)
