from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="CreateActorBodyScheduleType0")


@_attrs_define
class CreateActorBodyScheduleType0:
    """
    Attributes:
        cron (str): 5-field cron expression
        timezone (None | str | Unset): IANA timezone
    """

    cron: str
    timezone: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        cron = self.cron

        timezone: None | str | Unset
        if isinstance(self.timezone, Unset):
            timezone = UNSET
        else:
            timezone = self.timezone

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "cron": cron,
            }
        )
        if timezone is not UNSET:
            field_dict["timezone"] = timezone

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        cron = d.pop("cron")

        def _parse_timezone(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        timezone = _parse_timezone(d.pop("timezone", UNSET))

        create_actor_body_schedule_type_0 = cls(
            cron=cron,
            timezone=timezone,
        )

        return create_actor_body_schedule_type_0
