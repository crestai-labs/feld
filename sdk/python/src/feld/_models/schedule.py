from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="Schedule")


@_attrs_define
class Schedule:
    """
    Attributes:
        cron (str):
        timezone (str):
        enabled (bool):
        created_at (datetime.datetime):
        paused_at (datetime.datetime | None):
        enabled_at (datetime.datetime | None):
        next_run (datetime.datetime | None): Next run time (UTC) when enabled. `cron` is interpreted in the time of
            `timezone`
        skipped_ticks (int): Number of the Actor's `schedule.skipped` records
        overdue_tick (datetime.datetime | None): Planned time of the latest tick, after the daemon started and after the
            Actor's last change, for which neither a Session nor `schedule.skipped` nor `schedule.missed` remains 60 seconds
            past its planned time
    """

    cron: str
    timezone: str
    enabled: bool
    created_at: datetime.datetime
    paused_at: datetime.datetime | None
    enabled_at: datetime.datetime | None
    next_run: datetime.datetime | None
    skipped_ticks: int
    overdue_tick: datetime.datetime | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        cron = self.cron

        timezone = self.timezone

        enabled = self.enabled

        created_at = self.created_at.isoformat()

        paused_at: None | str
        if isinstance(self.paused_at, datetime.datetime):
            paused_at = self.paused_at.isoformat()
        else:
            paused_at = self.paused_at

        enabled_at: None | str
        if isinstance(self.enabled_at, datetime.datetime):
            enabled_at = self.enabled_at.isoformat()
        else:
            enabled_at = self.enabled_at

        next_run: None | str
        if isinstance(self.next_run, datetime.datetime):
            next_run = self.next_run.isoformat()
        else:
            next_run = self.next_run

        skipped_ticks = self.skipped_ticks

        overdue_tick: None | str
        if isinstance(self.overdue_tick, datetime.datetime):
            overdue_tick = self.overdue_tick.isoformat()
        else:
            overdue_tick = self.overdue_tick

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "cron": cron,
                "timezone": timezone,
                "enabled": enabled,
                "createdAt": created_at,
                "pausedAt": paused_at,
                "enabledAt": enabled_at,
                "nextRun": next_run,
                "skippedTicks": skipped_ticks,
                "overdueTick": overdue_tick,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        cron = d.pop("cron")

        timezone = d.pop("timezone")

        enabled = d.pop("enabled")

        created_at = datetime.datetime.fromisoformat(d.pop("createdAt"))

        def _parse_paused_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                paused_at_type_0 = datetime.datetime.fromisoformat(data)

                return paused_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        paused_at = _parse_paused_at(d.pop("pausedAt"))

        def _parse_enabled_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                enabled_at_type_0 = datetime.datetime.fromisoformat(data)

                return enabled_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        enabled_at = _parse_enabled_at(d.pop("enabledAt"))

        def _parse_next_run(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                next_run_type_0 = datetime.datetime.fromisoformat(data)

                return next_run_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        next_run = _parse_next_run(d.pop("nextRun"))

        skipped_ticks = d.pop("skippedTicks")

        def _parse_overdue_tick(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                overdue_tick_type_0 = datetime.datetime.fromisoformat(data)

                return overdue_tick_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        overdue_tick = _parse_overdue_tick(d.pop("overdueTick"))

        schedule = cls(
            cron=cron,
            timezone=timezone,
            enabled=enabled,
            created_at=created_at,
            paused_at=paused_at,
            enabled_at=enabled_at,
            next_run=next_run,
            skipped_ticks=skipped_ticks,
            overdue_tick=overdue_tick,
        )

        schedule.additional_properties = d
        return schedule

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
