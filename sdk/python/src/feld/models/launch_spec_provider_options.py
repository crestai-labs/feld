from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="LaunchSpecProviderOptions")


@_attrs_define
class LaunchSpecProviderOptions:
    """Provider-specific options, built from the CLI's `--<provider>-<key> <value>`.
    tmux has `socket`, `unit` (`session` / `window` / `pane`), and the borrowed `session` / `window`.
    herdr has `session` (a session name or socket path), `unit` (`workspace` / `tab` / `pane`), the borrowed `workspace`
    / `pane`, and `split` (`right` / `down`) when `unit` is `pane`.

    """

    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        launch_spec_provider_options = cls()

        launch_spec_provider_options.additional_properties = d
        return launch_spec_provider_options

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
