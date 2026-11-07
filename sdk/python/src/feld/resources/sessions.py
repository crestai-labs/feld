"""Sessions: `/v1/sessions` and `/v1/sessions/{session}/...`."""

from __future__ import annotations

import datetime

from .._api.session import (
    get_output,
    get_session,
    get_transcript,
    kill_session,
    list_sessions,
    observe_session,
    stop_session,
)
from .._resource import Resource, sent
from ..models import (
    ActorKind,
    LifecycleResponse,
    OutputPage,
    Session,
    SessionResponse,
    SubAgentSession,
    TranscriptPage,
    TranscriptView,
)


class Sessions(Resource):
    def list(
        self,
        *,
        workspace: str | None = None,
        actor: str | None = None,
        kind: ActorKind | None = None,
        current: bool | None = None,
        start: datetime.datetime | None = None,
        end: datetime.datetime | None = None,
    ) -> list[Session]:
        """The Sessions that match the filters; `current` keeps the running ones."""
        return list_sessions.sync(
            client=self._api,
            workspace=sent(workspace),
            actor=sent(actor),
            kind=sent(kind),
            current=sent(current),
            start=sent(start),
            end=sent(end),
        ).sessions

    def get(self, session: str) -> SessionResponse | SubAgentSession:
        """The Session with the Sub Agent Sessions projected from its transcript, or the Sub
        Agent Session when `session` is the ID of one."""
        answer = get_session.sync(session, client=self._api)
        if isinstance(answer, SessionResponse):
            return answer
        return answer.sub_agent_session

    def stop(self, session: str) -> LifecycleResponse:
        """Requests the Session to stop."""
        return stop_session.sync(session, client=self._api)

    def kill(self, session: str) -> LifecycleResponse:
        """Requests the Session to be killed."""
        return kill_session.sync(session, client=self._api)

    def observe(self, session: str) -> Session:
        """Has the Provider observe the Session's entity now, and answers the Session after it."""
        return observe_session.sync(session, client=self._api).session

    def output(
        self,
        session: str,
        *,
        after: int | None = None,
        before: int | None = None,
        limit: int | None = None,
        tail: bool | None = None,
    ) -> OutputPage:
        """A page of the output of a `program` Session."""
        return get_output.sync(
            session,
            client=self._api,
            after=sent(after),
            before=sent(before),
            limit=sent(limit),
            tail=sent(tail),
        )

    def transcript(
        self,
        session: str,
        *,
        after: str | None = None,
        before: str | None = None,
        limit: int | None = None,
        tail: bool | None = None,
        view: TranscriptView | None = None,
    ) -> TranscriptPage:
        """A page of the transcript of an `agent-runtime` Session."""
        return get_transcript.sync(
            session,
            client=self._api,
            after=sent(after),
            before=sent(before),
            limit=sent(limit),
            tail=sent(tail),
            view=sent(view),
        )
