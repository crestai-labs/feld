from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="CreatePairingCodeBody")


@_attrs_define
class CreatePairingCodeBody:
    """
    Attributes:
        name (str | Unset):  Default: 'browser'.
    """

    name: str | Unset = "browser"

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if name is not UNSET:
            field_dict["name"] = name

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        name = d.pop("name", UNSET)

        create_pairing_code_body = cls(
            name=name,
        )

        return create_pairing_code_body
