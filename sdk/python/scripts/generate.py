"""Generates the private modules of `feld` from the daemon API's OpenAPI definition.

    uv run python scripts/generate.py

This prunes the definition to the operations the SDK wraps (`prune`), has
`openapi-python-client` write the package into a temporary directory, renames its modules to
`_api`, `_models`, `_client`, `_errors`, and `_types`, and copies them into `src/feld`, beside
the hand-written modules.
"""

from __future__ import annotations

import copy
import re
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import yaml

ROOT = Path(__file__).resolve().parent.parent
OPENAPI = ROOT.parent.parent / "docs" / "guide" / "daemon-api.openapi.yaml"
PACKAGE = ROOT / "src" / "feld"
GENERATED = {"api": "_api", "models": "_models"}
GENERATED_FILES = {
    "client.py": "_client.py",
    "errors.py": "_errors.py",
    "types.py": "_types.py",
}

METHODS = {"get", "put", "post", "delete", "options", "head", "patch", "trace"}
TAGS = {"Daemon", "Workspace", "Actor", "Session", "Message", "Record", "Events"}
DAEMON_OPERATIONS = {"getDaemonStatus", "listRuntimes", "listProviders"}
# The example that marks a free-form object for `templates/model.py.jinja`.
MAP = "feld:map"

# Inline schemas given a name, so that their models are named after what they are rather than
# where they appear: (dotted path to the schema in the definition, component name).
NAMED = [
    ("components.schemas.Grants.items", "Grant"),
    ("components.schemas.Requester.oneOf.0", "AdminRequester"),
    ("components.schemas.Requester.oneOf.1", "ActorRequester"),
    ("components.schemas.OutputPage.properties.lines.items", "OutputLine"),
    ("components.schemas.OutputLine.properties.stream", "OutputStream"),
    (
        "components.schemas.DaemonStatus.properties.scheduler.properties.planned.items",
        "PlannedTick",
    ),
    (
        "components.responses.Lifecycle.content.application/json.schema",
        "LifecycleResponse",
    ),
]
# Inline object schemas that may be null, named as the object.
NAMED_NULLABLE = [
    (
        "components.schemas.DaemonStatus.properties.scheduler.properties.lastRestart",
        "SchedulerRestart",
    ),
]
# Query parameter schemas given a name: (operationId, parameter, component name).
NAMED_PARAMETERS = [("getTranscript", "view", "TranscriptView")]
# Descriptions that name the options of the bundled Providers, which the Providers document.
DESCRIPTIONS = [
    (
        "components.schemas.LaunchSpec.properties.providerOptions",
        "Options of the Execution Provider, from the CLI's `--<provider>-<key> <value>`.",
    ),
]
# Request body properties given a name: (operationId, property, component name).
NAMED_BODY_PROPERTIES = [
    ("createActor", "schedule", "ScheduleSpec"),
    ("updateActor", "schedule", "SchedulePatch"),
]


def operations(spec: dict[str, Any]) -> Iterator[dict[str, Any]]:
    for item in spec["paths"].values():
        for method, operation in item.items():
            if method in METHODS:
                yield operation


def kept(operation: dict[str, Any]) -> bool:
    (tag,) = operation["tags"]
    if tag == "Daemon":
        return operation["operationId"] in DAEMON_OPERATIONS
    return tag in TAGS


def prune(source: dict[str, Any]) -> dict[str, Any]:
    """The definition with only the operations of `TAGS` (and of the Daemon, only
    `DAEMON_OPERATIONS`), the components they reach, free-form objects as maps, and the
    inline schemas of `NAMED*` as components."""
    spec = copy.deepcopy(source)
    paths: dict[str, Any] = {}
    for path, item in spec["paths"].items():
        item = {k: v for k, v in item.items() if k not in METHODS or kept(v)}
        if METHODS & item.keys():
            paths[path] = item
    spec["paths"] = paths
    spec["tags"] = [tag for tag in spec["tags"] if tag["name"] in TAGS]
    spec.pop("x-tagGroups", None)

    placement = spec["components"]["schemas"]["Placement"]["properties"]["placement"]
    del placement["anyOf"]
    placement["type"] = "object"

    schemas = spec["components"]["schemas"]
    for path, name in NAMED:
        parent, _, key = path.rpartition(".")
        node = _at(spec, parent)
        index = int(key) if isinstance(node, list) else key
        schemas[name] = node[index]
        node[index] = {"$ref": f"#/components/schemas/{name}"}
    for path, name in NAMED_NULLABLE:
        parent, _, key = path.rpartition(".")
        node = _at(spec, parent)
        schemas[name] = _not_null(node[key])
        node[key] = _nullable_ref(name)
    by_id = {operation["operationId"]: operation for operation in operations(spec)}
    for operation_id, prop, name in NAMED_BODY_PROPERTIES:
        properties = by_id[operation_id]["requestBody"]["content"]["application/json"][
            "schema"
        ]["properties"]
        schema = properties[prop]
        if "null" in schema["type"]:
            schemas[name] = _not_null(schema)
            properties[prop] = _nullable_ref(name)
        else:
            schemas[name] = schema
            properties[prop] = {"$ref": f"#/components/schemas/{name}"}

    for operation_id, parameter, name in NAMED_PARAMETERS:
        (found,) = (
            p for p in by_id[operation_id]["parameters"] if p.get("name") == parameter
        )
        schemas[name] = found["schema"]
        found["schema"] = {"$ref": f"#/components/schemas/{name}"}
    for path, description in DESCRIPTIONS:
        _at(spec, path)["description"] = description

    _drop_unreferenced(spec)
    _maps(spec)
    return spec


def _at(node: Any, path: str) -> Any:
    for key in path.split("."):
        node = node[int(key)] if isinstance(node, list) else node[key]
    return node


def _not_null(schema: dict[str, Any]) -> dict[str, Any]:
    types = [t for t in schema["type"] if t != "null"]
    return {**schema, "type": types[0] if len(types) == 1 else types}


def _nullable_ref(name: str) -> dict[str, Any]:
    return {"oneOf": [{"$ref": f"#/components/schemas/{name}"}, {"type": "null"}]}


def _refs(node: Any) -> Iterator[str]:
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "$ref" and isinstance(value, str):
                yield value
            else:
                yield from _refs(value)
    elif isinstance(node, list):
        for value in node:
            yield from _refs(value)


def _drop_unreferenced(spec: dict[str, Any]) -> None:
    """Removes the components (other than security schemes) that no kept operation reaches."""
    components = spec["components"]
    reached: set[str] = set()
    pending = list(_refs(spec["paths"]))
    while pending:
        ref = pending.pop()
        if ref in reached:
            continue
        reached.add(ref)
        _, _, section, name = ref.split("/")
        pending.extend(_refs(components[section][name]))
    for section, entries in components.items():
        if section == "securitySchemes":
            continue
        components[section] = {
            name: entry
            for name, entry in entries.items()
            if f"#/components/{section}/{name}" in reached
        }


def _free_form(node: dict[str, Any]) -> bool:
    types = node.get("type")
    types = types if isinstance(types, list) else [types]
    return (
        "object" in types
        and not {"properties", "allOf", "anyOf", "oneOf"} & node.keys()
        and node.get("additionalProperties", True) is not False
    )


def _maps(node: Any) -> None:
    """Rewrites each object schema without properties into an empty schema marked with the
    example `MAP`, which the generator types as `Any` and the model template declares as
    `dict[str, Any]`; one that may be null becomes `oneOf` of it and null."""
    if isinstance(node, list):
        for value in node:
            _maps(value)
        return
    if not isinstance(node, dict):
        return
    for key, value in node.items():
        if isinstance(value, dict) and _free_form(value):
            described = {k: v for k, v in value.items() if k == "description"}
            types = (
                value["type"] if isinstance(value["type"], list) else [value["type"]]
            )
            if "null" in types:
                node[key] = {
                    **described,
                    "oneOf": [{"example": MAP}, {"type": "null"}],
                }
            else:
                node[key] = {**described, "example": MAP}
        else:
            _maps(value)


def _private(text: str) -> str:
    """The generated imports, pointing at the private module names."""
    text = re.sub(r"from (\.+)(models|api|client|errors|types)\b", r"from \1_\2", text)
    return re.sub(
        r"from (\.+) import errors\b", r"from \1 import _errors as errors", text
    )


def main() -> None:
    source = yaml.safe_load(OPENAPI.read_text())
    with tempfile.TemporaryDirectory() as temp:
        pruned = Path(temp) / "openapi.yaml"
        pruned.write_text(yaml.safe_dump(prune(source), sort_keys=False))
        output = Path(temp) / "feld"
        subprocess.run(
            [
                sys.executable,
                "-m",
                "openapi_python_client",
                "generate",
                "--path",
                str(pruned),
                "--config",
                str(ROOT / "openapi-python-client.yaml"),
                "--custom-template-path",
                str(ROOT / "templates"),
                "--meta",
                "none",
                "--output-path",
                str(output),
                "--fail-on-warning",
            ],
            check=True,
            cwd=temp,
        )
        for name, private in GENERATED.items():
            (output / name).rename(output / private)
        for name, private in GENERATED_FILES.items():
            (output / name).rename(output / private)
        for path in output.rglob("*.py"):
            path.write_text(_private(path.read_text()))
        subprocess.run(
            [sys.executable, "-m", "ruff", "format", "--quiet", str(output)], check=True
        )
        for private in GENERATED.values():
            shutil.rmtree(PACKAGE / private, ignore_errors=True)
            shutil.copytree(output / private, PACKAGE / private)
        for private in GENERATED_FILES.values():
            shutil.copy2(output / private, PACKAGE / private)


if __name__ == "__main__":
    main()
