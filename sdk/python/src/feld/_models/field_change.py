from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="FieldChange")


@_attrs_define
class FieldChange:
    """One field of an Actor declaration as it changes, by its dotted path

    Attributes:
        field (str):
        before (Any): The value the live Actor holds; null when the field was absent
        after (Any): The value the declaration gives; null when it removes the field
    """

    field: str
    before: Any
    after: Any
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        field = self.field

        before = self.before

        after = self.after

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "field": field,
                "before": before,
                "after": after,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        field = d.pop("field")

        before = d.pop("before")

        after = d.pop("after")

        field_change = cls(
            field=field,
            before=before,
            after=after,
        )

        field_change.additional_properties = d
        return field_change

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
