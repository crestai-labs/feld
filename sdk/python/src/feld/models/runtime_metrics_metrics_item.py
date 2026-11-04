from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.metric_name import MetricName
from ..models.metric_status import MetricStatus
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.runtime_metrics_metrics_item_accounts_item import (
        RuntimeMetricsMetricsItemAccountsItem,
    )
    from ..models.runtime_metrics_metrics_item_sessions import (
        RuntimeMetricsMetricsItemSessions,
    )


T = TypeVar("T", bound="RuntimeMetricsMetricsItem")


@_attrs_define
class RuntimeMetricsMetricsItem:
    """
    Attributes:
        metric (MetricName):
        status (MetricStatus):
        source (str):
        sessions (RuntimeMetricsMetricsItemSessions): Count of aggregated Sessions per status
        accounts (list[RuntimeMetricsMetricsItemAccountsItem] | Unset): Series aggregated from `reported` Sessions per
            account
    """

    metric: MetricName
    status: MetricStatus
    source: str
    sessions: RuntimeMetricsMetricsItemSessions
    accounts: list[RuntimeMetricsMetricsItemAccountsItem] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        metric = self.metric.value

        status = self.status.value

        source = self.source

        sessions = self.sessions.to_dict()

        accounts: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.accounts, Unset):
            accounts = []
            for accounts_item_data in self.accounts:
                accounts_item = accounts_item_data.to_dict()
                accounts.append(accounts_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "metric": metric,
                "status": status,
                "source": source,
                "sessions": sessions,
            }
        )
        if accounts is not UNSET:
            field_dict["accounts"] = accounts

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.runtime_metrics_metrics_item_accounts_item import (
            RuntimeMetricsMetricsItemAccountsItem,
        )
        from ..models.runtime_metrics_metrics_item_sessions import (
            RuntimeMetricsMetricsItemSessions,
        )

        d = dict(src_dict)
        metric = MetricName(d.pop("metric"))

        status = MetricStatus(d.pop("status"))

        source = d.pop("source")

        sessions = RuntimeMetricsMetricsItemSessions.from_dict(d.pop("sessions"))

        _accounts = d.pop("accounts", UNSET)
        accounts: list[RuntimeMetricsMetricsItemAccountsItem] | Unset = UNSET
        if _accounts is not UNSET:
            accounts = []
            for accounts_item_data in _accounts:
                accounts_item = RuntimeMetricsMetricsItemAccountsItem.from_dict(
                    accounts_item_data
                )

                accounts.append(accounts_item)

        runtime_metrics_metrics_item = cls(
            metric=metric,
            status=status,
            source=source,
            sessions=sessions,
            accounts=accounts,
        )

        runtime_metrics_metrics_item.additional_properties = d
        return runtime_metrics_metrics_item

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
