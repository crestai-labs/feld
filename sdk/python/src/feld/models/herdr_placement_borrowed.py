from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="HerdrPlacementBorrowed")


@_attrs_define
class HerdrPlacementBorrowed:
    """Borrowed existing resource. Consists of the selector a person wrote and IDs checked right before creating resources

    Attributes:
        selector (str):
        workspace_id (str):
        tab_id (str | Unset):
        pane_id (str | Unset):
        terminal_id (str | Unset):
    """

    selector: str
    workspace_id: str
    tab_id: str | Unset = UNSET
    pane_id: str | Unset = UNSET
    terminal_id: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        selector = self.selector

        workspace_id = self.workspace_id

        tab_id = self.tab_id

        pane_id = self.pane_id

        terminal_id = self.terminal_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "selector": selector,
                "workspaceId": workspace_id,
            }
        )
        if tab_id is not UNSET:
            field_dict["tabId"] = tab_id
        if pane_id is not UNSET:
            field_dict["paneId"] = pane_id
        if terminal_id is not UNSET:
            field_dict["terminalId"] = terminal_id

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        selector = d.pop("selector")

        workspace_id = d.pop("workspaceId")

        tab_id = d.pop("tabId", UNSET)

        pane_id = d.pop("paneId", UNSET)

        terminal_id = d.pop("terminalId", UNSET)

        herdr_placement_borrowed = cls(
            selector=selector,
            workspace_id=workspace_id,
            tab_id=tab_id,
            pane_id=pane_id,
            terminal_id=terminal_id,
        )

        herdr_placement_borrowed.additional_properties = d
        return herdr_placement_borrowed

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
