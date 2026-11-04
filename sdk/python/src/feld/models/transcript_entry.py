from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.transcript_entry_kind import TranscriptEntryKind
from ..models.transcript_entry_state import TranscriptEntryState
from ..types import UNSET, Unset

T = TypeVar("T", bound="TranscriptEntry")


@_attrs_define
class TranscriptEntry:
    """Transcript entry in the common form projected by the Runtime Adapter

    Attributes:
        cursor (str): Adapter-defined string pointing to this transcript entry
        at (datetime.datetime | None):
        kind (TranscriptEntryKind):
        text (str): Body for display. The name and input for a tool call, the result for a tool result
        raw (Any): The line as written by the Runtime to the transcript file
        error (bool | Unset): Whether the tool result reports a failure. Present only on `tool-result`
        state (TranscriptEntryState | Unset): Whether what this transcript entry represents is readable. `lost` means
            the transcript file is unreachable, and `text` gives the reason
        sub_agent (str | Unset): ID of the Sub Agent this transcript entry belongs to
        sub_agent_ended (str | Unset): ID of the Sub Agent whose completion this transcript entry indicates
    """

    cursor: str
    at: datetime.datetime | None
    kind: TranscriptEntryKind
    text: str
    raw: Any
    error: bool | Unset = UNSET
    state: TranscriptEntryState | Unset = UNSET
    sub_agent: str | Unset = UNSET
    sub_agent_ended: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        cursor = self.cursor

        at: None | str
        if isinstance(self.at, datetime.datetime):
            at = self.at.isoformat()
        else:
            at = self.at

        kind = self.kind.value

        text = self.text

        raw = self.raw

        error = self.error

        state: str | Unset = UNSET
        if not isinstance(self.state, Unset):
            state = self.state.value

        sub_agent = self.sub_agent

        sub_agent_ended = self.sub_agent_ended

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "cursor": cursor,
                "at": at,
                "kind": kind,
                "text": text,
                "raw": raw,
            }
        )
        if error is not UNSET:
            field_dict["error"] = error
        if state is not UNSET:
            field_dict["state"] = state
        if sub_agent is not UNSET:
            field_dict["subAgent"] = sub_agent
        if sub_agent_ended is not UNSET:
            field_dict["subAgentEnded"] = sub_agent_ended

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        cursor = d.pop("cursor")

        def _parse_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                at_type_0 = datetime.datetime.fromisoformat(data)

                return at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        at = _parse_at(d.pop("at"))

        kind = TranscriptEntryKind(d.pop("kind"))

        text = d.pop("text")

        raw = d.pop("raw")

        error = d.pop("error", UNSET)

        _state = d.pop("state", UNSET)
        state: TranscriptEntryState | Unset
        if isinstance(_state, Unset):
            state = UNSET
        else:
            state = TranscriptEntryState(_state)

        sub_agent = d.pop("subAgent", UNSET)

        sub_agent_ended = d.pop("subAgentEnded", UNSET)

        transcript_entry = cls(
            cursor=cursor,
            at=at,
            kind=kind,
            text=text,
            raw=raw,
            error=error,
            state=state,
            sub_agent=sub_agent,
            sub_agent_ended=sub_agent_ended,
        )

        transcript_entry.additional_properties = d
        return transcript_entry

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
