from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="PairBrowserBody")


@_attrs_define
class PairBrowserBody:
    """
    Attributes:
        code (str):
    """

    code: str

    def to_dict(self) -> dict[str, Any]:
        code = self.code

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "code": code,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        code = d.pop("code")

        pair_browser_body = cls(
            code=code,
        )

        return pair_browser_body
