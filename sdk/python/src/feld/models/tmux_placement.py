from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.tmux_placement_pane_origin import TmuxPlacementPaneOrigin
from ..models.tmux_placement_session_origin import TmuxPlacementSessionOrigin
from ..models.tmux_placement_unit import TmuxPlacementUnit
from ..models.tmux_placement_window_origin import TmuxPlacementWindowOrigin
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.process_incarnation import ProcessIncarnation
    from ..models.tmux_placement_endpoint import TmuxPlacementEndpoint


T = TypeVar("T", bound="TmuxPlacement")


@_attrs_define
class TmuxPlacement:
    """
    Attributes:
        summary (str): Unit, stable ID, name at read time, and origin
        endpoint (TmuxPlacementEndpoint):
        unit (TmuxPlacementUnit):
        borrowed (None | str):
        workdir (str):
        server (ProcessIncarnation):
        session_id (str):
        session_created_at (int):
        session_origin (TmuxPlacementSessionOrigin):
        window_id (str):
        window_origin (TmuxPlacementWindowOrigin):
        pane_id (str):
        pane_origin (TmuxPlacementPaneOrigin):
        pane_process (ProcessIncarnation):
        attach_command (str | Unset):
        program (ProcessIncarnation | Unset):
        process (ProcessIncarnation | Unset):
    """

    summary: str
    endpoint: TmuxPlacementEndpoint
    unit: TmuxPlacementUnit
    borrowed: None | str
    workdir: str
    server: ProcessIncarnation
    session_id: str
    session_created_at: int
    session_origin: TmuxPlacementSessionOrigin
    window_id: str
    window_origin: TmuxPlacementWindowOrigin
    pane_id: str
    pane_origin: TmuxPlacementPaneOrigin
    pane_process: ProcessIncarnation
    attach_command: str | Unset = UNSET
    program: ProcessIncarnation | Unset = UNSET
    process: ProcessIncarnation | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        summary = self.summary

        endpoint = self.endpoint.to_dict()

        unit = self.unit.value

        borrowed: None | str
        borrowed = self.borrowed

        workdir = self.workdir

        server = self.server.to_dict()

        session_id = self.session_id

        session_created_at = self.session_created_at

        session_origin = self.session_origin.value

        window_id = self.window_id

        window_origin = self.window_origin.value

        pane_id = self.pane_id

        pane_origin = self.pane_origin.value

        pane_process = self.pane_process.to_dict()

        attach_command = self.attach_command

        program: dict[str, Any] | Unset = UNSET
        if not isinstance(self.program, Unset):
            program = self.program.to_dict()

        process: dict[str, Any] | Unset = UNSET
        if not isinstance(self.process, Unset):
            process = self.process.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "summary": summary,
                "endpoint": endpoint,
                "unit": unit,
                "borrowed": borrowed,
                "workdir": workdir,
                "server": server,
                "sessionId": session_id,
                "sessionCreatedAt": session_created_at,
                "sessionOrigin": session_origin,
                "windowId": window_id,
                "windowOrigin": window_origin,
                "paneId": pane_id,
                "paneOrigin": pane_origin,
                "paneProcess": pane_process,
            }
        )
        if attach_command is not UNSET:
            field_dict["attachCommand"] = attach_command
        if program is not UNSET:
            field_dict["program"] = program
        if process is not UNSET:
            field_dict["process"] = process

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.process_incarnation import ProcessIncarnation
        from ..models.tmux_placement_endpoint import TmuxPlacementEndpoint

        d = dict(src_dict)
        summary = d.pop("summary")

        endpoint = TmuxPlacementEndpoint.from_dict(d.pop("endpoint"))

        unit = TmuxPlacementUnit(d.pop("unit"))

        def _parse_borrowed(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        borrowed = _parse_borrowed(d.pop("borrowed"))

        workdir = d.pop("workdir")

        server = ProcessIncarnation.from_dict(d.pop("server"))

        session_id = d.pop("sessionId")

        session_created_at = d.pop("sessionCreatedAt")

        session_origin = TmuxPlacementSessionOrigin(d.pop("sessionOrigin"))

        window_id = d.pop("windowId")

        window_origin = TmuxPlacementWindowOrigin(d.pop("windowOrigin"))

        pane_id = d.pop("paneId")

        pane_origin = TmuxPlacementPaneOrigin(d.pop("paneOrigin"))

        pane_process = ProcessIncarnation.from_dict(d.pop("paneProcess"))

        attach_command = d.pop("attachCommand", UNSET)

        _program = d.pop("program", UNSET)
        program: ProcessIncarnation | Unset
        if isinstance(_program, Unset):
            program = UNSET
        else:
            program = ProcessIncarnation.from_dict(_program)

        _process = d.pop("process", UNSET)
        process: ProcessIncarnation | Unset
        if isinstance(_process, Unset):
            process = UNSET
        else:
            process = ProcessIncarnation.from_dict(_process)

        tmux_placement = cls(
            summary=summary,
            endpoint=endpoint,
            unit=unit,
            borrowed=borrowed,
            workdir=workdir,
            server=server,
            session_id=session_id,
            session_created_at=session_created_at,
            session_origin=session_origin,
            window_id=window_id,
            window_origin=window_origin,
            pane_id=pane_id,
            pane_origin=pane_origin,
            pane_process=pane_process,
            attach_command=attach_command,
            program=program,
            process=process,
        )

        tmux_placement.additional_properties = d
        return tmux_placement

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
