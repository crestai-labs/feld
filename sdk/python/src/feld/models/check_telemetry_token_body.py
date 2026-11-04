from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="CheckTelemetryTokenBody")


@_attrs_define
class CheckTelemetryTokenBody:
    """
    Attributes:
        token (str): The telemetry token the Runtime Adapter put in the OTLP header
    """

    token: str

    def to_dict(self) -> dict[str, Any]:
        token = self.token

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "token": token,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        token = d.pop("token")

        check_telemetry_token_body = cls(
            token=token,
        )

        return check_telemetry_token_body
