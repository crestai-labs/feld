from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.requester_type_0 import RequesterType0
    from ..models.requester_type_1 import RequesterType1


T = TypeVar("T", bound="SessionSpawnedBy")


@_attrs_define
class SessionSpawnedBy:
    """Projection of the spawned record that launched this Session

    Attributes:
        spawner (RequesterType0 | RequesterType1):
        spawner_session_id (None | str):
    """

    spawner: RequesterType0 | RequesterType1
    spawner_session_id: None | str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.requester_type_0 import RequesterType0

        spawner: dict[str, Any]
        if isinstance(self.spawner, RequesterType0):
            spawner = self.spawner.to_dict()
        else:
            spawner = self.spawner.to_dict()

        spawner_session_id: None | str
        spawner_session_id = self.spawner_session_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "spawner": spawner,
                "spawnerSessionId": spawner_session_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.requester_type_0 import RequesterType0
        from ..models.requester_type_1 import RequesterType1

        d = dict(src_dict)

        def _parse_spawner(data: object) -> RequesterType0 | RequesterType1:
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_requester_type_0 = RequesterType0.from_dict(data)

                return componentsschemas_requester_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_requester_type_1 = RequesterType1.from_dict(data)

            return componentsschemas_requester_type_1

        spawner = _parse_spawner(d.pop("spawner"))

        def _parse_spawner_session_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        spawner_session_id = _parse_spawner_session_id(d.pop("spawnerSessionId"))

        session_spawned_by = cls(
            spawner=spawner,
            spawner_session_id=spawner_session_id,
        )

        session_spawned_by.additional_properties = d
        return session_spawned_by

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
