from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="Workspace")


@_attrs_define
class Workspace:
    """
    Attributes:
        id (str):
        key (str):
        actor_count (int):
        created_at (datetime.datetime):
        deleted_at (datetime.datetime | None):
    """

    id: str
    key: str
    actor_count: int
    created_at: datetime.datetime
    deleted_at: datetime.datetime | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        key = self.key

        actor_count = self.actor_count

        created_at = self.created_at.isoformat()

        deleted_at: None | str
        if isinstance(self.deleted_at, datetime.datetime):
            deleted_at = self.deleted_at.isoformat()
        else:
            deleted_at = self.deleted_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "key": key,
                "actorCount": actor_count,
                "createdAt": created_at,
                "deletedAt": deleted_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        id = d.pop("id")

        key = d.pop("key")

        actor_count = d.pop("actorCount")

        created_at = datetime.datetime.fromisoformat(d.pop("createdAt"))

        def _parse_deleted_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                deleted_at_type_0 = datetime.datetime.fromisoformat(data)

                return deleted_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        deleted_at = _parse_deleted_at(d.pop("deletedAt"))

        workspace = cls(
            id=id,
            key=key,
            actor_count=actor_count,
            created_at=created_at,
            deleted_at=deleted_at,
        )

        workspace.additional_properties = d
        return workspace

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
