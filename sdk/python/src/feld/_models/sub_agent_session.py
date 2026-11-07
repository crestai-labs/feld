from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    Literal,
    Self,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from .._models.console_state import ConsoleState
from .._models.outcome import Outcome

if TYPE_CHECKING:
    from .._models.execution import Execution


T = TypeVar("T", bound="SubAgentSession")


@_attrs_define
class SubAgentSession:
    """
    Attributes:
        id (str): Parent Session ID and Sub Agent ID joined with `_subagent_`
        workspace (str):
        actor (str):
        sub_agent (str): ID of the Sub Agent indicated by the transcript entry's `subAgent`
        parent_session (str):
        start (datetime.datetime | None):
        end (datetime.datetime | None):
        start_kind (Literal['sub-agent']):
        ownership (None):
        end_kind (None):
        exit_code (int | None):
        execution (Execution):
        model (list[Any | str] | None):
        placement (Any):
        observation (Any):
        outcome (Outcome):
        console_state (ConsoleState):
        console_detail (None | str):
    """

    id: str
    workspace: str
    actor: str
    sub_agent: str
    parent_session: str
    start: datetime.datetime | None
    end: datetime.datetime | None
    start_kind: Literal["sub-agent"]
    ownership: None
    end_kind: None
    exit_code: int | None
    execution: Execution
    model: list[Any | str] | None
    placement: Any
    observation: Any
    outcome: Outcome
    console_state: ConsoleState
    console_detail: None | str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        workspace = self.workspace

        actor = self.actor

        sub_agent = self.sub_agent

        parent_session = self.parent_session

        start: None | str
        if isinstance(self.start, datetime.datetime):
            start = self.start.isoformat()
        else:
            start = self.start

        end: None | str
        if isinstance(self.end, datetime.datetime):
            end = self.end.isoformat()
        else:
            end = self.end

        start_kind = self.start_kind

        ownership = self.ownership

        end_kind = self.end_kind

        exit_code: int | None
        exit_code = self.exit_code

        execution = self.execution.to_dict()

        model: list[Any | str] | None
        if isinstance(self.model, list):
            model = []
            for componentsschemas_session_model_item_data in self.model:
                componentsschemas_session_model_item: Any | str
                componentsschemas_session_model_item = (
                    componentsschemas_session_model_item_data
                )
                model.append(componentsschemas_session_model_item)

        else:
            model = self.model

        placement = self.placement

        observation = self.observation

        outcome = self.outcome.value

        console_state = self.console_state.value

        console_detail: None | str
        console_detail = self.console_detail

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "workspace": workspace,
                "actor": actor,
                "subAgent": sub_agent,
                "parentSession": parent_session,
                "start": start,
                "end": end,
                "startKind": start_kind,
                "ownership": ownership,
                "endKind": end_kind,
                "exitCode": exit_code,
                "execution": execution,
                "model": model,
                "placement": placement,
                "observation": observation,
                "outcome": outcome,
                "consoleState": console_state,
                "consoleDetail": console_detail,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.execution import Execution

        d = dict(src_dict)
        id = d.pop("id")

        workspace = d.pop("workspace")

        actor = d.pop("actor")

        sub_agent = d.pop("subAgent")

        parent_session = d.pop("parentSession")

        def _parse_start(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                start_type_0 = datetime.datetime.fromisoformat(data)

                return start_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        start = _parse_start(d.pop("start"))

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

        start_kind = cast(Literal["sub-agent"], d.pop("startKind"))
        if start_kind != "sub-agent":
            raise ValueError(
                f"startKind must match const 'sub-agent', got '{start_kind}'"
            )

        ownership = d.pop("ownership")

        end_kind = d.pop("endKind")

        def _parse_exit_code(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        exit_code = _parse_exit_code(d.pop("exitCode"))

        execution = Execution.from_dict(d.pop("execution"))

        def _parse_model(data: object) -> list[Any | str] | None:
            if data is None:
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                model_type_0 = []
                _model_type_0 = data
                for componentsschemas_session_model_item_data in _model_type_0:

                    def _parse_componentsschemas_session_model_item(
                        data: object,
                    ) -> Any | str:
                        return cast(Any | str, data)

                    componentsschemas_session_model_item = (
                        _parse_componentsschemas_session_model_item(
                            componentsschemas_session_model_item_data
                        )
                    )

                    model_type_0.append(componentsschemas_session_model_item)

                return model_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[Any | str] | None, data)

        model = _parse_model(d.pop("model"))

        placement = d.pop("placement")

        observation = d.pop("observation")

        outcome = Outcome(d.pop("outcome"))

        console_state = ConsoleState(d.pop("consoleState"))

        def _parse_console_detail(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        console_detail = _parse_console_detail(d.pop("consoleDetail"))

        sub_agent_session = cls(
            id=id,
            workspace=workspace,
            actor=actor,
            sub_agent=sub_agent,
            parent_session=parent_session,
            start=start,
            end=end,
            start_kind=start_kind,
            ownership=ownership,
            end_kind=end_kind,
            exit_code=exit_code,
            execution=execution,
            model=model,
            placement=placement,
            observation=observation,
            outcome=outcome,
            console_state=console_state,
            console_detail=console_detail,
        )

        sub_agent_session.additional_properties = d
        return sub_agent_session

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
