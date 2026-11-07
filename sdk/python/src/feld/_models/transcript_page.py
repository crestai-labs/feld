from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from .._models.console_state import ConsoleState

if TYPE_CHECKING:
    from .._models.transcript_entry import TranscriptEntry


T = TypeVar("T", bound="TranscriptPage")


@_attrs_define
class TranscriptPage:
    """
    Attributes:
        session (str):
        state (ConsoleState):
        detail (None | str):
        after (None | str):
        next_after (None | str): Cursor to pass to `after` next
        previous_before (None | str): Cursor to pass to `before` next
        has_older (bool):
        records (list[TranscriptEntry]):
    """

    session: str
    state: ConsoleState
    detail: None | str
    after: None | str
    next_after: None | str
    previous_before: None | str
    has_older: bool
    records: list[TranscriptEntry]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        session = self.session

        state = self.state.value

        detail: None | str
        detail = self.detail

        after: None | str
        after = self.after

        next_after: None | str
        next_after = self.next_after

        previous_before: None | str
        previous_before = self.previous_before

        has_older = self.has_older

        records = []
        for records_item_data in self.records:
            records_item = records_item_data.to_dict()
            records.append(records_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "session": session,
                "state": state,
                "detail": detail,
                "after": after,
                "nextAfter": next_after,
                "previousBefore": previous_before,
                "hasOlder": has_older,
                "records": records,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.transcript_entry import TranscriptEntry

        d = dict(src_dict)
        session = d.pop("session")

        state = ConsoleState(d.pop("state"))

        def _parse_detail(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        detail = _parse_detail(d.pop("detail"))

        def _parse_after(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        after = _parse_after(d.pop("after"))

        def _parse_next_after(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        next_after = _parse_next_after(d.pop("nextAfter"))

        def _parse_previous_before(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        previous_before = _parse_previous_before(d.pop("previousBefore"))

        has_older = d.pop("hasOlder")

        records = []
        _records = d.pop("records")
        for records_item_data in _records:
            records_item = TranscriptEntry.from_dict(records_item_data)

            records.append(records_item)

        transcript_page = cls(
            session=session,
            state=state,
            detail=detail,
            after=after,
            next_after=next_after,
            previous_before=previous_before,
            has_older=has_older,
            records=records,
        )

        transcript_page.additional_properties = d
        return transcript_page

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
