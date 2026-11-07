from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from .._models.actor_reference import ActorReference
    from .._models.actor_requester import ActorRequester
    from .._models.admin_requester import AdminRequester
    from .._models.message_queued import MessageQueued
    from .._models.message_read import MessageRead


T = TypeVar("T", bound="Message")


@_attrs_define
class Message:
    """
    Attributes:
        id (str):
        fifo (int): Order within the destination mailbox
        from_ (ActorRequester | AdminRequester):
        from_session_id (None | str):
        to (ActorReference):
        to_session_id (None | str):
        accepted (datetime.datetime):
        label (None | str):
        queued (MessageQueued | None):
        read (MessageRead | None):
        preview (str): First 48 characters of the body after redaction. Ends with `…` if more follows
        anchor (str):
        waiting (bool):
        deleted_at (datetime.datetime | None):
        body (str):
    """

    id: str
    fifo: int
    from_: ActorRequester | AdminRequester
    from_session_id: None | str
    to: ActorReference
    to_session_id: None | str
    accepted: datetime.datetime
    label: None | str
    queued: MessageQueued | None
    read: MessageRead | None
    preview: str
    anchor: str
    waiting: bool
    deleted_at: datetime.datetime | None
    body: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from .._models.admin_requester import AdminRequester
        from .._models.message_queued import MessageQueued
        from .._models.message_read import MessageRead

        id = self.id

        fifo = self.fifo

        from_: dict[str, Any]
        if isinstance(self.from_, AdminRequester):
            from_ = self.from_.to_dict()
        else:
            from_ = self.from_.to_dict()

        from_session_id: None | str
        from_session_id = self.from_session_id

        to = self.to.to_dict()

        to_session_id: None | str
        to_session_id = self.to_session_id

        accepted = self.accepted.isoformat()

        label: None | str
        label = self.label

        queued: dict[str, Any] | None
        if isinstance(self.queued, MessageQueued):
            queued = self.queued.to_dict()
        else:
            queued = self.queued

        read: dict[str, Any] | None
        if isinstance(self.read, MessageRead):
            read = self.read.to_dict()
        else:
            read = self.read

        preview = self.preview

        anchor = self.anchor

        waiting = self.waiting

        deleted_at: None | str
        if isinstance(self.deleted_at, datetime.datetime):
            deleted_at = self.deleted_at.isoformat()
        else:
            deleted_at = self.deleted_at

        body = self.body

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "fifo": fifo,
                "from": from_,
                "fromSessionId": from_session_id,
                "to": to,
                "toSessionId": to_session_id,
                "accepted": accepted,
                "label": label,
                "queued": queued,
                "read": read,
                "preview": preview,
                "anchor": anchor,
                "waiting": waiting,
                "deletedAt": deleted_at,
                "body": body,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.actor_reference import ActorReference
        from .._models.actor_requester import ActorRequester
        from .._models.admin_requester import AdminRequester
        from .._models.message_queued import MessageQueued
        from .._models.message_read import MessageRead

        d = dict(src_dict)
        id = d.pop("id")

        fifo = d.pop("fifo")

        def _parse_from_(data: object) -> ActorRequester | AdminRequester:
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_requester_type_0 = AdminRequester.from_dict(data)

                return componentsschemas_requester_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_requester_type_1 = ActorRequester.from_dict(data)

            return componentsschemas_requester_type_1

        from_ = _parse_from_(d.pop("from"))

        def _parse_from_session_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        from_session_id = _parse_from_session_id(d.pop("fromSessionId"))

        to = ActorReference.from_dict(d.pop("to"))

        def _parse_to_session_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        to_session_id = _parse_to_session_id(d.pop("toSessionId"))

        accepted = datetime.datetime.fromisoformat(d.pop("accepted"))

        def _parse_label(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        label = _parse_label(d.pop("label"))

        def _parse_queued(data: object) -> MessageQueued | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                queued_type_0 = MessageQueued.from_dict(data)

                return queued_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(MessageQueued | None, data)

        queued = _parse_queued(d.pop("queued"))

        def _parse_read(data: object) -> MessageRead | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                read_type_0 = MessageRead.from_dict(data)

                return read_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(MessageRead | None, data)

        read = _parse_read(d.pop("read"))

        preview = d.pop("preview")

        anchor = d.pop("anchor")

        waiting = d.pop("waiting")

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

        body = d.pop("body")

        message = cls(
            id=id,
            fifo=fifo,
            from_=from_,
            from_session_id=from_session_id,
            to=to,
            to_session_id=to_session_id,
            accepted=accepted,
            label=label,
            queued=queued,
            read=read,
            preview=preview,
            anchor=anchor,
            waiting=waiting,
            deleted_at=deleted_at,
            body=body,
        )

        message.additional_properties = d
        return message

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
