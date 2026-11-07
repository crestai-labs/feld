from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RuntimeEntry")


@_attrs_define
class RuntimeEntry:
    """An installed Runtime. runningSessions is the number of current Sessions across all Workspaces whose execution uses
    that Runtime.

        Attributes:
            name (str):
            label (str):
            running_sessions (int):
    """

    name: str
    label: str
    running_sessions: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        label = self.label

        running_sessions = self.running_sessions

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
                "label": label,
                "runningSessions": running_sessions,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        name = d.pop("name")

        label = d.pop("label")

        running_sessions = d.pop("runningSessions")

        runtime_entry = cls(
            name=name,
            label=label,
            running_sessions=running_sessions,
        )

        runtime_entry.additional_properties = d
        return runtime_entry

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
