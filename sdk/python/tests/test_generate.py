"""`scripts/generate.py` prunes the definition to the operations the SDK wraps."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from types import ModuleType
from typing import Any

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture(scope="module")
def generate() -> ModuleType:
    spec = importlib.util.spec_from_file_location(
        "generate", ROOT / "scripts" / "generate.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.fixture(scope="module")
def pruned(generate: ModuleType) -> dict[str, Any]:
    source: dict[str, Any] = yaml.safe_load(generate.OPENAPI.read_text())
    result: dict[str, Any] = generate.prune(source)
    return result


def test_only_the_wrapped_tags_and_daemon_reads_remain(
    generate: ModuleType, pruned: dict[str, Any]
) -> None:
    operations = list(generate.operations(pruned))
    assert {tag for op in operations for tag in op["tags"]} == {
        "Daemon",
        "Workspace",
        "Actor",
        "Session",
        "Message",
        "Record",
        "Events",
    }
    daemon = {op["operationId"] for op in operations if op["tags"] == ["Daemon"]}
    assert daemon == {"getDaemonStatus", "listRuntimes", "listProviders"}
    assert [tag["name"] for tag in pruned["tags"]] == [
        "Daemon",
        "Workspace",
        "Actor",
        "Session",
        "Record",
        "Message",
        "Events",
    ]
    for item in pruned["paths"].values():
        assert generate.METHODS & item.keys()


def test_unreachable_and_provider_schemas_are_dropped(pruned: dict[str, Any]) -> None:
    schemas = pruned["components"]["schemas"]
    for name in ("TmuxPlacement", "HerdrPlacement", "Browser", "RuntimeMetrics"):
        assert name not in schemas


def test_the_placement_is_a_map(generate: ModuleType, pruned: dict[str, Any]) -> None:
    placement = pruned["components"]["schemas"]["Placement"]["properties"]["placement"]
    assert placement["example"] == generate.MAP
    assert not {"type", "anyOf", "properties"} & placement.keys()


def test_pruning_is_deterministic(generate: ModuleType) -> None:
    source = generate.OPENAPI.read_text()
    first = yaml.safe_dump(generate.prune(yaml.safe_load(source)), sort_keys=False)
    second = yaml.safe_dump(generate.prune(yaml.safe_load(source)), sort_keys=False)
    assert first == second


def test_the_generated_package_has_no_provider_models() -> None:
    package = ROOT / "src" / "feld"
    tags = (p.name for p in (package / "_api").iterdir() if p.is_dir())
    assert sorted(tag for tag in tags if not tag.startswith("__")) == [
        "actor",
        "daemon",
        "events",
        "message",
        "record",
        "session",
        "workspace",
    ]
    for path in package.rglob("*.py"):
        assert "herdr" not in path.read_text().lower(), path
