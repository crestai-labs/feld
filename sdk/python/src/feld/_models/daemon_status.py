from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import (
    TYPE_CHECKING,
    Any,
    Literal,
    Self,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from .._models.daemon_status_restored import DaemonStatusRestored
    from .._models.daemon_status_scheduler import DaemonStatusScheduler
    from .._models.plugin import Plugin


T = TypeVar("T", bound="DaemonStatus")


@_attrs_define
class DaemonStatus:
    """
    Attributes:
        state (Literal['running']):
        version (str): The same value as `feld --version`.
        instance_id (str):
        pid (int):
        home (str):
        api (str):
        started_at (datetime.datetime):
        server_time (datetime.datetime):
        history_start (datetime.datetime | None): The at of the oldest record. null when there is no record.
        timezone (str): The daemon's local timezone (IANA). Used as the default for Schedules. UTC when it cannot be
            determined.
        runtimes (list[Plugin]): Installed Runtime Adapters.
        providers (list[Plugin]): Installed Execution Providers.
        nodes (list[Literal['local']]):
        web_ui (None | str):
        allowed_hosts (list[str]): Names allowed with `--allow-host`. Excludes loopback names.
        anonymous_read (bool): Whether the daemon serves reads without a token as Admin (`--anonymous-read`).
        otlp (None | str): OTLP endpoint of the Collector. null if the Collector is not running.
        restored (DaemonStatusRestored):
        scheduler (DaemonStatusScheduler):
    """

    state: Literal["running"]
    version: str
    instance_id: str
    pid: int
    home: str
    api: str
    started_at: datetime.datetime
    server_time: datetime.datetime
    history_start: datetime.datetime | None
    timezone: str
    runtimes: list[Plugin]
    providers: list[Plugin]
    nodes: list[Literal["local"]]
    web_ui: None | str
    allowed_hosts: list[str]
    anonymous_read: bool
    otlp: None | str
    restored: DaemonStatusRestored
    scheduler: DaemonStatusScheduler
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        state = self.state

        version = self.version

        instance_id = self.instance_id

        pid = self.pid

        home = self.home

        api = self.api

        started_at = self.started_at.isoformat()

        server_time = self.server_time.isoformat()

        history_start: None | str
        if isinstance(self.history_start, datetime.datetime):
            history_start = self.history_start.isoformat()
        else:
            history_start = self.history_start

        timezone = self.timezone

        runtimes = []
        for runtimes_item_data in self.runtimes:
            runtimes_item = runtimes_item_data.to_dict()
            runtimes.append(runtimes_item)

        providers = []
        for providers_item_data in self.providers:
            providers_item = providers_item_data.to_dict()
            providers.append(providers_item)

        nodes = self.nodes

        web_ui: None | str
        web_ui = self.web_ui

        allowed_hosts = self.allowed_hosts

        anonymous_read = self.anonymous_read

        otlp: None | str
        otlp = self.otlp

        restored = self.restored.to_dict()

        scheduler = self.scheduler.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "state": state,
                "version": version,
                "instanceId": instance_id,
                "pid": pid,
                "home": home,
                "api": api,
                "startedAt": started_at,
                "serverTime": server_time,
                "historyStart": history_start,
                "timezone": timezone,
                "runtimes": runtimes,
                "providers": providers,
                "nodes": nodes,
                "webUi": web_ui,
                "allowedHosts": allowed_hosts,
                "anonymousRead": anonymous_read,
                "otlp": otlp,
                "restored": restored,
                "scheduler": scheduler,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.daemon_status_restored import DaemonStatusRestored
        from .._models.daemon_status_scheduler import DaemonStatusScheduler
        from .._models.plugin import Plugin

        d = dict(src_dict)
        state = cast(Literal["running"], d.pop("state"))
        if state != "running":
            raise ValueError(f"state must match const 'running', got '{state}'")

        version = d.pop("version")

        instance_id = d.pop("instanceId")

        pid = d.pop("pid")

        home = d.pop("home")

        api = d.pop("api")

        started_at = datetime.datetime.fromisoformat(d.pop("startedAt"))

        server_time = datetime.datetime.fromisoformat(d.pop("serverTime"))

        def _parse_history_start(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                history_start_type_0 = datetime.datetime.fromisoformat(data)

                return history_start_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        history_start = _parse_history_start(d.pop("historyStart"))

        timezone = d.pop("timezone")

        runtimes = []
        _runtimes = d.pop("runtimes")
        for runtimes_item_data in _runtimes:
            runtimes_item = Plugin.from_dict(runtimes_item_data)

            runtimes.append(runtimes_item)

        providers = []
        _providers = d.pop("providers")
        for providers_item_data in _providers:
            providers_item = Plugin.from_dict(providers_item_data)

            providers.append(providers_item)

        nodes = []
        _nodes = d.pop("nodes")
        for nodes_item_data in _nodes:
            nodes_item = cast(Literal["local"], nodes_item_data)
            if nodes_item != "local":
                raise ValueError(
                    f"nodes_item must match const 'local', got '{nodes_item}'"
                )
            nodes.append(nodes_item)

        def _parse_web_ui(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        web_ui = _parse_web_ui(d.pop("webUi"))

        allowed_hosts = cast(list[str], d.pop("allowedHosts"))

        anonymous_read = d.pop("anonymousRead")

        def _parse_otlp(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        otlp = _parse_otlp(d.pop("otlp"))

        restored = DaemonStatusRestored.from_dict(d.pop("restored"))

        scheduler = DaemonStatusScheduler.from_dict(d.pop("scheduler"))

        daemon_status = cls(
            state=state,
            version=version,
            instance_id=instance_id,
            pid=pid,
            home=home,
            api=api,
            started_at=started_at,
            server_time=server_time,
            history_start=history_start,
            timezone=timezone,
            runtimes=runtimes,
            providers=providers,
            nodes=nodes,
            web_ui=web_ui,
            allowed_hosts=allowed_hosts,
            anonymous_read=anonymous_read,
            otlp=otlp,
            restored=restored,
            scheduler=scheduler,
        )

        daemon_status.additional_properties = d
        return daemon_status

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
