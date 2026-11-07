"""Records: `GET /v1/records`, read in `sequence` order page by page (`iterate`) or as they
are appended (`follow`)."""

from __future__ import annotations

import datetime
import time
from collections.abc import Iterator

from .._api.record import list_records
from .._resource import Resource, sent
from ..models import Record, RecordKind


class Records(Resource):
    def iterate(
        self,
        *,
        workspace: str | None = None,
        actor: str | None = None,
        session: str | None = None,
        kind: RecordKind | None = None,
        start: datetime.datetime | None = None,
        end: datetime.datetime | None = None,
        after: int = 0,
    ) -> Iterator[Record]:
        """The records after `after` that match the filters, until the last one there is
        now."""
        yield from self._pages(workspace, actor, session, kind, start, end, after, None)

    def follow(
        self,
        *,
        workspace: str | None = None,
        actor: str | None = None,
        session: str | None = None,
        kind: RecordKind | None = None,
        start: datetime.datetime | None = None,
        after: int = 0,
        interval: float = 1.0,
    ) -> Iterator[Record]:
        """The records after `after` that match the filters, then each one as it is
        appended: it reads again every `interval` seconds once it has read them all, and
        never ends."""
        yield from self._pages(
            workspace, actor, session, kind, start, None, after, interval
        )

    def _pages(
        self,
        workspace: str | None,
        actor: str | None,
        session: str | None,
        kind: RecordKind | None,
        start: datetime.datetime | None,
        end: datetime.datetime | None,
        after: int,
        interval: float | None,
    ) -> Iterator[Record]:
        while True:
            page = list_records.sync(
                client=self._api,
                workspace=sent(workspace),
                actor=sent(actor),
                session=sent(session),
                kind=sent(kind),
                start=sent(start),
                end=sent(end),
                after=after,
            )
            yield from page.records
            if page.records:
                after = page.next_after
            elif interval is None:
                return
            else:
                time.sleep(interval)
