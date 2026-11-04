from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="UpdateActorBodySchedule")


@_attrs_define
class UpdateActorBodySchedule:
    """
    Attributes:
        cron (str | Unset):
        timezone (str | Unset):
        enabled (bool | Unset):
    """

    cron: str | Unset = UNSET
    timezone: str | Unset = UNSET
    enabled: bool | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        cron = self.cron

        timezone = self.timezone

        enabled = self.enabled

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if cron is not UNSET:
            field_dict["cron"] = cron
        if timezone is not UNSET:
            field_dict["timezone"] = timezone
        if enabled is not UNSET:
            field_dict["enabled"] = enabled

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        cron = d.pop("cron", UNSET)

        timezone = d.pop("timezone", UNSET)

        enabled = d.pop("enabled", UNSET)

        update_actor_body_schedule = cls(
            cron=cron,
            timezone=timezone,
            enabled=enabled,
        )

        return update_actor_body_schedule
