from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RecordTelemetryResponse200")


@_attrs_define
class RecordTelemetryResponse200:
    """
    Attributes:
        accepted (bool):
        duplicate (bool):
        session (str):
    """

    accepted: bool
    duplicate: bool
    session: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        accepted = self.accepted

        duplicate = self.duplicate

        session = self.session

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "accepted": accepted,
                "duplicate": duplicate,
                "session": session,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        accepted = d.pop("accepted")

        duplicate = d.pop("duplicate")

        session = d.pop("session")

        record_telemetry_response_200 = cls(
            accepted=accepted,
            duplicate=duplicate,
            session=session,
        )

        record_telemetry_response_200.additional_properties = d
        return record_telemetry_response_200

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
