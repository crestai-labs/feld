from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.runtime_entry import RuntimeEntry


T = TypeVar("T", bound="ListRuntimesResponse200")


@_attrs_define
class ListRuntimesResponse200:
    """
    Attributes:
        runtimes (list[RuntimeEntry]):
    """

    runtimes: list[RuntimeEntry]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        runtimes = []
        for runtimes_item_data in self.runtimes:
            runtimes_item = runtimes_item_data.to_dict()
            runtimes.append(runtimes_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "runtimes": runtimes,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.runtime_entry import RuntimeEntry

        d = dict(src_dict)
        runtimes = []
        _runtimes = d.pop("runtimes")
        for runtimes_item_data in _runtimes:
            runtimes_item = RuntimeEntry.from_dict(runtimes_item_data)

            runtimes.append(runtimes_item)

        list_runtimes_response_200 = cls(
            runtimes=runtimes,
        )

        list_runtimes_response_200.additional_properties = d
        return list_runtimes_response_200

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
