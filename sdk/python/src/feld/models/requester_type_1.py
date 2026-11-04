from __future__ import annotations

from collections.abc import Mapping
from typing import (
    Any,
    Literal,
    Self,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RequesterType1")


@_attrs_define
class RequesterType1:
    """
    Attributes:
        type_ (Literal['actor']):
        id (str): `id` of the Actor
        workspace (str):
        name (str):
    """

    type_: Literal["actor"]
    id: str
    workspace: str
    name: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        type_ = self.type_

        id = self.id

        workspace = self.workspace

        name = self.name

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "type": type_,
                "id": id,
                "workspace": workspace,
                "name": name,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        type_ = cast(Literal["actor"], d.pop("type"))
        if type_ != "actor":
            raise ValueError(f"type must match const 'actor', got '{type_}'")

        id = d.pop("id")

        workspace = d.pop("workspace")

        name = d.pop("name")

        requester_type_1 = cls(
            type_=type_,
            id=id,
            workspace=workspace,
            name=name,
        )

        requester_type_1.additional_properties = d
        return requester_type_1

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
