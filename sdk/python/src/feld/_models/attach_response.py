from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from .._models.session import Session


T = TypeVar("T", bound="AttachResponse")


@_attrs_define
class AttachResponse:
    """
    Attributes:
        session (Session):
        token (str): Session token
        reattached (bool): Whether the token of an existing Session was reissued
    """

    session: Session
    token: str
    reattached: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        session = self.session.to_dict()

        token = self.token

        reattached = self.reattached

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "session": session,
                "token": token,
                "reattached": reattached,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.session import Session

        d = dict(src_dict)
        session = Session.from_dict(d.pop("session"))

        token = d.pop("token")

        reattached = d.pop("reattached")

        attach_response = cls(
            session=session,
            token=token,
            reattached=reattached,
        )

        attach_response.additional_properties = d
        return attach_response

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
