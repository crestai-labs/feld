from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.observation import Observation


T = TypeVar("T", bound="LifecycleAction")


@_attrs_define
class LifecycleAction:
    """
    Attributes:
        accepted (datetime.datetime):
        requester_session_id (None | str): Requester's Session when requested with `sessionToken`
        attempt (None | str): What the Provider attempted. null while not settled
        observed (None | Observation):
    """

    accepted: datetime.datetime
    requester_session_id: None | str
    attempt: None | str
    observed: None | Observation
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.observation import Observation

        accepted = self.accepted.isoformat()

        requester_session_id: None | str
        requester_session_id = self.requester_session_id

        attempt: None | str
        attempt = self.attempt

        observed: dict[str, Any] | None
        if isinstance(self.observed, Observation):
            observed = self.observed.to_dict()
        else:
            observed = self.observed

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "accepted": accepted,
                "requesterSessionId": requester_session_id,
                "attempt": attempt,
                "observed": observed,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.observation import Observation

        d = dict(src_dict)
        accepted = datetime.datetime.fromisoformat(d.pop("accepted"))

        def _parse_requester_session_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        requester_session_id = _parse_requester_session_id(d.pop("requesterSessionId"))

        def _parse_attempt(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        attempt = _parse_attempt(d.pop("attempt"))

        def _parse_observed(data: object) -> None | Observation:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                observed_type_0 = Observation.from_dict(data)

                return observed_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Observation, data)

        observed = _parse_observed(d.pop("observed"))

        lifecycle_action = cls(
            accepted=accepted,
            requester_session_id=requester_session_id,
            attempt=attempt,
            observed=observed,
        )

        lifecycle_action.additional_properties = d
        return lifecycle_action

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
