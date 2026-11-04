from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="Browser")


@_attrs_define
class Browser:
    """
    Attributes:
        id (str):
        name (str):
        paired_at (datetime.datetime):
        last_used_at (datetime.datetime):
        expires_at (datetime.datetime): The earlier of 7 days from `lastUsedAt` and 30 days from `pairedAt`
        revoked_at (datetime.datetime | Unset): Present only in the revocation response
    """

    id: str
    name: str
    paired_at: datetime.datetime
    last_used_at: datetime.datetime
    expires_at: datetime.datetime
    revoked_at: datetime.datetime | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        name = self.name

        paired_at = self.paired_at.isoformat()

        last_used_at = self.last_used_at.isoformat()

        expires_at = self.expires_at.isoformat()

        revoked_at: str | Unset = UNSET
        if not isinstance(self.revoked_at, Unset):
            revoked_at = self.revoked_at.isoformat()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "pairedAt": paired_at,
                "lastUsedAt": last_used_at,
                "expiresAt": expires_at,
            }
        )
        if revoked_at is not UNSET:
            field_dict["revokedAt"] = revoked_at

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        paired_at = datetime.datetime.fromisoformat(d.pop("pairedAt"))

        last_used_at = datetime.datetime.fromisoformat(d.pop("lastUsedAt"))

        expires_at = datetime.datetime.fromisoformat(d.pop("expiresAt"))

        _revoked_at = d.pop("revokedAt", UNSET)
        revoked_at: datetime.datetime | Unset
        if isinstance(_revoked_at, Unset):
            revoked_at = UNSET
        else:
            revoked_at = datetime.datetime.fromisoformat(_revoked_at)

        browser = cls(
            id=id,
            name=name,
            paired_at=paired_at,
            last_used_at=last_used_at,
            expires_at=expires_at,
            revoked_at=revoked_at,
        )

        browser.additional_properties = d
        return browser

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
