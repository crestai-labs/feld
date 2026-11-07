from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="PlannedTick")


@_attrs_define
class PlannedTick:
    """
    Attributes:
        workspace (str):
        actor (str):
        planned (datetime.datetime):
    """

    workspace: str
    actor: str
    planned: datetime.datetime
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        workspace = self.workspace

        actor = self.actor

        planned = self.planned.isoformat()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "workspace": workspace,
                "actor": actor,
                "planned": planned,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        workspace = d.pop("workspace")

        actor = d.pop("actor")

        planned = datetime.datetime.fromisoformat(d.pop("planned"))

        planned_tick = cls(
            workspace=workspace,
            actor=actor,
            planned=planned,
        )

        planned_tick.additional_properties = d
        return planned_tick

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
