from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="CreateWorkspaceBody")


@_attrs_define
class CreateWorkspaceBody:
    """
    Attributes:
        key (str):
    """

    key: str

    def to_dict(self) -> dict[str, Any]:
        key = self.key

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key": key,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        key = d.pop("key")

        create_workspace_body = cls(
            key=key,
        )

        return create_workspace_body
