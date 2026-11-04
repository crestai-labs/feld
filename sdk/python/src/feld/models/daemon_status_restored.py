from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="DaemonStatusRestored")


@_attrs_define
class DaemonStatusRestored:
    """
    Attributes:
        actors (int):
        running_sessions (int):
        schedules (int):
    """

    actors: int
    running_sessions: int
    schedules: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        actors = self.actors

        running_sessions = self.running_sessions

        schedules = self.schedules

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "actors": actors,
                "runningSessions": running_sessions,
                "schedules": schedules,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        actors = d.pop("actors")

        running_sessions = d.pop("runningSessions")

        schedules = d.pop("schedules")

        daemon_status_restored = cls(
            actors=actors,
            running_sessions=running_sessions,
            schedules=schedules,
        )

        daemon_status_restored.additional_properties = d
        return daemon_status_restored

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
