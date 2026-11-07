"""The public API is the hand-written client and the types it returns and accepts; the
generated layer and the generator's wrapper types stay private."""

from __future__ import annotations

import importlib.util
import inspect
import re

import attrs

import feld
from feld import _models, models, resources

PRIVATE = re.compile(r"Body$|Response200$|Type\d+$")
NAME = re.compile(r"\b[A-Z]\w*")


def test_the_models_are_not_generator_wrappers() -> None:
    assert sorted(models.__all__) == models.__all__
    assert [name for name in models.__all__ if PRIVATE.search(name)] == []
    for name in models.__all__:
        assert hasattr(models, name)


def generated(annotation: str) -> set[str]:
    return {name for name in NAME.findall(annotation) if name in _models.__all__}


def test_what_the_client_returns_and_accepts_is_public() -> None:
    referenced: set[str] = set()
    for _, resource in inspect.getmembers(resources, inspect.isclass):
        for name, method in inspect.getmembers(resource, inspect.isfunction):
            if not name.startswith("_"):
                for annotation in method.__annotations__.values():
                    referenced |= generated(str(annotation))
    pending = list(referenced)
    while pending:
        cls = getattr(_models, pending.pop())
        if attrs.has(cls):
            for field in attrs.fields(cls):
                for name in generated(str(field.type)) - referenced:
                    referenced.add(name)
                    pending.append(name)
    assert referenced - set(models.__all__) == set()


def test_no_public_signature_takes_the_generated_placeholder() -> None:
    for _, resource in inspect.getmembers(resources, inspect.isclass):
        for name, method in inspect.getmembers(resource, inspect.isfunction):
            if not name.startswith("_"):
                for annotation in method.__annotations__.values():
                    assert "Unset" not in str(annotation), (resource, name)
    for annotation in feld.Client.__init__.__annotations__.values():
        assert "Unset" not in str(annotation)


def test_the_generated_layer_is_private() -> None:
    for name in ("feld.api", "feld.types", "feld.errors"):
        assert importlib.util.find_spec(name) is None
    assert set(feld.__all__) == {
        "Client",
        "FeldError",
        "FeldOperational",
        "FeldRejected",
        "connect",
    }
