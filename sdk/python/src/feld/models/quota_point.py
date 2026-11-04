from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="QuotaPoint")


@_attrs_define
class QuotaPoint:
    """
    Attributes:
        at (datetime.datetime):
        used_percent (float):
    """

    at: datetime.datetime
    used_percent: float
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        at = self.at.isoformat()

        used_percent = self.used_percent

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "at": at,
                "usedPercent": used_percent,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        at = datetime.datetime.fromisoformat(d.pop("at"))

        used_percent = d.pop("usedPercent")

        quota_point = cls(
            at=at,
            used_percent=used_percent,
        )

        quota_point.additional_properties = d
        return quota_point

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
