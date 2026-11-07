"""Records: `GET /v1/records`, read in `sequence` order page by page (`iterate`) or as they
are appended (`follow`)."""

from __future__ import annotations

import datetime
import time
from collections.abc import Iterator

from .._api.record import list_records
from .._types import UNSET, Unset
from ..models import Record, RecordKind
from ._resource import Resource


class Records(Resource):
    def iterate(
        self,
        *,
        workspace: str | Unset = UNSET,
        actor: str | Unset = UNSET,
        session: str | Unset = UNSET,
        kind: RecordKind | Unset = UNSET,
        start: datetime.datetime | Unset = UNSET,
        end: datetime.datetime | Unset = UNSET,
        after: int = 0,
    ) -> Iterator[Record]:
        """The records after `after` that match the filters, until the last one there is
        now."""
        yield from self._pages(workspace, actor, session, kind, start, end, after, None)

    def follow(
        self,
        *,
        workspace: str | Unset = UNSET,
        actor: str | Unset = UNSET,
        session: str | Unset = UNSET,
        kind: RecordKind | Unset = UNSET,
        start: datetime.datetime | Unset = UNSET,
        after: int = 0,
        interval: float = 1.0,
    ) -> Iterator[Record]:
        """The records after `after` that match the filters, then each one as it is
        appended: it reads again every `interval` seconds once it has read them all, and
        never ends."""
        yield from self._pages(
            workspace, actor, session, kind, start, UNSET, after, interval
        )

    def _pages(
        self,
        workspace: str | Unset,
        actor: str | Unset,
        session: str | Unset,
        kind: RecordKind | Unset,
        start: datetime.datetime | Unset,
        end: datetime.datetime | Unset,
        after: int,
        interval: float | None,
    ) -> Iterator[Record]:
        while True:
            page = list_records.sync(
                client=self._api,
                workspace=workspace,
                actor=actor,
                session=session,
                kind=kind,
                start=start,
                end=end,
                after=after,
            )
            yield from page.records
            if page.records:
                after = page.next_after
            elif interval is None:
                return
            else:
                time.sleep(interval)
