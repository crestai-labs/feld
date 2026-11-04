from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.metric_name import MetricName
from ..models.metric_status import MetricStatus
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.metric_bin import MetricBin


T = TypeVar("T", bound="SessionMetricsMetricsItem")


@_attrs_define
class SessionMetricsMetricsItem:
    """
    Attributes:
        metric (MetricName):
        status (MetricStatus):
        source (str): Source of the value, or the reason there is no value
        series (list[MetricBin] | Unset): Time series when `reported`
    """

    metric: MetricName
    status: MetricStatus
    source: str
    series: list[MetricBin] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        metric = self.metric.value

        status = self.status.value

        source = self.source

        series: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.series, Unset):
            series = []
            for series_item_data in self.series:
                series_item = series_item_data.to_dict()
                series.append(series_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "metric": metric,
                "status": status,
                "source": source,
            }
        )
        if series is not UNSET:
            field_dict["series"] = series

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.metric_bin import MetricBin

        d = dict(src_dict)
        metric = MetricName(d.pop("metric"))

        status = MetricStatus(d.pop("status"))

        source = d.pop("source")

        _series = d.pop("series", UNSET)
        series: list[MetricBin] | Unset = UNSET
        if _series is not UNSET:
            series = []
            for series_item_data in _series:
                series_item = MetricBin.from_dict(series_item_data)

                series.append(series_item)

        session_metrics_metrics_item = cls(
            metric=metric,
            status=status,
            source=source,
            series=series,
        )

        session_metrics_metrics_item.additional_properties = d
        return session_metrics_metrics_item

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
