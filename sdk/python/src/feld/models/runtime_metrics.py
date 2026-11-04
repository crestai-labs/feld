from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.runtime_metrics_metrics_item import RuntimeMetricsMetricsItem
    from ..models.runtime_quota import RuntimeQuota


T = TypeVar("T", bound="RuntimeMetrics")


@_attrs_define
class RuntimeMetrics:
    """
    Attributes:
        runtime (str):
        start (datetime.datetime | None):
        end (datetime.datetime | None):
        bucket (int | None):
        metrics (list[RuntimeMetricsMetricsItem]): Session metrics aggregated by the rule of each metric
        quota (RuntimeQuota | Unset): 5-hour / 7-day quota of the account
    """

    runtime: str
    start: datetime.datetime | None
    end: datetime.datetime | None
    bucket: int | None
    metrics: list[RuntimeMetricsMetricsItem]
    quota: RuntimeQuota | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        runtime = self.runtime

        start: None | str
        if isinstance(self.start, datetime.datetime):
            start = self.start.isoformat()
        else:
            start = self.start

        end: None | str
        if isinstance(self.end, datetime.datetime):
            end = self.end.isoformat()
        else:
            end = self.end

        bucket: int | None
        bucket = self.bucket

        metrics = []
        for metrics_item_data in self.metrics:
            metrics_item = metrics_item_data.to_dict()
            metrics.append(metrics_item)

        quota: dict[str, Any] | Unset = UNSET
        if not isinstance(self.quota, Unset):
            quota = self.quota.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "runtime": runtime,
                "start": start,
                "end": end,
                "bucket": bucket,
                "metrics": metrics,
            }
        )
        if quota is not UNSET:
            field_dict["quota"] = quota

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.runtime_metrics_metrics_item import RuntimeMetricsMetricsItem
        from ..models.runtime_quota import RuntimeQuota

        d = dict(src_dict)
        runtime = d.pop("runtime")

        def _parse_start(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                start_type_0 = datetime.datetime.fromisoformat(data)

                return start_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        start = _parse_start(d.pop("start"))

        def _parse_end(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                end_type_0 = datetime.datetime.fromisoformat(data)

                return end_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        end = _parse_end(d.pop("end"))

        def _parse_bucket(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        bucket = _parse_bucket(d.pop("bucket"))

        metrics = []
        _metrics = d.pop("metrics")
        for metrics_item_data in _metrics:
            metrics_item = RuntimeMetricsMetricsItem.from_dict(metrics_item_data)

            metrics.append(metrics_item)

        _quota = d.pop("quota", UNSET)
        quota: RuntimeQuota | Unset
        if isinstance(_quota, Unset):
            quota = UNSET
        else:
            quota = RuntimeQuota.from_dict(_quota)

        runtime_metrics = cls(
            runtime=runtime,
            start=start,
            end=end,
            bucket=bucket,
            metrics=metrics,
            quota=quota,
        )

        runtime_metrics.additional_properties = d
        return runtime_metrics

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
