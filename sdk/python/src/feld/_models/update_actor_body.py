from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from .._models.grant import Grant
from .._types import UNSET, Unset

if TYPE_CHECKING:
    from .._models.launch_spec_patch import LaunchSpecPatch
    from .._models.schedule_patch import SchedulePatch


T = TypeVar("T", bound="UpdateActorBody")


@_attrs_define
class UpdateActorBody:
    """
    Attributes:
        name (str | Unset):
        launch_spec (LaunchSpecPatch | Unset): For each top-level field, replaces it if present, keeps it if absent, and
            removes it if `null`.
        schedule (SchedulePatch | Unset):
        grants (list[Grant] | Unset): Operations on other Actors in the same Workspace that Admin allows the Actor's
            Session
        accept_from (list[str] | None | Unset):
        caller (dict[str, Any] | Unset): The caller's own location, obtained by the creating or updating CLI from the
            Provider's `self`. The Provider's `resolve` fills omitted `providerOptions` from it.
    """

    name: str | Unset = UNSET
    launch_spec: LaunchSpecPatch | Unset = UNSET
    schedule: SchedulePatch | Unset = UNSET
    grants: list[Grant] | Unset = UNSET
    accept_from: list[str] | None | Unset = UNSET
    caller: dict[str, Any] | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        launch_spec: dict[str, Any] | Unset = UNSET
        if not isinstance(self.launch_spec, Unset):
            launch_spec = self.launch_spec.to_dict()

        schedule: dict[str, Any] | Unset = UNSET
        if not isinstance(self.schedule, Unset):
            schedule = self.schedule.to_dict()

        grants: list[str] | Unset = UNSET
        if not isinstance(self.grants, Unset):
            grants = []
            for componentsschemas_grants_item_data in self.grants:
                componentsschemas_grants_item = componentsschemas_grants_item_data.value
                grants.append(componentsschemas_grants_item)

        accept_from: list[str] | None | Unset
        if isinstance(self.accept_from, Unset):
            accept_from = UNSET
        elif isinstance(self.accept_from, list):
            accept_from = self.accept_from

        else:
            accept_from = self.accept_from

        caller = self.caller

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if name is not UNSET:
            field_dict["name"] = name
        if launch_spec is not UNSET:
            field_dict["launchSpec"] = launch_spec
        if schedule is not UNSET:
            field_dict["schedule"] = schedule
        if grants is not UNSET:
            field_dict["grants"] = grants
        if accept_from is not UNSET:
            field_dict["acceptFrom"] = accept_from
        if caller is not UNSET:
            field_dict["caller"] = caller

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.launch_spec_patch import LaunchSpecPatch
        from .._models.schedule_patch import SchedulePatch

        d = dict(src_dict)
        name = d.pop("name", UNSET)

        _launch_spec = d.pop("launchSpec", UNSET)
        launch_spec: LaunchSpecPatch | Unset
        if isinstance(_launch_spec, Unset):
            launch_spec = UNSET
        else:
            launch_spec = LaunchSpecPatch.from_dict(_launch_spec)

        _schedule = d.pop("schedule", UNSET)
        schedule: SchedulePatch | Unset
        if isinstance(_schedule, Unset):
            schedule = UNSET
        else:
            schedule = SchedulePatch.from_dict(_schedule)

        _grants = d.pop("grants", UNSET)
        grants: list[Grant] | Unset = UNSET
        if _grants is not UNSET:
            grants = []
            for componentsschemas_grants_item_data in _grants:
                componentsschemas_grants_item = Grant(
                    componentsschemas_grants_item_data
                )

                grants.append(componentsschemas_grants_item)

        def _parse_accept_from(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                accept_from_type_0 = cast(list[str], data)

                return accept_from_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        accept_from = _parse_accept_from(d.pop("acceptFrom", UNSET))

        caller = d.pop("caller", UNSET)

        update_actor_body = cls(
            name=name,
            launch_spec=launch_spec,
            schedule=schedule,
            grants=grants,
            accept_from=accept_from,
            caller=caller,
        )

        return update_actor_body
