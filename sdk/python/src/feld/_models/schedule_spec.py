from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from .._types import UNSET, Unset

T = TypeVar("T", bound="ScheduleSpec")


@_attrs_define
class ScheduleSpec:
    """
    Attributes:
        cron (str): 5-field cron expression
        timezone (str | Unset): IANA timezone
        enabled (bool | Unset):
    """

    cron: str
    timezone: str | Unset = UNSET
    enabled: bool | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        cron = self.cron

        timezone = self.timezone

        enabled = self.enabled

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "cron": cron,
            }
        )
        if timezone is not UNSET:
            field_dict["timezone"] = timezone
        if enabled is not UNSET:
            field_dict["enabled"] = enabled

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        cron = d.pop("cron")

        timezone = d.pop("timezone", UNSET)

        enabled = d.pop("enabled", UNSET)

        schedule_spec = cls(
            cron=cron,
            timezone=timezone,
            enabled=enabled,
        )

        return schedule_spec
