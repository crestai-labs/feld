import { createContext, useContext, useEffect, useMemo, useRef, useState } from "react";
import { InfiniteQueryObserver, infiniteQueryOptions, replaceEqualDeep, useInfiniteQuery, useQuery, useQueryClient, type InfiniteData, type QueryClient } from "@tanstack/react-query";
import { readOutput, readSessionRecords, readTranscript } from "../api/client.ts";
import { subscribeDaemonEvents } from "../api/events.ts";
import {
  consoleKindOf,
  consoleSessionOf,
  lastLogLineIn,
  lastRuntimeRecordIn,
  logFromFixture,
  logFromRead,
  nextAfterOf,
  readOf,
  recordingFromFixture,
  recordingFromRead,
  runtimeRecordIn,
  withoutEmptyTail,
  type ConsoleKind,
  type ConsolePage,
  type ConsoleRead,
} from "./model.ts";
import type { ConsoleReads } from "./reads.ts";

// ---- The Session's own reads ---------------------------------------------------------
// The transcript and the program output are appended columns, too long to hold whole, so
// the Console pages them: the newest page when it opens, the page before when the reader
// scrolls to the top, and whatever was appended when the Session is still running. Each
// Session's pages are one infinite query, which the open Console and the Actor line share.
const PAGE_LIMIT = 200;
// How often a running Session's tail is read again, and its records with it.
const TAIL_INTERVAL = 1000;
const RECORDS_INTERVAL = 2000;
// After a read that failed the daemon is left alone for longer, so a Console that cannot
// be read does not ask once a second for as long as it is open.
const RETRY_INTERVAL = 5000;

export const consoleKeys = {
  all: () => ["console"] as const,
  pages: (kind: ConsoleKind, sessionId: string) => ["console", kind, sessionId] as const,
  // Under `records`, so a record notification (events.ts) reads them again.
  records: (sessionId: string) => ["records", "session", sessionId] as const,
};

type Pages = InfiniteData<ConsolePage, FeldConsoleCursor>;
type PagesKey = ReturnType<typeof consoleKeys.pages>;

// The pages are read only through their cursors: the first read, `before` at the top, and
// `after` behind the tail. A refetch replays every page from the first cursor, which for a
// column that grew is neither the page held nor the newest one, so the query is disabled:
// an invalidation (a reconnected event stream reads everything again) passes it over, and
// the reads below ask for each page themselves.
export function consolePagesOptions(kind: ConsoleKind, sessionId: string) {
  return infiniteQueryOptions<ConsolePage, Error, Pages, PagesKey, FeldConsoleCursor>({
    queryKey: consoleKeys.pages(kind, sessionId),
    queryFn: ({ pageParam, signal }): Promise<ConsolePage> => kind === "transcript"
      ? readTranscript(sessionId, pageParam, signal)
      : readOutput(sessionId, pageParam, signal),
    initialPageParam: { limit: PAGE_LIMIT },
    getNextPageParam: (_last, pages, _cursor, cursors): FeldConsoleCursor | undefined => {
      const after = nextAfterOf({ pages, pageParams: cursors });
      return after === null ? undefined : { after, limit: PAGE_LIMIT };
    },
    getPreviousPageParam: (first): FeldConsoleCursor | undefined => first.hasOlder && first.previousBefore !== null && first.previousBefore !== undefined
      ? { before: first.previousBefore, limit: PAGE_LIMIT }
      : undefined,
    enabled: false,
    // An empty page behind the tail is not held, and a read that found nothing new keeps
    // the data the Console drew, so a tick of a quiet Session changes nothing.
    structuralSharing: (held, read) => replaceEqualDeep(held, withoutEmptyTail(read as Pages)),
    // A failed read is not asked again at once: a running Session's is, after RETRY_INTERVAL.
    retry: false,
    // What was read of a Session stays for the page's lifetime: the Actors page quotes it
    // after its Console closed.
    gcTime: Infinity,
  });
}

// ---- Holding a Session's column ---------------------------------------------------------
// Whoever shows a Session's column holds it: the open Console, and the Actor line of a
// current Session. A held column is read once; a live one is followed while anyone holds
// it live. Held by count so two holders of one Session read it once.
interface Holding {
  holders: number;
  live: number;
  observer: InfiniteQueryObserver<ConsolePage, Error, Pages, PagesKey, FeldConsoleCursor>;
  release: () => void;
  stopFollowing: (() => void) | null;
}

const holdings = new Map<string, Holding>();

function readFirst(holding: Holding) {
  const query = holding.observer.getCurrentQuery();
  if (query.state.data === undefined && query.state.fetchStatus === "idle") holding.observer.refetch().catch(() => {});
}

// One step of following the tail: whatever was appended since the last page, or — while
// nothing is held — the newest page again, because a Session whose transcript file only
// now appeared may already be longer than one page. A read in flight (the first page, or
// the page before) is let finish instead.
async function followStep(holding: Holding) {
  const query = holding.observer.getCurrentQuery();
  if (query.state.fetchStatus !== "idle") return;
  const read = readOf(query.state.data, query.state.error);
  if (read.rows.length && read.nextAfter !== null) {
    await holding.observer.fetchNextPage({ cancelRefetch: false });
  } else {
    await holding.observer.refetch({ cancelRefetch: false });
  }
}

// A running Session's tail is read every TAIL_INTERVAL. The daemon's `log` notification
// names a Session whose column grew, and reads it at once; it is not enough on its own —
// it comes when the Runtime's hook arrives rather than with every entry, and names the
// parent rather than a Sub Agent Session — so the interval stays.
function follow(sessionId: string, holding: Holding) {
  let timer: number | null = null;
  let stopped = false;
  let stepping = false;
  async function step() {
    timer = null;
    if (stepping || stopped) return;
    stepping = true;
    await followStep(holding).catch(() => {});
    stepping = false;
    if (stopped) return;
    const failed = holding.observer.getCurrentQuery().state.status === "error";
    timer = window.setTimeout(step, failed ? RETRY_INTERVAL : TAIL_INTERVAL);
  }
  timer = window.setTimeout(step, TAIL_INTERVAL);
  const unsubscribe = subscribeDaemonEvents((event) => {
    if (event.resource !== "log" || event.id !== sessionId || stepping || timer === null) return;
    window.clearTimeout(timer);
    step();
  });
  return () => {
    stopped = true;
    if (timer !== null) window.clearTimeout(timer);
    unsubscribe();
  };
}

// Holds one Session's column until the returned release is called; the hooks below hold
// the columns they show.
export function holdConsole(client: QueryClient, kind: ConsoleKind, sessionId: string, live: boolean) {
  const id = `${kind}|${sessionId}`;
  let holding = holdings.get(id);
  if (!holding) {
    const observer = new InfiniteQueryObserver(client, consolePagesOptions(kind, sessionId));
    holding = { holders: 0, live: 0, observer, release: observer.subscribe(() => {}), stopFollowing: null };
    holdings.set(id, holding);
  }
  const held = holding;
  held.holders += 1;
  if (live) held.live += 1;
  readFirst(held);
  if (held.live && !held.stopFollowing) held.stopFollowing = follow(sessionId, held);
  return () => {
    held.holders -= 1;
    if (live) held.live -= 1;
    if (!held.live && held.stopFollowing) {
      held.stopFollowing();
      held.stopFollowing = null;
    }
    if (!held.holders) {
      held.release();
      holdings.delete(id);
    }
  };
}

// Reads once whatever was appended to the held columns of these Sessions that nobody
// follows: the Actor lines' reads on Refresh while the page does not follow Live.
export function readTails(client: QueryClient, sessions: readonly FeldConsoleSession[]) {
  for (const session of sessions) {
    const kind = consoleKindOf(session);
    const holding = holdings.get(`${kind}|${session.id}`);
    if (holding && !holding.stopFollowing) followStep(holding).catch(() => {});
  }
}

// ---- Fixture mode ---------------------------------------------------------------------------
// A fixture page reads no daemon: its recordings and program logs are the fixture's, keyed
// by Session id (a fixture Session may name another's recording).
export interface ConsoleFixture {
  recordings: Readonly<Record<string, FeldConsoleRecording>>;
  logs: Readonly<Record<string, FeldConsoleLog>>;
}

export const ConsoleFixtureContext = createContext<ConsoleFixture | null>(null);

// ---- The data one Console draws ----------------------------------------------------------

export interface ConsoleData {
  // The Session with what its own read found about its transcript file.
  session: FeldConsoleSession;
  recording: FeldConsoleRecording | null;
  log: FeldConsoleLog | null;
  hasOlder: boolean;
  loadingOlder: boolean;
  // The read of the page before the first page held failed; it is read again only when
  // asked for.
  olderFailed: boolean;
  loadOlder: () => void;
  // The cursor of the first page held; it changes when a page is added in front.
  head: FeldConsoleCursor | undefined;
  // Whether the first read has answered, an empty column included.
  loaded: boolean;
}

const NO_OLDER = () => {};

// Everything the Console of one Session draws, from the daemon or from the fixture. The
// open Console reads the Session's records as well, and follows a live one's.
export function useConsoleData(session: FeldConsoleSession, live: boolean, sessionStart: number | null): ConsoleData {
  const fixture = useContext(ConsoleFixtureContext);
  const client = useQueryClient();
  const kind = consoleKindOf(session);
  const reading = !fixture;
  const pages = useInfiniteQuery(consolePagesOptions(kind, session.id));
  useQuery({
    queryKey: consoleKeys.records(session.id),
    queryFn: ({ signal }) => readSessionRecords(session.id, signal),
    refetchInterval: live ? RECORDS_INTERVAL : false,
    gcTime: Infinity,
    enabled: reading,
  });
  useEffect(() => {
    if (!reading) return;
    return holdConsole(client, kind, session.id, live);
  }, [client, kind, session.id, live, reading]);

  const { data, error, fetchPreviousPage, hasPreviousPage, isFetchingPreviousPage, isFetchPreviousPageError, isFetching } = pages;
  const read = useMemo(() => (reading ? readOf(data, error) : null), [reading, data, error]);
  // The column is made again only when a read brings something: the fetching states in
  // between leave it as it was drawn.
  const known = useMemo(() => fixture ? session : consoleSessionOf(session, read), [fixture, session, read]);
  const recording = useMemo(() => fixture ? recordingFromFixture(session, fixture.recordings) : recordingFromRead(known, read), [fixture, session, known, read]);
  const log = useMemo(() => fixture ? logFromFixture(session, fixture.logs) : logFromRead(known, read, sessionStart), [fixture, session, known, read, sessionStart]);
  // The first page held when the page before it failed to read. The query's own error
  // clears at the next read of the tail, which leaves that page as unread as before.
  const head = data?.pageParams[0];
  const failedHead = useRef<FeldConsoleCursor | undefined>(undefined);
  if (isFetchPreviousPageError) failedHead.current = head;
  const olderFailed = head !== undefined && failedHead.current === head;
  return useMemo(() => {
    if (fixture) {
      return { session, recording, log, hasOlder: Boolean(session.hasOlder), loadingOlder: false, olderFailed: false, loadOlder: NO_OLDER, head: undefined, loaded: true };
    }
    return {
      session: known,
      recording,
      log,
      hasOlder: Boolean(read?.hasOlder),
      loadingOlder: isFetchingPreviousPage,
      olderFailed,
      // The page before is read only when no other read of this Session is in flight.
      loadOlder: () => {
        if (hasPreviousPage && !isFetching) fetchPreviousPage({ cancelRefetch: false });
      },
      head,
      loaded: Boolean(read?.loaded),
    };
  }, [fixture, session, known, recording, log, read, isFetchingPreviousPage, olderFailed, hasPreviousPage, isFetching, fetchPreviousPage, head]);
}

// ---- What the Actors page reads -----------------------------------------------------------

// A Session the Actors page knows. `read` marks one whose column it reads itself (a
// current Session's Actor line states its turn and last record from it); `live` follows it
// as the open Console does, which the page asks for only on a Live range that is not paused.
export interface ConsoleSubject {
  session: FeldConsoleSession;
  live: boolean;
  read?: boolean;
}

// Every settled read of a Console column or of a Session's records asks the reader to
// derive again; the fetching states in between do not.
function useConsoleCacheVersion(client: QueryClient, enabled: boolean) {
  const [version, setVersion] = useState(0);
  useEffect(() => {
    if (!enabled) return;
    return client.getQueryCache().subscribe((event) => {
      if (event.type !== "updated" || (event.action.type !== "success" && event.action.type !== "error")) return;
      const [root, second] = event.query.queryKey as readonly unknown[];
      if (root === "console" || (root === "records" && second === "session")) setVersion((value) => value + 1);
    });
  }, [client, enabled]);
  return version;
}

// What the Actors page reads from the Consoles' reads, answered from the query cache; the
// caller renders again whenever one of them settles. The subjects marked `read` are read
// (and followed while live) for as long as the caller is mounted.
export function useConsoleReads(subjects: readonly ConsoleSubject[]): ConsoleReads {
  const fixture = useContext(ConsoleFixtureContext);
  const client = useQueryClient();
  const reading = !fixture;
  const version = useConsoleCacheVersion(client, reading);

  const held = subjects.filter((subject) => subject.read).map((subject) => `${consoleKindOf(subject.session)}|${subject.live ? 1 : 0}|${subject.session.id}`).sort().join("\n");
  useEffect(() => {
    if (!reading || !held) return;
    const releases = held.split("\n").map((line) => {
      const [kind, live, ...id] = line.split("|");
      return holdConsole(client, kind as ConsoleKind, id.join("|"), live === "1");
    });
    return () => releases.forEach((release) => release());
  }, [client, reading, held]);

  const sessions = useMemo(() => new Map(subjects.map((subject) => [subject.session.id, subject.session])), [subjects]);
  return useMemo(() => {
    const reads = new Map<string, ConsoleRead | null>();
    function readFor(kind: ConsoleKind, sessionId: string) {
      const key = `${kind}|${sessionId}`;
      if (!reads.has(key)) {
        const query = client.getQueryCache().find<ConsolePage, Error, Pages>({ queryKey: consoleKeys.pages(kind, sessionId), exact: true });
        reads.set(key, query ? readOf(query.state.data, query.state.error) : null);
      }
      return reads.get(key)!;
    }
    function recordingFor(sessionId: string) {
      const session = sessions.get(sessionId);
      if (fixture) return session ? recordingFromFixture(session, fixture.recordings) : null;
      const read = readFor("transcript", sessionId);
      return recordingFromRead(consoleSessionOf(session || { id: sessionId, actor: "" }, read), read);
    }
    function logFor(sessionId: string, sessionStart: number) {
      const session = sessions.get(sessionId);
      if (fixture) return session ? logFromFixture(session, fixture.logs) : null;
      const read = readFor("output", sessionId);
      return logFromRead(consoleSessionOf(session || { id: sessionId, actor: "", console: "log" }, read), read, sessionStart);
    }
    return {
      recordingFor,
      runtimeRecordFor: (sessionId, anchor, timeOrigin) => runtimeRecordIn(recordingFor(sessionId), anchor, timeOrigin),
      lastRuntimeRecordFor: (sessionId, timeOrigin) => lastRuntimeRecordIn(recordingFor(sessionId), timeOrigin),
      lastLogLineFor: (sessionId, sessionStart) => lastLogLineIn(logFor(sessionId, sessionStart), sessionStart),
      feldRecordsFor: (sessionId) => fixture ? null : client.getQueryData<{ records: FeldApi.FeldRecord[] }>(consoleKeys.records(sessionId))?.records || null,
      consoleSessionFor: (session) => fixture ? session : consoleSessionOf(session, readFor(consoleKindOf(session), session.id)),
    };
    // `version` stands for the query cache's content, which the closures read.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client, fixture, sessions, version]);
}
