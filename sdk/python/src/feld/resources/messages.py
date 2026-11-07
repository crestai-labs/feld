"""Messages: `/v1/messages` and `POST /v1/workspaces/{workspace}/actors/{actor}/messages`."""

from __future__ import annotations

import datetime

from .._api.message import delete_message, get_message, list_messages, send_message
from .._models import SendMessageBody
from .._resource import Resource, sent
from ..models import Message, MessageSummary


class Messages(Resource):
    def list(
        self,
        *,
        workspace: str | None = None,
        from_: str | None = None,
        to: str | None = None,
        label: str | None = None,
        waiting: bool | None = None,
        start: datetime.datetime | None = None,
        end: datetime.datetime | None = None,
    ) -> list[MessageSummary]:
        """The Messages that match the filters: `from_` and `to` are the `name`s of the sender
        and recipient Actors, and `waiting` keeps those without `read`."""
        return list_messages.sync(
            client=self._api,
            workspace=sent(workspace),
            from_=sent(from_),
            to=sent(to),
            label=sent(label),
            waiting=sent(waiting),
            start=sent(start),
            end=sent(end),
        ).messages

    def send(
        self, workspace: str, actor: str, *, body: str, label: str | None = None
    ) -> Message:
        """Sends `body` to the Actor `actor` of `workspace`."""
        return send_message.sync(
            workspace,
            actor,
            client=self._api,
            body=SendMessageBody(body=body, label=sent(label)),
        ).message

    def get(self, message: str) -> Message:
        return get_message.sync(message, client=self._api).message

    def delete(self, message: str) -> Message:
        return delete_message.sync(message, client=self._api).message
