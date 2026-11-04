from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.execution_type import ExecutionType
from ..types import UNSET, Unset

T = TypeVar("T", bound="Execution")


@_attrs_define
class Execution:
    """
    Attributes:
        type_ (ExecutionType):
        label (str): Label from the Runtime Adapter's manifest, or a display name built from the program's argv.
        command (list[str] | None): argv of the launched `program`. null for `agent-runtime`, where the Adapter manages
            the launch. Also null for the Session of another Actor read with a `sessionToken`.
        ai (bool):
        runtime (str | Unset): Name of the Runtime Adapter. Present on Sessions whose `type` is `agent-runtime`.
    """

    type_: ExecutionType
    label: str
    command: list[str] | None
    ai: bool
    runtime: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        type_ = self.type_.value

        label = self.label

        command: list[str] | None
        if isinstance(self.command, list):
            command = self.command

        else:
            command = self.command

        ai = self.ai

        runtime = self.runtime

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "type": type_,
                "label": label,
                "command": command,
                "ai": ai,
            }
        )
        if runtime is not UNSET:
            field_dict["runtime"] = runtime

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        type_ = ExecutionType(d.pop("type"))

        label = d.pop("label")

        def _parse_command(data: object) -> list[str] | None:
            if data is None:
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                command_type_0 = cast(list[str], data)

                return command_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None, data)

        command = _parse_command(d.pop("command"))

        ai = d.pop("ai")

        runtime = d.pop("runtime", UNSET)

        execution = cls(
            type_=type_,
            label=label,
            command=command,
            ai=ai,
            runtime=runtime,
        )

        execution.additional_properties = d
        return execution

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
