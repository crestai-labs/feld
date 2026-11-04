from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.browser import Browser


T = TypeVar("T", bound="ListBrowsersResponse200")


@_attrs_define
class ListBrowsersResponse200:
    """
    Attributes:
        browsers (list[Browser]):
    """

    browsers: list[Browser]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        browsers = []
        for browsers_item_data in self.browsers:
            browsers_item = browsers_item_data.to_dict()
            browsers.append(browsers_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "browsers": browsers,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.browser import Browser

        d = dict(src_dict)
        browsers = []
        _browsers = d.pop("browsers")
        for browsers_item_data in _browsers:
            browsers_item = Browser.from_dict(browsers_item_data)

            browsers.append(browsers_item)

        list_browsers_response_200 = cls(
            browsers=browsers,
        )

        list_browsers_response_200.additional_properties = d
        return list_browsers_response_200

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
