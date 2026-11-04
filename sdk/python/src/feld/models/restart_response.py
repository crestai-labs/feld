from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.session import Session


T = TypeVar("T", bound="RestartResponse")


@_attrs_define
class RestartResponse:
    """
    Attributes:
        stopped (Session):
        session (None | Session): The next generation, when the stopped Session closed before the response. Otherwise
            null, and it launches when the Session closes
    """

    stopped: Session
    session: None | Session
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.session import Session

        stopped = self.stopped.to_dict()

        session: dict[str, Any] | None
        if isinstance(self.session, Session):
            session = self.session.to_dict()
        else:
            session = self.session

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "stopped": stopped,
                "session": session,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.session import Session

        d = dict(src_dict)
        stopped = Session.from_dict(d.pop("stopped"))

        def _parse_session(data: object) -> None | Session:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                session_type_0 = Session.from_dict(data)

                return session_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Session, data)

        session = _parse_session(d.pop("session"))

        restart_response = cls(
            stopped=stopped,
            session=session,
        )

        restart_response.additional_properties = d
        return restart_response

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
