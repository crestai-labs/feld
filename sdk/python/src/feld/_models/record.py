from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from .._models.record_kind import RecordKind

if TYPE_CHECKING:
    from .._models.actor_reference import ActorReference


T = TypeVar("T", bound="Record")


@_attrs_define
class Record:
    """record

    Attributes:
        sequence (int): Global order
        at (datetime.datetime):
        kind (RecordKind):
        actor (ActorReference):
        session_id (None | str):
        data (dict[str, Any]): Contents per kind.
            `spawned`: `spawner`, `spawnerSessionId`.
            `session.started`: `startKind`, `planned`.
            `session.ended`: `endKind`, `exitCode`.
            `observation`: `kind`, `detail`, `exitCode`.
            `stop.requested` / `kill.requested`: `requesterSessionId`.
            `message.sent`: `messageId`, `to`, `toSessionId`, `anchor`.
            `message.accepted`: `messageId`, `from`, `fromSessionId`, and `waiting` at read time.
            `message.read`: `messageId`, `readId`, `fifo`.
            `schedule.skipped`: `planned`, `currentSessionId`.
            `schedule.missed`: `planned`, `reason`.
    """

    sequence: int
    at: datetime.datetime
    kind: RecordKind
    actor: ActorReference
    session_id: None | str
    data: dict[str, Any]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        sequence = self.sequence

        at = self.at.isoformat()

        kind = self.kind.value

        actor = self.actor.to_dict()

        session_id: None | str
        session_id = self.session_id

        data = self.data

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "sequence": sequence,
                "at": at,
                "kind": kind,
                "actor": actor,
                "sessionId": session_id,
                "data": data,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.actor_reference import ActorReference

        d = dict(src_dict)
        sequence = d.pop("sequence")

        at = datetime.datetime.fromisoformat(d.pop("at"))

        kind = RecordKind(d.pop("kind"))

        actor = ActorReference.from_dict(d.pop("actor"))

        def _parse_session_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        session_id = _parse_session_id(d.pop("sessionId"))

        data = d.pop("data")

        record = cls(
            sequence=sequence,
            at=at,
            kind=kind,
            actor=actor,
            session_id=session_id,
            data=data,
        )

        record.additional_properties = d
        return record

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
