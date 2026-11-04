from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="MessageRead")


@_attrs_define
class MessageRead:
    """
    Attributes:
        session_id (str):
        at (datetime.datetime):
        read_id (str):
        fifo (int):
    """

    session_id: str
    at: datetime.datetime
    read_id: str
    fifo: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        session_id = self.session_id

        at = self.at.isoformat()

        read_id = self.read_id

        fifo = self.fifo

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "sessionId": session_id,
                "at": at,
                "readId": read_id,
                "fifo": fifo,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        session_id = d.pop("sessionId")

        at = datetime.datetime.fromisoformat(d.pop("at"))

        read_id = d.pop("readId")

        fifo = d.pop("fifo")

        message_read = cls(
            session_id=session_id,
            at=at,
            read_id=read_id,
            fifo=fifo,
        )

        message_read.additional_properties = d
        return message_read

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
