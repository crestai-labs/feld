from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.lifecycle_action import LifecycleAction


T = TypeVar("T", bound="SessionLifecycle")


@_attrs_define
class SessionLifecycle:
    """
    Attributes:
        start_settled (datetime.datetime | None):
        start_failed (datetime.datetime | None): Time it was settled that the Provider finished the launch without
            creating the entity and the Session did not start
        stop (LifecycleAction | None):
        kill (LifecycleAction | None):
    """

    start_settled: datetime.datetime | None
    start_failed: datetime.datetime | None
    stop: LifecycleAction | None
    kill: LifecycleAction | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.lifecycle_action import LifecycleAction

        start_settled: None | str
        if isinstance(self.start_settled, datetime.datetime):
            start_settled = self.start_settled.isoformat()
        else:
            start_settled = self.start_settled

        start_failed: None | str
        if isinstance(self.start_failed, datetime.datetime):
            start_failed = self.start_failed.isoformat()
        else:
            start_failed = self.start_failed

        stop: dict[str, Any] | None
        if isinstance(self.stop, LifecycleAction):
            stop = self.stop.to_dict()
        else:
            stop = self.stop

        kill: dict[str, Any] | None
        if isinstance(self.kill, LifecycleAction):
            kill = self.kill.to_dict()
        else:
            kill = self.kill

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "startSettled": start_settled,
                "startFailed": start_failed,
                "stop": stop,
                "kill": kill,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.lifecycle_action import LifecycleAction

        d = dict(src_dict)

        def _parse_start_settled(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                start_settled_type_0 = datetime.datetime.fromisoformat(data)

                return start_settled_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        start_settled = _parse_start_settled(d.pop("startSettled"))

        def _parse_start_failed(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                start_failed_type_0 = datetime.datetime.fromisoformat(data)

                return start_failed_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        start_failed = _parse_start_failed(d.pop("startFailed"))

        def _parse_stop(data: object) -> LifecycleAction | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                stop_type_0 = LifecycleAction.from_dict(data)

                return stop_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(LifecycleAction | None, data)

        stop = _parse_stop(d.pop("stop"))

        def _parse_kill(data: object) -> LifecycleAction | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                kill_type_0 = LifecycleAction.from_dict(data)

                return kill_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(LifecycleAction | None, data)

        kill = _parse_kill(d.pop("kill"))

        session_lifecycle = cls(
            start_settled=start_settled,
            start_failed=start_failed,
            stop=stop,
            kill=kill,
        )

        session_lifecycle.additional_properties = d
        return session_lifecycle

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
