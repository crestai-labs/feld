from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.runtime_quota_series_item_window import RuntimeQuotaSeriesItemWindow
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.account import Account
    from ..models.quota_point import QuotaPoint
    from ..models.runtime_quota_series_item_reported_by import (
        RuntimeQuotaSeriesItemReportedBy,
    )


T = TypeVar("T", bound="RuntimeQuotaSeriesItem")


@_attrs_define
class RuntimeQuotaSeriesItem:
    """
    Attributes:
        window (RuntimeQuotaSeriesItemWindow):
        resets_at (datetime.datetime):
        end (datetime.datetime):
        points (list[QuotaPoint]):
        latest (QuotaPoint):
        reported_by (RuntimeQuotaSeriesItemReportedBy):
        account (Account | Unset):
    """

    window: RuntimeQuotaSeriesItemWindow
    resets_at: datetime.datetime
    end: datetime.datetime
    points: list[QuotaPoint]
    latest: QuotaPoint
    reported_by: RuntimeQuotaSeriesItemReportedBy
    account: Account | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        window = self.window.value

        resets_at = self.resets_at.isoformat()

        end = self.end.isoformat()

        points = []
        for points_item_data in self.points:
            points_item = points_item_data.to_dict()
            points.append(points_item)

        latest = self.latest.to_dict()

        reported_by = self.reported_by.to_dict()

        account: dict[str, Any] | Unset = UNSET
        if not isinstance(self.account, Unset):
            account = self.account.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "window": window,
                "resetsAt": resets_at,
                "end": end,
                "points": points,
                "latest": latest,
                "reportedBy": reported_by,
            }
        )
        if account is not UNSET:
            field_dict["account"] = account

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.account import Account
        from ..models.quota_point import QuotaPoint
        from ..models.runtime_quota_series_item_reported_by import (
            RuntimeQuotaSeriesItemReportedBy,
        )

        d = dict(src_dict)
        window = RuntimeQuotaSeriesItemWindow(d.pop("window"))

        resets_at = datetime.datetime.fromisoformat(d.pop("resetsAt"))

        end = datetime.datetime.fromisoformat(d.pop("end"))

        points = []
        _points = d.pop("points")
        for points_item_data in _points:
            points_item = QuotaPoint.from_dict(points_item_data)

            points.append(points_item)

        latest = QuotaPoint.from_dict(d.pop("latest"))

        reported_by = RuntimeQuotaSeriesItemReportedBy.from_dict(d.pop("reportedBy"))

        _account = d.pop("account", UNSET)
        account: Account | Unset
        if isinstance(_account, Unset):
            account = UNSET
        else:
            account = Account.from_dict(_account)

        runtime_quota_series_item = cls(
            window=window,
            resets_at=resets_at,
            end=end,
            points=points,
            latest=latest,
            reported_by=reported_by,
            account=account,
        )

        runtime_quota_series_item.additional_properties = d
        return runtime_quota_series_item

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
