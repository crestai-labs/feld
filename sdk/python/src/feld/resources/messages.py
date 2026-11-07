"""Messages: `/v1/messages` and `POST /v1/workspaces/{workspace}/actors/{actor}/messages`."""

from __future__ import annotations

import datetime

from .._api.message import delete_message, get_message, list_messages, send_message
from .._models import SendMessageBody
from .._types import UNSET, Unset
from ..models import Message, MessageSummary
from ._resource import Resource


class Messages(Resource):
    def list(
        self,
        *,
        workspace: str | Unset = UNSET,
        from_: str | Unset = UNSET,
        to: str | Unset = UNSET,
        label: str | Unset = UNSET,
        waiting: bool | Unset = UNSET,
        start: datetime.datetime | Unset = UNSET,
        end: datetime.datetime | Unset = UNSET,
    ) -> list[MessageSummary]:
        """The Messages that match the filters: `from_` and `to` are the `name`s of the sender
        and recipient Actors, and `waiting` keeps those without `read`."""
        return list_messages.sync(
            client=self._api,
            workspace=workspace,
            from_=from_,
            to=to,
            label=label,
            waiting=waiting,
            start=start,
            end=end,
        ).messages

    def send(
        self, workspace: str, actor: str, *, body: str, label: str | Unset = UNSET
    ) -> Message:
        """Sends `body` to the Actor `actor` of `workspace`."""
        return send_message.sync(
            workspace,
            actor,
            client=self._api,
            body=SendMessageBody(body=body, label=label),
        ).message

    def get(self, message: str) -> Message:
        return get_message.sync(message, client=self._api).message

    def delete(self, message: str) -> Message:
        return delete_message.sync(message, client=self._api).message
