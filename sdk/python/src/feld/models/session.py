from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.actor_kind import ActorKind
from ..models.outcome import Outcome
from ..models.session_end_kind import SessionEndKind
from ..models.session_ownership import SessionOwnership
from ..models.start_kind import StartKind
from ..models.turn import Turn
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.execution import Execution
    from ..models.observation import Observation
    from ..models.placement import Placement
    from ..models.session_lifecycle import SessionLifecycle
    from ..models.session_spawned_by import SessionSpawnedBy


T = TypeVar("T", bound="Session")


@_attrs_define
class Session:
    """
    Attributes:
        id (str):
        workspace (str):
        actor_id (str): `id` of the Actor
        actor (str): `name` of the Actor
        actor_kind (ActorKind):
        generation (int):
        start (datetime.datetime):
        end (datetime.datetime | None):
        start_kind (StartKind):
        ownership (SessionOwnership):
        end_kind (SessionEndKind):
        exit_code (int | None):
        planned (datetime.datetime | None): Planned time (UTC) of a launch by the Schedule
        execution (Execution):
        placement (Placement):
        lifecycle (SessionLifecycle):
        observation (None | Observation):
        outcome (Outcome):
        model (list[Any | str] | Unset): Pair of the Runtime's display name and the model reported by the Runtime
            Adapter (null if not reported). Present on Sessions whose Runtime identity has arrived
        spawned_by (SessionSpawnedBy | Unset): Projection of the spawned record that launched this Session
        turn (Turn | Unset):
        last_entry_at (datetime.datetime | None | Unset): `at` of the transcript entry that the turn was derived from.
            Present only on Sessions with `turn`; null for `unavailable` and for a turn derived from an entry without `at`
    """

    id: str
    workspace: str
    actor_id: str
    actor: str
    actor_kind: ActorKind
    generation: int
    start: datetime.datetime
    end: datetime.datetime | None
    start_kind: StartKind
    ownership: SessionOwnership
    end_kind: SessionEndKind
    exit_code: int | None
    planned: datetime.datetime | None
    execution: Execution
    placement: Placement
    lifecycle: SessionLifecycle
    observation: None | Observation
    outcome: Outcome
    model: list[Any | str] | Unset = UNSET
    spawned_by: SessionSpawnedBy | Unset = UNSET
    turn: Turn | Unset = UNSET
    last_entry_at: datetime.datetime | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.observation import Observation

        id = self.id

        workspace = self.workspace

        actor_id = self.actor_id

        actor = self.actor

        actor_kind = self.actor_kind.value

        generation = self.generation

        start = self.start.isoformat()

        end: None | str
        if isinstance(self.end, datetime.datetime):
            end = self.end.isoformat()
        else:
            end = self.end

        start_kind = self.start_kind.value

        ownership = self.ownership.value

        end_kind = self.end_kind.value

        exit_code: int | None
        exit_code = self.exit_code

        planned: None | str
        if isinstance(self.planned, datetime.datetime):
            planned = self.planned.isoformat()
        else:
            planned = self.planned

        execution = self.execution.to_dict()

        placement = self.placement.to_dict()

        lifecycle = self.lifecycle.to_dict()

        observation: dict[str, Any] | None
        if isinstance(self.observation, Observation):
            observation = self.observation.to_dict()
        else:
            observation = self.observation

        outcome = self.outcome.value

        model: list[Any | str] | Unset = UNSET
        if not isinstance(self.model, Unset):
            model = []
            for componentsschemas_session_model_item_data in self.model:
                componentsschemas_session_model_item: Any | str
                componentsschemas_session_model_item = (
                    componentsschemas_session_model_item_data
                )
                model.append(componentsschemas_session_model_item)

        spawned_by: dict[str, Any] | Unset = UNSET
        if not isinstance(self.spawned_by, Unset):
            spawned_by = self.spawned_by.to_dict()

        turn: str | Unset = UNSET
        if not isinstance(self.turn, Unset):
            turn = self.turn.value

        last_entry_at: None | str | Unset
        if isinstance(self.last_entry_at, Unset):
            last_entry_at = UNSET
        elif isinstance(self.last_entry_at, datetime.datetime):
            last_entry_at = self.last_entry_at.isoformat()
        else:
            last_entry_at = self.last_entry_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "workspace": workspace,
                "actorId": actor_id,
                "actor": actor,
                "actorKind": actor_kind,
                "generation": generation,
                "start": start,
                "end": end,
                "startKind": start_kind,
                "ownership": ownership,
                "endKind": end_kind,
                "exitCode": exit_code,
                "planned": planned,
                "execution": execution,
                "placement": placement,
                "lifecycle": lifecycle,
                "observation": observation,
                "outcome": outcome,
            }
        )
        if model is not UNSET:
            field_dict["model"] = model
        if spawned_by is not UNSET:
            field_dict["spawnedBy"] = spawned_by
        if turn is not UNSET:
            field_dict["turn"] = turn
        if last_entry_at is not UNSET:
            field_dict["lastEntryAt"] = last_entry_at

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.execution import Execution
        from ..models.observation import Observation
        from ..models.placement import Placement
        from ..models.session_lifecycle import SessionLifecycle
        from ..models.session_spawned_by import SessionSpawnedBy

        d = dict(src_dict)
        id = d.pop("id")

        workspace = d.pop("workspace")

        actor_id = d.pop("actorId")

        actor = d.pop("actor")

        actor_kind = ActorKind(d.pop("actorKind"))

        generation = d.pop("generation")

        start = datetime.datetime.fromisoformat(d.pop("start"))

        def _parse_end(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                end_type_0 = datetime.datetime.fromisoformat(data)

                return end_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        end = _parse_end(d.pop("end"))

        start_kind = StartKind(d.pop("startKind"))

        ownership = SessionOwnership(d.pop("ownership"))

        end_kind = SessionEndKind(d.pop("endKind"))

        def _parse_exit_code(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        exit_code = _parse_exit_code(d.pop("exitCode"))

        def _parse_planned(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                planned_type_0 = datetime.datetime.fromisoformat(data)

                return planned_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        planned = _parse_planned(d.pop("planned"))

        execution = Execution.from_dict(d.pop("execution"))

        placement = Placement.from_dict(d.pop("placement"))

        lifecycle = SessionLifecycle.from_dict(d.pop("lifecycle"))

        def _parse_observation(data: object) -> None | Observation:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                observation_type_0 = Observation.from_dict(data)

                return observation_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Observation, data)

        observation = _parse_observation(d.pop("observation"))

        outcome = Outcome(d.pop("outcome"))

        _model = d.pop("model", UNSET)
        model: list[Any | str] | Unset = UNSET
        if _model is not UNSET:
            model = []
            for componentsschemas_session_model_item_data in _model:

                def _parse_componentsschemas_session_model_item(
                    data: object,
                ) -> Any | str:
                    return cast(Any | str, data)

                componentsschemas_session_model_item = (
                    _parse_componentsschemas_session_model_item(
                        componentsschemas_session_model_item_data
                    )
                )

                model.append(componentsschemas_session_model_item)

        _spawned_by = d.pop("spawnedBy", UNSET)
        spawned_by: SessionSpawnedBy | Unset
        if isinstance(_spawned_by, Unset):
            spawned_by = UNSET
        else:
            spawned_by = SessionSpawnedBy.from_dict(_spawned_by)

        _turn = d.pop("turn", UNSET)
        turn: Turn | Unset
        if isinstance(_turn, Unset):
            turn = UNSET
        else:
            turn = Turn(_turn)

        def _parse_last_entry_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                last_entry_at_type_0 = datetime.datetime.fromisoformat(data)

                return last_entry_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        last_entry_at = _parse_last_entry_at(d.pop("lastEntryAt", UNSET))

        session = cls(
            id=id,
            workspace=workspace,
            actor_id=actor_id,
            actor=actor,
            actor_kind=actor_kind,
            generation=generation,
            start=start,
            end=end,
            start_kind=start_kind,
            ownership=ownership,
            end_kind=end_kind,
            exit_code=exit_code,
            planned=planned,
            execution=execution,
            placement=placement,
            lifecycle=lifecycle,
            observation=observation,
            outcome=outcome,
            model=model,
            spawned_by=spawned_by,
            turn=turn,
            last_entry_at=last_entry_at,
        )

        session.additional_properties = d
        return session

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
