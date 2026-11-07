from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from .._models.actor_kind import ActorKind
from .._models.grant import Grant
from .._types import UNSET, Unset

if TYPE_CHECKING:
    from .._models.launch_spec import LaunchSpec
    from .._models.schedule_spec import ScheduleSpec


T = TypeVar("T", bound="CreateActorBody")


@_attrs_define
class CreateActorBody:
    """
    Attributes:
        name (str):
        kind (ActorKind):
        launch_spec (LaunchSpec): Has exactly one of `runtime` and `command`. `runtime` is the name of an installed
            Runtime Adapter, and `provider`
            the name of an installed Execution Provider; an unknown name is rejected with 400.
            `workdir` must be an absolute path, and `node` must be `local`.
            `providerOptions` is stored with omitted values filled by the Provider's `resolve`; options the Provider rejects
            return 400.
        grants (list[Grant] | Unset): Operations on other Actors in the same Workspace that Admin allows the Actor's
            Session
        accept_from (list[str] | Unset): Names of Actors in the same Workspace from which the Actor receives Messages in
            addition to Admin
        caller (dict[str, Any] | Unset): The caller's own location, obtained by the creating or updating CLI from the
            Provider's `self`. The Provider's `resolve` fills omitted `providerOptions` from it.
        schedule (None | ScheduleSpec | Unset):
    """

    name: str
    kind: ActorKind
    launch_spec: LaunchSpec
    grants: list[Grant] | Unset = UNSET
    accept_from: list[str] | Unset = UNSET
    caller: dict[str, Any] | Unset = UNSET
    schedule: None | ScheduleSpec | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from .._models.schedule_spec import ScheduleSpec

        name = self.name

        kind = self.kind.value

        launch_spec = self.launch_spec.to_dict()

        grants: list[str] | Unset = UNSET
        if not isinstance(self.grants, Unset):
            grants = []
            for componentsschemas_grants_item_data in self.grants:
                componentsschemas_grants_item = componentsschemas_grants_item_data.value
                grants.append(componentsschemas_grants_item)

        accept_from: list[str] | Unset = UNSET
        if not isinstance(self.accept_from, Unset):
            accept_from = self.accept_from

        caller = self.caller

        schedule: dict[str, Any] | None | Unset
        if isinstance(self.schedule, Unset):
            schedule = UNSET
        elif isinstance(self.schedule, ScheduleSpec):
            schedule = self.schedule.to_dict()
        else:
            schedule = self.schedule

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "name": name,
                "kind": kind,
                "launchSpec": launch_spec,
            }
        )
        if grants is not UNSET:
            field_dict["grants"] = grants
        if accept_from is not UNSET:
            field_dict["acceptFrom"] = accept_from
        if caller is not UNSET:
            field_dict["caller"] = caller
        if schedule is not UNSET:
            field_dict["schedule"] = schedule

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.launch_spec import LaunchSpec
        from .._models.schedule_spec import ScheduleSpec

        d = dict(src_dict)
        name = d.pop("name")

        kind = ActorKind(d.pop("kind"))

        launch_spec = LaunchSpec.from_dict(d.pop("launchSpec"))

        _grants = d.pop("grants", UNSET)
        grants: list[Grant] | Unset = UNSET
        if _grants is not UNSET:
            grants = []
            for componentsschemas_grants_item_data in _grants:
                componentsschemas_grants_item = Grant(
                    componentsschemas_grants_item_data
                )

                grants.append(componentsschemas_grants_item)

        accept_from = cast(list[str], d.pop("acceptFrom", UNSET))

        caller = d.pop("caller", UNSET)

        def _parse_schedule(data: object) -> None | ScheduleSpec | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                schedule_type_0 = ScheduleSpec.from_dict(data)

                return schedule_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ScheduleSpec | Unset, data)

        schedule = _parse_schedule(d.pop("schedule", UNSET))

        create_actor_body = cls(
            name=name,
            kind=kind,
            launch_spec=launch_spec,
            grants=grants,
            accept_from=accept_from,
            caller=caller,
            schedule=schedule,
        )

        return create_actor_body
