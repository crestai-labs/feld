from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from .._models.planned_tick import PlannedTick
    from .._models.scheduler_restart import SchedulerRestart


T = TypeVar("T", bound="DaemonStatusScheduler")


@_attrs_define
class DaemonStatusScheduler:
    """
    Attributes:
        woke_at (datetime.datetime | None): Time the Scheduler last woke
        planned (list[PlannedTick]): Ticks that enabled Schedules are waiting for, earliest planned time first
        restarts (int): Number of times the Scheduler has been restarted since the daemon started
        last_restart (None | SchedulerRestart):
    """

    woke_at: datetime.datetime | None
    planned: list[PlannedTick]
    restarts: int
    last_restart: None | SchedulerRestart
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from .._models.scheduler_restart import SchedulerRestart

        woke_at: None | str
        if isinstance(self.woke_at, datetime.datetime):
            woke_at = self.woke_at.isoformat()
        else:
            woke_at = self.woke_at

        planned = []
        for planned_item_data in self.planned:
            planned_item = planned_item_data.to_dict()
            planned.append(planned_item)

        restarts = self.restarts

        last_restart: dict[str, Any] | None
        if isinstance(self.last_restart, SchedulerRestart):
            last_restart = self.last_restart.to_dict()
        else:
            last_restart = self.last_restart

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "wokeAt": woke_at,
                "planned": planned,
                "restarts": restarts,
                "lastRestart": last_restart,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.planned_tick import PlannedTick
        from .._models.scheduler_restart import SchedulerRestart

        d = dict(src_dict)

        def _parse_woke_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                woke_at_type_0 = datetime.datetime.fromisoformat(data)

                return woke_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        woke_at = _parse_woke_at(d.pop("wokeAt"))

        planned = []
        _planned = d.pop("planned")
        for planned_item_data in _planned:
            planned_item = PlannedTick.from_dict(planned_item_data)

            planned.append(planned_item)

        restarts = d.pop("restarts")

        def _parse_last_restart(data: object) -> None | SchedulerRestart:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                last_restart_type_0 = SchedulerRestart.from_dict(data)

                return last_restart_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | SchedulerRestart, data)

        last_restart = _parse_last_restart(d.pop("lastRestart"))

        daemon_status_scheduler = cls(
            woke_at=woke_at,
            planned=planned,
            restarts=restarts,
            last_restart=last_restart,
        )

        daemon_status_scheduler.additional_properties = d
        return daemon_status_scheduler

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
