from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.workspace import Workspace


T = TypeVar("T", bound="ListWorkspacesResponse200")


@_attrs_define
class ListWorkspacesResponse200:
    """
    Attributes:
        workspaces (list[Workspace]):
    """

    workspaces: list[Workspace]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        workspaces = []
        for workspaces_item_data in self.workspaces:
            workspaces_item = workspaces_item_data.to_dict()
            workspaces.append(workspaces_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "workspaces": workspaces,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.workspace import Workspace

        d = dict(src_dict)
        workspaces = []
        _workspaces = d.pop("workspaces")
        for workspaces_item_data in _workspaces:
            workspaces_item = Workspace.from_dict(workspaces_item_data)

            workspaces.append(workspaces_item)

        list_workspaces_response_200 = cls(
            workspaces=workspaces,
        )

        list_workspaces_response_200.additional_properties = d
        return list_workspaces_response_200

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
