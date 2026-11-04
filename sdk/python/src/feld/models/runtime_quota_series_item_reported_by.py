from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RuntimeQuotaSeriesItemReportedBy")


@_attrs_define
class RuntimeQuotaSeriesItemReportedBy:
    """
    Attributes:
        session (str):
        workspace (str):
        actor (str):
    """

    session: str
    workspace: str
    actor: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        session = self.session

        workspace = self.workspace

        actor = self.actor

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "session": session,
                "workspace": workspace,
                "actor": actor,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        session = d.pop("session")

        workspace = d.pop("workspace")

        actor = d.pop("actor")

        runtime_quota_series_item_reported_by = cls(
            session=session,
            workspace=workspace,
            actor=actor,
        )

        runtime_quota_series_item_reported_by.additional_properties = d
        return runtime_quota_series_item_reported_by

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
