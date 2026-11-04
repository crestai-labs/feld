from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.output_page_lines_item import OutputPageLinesItem


T = TypeVar("T", bound="OutputPage")


@_attrs_define
class OutputPage:
    """
    Attributes:
        session (str):
        after (int):
        next_after (int):
        previous_before (int | None):
        has_older (bool):
        lines (list[OutputPageLinesItem]):
    """

    session: str
    after: int
    next_after: int
    previous_before: int | None
    has_older: bool
    lines: list[OutputPageLinesItem]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        session = self.session

        after = self.after

        next_after = self.next_after

        previous_before: int | None
        previous_before = self.previous_before

        has_older = self.has_older

        lines = []
        for lines_item_data in self.lines:
            lines_item = lines_item_data.to_dict()
            lines.append(lines_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "session": session,
                "after": after,
                "nextAfter": next_after,
                "previousBefore": previous_before,
                "hasOlder": has_older,
                "lines": lines,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.output_page_lines_item import OutputPageLinesItem

        d = dict(src_dict)
        session = d.pop("session")

        after = d.pop("after")

        next_after = d.pop("nextAfter")

        def _parse_previous_before(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        previous_before = _parse_previous_before(d.pop("previousBefore"))

        has_older = d.pop("hasOlder")

        lines = []
        _lines = d.pop("lines")
        for lines_item_data in _lines:
            lines_item = OutputPageLinesItem.from_dict(lines_item_data)

            lines.append(lines_item)

        output_page = cls(
            session=session,
            after=after,
            next_after=next_after,
            previous_before=previous_before,
            has_older=has_older,
            lines=lines,
        )

        output_page.additional_properties = d
        return output_page

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
