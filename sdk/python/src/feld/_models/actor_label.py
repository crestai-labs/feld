from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from .._models.actor_label_by import ActorLabelBy

T = TypeVar("T", bound="ActorLabel")


@_attrs_define
class ActorLabel:
    """
    Attributes:
        key (str):
        until (datetime.datetime | None): When the label lapses. null stays until cleared
        reason (None | str):
        by (ActorLabelBy): `admin`, or `actor` for a Session of the Actor
        set_at (datetime.datetime):
    """

    key: str
    until: datetime.datetime | None
    reason: None | str
    by: ActorLabelBy
    set_at: datetime.datetime
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        key = self.key

        until: None | str
        if isinstance(self.until, datetime.datetime):
            until = self.until.isoformat()
        else:
            until = self.until

        reason: None | str
        reason = self.reason

        by = self.by.value

        set_at = self.set_at.isoformat()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "key": key,
                "until": until,
                "reason": reason,
                "by": by,
                "setAt": set_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        key = d.pop("key")

        def _parse_until(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                until_type_0 = datetime.datetime.fromisoformat(data)

                return until_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        until = _parse_until(d.pop("until"))

        def _parse_reason(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        reason = _parse_reason(d.pop("reason"))

        by = ActorLabelBy(d.pop("by"))

        set_at = datetime.datetime.fromisoformat(d.pop("setAt"))

        actor_label = cls(
            key=key,
            until=until,
            reason=reason,
            by=by,
            set_at=set_at,
        )

        actor_label.additional_properties = d
        return actor_label

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
