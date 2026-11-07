from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from .._models.declare_response_action import DeclareResponseAction

if TYPE_CHECKING:
    from .._models.actor import Actor
    from .._models.field_change import FieldChange


T = TypeVar("T", bound="DeclareResponse")


@_attrs_define
class DeclareResponse:
    """
    Attributes:
        action (DeclareResponseAction):
        fields (list[FieldChange]): The fields the declaration changes; empty when `unchanged`
        actor (Actor | None): The Actor after the write; with `dryRun`, the live Actor, or null for a create
    """

    action: DeclareResponseAction
    fields: list[FieldChange]
    actor: Actor | None
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from .._models.actor import Actor

        action = self.action.value

        fields = []
        for fields_item_data in self.fields:
            fields_item = fields_item_data.to_dict()
            fields.append(fields_item)

        actor: dict[str, Any] | None
        if isinstance(self.actor, Actor):
            actor = self.actor.to_dict()
        else:
            actor = self.actor

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "action": action,
                "fields": fields,
                "actor": actor,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.actor import Actor
        from .._models.field_change import FieldChange

        d = dict(src_dict)
        action = DeclareResponseAction(d.pop("action"))

        fields = []
        _fields = d.pop("fields")
        for fields_item_data in _fields:
            fields_item = FieldChange.from_dict(fields_item_data)

            fields.append(fields_item)

        def _parse_actor(data: object) -> Actor | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                actor_type_0 = Actor.from_dict(data)

                return actor_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Actor | None, data)

        actor = _parse_actor(d.pop("actor"))

        declare_response = cls(
            action=action,
            fields=fields,
            actor=actor,
        )

        declare_response.additional_properties = d
        return declare_response

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
