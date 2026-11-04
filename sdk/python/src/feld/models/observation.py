from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.observation_kind import ObservationKind
from ..types import UNSET, Unset

T = TypeVar("T", bound="Observation")


@_attrs_define
class Observation:
    """
    Attributes:
        kind (ObservationKind):
        at (datetime.datetime):
        since (datetime.datetime | Unset): Time the current kind was recorded (the time of the `observation` record
            where the kind changed). Present on the latest observation of a Session
        detail (str | Unset):
        exit_code (int | Unset):
    """

    kind: ObservationKind
    at: datetime.datetime
    since: datetime.datetime | Unset = UNSET
    detail: str | Unset = UNSET
    exit_code: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind.value

        at = self.at.isoformat()

        since: str | Unset = UNSET
        if not isinstance(self.since, Unset):
            since = self.since.isoformat()

        detail = self.detail

        exit_code = self.exit_code

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "kind": kind,
                "at": at,
            }
        )
        if since is not UNSET:
            field_dict["since"] = since
        if detail is not UNSET:
            field_dict["detail"] = detail
        if exit_code is not UNSET:
            field_dict["exitCode"] = exit_code

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        kind = ObservationKind(d.pop("kind"))

        at = datetime.datetime.fromisoformat(d.pop("at"))

        _since = d.pop("since", UNSET)
        since: datetime.datetime | Unset
        if isinstance(_since, Unset):
            since = UNSET
        else:
            since = datetime.datetime.fromisoformat(_since)

        detail = d.pop("detail", UNSET)

        exit_code = d.pop("exitCode", UNSET)

        observation = cls(
            kind=kind,
            at=at,
            since=since,
            detail=detail,
            exit_code=exit_code,
        )

        observation.additional_properties = d
        return observation

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
