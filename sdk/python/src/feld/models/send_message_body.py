from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="SendMessageBody")


@_attrs_define
class SendMessageBody:
    """
    Attributes:
        body (str):
        label (str | Unset): A name given to the kind of Message. Follows the same grammar as `Key`
    """

    body: str
    label: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        body = self.body

        label = self.label

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "body": body,
            }
        )
        if label is not UNSET:
            field_dict["label"] = label

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        body = d.pop("body")

        label = d.pop("label", UNSET)

        send_message_body = cls(
            body=body,
            label=label,
        )

        return send_message_body
