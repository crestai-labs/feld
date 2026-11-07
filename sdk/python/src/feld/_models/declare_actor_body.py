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


T = TypeVar("T", bound="DeclareActorBody")


@_attrs_define
class DeclareActorBody:
    """
    Attributes:
        kind (ActorKind):
        launch_spec (LaunchSpec): Has exactly one of `runtime` and `command`. `runtime` is the name of an installed
            Runtime Adapter, and `provider`
            the name of an installed Execution Provider; an unknown name is rejected with 400.
            `workdir` must be an absolute path, and `node` must be `local`.
            `providerOptions` is stored with omitted values filled by the Provider's `resolve`; options the Provider rejects
            return 400.
        grants (list[Grant] | Unset): Operations on other Actors in the same Workspace that Admin allows the Actor's
            Session
        accept_from (list[str] | None | Unset):
        declared_in (str | Unset):
        caller (dict[str, Any] | Unset): The caller's own location, obtained by the creating or updating CLI from the
            Provider's `self`. The Provider's `resolve` fills omitted `providerOptions` from it.
        schedule (ScheduleSpec | Unset):
    """

    kind: ActorKind
    launch_spec: LaunchSpec
    grants: list[Grant] | Unset = UNSET
    accept_from: list[str] | None | Unset = UNSET
    declared_in: str | Unset = UNSET
    caller: dict[str, Any] | Unset = UNSET
    schedule: ScheduleSpec | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind.value

        launch_spec = self.launch_spec.to_dict()

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

        declared_in = self.declared_in

        caller = self.caller

        schedule: dict[str, Any] | Unset = UNSET
        if not isinstance(self.schedule, Unset):
            schedule = self.schedule.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "kind": kind,
                "launchSpec": launch_spec,
            }
        )
        if grants is not UNSET:
            field_dict["grants"] = grants
        if accept_from is not UNSET:
            field_dict["acceptFrom"] = accept_from
        if declared_in is not UNSET:
            field_dict["declaredIn"] = declared_in
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

        declared_in = d.pop("declaredIn", UNSET)

        caller = d.pop("caller", UNSET)

        _schedule = d.pop("schedule", UNSET)
        schedule: ScheduleSpec | Unset
        if isinstance(_schedule, Unset):
            schedule = UNSET
        else:
            schedule = ScheduleSpec.from_dict(_schedule)

        declare_actor_body = cls(
            kind=kind,
            launch_spec=launch_spec,
            grants=grants,
            accept_from=accept_from,
            declared_in=declared_in,
            caller=caller,
            schedule=schedule,
        )

        return declare_actor_body
