from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RestartDaemonResponse200")


@_attrs_define
class RestartDaemonResponse200:
    """
    Attributes:
        accepted (bool):
        pid (int): The pid of the daemon that stops.
        instance_id (str): The instance ID of the daemon that stops.
    """

    accepted: bool
    pid: int
    instance_id: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        accepted = self.accepted

        pid = self.pid

        instance_id = self.instance_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "accepted": accepted,
                "pid": pid,
                "instanceId": instance_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        accepted = d.pop("accepted")

        pid = d.pop("pid")

        instance_id = d.pop("instanceId")

        restart_daemon_response_200 = cls(
            accepted=accepted,
            pid=pid,
            instance_id=instance_id,
        )

        restart_daemon_response_200.additional_properties = d
        return restart_daemon_response_200

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
