from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from .._models.record import Record


T = TypeVar("T", bound="ListRecordsResponse200")


@_attrs_define
class ListRecordsResponse200:
    """
    Attributes:
        records (list[Record]):
        next_after (int): The `sequence` of the last record. The request's `after` when there is none
    """

    records: list[Record]
    next_after: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        records = []
        for records_item_data in self.records:
            records_item = records_item_data.to_dict()
            records.append(records_item)

        next_after = self.next_after

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "records": records,
                "nextAfter": next_after,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.record import Record

        d = dict(src_dict)
        records = []
        _records = d.pop("records")
        for records_item_data in _records:
            records_item = Record.from_dict(records_item_data)

            records.append(records_item)

        next_after = d.pop("nextAfter")

        list_records_response_200 = cls(
            records=records,
            next_after=next_after,
        )

        list_records_response_200.additional_properties = d
        return list_records_response_200

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
