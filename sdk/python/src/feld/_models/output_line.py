from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from .._models.output_stream import OutputStream

T = TypeVar("T", bound="OutputLine")


@_attrs_define
class OutputLine:
    """
    Attributes:
        sequence (int):
        stream (OutputStream):
        timestamp (datetime.datetime):
        text (str):
    """

    sequence: int
    stream: OutputStream
    timestamp: datetime.datetime
    text: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        sequence = self.sequence

        stream = self.stream.value

        timestamp = self.timestamp.isoformat()

        text = self.text

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "sequence": sequence,
                "stream": stream,
                "timestamp": timestamp,
                "text": text,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        sequence = d.pop("sequence")

        stream = OutputStream(d.pop("stream"))

        timestamp = datetime.datetime.fromisoformat(d.pop("timestamp"))

        text = d.pop("text")

        output_line = cls(
            sequence=sequence,
            stream=stream,
            timestamp=timestamp,
            text=text,
        )

        output_line.additional_properties = d
        return output_line

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
