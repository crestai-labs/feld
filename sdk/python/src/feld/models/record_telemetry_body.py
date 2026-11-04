from __future__ import annotations

from collections.abc import Mapping
from typing import (
    Any,
    Literal,
    Self,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define

from ..models.record_telemetry_body_signal import RecordTelemetryBodySignal

T = TypeVar("T", bound="RecordTelemetryBody")


@_attrs_define
class RecordTelemetryBody:
    """
    Attributes:
        request_id (str): The ID the Collector gave the request
        token (str): The telemetry token the Runtime Adapter put in the OTLP header (a value derived from the Session
            token)
        signal (RecordTelemetryBodySignal):
        content_type (Literal['application/x-protobuf']):
        payload (str):
    """

    request_id: str
    token: str
    signal: RecordTelemetryBodySignal
    content_type: Literal["application/x-protobuf"]
    payload: str

    def to_dict(self) -> dict[str, Any]:
        request_id = self.request_id

        token = self.token

        signal = self.signal.value

        content_type = self.content_type

        payload = self.payload

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "requestId": request_id,
                "token": token,
                "signal": signal,
                "contentType": content_type,
                "payload": payload,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        request_id = d.pop("requestId")

        token = d.pop("token")

        signal = RecordTelemetryBodySignal(d.pop("signal"))

        content_type = cast(Literal["application/x-protobuf"], d.pop("contentType"))
        if content_type != "application/x-protobuf":
            raise ValueError(
                f"contentType must match const 'application/x-protobuf', got '{content_type}'"
            )

        payload = d.pop("payload")

        record_telemetry_body = cls(
            request_id=request_id,
            token=token,
            signal=signal,
            content_type=content_type,
            payload=payload,
        )

        return record_telemetry_body
