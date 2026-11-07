from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from .._types import UNSET, Unset

if TYPE_CHECKING:
    from .._models.session import Session
    from .._models.sub_agent_session import SubAgentSession


T = TypeVar("T", bound="ListSessionsResponse200")


@_attrs_define
class ListSessionsResponse200:
    """
    Attributes:
        sessions (list[Session]):
        sub_agents (list[SubAgentSession] | Unset): Returned only when `subAgents` is specified.
    """

    sessions: list[Session]
    sub_agents: list[SubAgentSession] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        sessions = []
        for sessions_item_data in self.sessions:
            sessions_item = sessions_item_data.to_dict()
            sessions.append(sessions_item)

        sub_agents: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.sub_agents, Unset):
            sub_agents = []
            for sub_agents_item_data in self.sub_agents:
                sub_agents_item = sub_agents_item_data.to_dict()
                sub_agents.append(sub_agents_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "sessions": sessions,
            }
        )
        if sub_agents is not UNSET:
            field_dict["subAgents"] = sub_agents

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.session import Session
        from .._models.sub_agent_session import SubAgentSession

        d = dict(src_dict)
        sessions = []
        _sessions = d.pop("sessions")
        for sessions_item_data in _sessions:
            sessions_item = Session.from_dict(sessions_item_data)

            sessions.append(sessions_item)

        _sub_agents = d.pop("subAgents", UNSET)
        sub_agents: list[SubAgentSession] | Unset = UNSET
        if _sub_agents is not UNSET:
            sub_agents = []
            for sub_agents_item_data in _sub_agents:
                sub_agents_item = SubAgentSession.from_dict(sub_agents_item_data)

                sub_agents.append(sub_agents_item)

        list_sessions_response_200 = cls(
            sessions=sessions,
            sub_agents=sub_agents,
        )

        list_sessions_response_200.additional_properties = d
        return list_sessions_response_200

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
