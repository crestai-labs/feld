from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="MetricBin")


@_attrs_define
class MetricBin:
    """
    Attributes:
        start (datetime.datetime):
        end (datetime.datetime | None):
        value (Any):
        session (str | Unset): Session whose value the bin took, for the context window utilization of Runtime metrics
    """

    start: datetime.datetime
    end: datetime.datetime | None
    value: Any
    session: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        start = self.start.isoformat()

        end: None | str
        if isinstance(self.end, datetime.datetime):
            end = self.end.isoformat()
        else:
            end = self.end

        value = self.value

        session = self.session

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "start": start,
                "end": end,
                "value": value,
            }
        )
        if session is not UNSET:
            field_dict["session"] = session

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        start = datetime.datetime.fromisoformat(d.pop("start"))

        def _parse_end(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                end_type_0 = datetime.datetime.fromisoformat(data)

                return end_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        end = _parse_end(d.pop("end"))

        value = d.pop("value")

        session = d.pop("session", UNSET)

        metric_bin = cls(
            start=start,
            end=end,
            value=value,
            session=session,
        )

        metric_bin.additional_properties = d
        return metric_bin

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
