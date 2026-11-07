from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from .._models.session import Session
    from .._models.sub_agent_session import SubAgentSession


T = TypeVar("T", bound="SessionResponse")


@_attrs_define
class SessionResponse:
    """
    Attributes:
        session (Session):
        sub_agents (list[SubAgentSession]):
    """

    session: Session
    sub_agents: list[SubAgentSession]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        session = self.session.to_dict()

        sub_agents = []
        for sub_agents_item_data in self.sub_agents:
            sub_agents_item = sub_agents_item_data.to_dict()
            sub_agents.append(sub_agents_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "session": session,
                "subAgents": sub_agents,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.session import Session
        from .._models.sub_agent_session import SubAgentSession

        d = dict(src_dict)
        session = Session.from_dict(d.pop("session"))

        sub_agents = []
        _sub_agents = d.pop("subAgents")
        for sub_agents_item_data in _sub_agents:
            sub_agents_item = SubAgentSession.from_dict(sub_agents_item_data)

            sub_agents.append(sub_agents_item)

        session_response = cls(
            session=session,
            sub_agents=sub_agents,
        )

        session_response.additional_properties = d
        return session_response

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
