from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from .._types import UNSET, Unset

T = TypeVar("T", bound="SetActorLabelBody")


@_attrs_define
class SetActorLabelBody:
    """
    Attributes:
        until (datetime.datetime | Unset): When the label lapses. Omitted
        reason (str | Unset):
    """

    until: datetime.datetime | Unset = UNSET
    reason: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        until: str | Unset = UNSET
        if not isinstance(self.until, Unset):
            until = self.until.isoformat()

        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if until is not UNSET:
            field_dict["until"] = until
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        _until = d.pop("until", UNSET)
        until: datetime.datetime | Unset
        if isinstance(_until, Unset):
            until = UNSET
        else:
            until = datetime.datetime.fromisoformat(_until)

        reason = d.pop("reason", UNSET)

        set_actor_label_body = cls(
            until=until,
            reason=reason,
        )

        return set_actor_label_body
