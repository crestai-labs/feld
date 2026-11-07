from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

from .._types import UNSET, Unset

if TYPE_CHECKING:
    from .._models.placement import Placement


T = TypeVar("T", bound="AttachActorBody")


@_attrs_define
class AttachActorBody:
    """
    Attributes:
        placement (Placement):
        runtime (str):
        runtime_telemetry_prepared (bool | Unset): Whether the Runtime process started with the telemetry environment
            Feld prepared. When false, the Session metrics other than context window utilization are `unavailable`, and
            `source` holds the reason Default: True.
    """

    placement: Placement
    runtime: str
    runtime_telemetry_prepared: bool | Unset = True

    def to_dict(self) -> dict[str, Any]:
        placement = self.placement.to_dict()

        runtime = self.runtime

        runtime_telemetry_prepared = self.runtime_telemetry_prepared

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "placement": placement,
                "runtime": runtime,
            }
        )
        if runtime_telemetry_prepared is not UNSET:
            field_dict["runtimeTelemetryPrepared"] = runtime_telemetry_prepared

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.placement import Placement

        d = dict(src_dict)
        placement = Placement.from_dict(d.pop("placement"))

        runtime = d.pop("runtime")

        runtime_telemetry_prepared = d.pop("runtimeTelemetryPrepared", UNSET)

        attach_actor_body = cls(
            placement=placement,
            runtime=runtime,
            runtime_telemetry_prepared=runtime_telemetry_prepared,
        )

        return attach_actor_body
