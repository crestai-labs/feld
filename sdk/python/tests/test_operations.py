"""The operations a harness uses, against the stand-in daemon of `feld.testing`."""

from __future__ import annotations

import datetime
import itertools

import httpx
import pytest

import feld
from feld.models import (
    Actor,
    ActorKind,
    LaunchSpec,
    LaunchSpecNode,
    LifecycleResponse,
    Message,
    OutputLine,
    OutputPage,
    OutputStream,
    Record,
    RecordKind,
    ScheduleSpec,
    Session,
    SessionEndKind,
)
from feld.testing import FakeDaemon

from .conftest import AT, DEV, message, record


def actor(session: Session) -> Actor:
    return Actor(
        id="actor_dev",
        name="dev",
        workspace="feld-dev",
        kind=ActorKind.JOB,
        launch_spec=None,
        schedule=None,
        grants=[],
        accept_from=None,
        created_at=AT,
        updated_at=AT,
        deleted_at=None,
        current_session=session,
        latest_session=session,
        sessions=[session],
        attention=False,
        waiting_count=0,
        unread_mail=False,
        skip_mark=False,
        unlinked_mark=False,
    )


def test_actors_list_answers_typed_actors(session: Session) -> None:
    dev = actor(session)
    fake = FakeDaemon().on(
        "GET",
        "/v1/workspaces/feld-dev/actors",
        {"workspace": "feld-dev", "actors": [dev]},
    )
    actors = fake.client().actors.list("feld-dev", include_deleted=True)
    assert actors == [dev]
    assert actors[0].current_session is not None
    assert actors[0].current_session.id == session.id
    assert fake.requests[0].url.params["includeDeleted"] == "true"
    assert fake.requests[0].headers["authorization"] == "Bearer test-token"


def test_optional_parameters_are_not_sent_unless_given() -> None:
    fake = FakeDaemon().on(
        "GET", "/v1/workspaces/feld-dev/actors", {"workspace": "feld-dev", "actors": []}
    )
    assert fake.client().actors.list("feld-dev") == []
    assert dict(fake.requests[0].url.params) == {}


def test_session_answers_datetimes_enums_and_the_placement(session: Session) -> None:
    assert session.start.tzinfo is not None
    assert isinstance(session.end, datetime.datetime)
    assert session.end_kind is SessionEndKind.STOPPED
    assert session.planned is None
    assert session.placement.placement["sessionId"] == "$0"


def test_sessions_output_reads_the_tail() -> None:
    page = OutputPage(
        session="session_1",
        after=0,
        next_after=2,
        previous_before=None,
        has_older=False,
        lines=[
            OutputLine(
                sequence=2, stream=OutputStream.STDOUT, timestamp=AT, text="ready"
            )
        ],
    )
    fake = FakeDaemon().on("GET", "/v1/sessions/session_1/output", page)
    assert fake.client().sessions.output("session_1", tail=True, limit=50) == page
    assert fake.requests[0].url.params["tail"] == "true"
    assert fake.requests[0].url.params["limit"] == "50"


@pytest.mark.parametrize("operation", ["stop", "kill"])
def test_sessions_stop_and_kill(session: Session, operation: str) -> None:
    fake = FakeDaemon().on(
        "POST",
        f"/v1/sessions/{session.id}/{operation}",
        LifecycleResponse(session=session, already_requested=True),
    )
    sessions = fake.client().sessions
    answer = (
        sessions.stop(session.id) if operation == "stop" else sessions.kill(session.id)
    )
    assert answer.session.id == session.id
    assert answer.already_requested is True


def test_messages_list_filters_by_recipient() -> None:
    fake = FakeDaemon().on("GET", "/v1/messages", {"messages": [message("message_1")]})
    listed = fake.client().messages.list(to="dev", from_="lead", waiting=True)
    assert [m.id for m in listed] == ["message_1"]
    assert listed[0].read is None
    assert dict(fake.requests[0].url.params) == {
        "to": "dev",
        "from": "lead",
        "waiting": "true",
    }


def test_messages_send_posts_the_body() -> None:
    summary = message("message_2")
    sent = Message(
        id=summary.id,
        fifo=summary.fifo,
        from_=summary.from_,
        from_session_id=None,
        to=DEV,
        to_session_id=None,
        accepted=AT,
        label="review",
        queued=None,
        read=None,
        preview="hello",
        anchor=summary.anchor,
        waiting=True,
        deleted_at=None,
        body="hello",
    )
    fake = FakeDaemon().on(
        "POST", "/v1/workspaces/feld-dev/actors/dev/messages", {"message": sent}
    )
    answer = fake.client().messages.send(
        "feld-dev", "dev", body="hello", label="review"
    )
    assert answer == sent
    assert fake.json() == {"body": "hello", "label": "review"}


def test_actors_create_sends_the_declaration() -> None:
    fake = FakeDaemon().reject(
        "POST", "/v1/workspaces/feld-dev/actors", 400, "unknown provider 'nope'"
    )
    with pytest.raises(feld.FeldRejected) as rejected:
        fake.client().actors.create(
            "feld-dev",
            name="dev",
            kind=ActorKind.JOB,
            launch_spec=LaunchSpec(
                workdir="/work",
                provider="nope",
                node=LaunchSpecNode.LOCAL,
                command=["true"],
                provider_options={"unit": "pane"},
            ),
            schedule=ScheduleSpec(cron="0 * * * *"),
        )
    assert rejected.value.status == 400
    assert rejected.value.kind == "rejected"
    assert fake.json() == {
        "name": "dev",
        "kind": "job",
        "launchSpec": {
            "workdir": "/work",
            "provider": "nope",
            "node": "local",
            "command": ["true"],
            "providerOptions": {"unit": "pane"},
        },
        "schedule": {"cron": "0 * * * *"},
    }


def test_actors_create_answers_the_actor(session: Session) -> None:
    dev = actor(session)
    fake = FakeDaemon().on("POST", "/v1/workspaces/feld-dev/actors", {"actor": dev})
    created = fake.client().actors.create(
        "feld-dev",
        name="dev",
        kind=ActorKind.JOB,
        launch_spec=LaunchSpec(
            workdir="/work", provider="tmux", node=LaunchSpecNode.LOCAL
        ),
        schedule=None,
    )
    assert created == dev
    assert fake.json()["schedule"] is None


def test_daemon_status() -> None:
    status = {
        "state": "running",
        "version": "0.2.1",
        "instanceId": "daemon_1",
        "pid": 42,
        "home": "/home/me/.feld",
        "api": "http://127.0.0.1:7381",
        "startedAt": "2026-11-04T09:00:00Z",
        "serverTime": "2026-11-04T09:00:01Z",
        "historyStart": None,
        "timezone": "UTC",
        "runtimes": [],
        "providers": [],
        "nodes": ["local"],
        "webUi": None,
        "allowedHosts": [],
        "otlp": None,
        "restored": {"actors": 0, "runningSessions": 0, "schedules": 0},
        "scheduler": {
            "wokeAt": None,
            "planned": [],
            "restarts": 0,
            "lastRestart": None,
        },
    }
    fake = FakeDaemon().on("GET", "/v1/daemon/status", status)
    answer = fake.client().daemon.status()
    assert (answer.instance_id, answer.pid) == ("daemon_1", 42)
    assert answer.scheduler.last_restart is None


def test_rejection_carries_the_message_and_next() -> None:
    fake = FakeDaemon().reject(
        "POST",
        "/v1/sessions/session_x/stop",
        404,
        "Session 'session_x' was not found",
        next="feld session list",
    )
    with pytest.raises(feld.FeldRejected) as rejected:
        fake.client().sessions.stop("session_x")
    assert rejected.value.message == "Session 'session_x' was not found"
    assert rejected.value.next == "feld session list"


def test_an_undocumented_error_status_is_rejected() -> None:
    fake = FakeDaemon().reject("GET", "/v1/daemon/status", 418, "teapot")
    with pytest.raises(feld.FeldRejected) as rejected:
        fake.client().daemon.status()
    assert (rejected.value.status, rejected.value.message) == (418, "teapot")


def test_unreachable_daemon_is_operational() -> None:
    client = feld.connect(api="http://127.0.0.1:1", token="x")
    with pytest.raises(feld.FeldOperational) as operational:
        client.actors.list("feld-dev")
    assert operational.value.kind == "operational"


def test_undecodable_answer_is_operational() -> None:
    fake = FakeDaemon().on(
        "GET", "/v1/messages", lambda request: httpx.Response(200, text="{")
    )
    with pytest.raises(feld.FeldOperational):
        fake.client().messages.list()


def page(*sequences: int, after: int) -> dict[str, object]:
    records: list[Record] = [record(sequence) for sequence in sequences]
    return {"records": records, "nextAfter": sequences[-1] if sequences else after}


def test_records_iterate_reads_every_page() -> None:
    fake = FakeDaemon().on(
        "GET", "/v1/records", page(1, 2, after=0), page(3, after=2), page(after=3)
    )
    records = fake.client().records.iterate(kind=RecordKind.MESSAGE_ACCEPTED)
    assert [r.sequence for r in records] == [1, 2, 3]
    assert [r.url.params["after"] for r in fake.requests] == ["0", "2", "3"]
    assert fake.requests[0].url.params["kind"] == "message.accepted"


def test_records_follow_waits_for_appended_records() -> None:
    fake = FakeDaemon().on(
        "GET", "/v1/records", page(5, after=4), page(after=5), page(6, after=5)
    )
    records = fake.client().records.follow(after=4, interval=0)
    assert [r.sequence for r in itertools.islice(records, 2)] == [5, 6]
    assert [r.url.params["after"] for r in fake.requests] == ["4", "5", "5"]
