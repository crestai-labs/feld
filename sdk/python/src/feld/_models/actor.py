from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from .._models.actor_kind import ActorKind
from .._models.grant import Grant

if TYPE_CHECKING:
    from .._models.actor_label import ActorLabel
    from .._models.launch_spec import LaunchSpec
    from .._models.schedule import Schedule
    from .._models.session import Session


T = TypeVar("T", bound="Actor")


@_attrs_define
class Actor:
    """
    Attributes:
        id (str): ID assigned by Feld. Remains on the Actor after deletion
        name (str):
        workspace (str):
        kind (ActorKind):
        launch_spec (LaunchSpec | None): null for an Actor created only by attach, and for another Actor read with a
            `sessionToken`
        schedule (None | Schedule):
        grants (list[Grant]): Operations on other Actors in the same Workspace that Admin allows the Actor's Session
        accept_from (list[str] | None): If null, receives from every Actor in the same Workspace
        declared_in (None | str): Name of the declaration file whose `feld apply` declares the Actor. null for an Actor
            declared otherwise
        labels (list[ActorLabel]): By key. The daemon removes a label within seconds after its `until`, records
            `label.expired`, and sends the Actor's resource event
        created_at (datetime.datetime):
        updated_at (datetime.datetime):
        deleted_at (datetime.datetime | None):
        current_session (None | Session): Session without `end`
        latest_session (None | Session): First of `sessions`
        sessions (list[Session]): Newest first
        attention (bool): True when the evaluated Session (current, or latest if none) has outcome `failed`, `exited`,
            or `unknown`, or is running with observation `lost` or `mismatched`
        waiting_count (int): Number of unread Messages
        unread_mail (bool):
        skip_mark (bool):
        unlinked_mark (bool): True when the current Session is an `owned` `agent-runtime` Session whose Runtime identity
            has not arrived yet
    """

    id: str
    name: str
    workspace: str
    kind: ActorKind
    launch_spec: LaunchSpec | None
    schedule: None | Schedule
    grants: list[Grant]
    accept_from: list[str] | None
    declared_in: None | str
    labels: list[ActorLabel]
    created_at: datetime.datetime
    updated_at: datetime.datetime
    deleted_at: datetime.datetime | None
    current_session: None | Session
    latest_session: None | Session
    sessions: list[Session]
    attention: bool
    waiting_count: int
    unread_mail: bool
    skip_mark: bool
    unlinked_mark: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from .._models.launch_spec import LaunchSpec
        from .._models.schedule import Schedule
        from .._models.session import Session

        id = self.id

        name = self.name

        workspace = self.workspace

        kind = self.kind.value

        launch_spec: dict[str, Any] | None
        if isinstance(self.launch_spec, LaunchSpec):
            launch_spec = self.launch_spec.to_dict()
        else:
            launch_spec = self.launch_spec

        schedule: dict[str, Any] | None
        if isinstance(self.schedule, Schedule):
            schedule = self.schedule.to_dict()
        else:
            schedule = self.schedule

        grants = []
        for componentsschemas_grants_item_data in self.grants:
            componentsschemas_grants_item = componentsschemas_grants_item_data.value
            grants.append(componentsschemas_grants_item)

        accept_from: list[str] | None
        if isinstance(self.accept_from, list):
            accept_from = self.accept_from

        else:
            accept_from = self.accept_from

        declared_in: None | str
        declared_in = self.declared_in

        labels = []
        for labels_item_data in self.labels:
            labels_item = labels_item_data.to_dict()
            labels.append(labels_item)

        created_at = self.created_at.isoformat()

        updated_at = self.updated_at.isoformat()

        deleted_at: None | str
        if isinstance(self.deleted_at, datetime.datetime):
            deleted_at = self.deleted_at.isoformat()
        else:
            deleted_at = self.deleted_at

        current_session: dict[str, Any] | None
        if isinstance(self.current_session, Session):
            current_session = self.current_session.to_dict()
        else:
            current_session = self.current_session

        latest_session: dict[str, Any] | None
        if isinstance(self.latest_session, Session):
            latest_session = self.latest_session.to_dict()
        else:
            latest_session = self.latest_session

        sessions = []
        for sessions_item_data in self.sessions:
            sessions_item = sessions_item_data.to_dict()
            sessions.append(sessions_item)

        attention = self.attention

        waiting_count = self.waiting_count

        unread_mail = self.unread_mail

        skip_mark = self.skip_mark

        unlinked_mark = self.unlinked_mark

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "workspace": workspace,
                "kind": kind,
                "launchSpec": launch_spec,
                "schedule": schedule,
                "grants": grants,
                "acceptFrom": accept_from,
                "declaredIn": declared_in,
                "labels": labels,
                "createdAt": created_at,
                "updatedAt": updated_at,
                "deletedAt": deleted_at,
                "currentSession": current_session,
                "latestSession": latest_session,
                "sessions": sessions,
                "attention": attention,
                "waitingCount": waiting_count,
                "unreadMail": unread_mail,
                "skipMark": skip_mark,
                "unlinkedMark": unlinked_mark,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from .._models.actor_label import ActorLabel
        from .._models.launch_spec import LaunchSpec
        from .._models.schedule import Schedule
        from .._models.session import Session

        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        workspace = d.pop("workspace")

        kind = ActorKind(d.pop("kind"))

        def _parse_launch_spec(data: object) -> LaunchSpec | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                launch_spec_type_0 = LaunchSpec.from_dict(data)

                return launch_spec_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(LaunchSpec | None, data)

        launch_spec = _parse_launch_spec(d.pop("launchSpec"))

        def _parse_schedule(data: object) -> None | Schedule:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                schedule_type_0 = Schedule.from_dict(data)

                return schedule_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Schedule, data)

        schedule = _parse_schedule(d.pop("schedule"))

        grants = []
        _grants = d.pop("grants")
        for componentsschemas_grants_item_data in _grants:
            componentsschemas_grants_item = Grant(componentsschemas_grants_item_data)

            grants.append(componentsschemas_grants_item)

        def _parse_accept_from(data: object) -> list[str] | None:
            if data is None:
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                accept_from_type_0 = cast(list[str], data)

                return accept_from_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None, data)

        accept_from = _parse_accept_from(d.pop("acceptFrom"))

        def _parse_declared_in(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        declared_in = _parse_declared_in(d.pop("declaredIn"))

        labels = []
        _labels = d.pop("labels")
        for labels_item_data in _labels:
            labels_item = ActorLabel.from_dict(labels_item_data)

            labels.append(labels_item)

        created_at = datetime.datetime.fromisoformat(d.pop("createdAt"))

        updated_at = datetime.datetime.fromisoformat(d.pop("updatedAt"))

        def _parse_deleted_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                deleted_at_type_0 = datetime.datetime.fromisoformat(data)

                return deleted_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        deleted_at = _parse_deleted_at(d.pop("deletedAt"))

        def _parse_current_session(data: object) -> None | Session:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                current_session_type_0 = Session.from_dict(data)

                return current_session_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Session, data)

        current_session = _parse_current_session(d.pop("currentSession"))

        def _parse_latest_session(data: object) -> None | Session:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                latest_session_type_0 = Session.from_dict(data)

                return latest_session_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Session, data)

        latest_session = _parse_latest_session(d.pop("latestSession"))

        sessions = []
        _sessions = d.pop("sessions")
        for sessions_item_data in _sessions:
            sessions_item = Session.from_dict(sessions_item_data)

            sessions.append(sessions_item)

        attention = d.pop("attention")

        waiting_count = d.pop("waitingCount")

        unread_mail = d.pop("unreadMail")

        skip_mark = d.pop("skipMark")

        unlinked_mark = d.pop("unlinkedMark")

        actor = cls(
            id=id,
            name=name,
            workspace=workspace,
            kind=kind,
            launch_spec=launch_spec,
            schedule=schedule,
            grants=grants,
            accept_from=accept_from,
            declared_in=declared_in,
            labels=labels,
            created_at=created_at,
            updated_at=updated_at,
            deleted_at=deleted_at,
            current_session=current_session,
            latest_session=latest_session,
            sessions=sessions,
            attention=attention,
            waiting_count=waiting_count,
            unread_mail=unread_mail,
            skip_mark=skip_mark,
            unlinked_mark=unlinked_mark,
        )

        actor.additional_properties = d
        return actor

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
