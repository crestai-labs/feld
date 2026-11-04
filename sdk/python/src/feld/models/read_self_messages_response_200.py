from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.message_read import MessageRead


T = TypeVar("T", bound="ReadSelfMessagesResponse200")


@_attrs_define
class ReadSelfMessagesResponse200:
    """
    Attributes:
        reads (list[MessageRead]):
    """

    reads: list[MessageRead]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        reads = []
        for reads_item_data in self.reads:
            reads_item = reads_item_data.to_dict()
            reads.append(reads_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "reads": reads,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.message_read import MessageRead

        d = dict(src_dict)
        reads = []
        _reads = d.pop("reads")
        for reads_item_data in _reads:
            reads_item = MessageRead.from_dict(reads_item_data)

            reads.append(reads_item)

        read_self_messages_response_200 = cls(
            reads=reads,
        )

        read_self_messages_response_200.additional_properties = d
        return read_self_messages_response_200

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
