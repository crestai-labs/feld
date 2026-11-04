from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.herdr_placement_pane_origin import HerdrPlacementPaneOrigin
from ..models.herdr_placement_split import HerdrPlacementSplit
from ..models.herdr_placement_tab_origin import HerdrPlacementTabOrigin
from ..models.herdr_placement_unit import HerdrPlacementUnit
from ..models.herdr_placement_workspace_origin import HerdrPlacementWorkspaceOrigin
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.herdr_placement_borrowed import HerdrPlacementBorrowed
    from ..models.herdr_placement_endpoint import HerdrPlacementEndpoint
    from ..models.process_incarnation import ProcessIncarnation


T = TypeVar("T", bound="HerdrPlacement")


@_attrs_define
class HerdrPlacement:
    """
    Attributes:
        summary (str): Unit, ID, name at read time, and origin
        endpoint (HerdrPlacementEndpoint):
        unit (HerdrPlacementUnit):
        workdir (str):
        attach_command (str | Unset):
        split (HerdrPlacementSplit | Unset): Split direction when `unit` is `pane`
        borrowed (HerdrPlacementBorrowed | Unset): Borrowed existing resource. Consists of the selector a person wrote
            and IDs checked right before creating resources
        workspace_id (str | Unset):
        workspace_origin (HerdrPlacementWorkspaceOrigin | Unset):
        tab_id (str | Unset):
        tab_origin (HerdrPlacementTabOrigin | Unset):
        pane_id (str | Unset):
        pane_origin (HerdrPlacementPaneOrigin | Unset):
        terminal_id (str | Unset): id that herdr assigns to the pane's actual terminal. Changes when the server restarts
        pane_process (ProcessIncarnation | Unset):
        program (ProcessIncarnation | Unset):
        process (ProcessIncarnation | Unset):
    """

    summary: str
    endpoint: HerdrPlacementEndpoint
    unit: HerdrPlacementUnit
    workdir: str
    attach_command: str | Unset = UNSET
    split: HerdrPlacementSplit | Unset = UNSET
    borrowed: HerdrPlacementBorrowed | Unset = UNSET
    workspace_id: str | Unset = UNSET
    workspace_origin: HerdrPlacementWorkspaceOrigin | Unset = UNSET
    tab_id: str | Unset = UNSET
    tab_origin: HerdrPlacementTabOrigin | Unset = UNSET
    pane_id: str | Unset = UNSET
    pane_origin: HerdrPlacementPaneOrigin | Unset = UNSET
    terminal_id: str | Unset = UNSET
    pane_process: ProcessIncarnation | Unset = UNSET
    program: ProcessIncarnation | Unset = UNSET
    process: ProcessIncarnation | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        summary = self.summary

        endpoint = self.endpoint.to_dict()

        unit = self.unit.value

        workdir = self.workdir

        attach_command = self.attach_command

        split: str | Unset = UNSET
        if not isinstance(self.split, Unset):
            split = self.split.value

        borrowed: dict[str, Any] | Unset = UNSET
        if not isinstance(self.borrowed, Unset):
            borrowed = self.borrowed.to_dict()

        workspace_id = self.workspace_id

        workspace_origin: str | Unset = UNSET
        if not isinstance(self.workspace_origin, Unset):
            workspace_origin = self.workspace_origin.value

        tab_id = self.tab_id

        tab_origin: str | Unset = UNSET
        if not isinstance(self.tab_origin, Unset):
            tab_origin = self.tab_origin.value

        pane_id = self.pane_id

        pane_origin: str | Unset = UNSET
        if not isinstance(self.pane_origin, Unset):
            pane_origin = self.pane_origin.value

        terminal_id = self.terminal_id

        pane_process: dict[str, Any] | Unset = UNSET
        if not isinstance(self.pane_process, Unset):
            pane_process = self.pane_process.to_dict()

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
                "workdir": workdir,
            }
        )
        if attach_command is not UNSET:
            field_dict["attachCommand"] = attach_command
        if split is not UNSET:
            field_dict["split"] = split
        if borrowed is not UNSET:
            field_dict["borrowed"] = borrowed
        if workspace_id is not UNSET:
            field_dict["workspaceId"] = workspace_id
        if workspace_origin is not UNSET:
            field_dict["workspaceOrigin"] = workspace_origin
        if tab_id is not UNSET:
            field_dict["tabId"] = tab_id
        if tab_origin is not UNSET:
            field_dict["tabOrigin"] = tab_origin
        if pane_id is not UNSET:
            field_dict["paneId"] = pane_id
        if pane_origin is not UNSET:
            field_dict["paneOrigin"] = pane_origin
        if terminal_id is not UNSET:
            field_dict["terminalId"] = terminal_id
        if pane_process is not UNSET:
            field_dict["paneProcess"] = pane_process
        if program is not UNSET:
            field_dict["program"] = program
        if process is not UNSET:
            field_dict["process"] = process

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.herdr_placement_borrowed import HerdrPlacementBorrowed
        from ..models.herdr_placement_endpoint import HerdrPlacementEndpoint
        from ..models.process_incarnation import ProcessIncarnation

        d = dict(src_dict)
        summary = d.pop("summary")

        endpoint = HerdrPlacementEndpoint.from_dict(d.pop("endpoint"))

        unit = HerdrPlacementUnit(d.pop("unit"))

        workdir = d.pop("workdir")

        attach_command = d.pop("attachCommand", UNSET)

        _split = d.pop("split", UNSET)
        split: HerdrPlacementSplit | Unset
        if isinstance(_split, Unset):
            split = UNSET
        else:
            split = HerdrPlacementSplit(_split)

        _borrowed = d.pop("borrowed", UNSET)
        borrowed: HerdrPlacementBorrowed | Unset
        if isinstance(_borrowed, Unset):
            borrowed = UNSET
        else:
            borrowed = HerdrPlacementBorrowed.from_dict(_borrowed)

        workspace_id = d.pop("workspaceId", UNSET)

        _workspace_origin = d.pop("workspaceOrigin", UNSET)
        workspace_origin: HerdrPlacementWorkspaceOrigin | Unset
        if isinstance(_workspace_origin, Unset):
            workspace_origin = UNSET
        else:
            workspace_origin = HerdrPlacementWorkspaceOrigin(_workspace_origin)

        tab_id = d.pop("tabId", UNSET)

        _tab_origin = d.pop("tabOrigin", UNSET)
        tab_origin: HerdrPlacementTabOrigin | Unset
        if isinstance(_tab_origin, Unset):
            tab_origin = UNSET
        else:
            tab_origin = HerdrPlacementTabOrigin(_tab_origin)

        pane_id = d.pop("paneId", UNSET)

        _pane_origin = d.pop("paneOrigin", UNSET)
        pane_origin: HerdrPlacementPaneOrigin | Unset
        if isinstance(_pane_origin, Unset):
            pane_origin = UNSET
        else:
            pane_origin = HerdrPlacementPaneOrigin(_pane_origin)

        terminal_id = d.pop("terminalId", UNSET)

        _pane_process = d.pop("paneProcess", UNSET)
        pane_process: ProcessIncarnation | Unset
        if isinstance(_pane_process, Unset):
            pane_process = UNSET
        else:
            pane_process = ProcessIncarnation.from_dict(_pane_process)

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

        herdr_placement = cls(
            summary=summary,
            endpoint=endpoint,
            unit=unit,
            workdir=workdir,
            attach_command=attach_command,
            split=split,
            borrowed=borrowed,
            workspace_id=workspace_id,
            workspace_origin=workspace_origin,
            tab_id=tab_id,
            tab_origin=tab_origin,
            pane_id=pane_id,
            pane_origin=pane_origin,
            terminal_id=terminal_id,
            pane_process=pane_process,
            program=program,
            process=process,
        )

        herdr_placement.additional_properties = d
        return herdr_placement

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
