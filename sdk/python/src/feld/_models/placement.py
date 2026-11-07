from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="Placement")


@_attrs_define
class Placement:
    """
    Attributes:
        provider (str):
        node (str):
        workdir (str):
        placement (dict[str, Any]): Placement returned by the Provider, stored as is. Until the launch is settled, it
            holds the in-progress placement (a subset of the fields).
            core reads only four keys; the rest belong to the Provider.
            `summary` is how to read the resource and `attachCommand` is the command a person uses to reach the resource;
            core writes both as answered by the Provider.
            The Runtime process that an `attached` Session attached goes into the Provider-independent `process`, and the
            process of the `<command>` that an `owned` Session launched goes into `program`.
    """

    provider: str
    node: str
    workdir: str
    placement: dict[str, Any]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        provider = self.provider

        node = self.node

        workdir = self.workdir

        placement = self.placement

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "provider": provider,
                "node": node,
                "workdir": workdir,
                "placement": placement,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        provider = d.pop("provider")

        node = d.pop("node")

        workdir = d.pop("workdir")

        placement = d.pop("placement")

        placement = cls(
            provider=provider,
            node=node,
            workdir=workdir,
            placement=placement,
        )

        placement.additional_properties = d
        return placement

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
