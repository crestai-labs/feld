from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RuntimeMetricsMetricsItemSessions")


@_attrs_define
class RuntimeMetricsMetricsItemSessions:
    """Count of aggregated Sessions per status

    Attributes:
        reported (int):
        unavailable (int):
        na (int):
    """

    reported: int
    unavailable: int
    na: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        reported = self.reported

        unavailable = self.unavailable

        na = self.na

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "reported": reported,
                "unavailable": unavailable,
                "n/a": na,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        reported = d.pop("reported")

        unavailable = d.pop("unavailable")

        na = d.pop("n/a")

        runtime_metrics_metrics_item_sessions = cls(
            reported=reported,
            unavailable=unavailable,
            na=na,
        )

        runtime_metrics_metrics_item_sessions.additional_properties = d
        return runtime_metrics_metrics_item_sessions

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
