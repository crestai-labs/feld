from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.account import Account
    from ..models.metric_bin import MetricBin


T = TypeVar("T", bound="RuntimeMetricsMetricsItemAccountsItem")


@_attrs_define
class RuntimeMetricsMetricsItemAccountsItem:
    """
    Attributes:
        series (list[MetricBin]):
        account (Account | Unset):
    """

    series: list[MetricBin]
    account: Account | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        series = []
        for series_item_data in self.series:
            series_item = series_item_data.to_dict()
            series.append(series_item)

        account: dict[str, Any] | Unset = UNSET
        if not isinstance(self.account, Unset):
            account = self.account.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "series": series,
            }
        )
        if account is not UNSET:
            field_dict["account"] = account

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.account import Account
        from ..models.metric_bin import MetricBin

        d = dict(src_dict)
        series = []
        _series = d.pop("series")
        for series_item_data in _series:
            series_item = MetricBin.from_dict(series_item_data)

            series.append(series_item)

        _account = d.pop("account", UNSET)
        account: Account | Unset
        if isinstance(_account, Unset):
            account = UNSET
        else:
            account = Account.from_dict(_account)

        runtime_metrics_metrics_item_accounts_item = cls(
            series=series,
            account=account,
        )

        runtime_metrics_metrics_item_accounts_item.additional_properties = d
        return runtime_metrics_metrics_item_accounts_item

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
