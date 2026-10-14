import { useMemo } from "react";
import { QueryClient, keepPreviousData, useQueries, useQuery, useQueryClient, type QueryKey, type UseQueryResult } from "@tanstack/react-query";
import { holdsRead, useEventStream } from "./events.ts";
import { api, isUnauthorized, listActors, readDaemonStatus, readSession } from "./client.ts";
import { bucketSecondsFor, compose, isoOf, subAgentsInRange } from "../lib/range-reads.ts";
import { readSlots } from "../lib/read-slots.ts";

// A Live range re-reads on each notification of the daemon's event stream (see events.ts),
// and on this interval only while the stream is unreachable. A paused Live range and a past
// range are read on Refresh. Everything else is read again when the event stream names its
// resource.
export const LIVE_REFRESH_INTERVAL = 5000;
const FIRST_RETRY_DELAY = 1000;
const MAX_RETRY_DELAY = 15000;
// The browser holds six connections to the daemon and the event stream keeps one. The
// per-Session reads take at most three of the rest, so the range's reads always find a
// free one and keep their interval however slow a Session's read is.
const PER_SESSION_READS = 3;
const perSession = readSlots(PER_SESSION_READS);
// The record kinds the Timeline draws: Spawn arrows and skipped ticks.
const TIMELINE_RECORD_KINDS = ["spawned", "schedule.skipped"];

export function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // A rejected token is not retried; anything else waits out a doubling delay.
        retry: (failureCount, error) => !isUnauthorized(error) && failureCount < 3,
        retryDelay: (attempt) => Math.min(FIRST_RETRY_DELAY * 2 ** attempt, MAX_RETRY_DELAY),
        refetchOnWindowFocus: false,
        // A held range read waits for its period or Refresh when the browser is back online.
        refetchOnReconnect: (query) => !holdsRead(query.queryKey),
        staleTime: Infinity,
      },
    },
  });
}

// A live range follows the clock, so its key leaves the instants out and the interval
// reads it again; a past range is keyed by its own instants.
export function rangeIdOf(query: FeldLiveQuery) {
  const workspace = query.workspace === null ? "*" : query.workspace;
  return query.end === null ? `live|${workspace}|${query.duration}` : `range|${workspace}|${query.start}|${query.end}`;
}

export const keys = {
  workspaces: () => ["workspaces"] as const,
  runtimes: () => ["runtimes"] as const,
  providers: () => ["providers"] as const,
  daemonStatus: () => ["daemon-status"] as const,
  actorsAll: () => ["actors"] as const,
  actors: (workspace: string, includeDeleted = false) => ["actors", workspace, includeDeleted ? "with-deleted" : "live"] as const,
  listingAll: () => ["listing"] as const,
  listing: (range: string) => ["listing", range] as const,
  // An Actor's Sessions over its whole history with their Sub Agent Sessions, which its
  // Actor's notification and those of the Sessions it lists make stale.
  actorListingsAll: () => ["actor-listing"] as const,
  actorListingsOf: (name: string) => ["actor-listing", name] as const,
  actorListing: (actor: { workspace: string; name: string; deletedId?: string }) => ["actor-listing", actor.name, actor.workspace, actor.deletedId || ""] as const,
  session: (id: string) => ["session", id] as const,
  metricsOf: (id: string) => ["metrics", id] as const,
  metrics: (id: string, range: string) => ["metrics", id, range] as const,
  // A Session's metrics over its own lifetime.
  lifetimeMetrics: (parent: string, id: string) => ["metrics", parent, "lifetime", id] as const,
  // A Sub Agent Session's metrics are keyed under its parent's, which its parent's
  // notifications make stale.
  subAgentMetrics: (parent: string, id: string, range: string) => ["metrics", parent, range, id] as const,
  recordsAll: () => ["records"] as const,
  records: (range: string) => ["records", range] as const,
  messagesAll: () => ["messages"] as const,
  // One Message's body, which never changes once accepted.
  message: (id: string) => ["message", id] as const,
  messages: (range: string) => ["messages", range] as const,
  runtimeMetrics: (runtime: string, range: string) => ["runtime-metrics", runtime, range] as const,
};

// The reads the Actors page composes its range from, which a notification or a reconnection
// reads again only on a Live range that is not paused (see events.ts).
const RANGE_ROOTS = new Set(["workspaces", "runtimes", "providers", "daemon-status", "actors", "listing", "actor-listing", "session", "metrics", "records", "messages", "runtime-metrics"]);

export function isRangeKey(queryKey: QueryKey) {
  return RANGE_ROOTS.has(String(queryKey[0]));
}

// The instants a read asks for: a live range ends now, at the moment it is read.
function instantsOf(query: FeldLiveQuery) {
  return query.end === null ? { ...query, start: Date.now() - query.duration } : query;
}

// The range as the daemon's range reads take it, with the Workspace when one is shown.
function rangeParameters(query: FeldLiveQuery, workspace = true) {
  const range = instantsOf(query);
  const parameters = new URLSearchParams();
  if (workspace && range.workspace !== null) parameters.set("workspace", range.workspace);
  parameters.set("start", isoOf(range.start));
  if (range.end !== null) parameters.set("end", isoOf(range.end));
  return parameters;
}

export function useWorkspaces(enabled = true) {
  return useQuery({
    queryKey: keys.workspaces(),
    queryFn: ({ signal }) => api<{ workspaces: FeldApi.Workspace[] }>("/v1/workspaces", { signal }),
    select: (listing) => (listing?.workspaces || []).map((workspace) => workspace.key),
    enabled,
  });
}

// The daemon's time zone, which the "daemon" time zone preference follows; null until read.
export function useDaemonTimeZone(enabled = true) {
  return useQuery({
    queryKey: keys.daemonStatus(),
    queryFn: ({ signal }) => readDaemonStatus(signal),
    select: (answer) => answer?.timezone || null,
    enabled,
  }).data ?? null;
}

// What the page says of its reads in each state; an error carries the failed read's message.
export function connectionOf(state: FeldLiveConnection["state"], failure = ""): FeldLiveConnection {
  switch (state) {
    case "error": return { state, message: "Read failed", detail: failure };
    case "reconnecting": return { state, message: "Reconnecting to live updates" };
    case "connecting": return { state, message: "Connecting…" };
    default: return { state, message: "Live" };
  }
}

// What the Actors page draws from the reads of one range: the live Actors with their
// latest Session, the Sessions the range lists, each one's own read (with its Sub Agent
// Sessions), the metrics of each Session and of its Sub Agent Sessions in the range, the
// Timeline's records, the Messages, and every Runtime's metrics over the Metrics window
// (`metricsQuery`, the range itself when it is not wider). `includeDeleted` lists the
// deleted Actors with the live ones.
// The reads answer independently and the page draws whatever has arrived: the Actors'
// bands first, each Session's Sub Agents and metrics as they come.
export function useRangeData(query: FeldLiveQuery, enabled = true, paused = false, metricsQuery: FeldLiveQuery = query, includeDeleted = false): { data: FeldLiveData | null; connection: FeldLiveConnection } {
  const client = useQueryClient();
  const on = enabled;
  const range = rangeIdOf(query);
  // The Runtime metrics read every Workspace's Sessions, so the Metrics window is keyed
  // without one.
  const metricsRange = rangeIdOf({ ...metricsQuery, workspace: null });
  const stream = useEventStream();
  // A read the daemon refused for the browser token is not read again (see auth.ts).
  const refetchInterval = query.end === null && !paused && !stream.connected
    ? (read: { state: { error: Error | null } }) => (isUnauthorized(read.state.error) ? false : LIVE_REFRESH_INTERVAL)
    : false;
  // A new range keeps drawing the last one's answers until its own arrive.
  const following = { refetchInterval, placeholderData: keepPreviousData, staleTime: 0, enabled: on } as const;

  const workspaces = useWorkspaces(on);
  const shown = query.workspace === null ? workspaces.data || [] : [query.workspace];
  const actors = useQueries({
    queries: shown.map((workspace) => ({
      queryKey: keys.actors(workspace, includeDeleted),
      queryFn: ({ signal }: { signal: AbortSignal }) => listActors(workspace, includeDeleted, signal),
      // Showing or hiding the deleted Actors keeps drawing the other listing until its own arrives.
      placeholderData: () => client.getQueryData<{ actors: FeldApi.Actor[] }>(keys.actors(workspace, !includeDeleted)),
      refetchInterval,
      staleTime: 0,
      enabled: on,
    })),
    combine: answersOf,
  });
  const listing = useQuery({
    queryKey: keys.listing(range),
    queryFn: ({ signal }) => api<{ sessions: FeldApi.Session[] }>(`/v1/sessions?${rangeParameters(query)}`, { signal }),
    ...following,
  });
  const listed = listing.data?.sessions || [];
  // Each Session's own read is newer than the listing's copy, and only its own
  // notifications change it.
  const bodies = useQueries({
    queries: listed.map((session) => ({
      queryKey: keys.session(session.id),
      queryFn: ({ signal }: { signal: AbortSignal }) => perSession(() => readSession(session.id, signal), signal),
      enabled: on,
    })),
    combine: answeredById,
  });
  // Each Session's metrics and its Sub Agent Sessions', over a new range starting from
  // their metrics over the last one.
  const measured = listed.flatMap((session) => {
    const body = bodies.get(session.id);
    const subAgents = body ? subAgentsInRange(query, body.session, body.subAgents || []) : [];
    return [
      { id: session.id, queryKey: keys.metrics(session.id, range), parent: session.id },
      ...subAgents.map((subAgent) => ({ id: subAgent.id, queryKey: keys.subAgentMetrics(session.id, subAgent.id, range), parent: session.id })),
    ];
  });
  const metrics = useQueries({
    queries: measured.map(({ id, queryKey, parent }) => ({
      queryKey,
      queryFn: ({ signal }: { signal: AbortSignal }) => {
        const parameters = rangeParameters(query, false);
        parameters.set("bucket", String(bucketSecondsFor(query.duration)));
        return perSession(() => api<FeldApi.SessionMetrics>(`/v1/sessions/${encodeURIComponent(id)}/metrics?${parameters}`, { signal }), signal);
      },
      placeholderData: () => client.getQueriesData<FeldApi.SessionMetrics>({ queryKey: keys.metricsOf(parent) }).find(([, data]) => data?.session === id)?.[1],
      refetchInterval,
      staleTime: 0,
      enabled: on,
    })),
    combine: answeredById,
  });
  const records = useQuery({
    queryKey: keys.records(range),
    queryFn: async ({ signal }) => {
      const lists = await Promise.all(TIMELINE_RECORD_KINDS.map((kind) => {
        const parameters = rangeParameters(query);
        parameters.set("kind", kind);
        return api<{ records: FeldApi.FeldRecord[] }>(`/v1/records?${parameters}`, { signal });
      }));
      return lists.flatMap((list) => list.records).sort((left, right) => left.sequence - right.sequence);
    },
    ...following,
  });
  const messages = useQuery({
    queryKey: keys.messages(range),
    queryFn: ({ signal }) => api<{ messages: FeldApi.MessageSummary[] }>(`/v1/messages?${rangeParameters(query)}`, { signal }),
    ...following,
  });
  // The installed Runtimes, which the daemon fixes at its start, each read for its metrics
  // and quota over the Metrics window: a window that reset inside it comes back as its
  // series in a row.
  const runtimes = useQuery({
    queryKey: keys.runtimes(),
    queryFn: ({ signal }) => api<{ runtimes: FeldApi.Plugin[] }>("/v1/runtimes", { signal }),
    select: (listing) => (listing?.runtimes || []).map(({ name, label }) => ({ name, label })),
    enabled: on,
  });
  // The installed Providers and the Nodes, fixed at the daemon's start like the Runtimes,
  // are what New actor offers.
  const providers = useQuery({
    queryKey: keys.providers(),
    queryFn: ({ signal }) => api<{ providers: FeldApi.Plugin[] }>("/v1/providers", { signal }),
    select: (listing) => (listing?.providers || []).map(({ name, label }) => ({ name, label })),
    enabled: on,
  });
  const status = useQuery({
    queryKey: keys.daemonStatus(),
    queryFn: ({ signal }) => readDaemonStatus(signal),
    select: (answer) => answer?.nodes || [],
    enabled: on,
  });
  // The Runtime metrics change with every telemetry a Session sends, and a wide window is
  // slow to read, so a Live range reads its window again on a period of a twelfth of its
  // bin (5 seconds for an hour, 7 minutes for 7 days), at least LIVE_REFRESH_INTERVAL,
  // rather than on a notification. A paused or historical range reads it on Refresh, as
  // its other reads.
  const binPeriod = bucketSecondsFor(metricsQuery.duration) * 1000 / 12;
  const metricsPeriod = query.end === null && !paused ? Math.max(LIVE_REFRESH_INTERVAL, binPeriod) : null;
  const runtimeMetrics = useQueries({
    queries: (runtimes.data || []).map(({ name: runtime }) => ({
      queryKey: keys.runtimeMetrics(runtime, metricsRange),
      queryFn: async ({ signal }: { signal: AbortSignal }): Promise<FeldLiveRuntimeMetrics> => {
        const parameters = rangeParameters(metricsQuery, false);
        parameters.set("bucket", String(bucketSecondsFor(metricsQuery.duration)));
        const answer = await api<FeldApi.RuntimeMetrics>(`/v1/runtimes/${encodeURIComponent(runtime)}/metrics?${parameters}`, { signal });
        return { ...answer, runtime };
      },
      ...following,
      refetchInterval: metricsPeriod === null ? false : (read: { state: { error: Error | null } }) => (isUnauthorized(read.state.error) ? false : metricsPeriod),
    })),
    combine: answersOf,
  });

  const core = [actors, answersOf([listing]), answersOf([records]), answersOf([messages]), runtimeMetrics];
  const failed = core.find((result) => result.error);
  const state = failed
    ? "error"
    : stream.reconnecting
      ? "reconnecting"
      : core.some((result) => result.pending) || !shown.length ? "connecting" : "live";
  const failure = failed?.error?.message;
  // The same state is the same connection, so its readers are not drawn again.
  const connection = useMemo(() => connectionOf(state, failure), [state, failure]);

  const actorLists = actors.data;
  const runtimeAnswers = runtimeMetrics.data;
  const data = useMemo(() => {
    if (!actorLists.some(Boolean)) return null;
    const held: FeldRangeHeld = {
      actors: new Map(shown.map((workspace, index) => [workspace, actorLists[index]?.actors || []])),
      sessions: new Map(listed.map((session) => [session.id, bodies.get(session.id)?.session || session])),
      subAgents: new Map(listed.flatMap((session) => {
        const body = bodies.get(session.id);
        return body ? [[session.id, body.subAgents || []]] : [];
      })),
      metrics,
      records: records.data || [],
      messages: messages.data?.messages || [],
      runtimeMetrics: runtimeAnswers.some(Boolean) ? runtimeAnswers.filter((entry): entry is FeldLiveRuntimeMetrics => Boolean(entry)) : null,
      runtimes: runtimes.data || [],
      providers: providers.data || [],
      nodes: status.data || [],
    };
    return compose(query, workspaces.data || [], held);
    // Every input is an answer the query cache shares structurally, so an unchanged read
    // keeps its identity and the snapshot is composed again only when one changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query.workspace, query.start, query.end, workspaces.data, listing.data, bodies, metrics, records.data, messages.data, actorLists, runtimeAnswers, runtimes.data, providers.data, status.data]);
  return { data, connection };
}

// A deleted Actor's Sessions, read by its `id` since a name names the live Actor only. Its
// read lists the Sessions themselves, and each Session's own read its Sub Agent Sessions.
export async function readDeletedActorSessions(actorId: string, signal: AbortSignal) {
  const { actor } = await api<{ actor: FeldApi.Actor }>(`/v1/actors/${encodeURIComponent(actorId)}`, { signal });
  const reads = await Promise.all(actor.sessions.map((session) => perSession(() => readSession(session.id, signal), signal)));
  return { sessions: actor.sessions, subAgents: reads.flatMap((read) => read.subAgents || []) };
}

// The Sessions of one Actor whatever the range, with their Sub Agent Sessions, which the
// Actor card lists; a deleted Actor's are read by its `id` (`deletedId`). The selected one
// is measured over its lifetime. The reads follow notifications as the range's do.
export function useActorSessions(actor: { workspace: string; name: string; deletedId?: string } | null, selected: string | null, enabled = true): FeldActorSessions | null {
  const on = enabled && actor !== null;
  const listing = useQuery({
    queryKey: keys.actorListing(actor || { workspace: "", name: "" }),
    queryFn: ({ signal }): Promise<{ sessions: FeldApi.Session[]; subAgents?: FeldApi.SubAgentSession[] }> => actor!.deletedId
      ? readDeletedActorSessions(actor!.deletedId, signal)
      : api(`/v1/sessions?${new URLSearchParams({ workspace: actor!.workspace, actor: actor!.name, subAgents: "true" })}`, { signal }),
    staleTime: 0,
    enabled: on,
  });
  const sessions = useMemo(() => (listing.data ? [...listing.data.sessions, ...(listing.data.subAgents || [])] : null), [listing.data]);
  const measured = sessions?.find((session) => session.id === selected) || null;
  const parent = measured && "parentSession" in measured ? measured.parentSession : measured?.id || "";
  const metrics = useQuery({
    queryKey: keys.lifetimeMetrics(parent, measured?.id || ""),
    queryFn: ({ signal }) => {
      const start = Date.parse(measured!.start!);
      const end = measured!.end === null ? Date.now() : Date.parse(measured!.end);
      const parameters = new URLSearchParams({ start: isoOf(start), bucket: String(bucketSecondsFor(end - start)) });
      if (measured!.end !== null) parameters.set("end", measured!.end);
      return perSession(() => api<FeldApi.SessionMetrics>(`/v1/sessions/${encodeURIComponent(measured!.id)}/metrics?${parameters}`, { signal }), signal);
    },
    staleTime: 0,
    enabled: on && Boolean(measured?.start),
  });
  return useMemo(() => {
    if (!sessions) return null;
    return { sessions, metrics: measured && metrics.data?.session === measured.id ? [metrics.data] : [] };
  }, [sessions, measured, metrics.data]);
}

// The answers of reads made side by side, with the first failure among them and whether
// any has yet to answer. The combined value keeps its identity while no answer changes.
function answersOf<T>(results: UseQueryResult<T>[]) {
  return {
    data: results.map((result) => result.data),
    error: results.find((result) => result.error)?.error || null,
    pending: results.some((result) => result.data === undefined),
  };
}

// The answers of per-Session reads by Session id, in the listing's order. The combined
// Map keeps its identity while no answer changes.
function answeredById<T extends { session: { id: string } } | { session: string }>(results: UseQueryResult<T>[]) {
  const answered = new Map<string, T>();
  for (const result of results) {
    if (!result.data) continue;
    const session = result.data.session;
    answered.set(typeof session === "string" ? session : session.id, result.data);
  }
  return answered;
}
