from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.tmux_placement_endpoint_kind import TmuxPlacementEndpointKind

T = TypeVar("T", bound="TmuxPlacementEndpoint")


@_attrs_define
class TmuxPlacementEndpoint:
    """
    Attributes:
        kind (TmuxPlacementEndpointKind):
        value (str):
    """

    kind: TmuxPlacementEndpointKind
    value: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind.value

        value = self.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "kind": kind,
                "value": value,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        kind = TmuxPlacementEndpointKind(d.pop("kind"))

        value = d.pop("value")

        tmux_placement_endpoint = cls(
            kind=kind,
            value=value,
        )

        tmux_placement_endpoint.additional_properties = d
        return tmux_placement_endpoint

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
