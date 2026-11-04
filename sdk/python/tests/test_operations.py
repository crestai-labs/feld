"""The operations a harness uses, against the stand-in daemon of `feld.testing`."""

from __future__ import annotations

import datetime
import itertools

import httpx
import pytest

import feld
from feld import records
from feld.api.actor import create_actor, list_actors
from feld.api.message import list_messages, send_message
from feld.api.session import get_output, stop_session
from feld.models import (
    Actor,
    ActorKind,
    CreateActorBody,
    LaunchSpec,
    LaunchSpecNode,
    ListActorsResponse200,
    ListRecordsResponse200,
    Message,
    MessageResponse,
    MessageSummaryList,
    OutputPage,
    OutputPageLinesItem,
    OutputPageLinesItemStream,
    RecordKind,
    SendMessageBody,
    Session,
    SessionEndKind,
    StopSessionResponse200,
)
from feld.testing import FakeDaemon

from .conftest import AT, DEV, message, record


def test_actor_list_answers_typed_actors(session: Session) -> None:
    actor = Actor(
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
    fake = FakeDaemon().on(
        "GET",
        "/v1/workspaces/feld-dev/actors",
        ListActorsResponse200(workspace="feld-dev", actors=[actor]),
    )
    answer = list_actors.sync(
        client=fake.client(), workspace="feld-dev", include_deleted=True
    )
    assert answer.actors == [actor]
    assert answer.actors[0].current_session is not None
    assert answer.actors[0].current_session.id == session.id
    assert fake.requests[0].url.params["includeDeleted"] == "true"
    assert fake.requests[0].headers["authorization"] == "Bearer test-token"


def test_session_answers_datetimes_and_enums(session: Session) -> None:
    assert session.start.tzinfo is not None
    assert isinstance(session.end, datetime.datetime)
    assert session.end_kind is SessionEndKind.STOPPED
    assert session.planned is None


def test_session_log_reads_the_tail() -> None:
    page = OutputPage(
        session="session_1",
        after=0,
        next_after=2,
        previous_before=None,
        has_older=False,
        lines=[
            OutputPageLinesItem(
                sequence=2,
                stream=OutputPageLinesItemStream.STDOUT,
                timestamp=AT,
                text="ready",
            )
        ],
    )
    fake = FakeDaemon().on("GET", "/v1/sessions/session_1/output", page)
    answer = get_output.sync(
        client=fake.client(), session="session_1", tail=True, limit=50
    )
    assert answer == page
    assert fake.requests[0].url.params["tail"] == "true"
    assert fake.requests[0].url.params["limit"] == "50"


def test_message_list_filters_by_recipient() -> None:
    fake = FakeDaemon().on(
        "GET", "/v1/messages", MessageSummaryList(messages=[message("message_1")])
    )
    listed = list_messages.sync(client=fake.client(), to="dev", waiting=True)
    assert [m.id for m in listed.messages] == ["message_1"]
    assert listed.messages[0].read is None
    assert fake.requests[0].url.params["to"] == "dev"


def test_message_send_posts_the_body() -> None:
    summary = message("message_2")
    answer = MessageResponse(
        message=Message(
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
    )
    fake = FakeDaemon().on(
        "POST", "/v1/workspaces/feld-dev/actors/dev/messages", answer
    )
    sent = send_message.sync(
        client=fake.client(),
        workspace="feld-dev",
        actor="dev",
        body=SendMessageBody(body="hello", label="review"),
    )
    assert sent.message.label == "review"
    assert fake.json() == {"body": "hello", "label": "review"}


def test_actor_create_sends_the_declaration() -> None:
    fake = FakeDaemon().on(
        "POST",
        "/v1/workspaces/feld-dev/actors",
        lambda request: httpx.Response(
            400, json={"message": "unknown provider 'nope'"}
        ),
    )
    body = CreateActorBody(
        name="dev",
        kind=ActorKind.JOB,
        launch_spec=LaunchSpec(
            workdir="/work",
            provider="nope",
            node=LaunchSpecNode.LOCAL,
            command=["true"],
        ),
    )
    with pytest.raises(feld.FeldRejected) as rejected:
        create_actor.sync(client=fake.client(), workspace="feld-dev", body=body)
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
        },
    }


def test_session_stop(session: Session) -> None:
    fake = FakeDaemon().on(
        "POST",
        f"/v1/sessions/{session.id}/stop",
        StopSessionResponse200(session=session, already_requested=False),
    )
    stopped = stop_session.sync(client=fake.client(), session=session.id)
    assert stopped.session.id == session.id
    assert stopped.already_requested is False


def test_rejection_carries_the_message_and_next() -> None:
    fake = FakeDaemon().reject(
        "POST",
        "/v1/sessions/session_x/stop",
        404,
        "Session 'session_x' was not found",
        next="feld session list",
    )
    with pytest.raises(feld.FeldRejected) as rejected:
        stop_session.sync(client=fake.client(), session="session_x")
    assert rejected.value.message == "Session 'session_x' was not found"
    assert rejected.value.next == "feld session list"


def test_unreachable_daemon_is_operational() -> None:
    client = feld.connect(api="http://127.0.0.1:1", token="x")
    with pytest.raises(feld.FeldOperational) as operational:
        list_actors.sync(client=client, workspace="feld-dev")
    assert operational.value.kind == "operational"


def test_undecodable_answer_is_operational() -> None:
    fake = FakeDaemon().on(
        "GET", "/v1/messages", lambda request: httpx.Response(200, text="{")
    )
    with pytest.raises(feld.FeldOperational):
        list_messages.sync(client=fake.client())


def page(*sequences: int, after: int) -> ListRecordsResponse200:
    return ListRecordsResponse200(
        records=[record(sequence) for sequence in sequences],
        next_after=sequences[-1] if sequences else after,
    )


def test_records_iterate_reads_every_page() -> None:
    fake = FakeDaemon().on(
        "GET", "/v1/records", page(1, 2, after=0), page(3, after=2), page(after=3)
    )
    seen = [
        r.sequence
        for r in records.iterate(fake.client(), kind=RecordKind.MESSAGE_ACCEPTED)
    ]
    assert seen == [1, 2, 3]
    assert [r.url.params["after"] for r in fake.requests] == ["0", "2", "3"]
    assert fake.requests[0].url.params["kind"] == "message.accepted"


def test_records_follow_waits_for_appended_records() -> None:
    fake = FakeDaemon().on(
        "GET", "/v1/records", page(5, after=4), page(after=5), page(6, after=5)
    )
    seen = [
        r.sequence
        for r in itertools.islice(records.follow(fake.client(), after=4, interval=0), 2)
    ]
    assert seen == [5, 6]
    assert [r.url.params["after"] for r in fake.requests] == ["4", "5", "5"]
