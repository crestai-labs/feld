from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.session import Session


T = TypeVar("T", bound="StartResponse")


@_attrs_define
class StartResponse:
    """
    Attributes:
        session (Session):
        replaced_session (None | str): Session ID of the previous generation that this launch replaced
    """

    session: Session
    replaced_session: None | str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        session = self.session.to_dict()

        replaced_session: None | str
        replaced_session = self.replaced_session

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "session": session,
                "replacedSession": replaced_session,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.session import Session

        d = dict(src_dict)
        session = Session.from_dict(d.pop("session"))

        def _parse_replaced_session(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        replaced_session = _parse_replaced_session(d.pop("replacedSession"))

        start_response = cls(
            session=session,
            replaced_session=replaced_session,
        )

        start_response.additional_properties = d
        return start_response

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
