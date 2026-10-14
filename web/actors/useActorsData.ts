// What the Actors page draws from: the records of the range the store names, the clock,
// and the derivations over both. Live, the records are the range reads' answers and the
// clock is the browser's, moved every few seconds so a Live window follows now; in
// fixture mode both come from the scenario and the clock stands still.

import { useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { holdRangeReads } from "../api/events.ts";
import { useShallow } from "zustand/react/shallow";
import { LIVE_REFRESH_INTERVAL, isRangeKey, rangeIdOf, useActorSessions, useDaemonTimeZone, useRangeData } from "../api/queries.ts";
import type { ConsoleReads } from "../console/reads.ts";
import { hourCycle as preferredHourCycle, timeZone as preferredTimeZone, usePreferences } from "../shell/preferences.ts";
import { createModel, followsLive, metricsQueryOf, type ActorsModel } from "./model.ts";
import { consoleSessionsOf, toActorSessions, toActorsInput } from "./snapshot.ts";
import { useActorsStore } from "./store.ts";
import type { ActorsInput, AgentsState, Instant, MenuRefusals } from "./types.ts";

// A scenario's records, the instant its page was observed at, and the timezone of its
// clock, which stands in for the daemon's.
export interface ActorsFixture {
  input: ActorsInput;
  now: Instant;
  timeZone: string;
  // How the page's reads fare (`?state=`); they succeed without one.
  connection: FeldLiveConnection | null;
  // A daemon that refuses the Actions menu's Start, Stop, and Kill with these messages.
  refusals: MenuRefusals | null;
}

export interface ActorsData {
  input: ActorsInput;
  model: ActorsModel;
  connection: FeldLiveConnection;
  refusals: MenuRefusals | null;
  consoleSessions: FeldConsoleSession[];
  // What the card over the Timeline reads (see useSettled).
  inspector: { model: ActorsModel; consoleReads: ConsoleReads };
  now: Instant;
  // Counts the moments the range is read again on the clock or on Refresh.
  pulse: number;
}

const FIXTURE_CONNECTION: FeldLiveConnection = Object.freeze({ state: "live", message: "Live" });

// The range the store's state asks the daemon for. A Live range ends at now, so its query
// has no end and follows the clock; a historical range is read once and stays.
export function rangeQueryOf(state: Pick<AgentsState, "workspace" | "mode" | "anchor" | "duration">, now: Instant): FeldLiveQuery {
  if (state.mode === "live") return { workspace: state.workspace, start: now - state.duration, end: null, duration: state.duration };
  return { workspace: state.workspace, start: state.anchor - state.duration / 2, end: state.anchor + state.duration / 2, duration: state.duration };
}

// How long the Metrics window stays put before it is read: a wheel pans in steps, and
// each step would otherwise read the whole window again.
const METRICS_SETTLE = 400;

// The Metrics window as it is read: held while a pointer gesture pans the range and
// until the window has settled, so a pan reads it once, where it stops.
function useSettledMetricsQuery(query: FeldLiveQuery, gesturing: boolean) {
  const id = rangeIdOf(query);
  const [settled, setSettled] = useState(query);
  const settledId = rangeIdOf(settled);
  useEffect(() => {
    if (gesturing || id === settledId) return undefined;
    const timer = window.setTimeout(() => setSettled(query), METRICS_SETTLE);
    return () => window.clearTimeout(timer);
  }, [id, settledId, gesturing, query]);
  return settled;
}

// What the card over the Timeline reads: while a historical range pans and until it has
// settled, what it read before the pan, so a pan draws the Console once, where it stops. A
// Live range is followed as it moves.
function useSettled<T>(value: T, panned: Instant | null, gesturing: boolean) {
  const [settled, setSettled] = useState(panned);
  const moving = panned !== null && (gesturing || panned !== settled);
  useEffect(() => {
    if (gesturing || panned === settled) return undefined;
    const timer = window.setTimeout(() => setSettled(panned), METRICS_SETTLE);
    return () => window.clearTimeout(timer);
  }, [panned, settled, gesturing]);
  const held = useRef(value);
  useEffect(() => {
    if (!moving) held.current = value;
  }, [moving, value]);
  return moving ? held.current : value;
}

// The Actor whose Sessions are read: the card's, else the selected Session's as the range
// reads it, else the one kept while a selected Session the range lacks is shown.
export function listedActorOf(cardActor: string | null, selected: string | null, selectedActor: string | null, keptActor: string | null) {
  return cardActor || (selected ? selectedActor || keptActor : null);
}

export function useActorsData({ consoleReads, fixture = null }: { consoleReads: ConsoleReads; fixture?: ActorsFixture | null }): ActorsData {
  const range = useActorsStore(useShallow((state) => ({ workspace: state.workspace, mode: state.mode, anchor: state.anchor, duration: state.duration })));
  const metricsDuration = useActorsStore((state) => state.metricsDuration);
  const gesturing = useActorsStore((state) => state.frozenBandIds !== null);
  const refresh = useActorsStore(useShallow((state) => ({ paused: state.paused, count: state.refreshes })));
  const following = useActorsStore(followsLive) && !fixture;

  // Only a Live range that is not paused re-reads on a notification. A paused and a
  // historical range keep a notification to a stale mark, read on Refresh; returning to
  // Live reads what went stale meanwhile.
  const client = useQueryClient();
  const held = !fixture && !following;
  const wasHeld = useRef(held);
  useEffect(() => {
    holdRangeReads(held);
    if (wasHeld.current && !held) client.invalidateQueries({ predicate: (query) => isRangeKey(query.queryKey) });
    wasHeld.current = held;
    return () => holdRangeReads(false);
  }, [client, held]);
  // In Live mode the window ends at now, so the clock moves it every few seconds, and only
  // on Refresh while paused. The reads follow on their own (see useRangeData).
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!following) return undefined;
    const timer = window.setInterval(() => setTick((count) => count + 1), LIVE_REFRESH_INTERVAL);
    return () => window.clearInterval(timer);
  }, [following]);

  // The instant the range is read for. It is taken again on every tick, on Refresh, on
  // pausing and returning to Live, and whenever the range changes, as the page took one
  // instant for every drawing.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const rangeNow = useMemo(() => Date.now(), [tick, refresh.count, refresh.paused, range.workspace, range.mode, range.anchor, range.duration]);
  const query = useMemo(() => rangeQueryOf(range, rangeNow), [range, rangeNow]);
  const metricsQuery = useSettledMetricsQuery(useMemo(() => metricsQueryOf({ ...range, metricsDuration }, rangeNow), [range, metricsDuration, rangeNow]), gesturing && !fixture);
  const showDeleted = useActorsStore((state) => state.showDeletedActors);
  const { data, connection } = useRangeData(query, !fixture, refresh.paused, metricsQuery, showDeleted);

  const rangeInput = useMemo(() => fixture ? fixture.input : toActorsInput(data), [fixture, data]);
  // The Actor card lists its Actor's Sessions whatever the range, and a Session opened from
  // it keeps its Actor's read while its Console shows. A selected Session is read with its
  // Actor's, which measure it over its lifetime.
  const cardActor = useActorsStore((state) => state.actorCard?.agentId ?? null);
  const selected = useActorsStore((state) => state.actorCard?.sessionId ?? state.inspectedSessionId);
  const [keptActor, setKeptActor] = useState<string | null>(null);
  const selectedActor = rangeInput.sessions.find((session) => session.id === selected)?.actor ?? null;
  const listedActor = listedActorOf(cardActor, selected, selectedActor, keptActor);
  if (listedActor !== keptActor) setKeptActor(listedActor);
  const actorRead = rangeInput.actors.find((actor) => actor.id === listedActor);
  const actorSessions = useActorSessions(actorRead ? { workspace: actorRead.workspace, name: actorRead.name, deletedId: actorRead.deletedAt ? actorRead.actorId : undefined } : null, selected, !fixture);
  const input = useMemo(() => (actorSessions && actorRead
    ? { ...rangeInput, actorSessions: toActorSessions(actorSessions, actorRead.id), measuredSession: actorSessions.metrics[0]?.session ?? null }
    : rangeInput), [rangeInput, actorSessions, actorRead]);
  // A new answer is drawn at the instant it arrived, so a Session that just ended and the
  // lines drawn through now agree.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const liveNow = useMemo(() => Date.now(), [rangeNow, data]);
  const now = fixture ? fixture.now : liveNow;
  // Instants are shown in the viewer's zone and hour cycle, read again when Settings changes them.
  const daemonZone = useDaemonTimeZone(!fixture);
  const preferences = usePreferences((state) => state.revision);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const clock = useMemo(() => ({ timeZone: preferredTimeZone(fixture ? fixture.timeZone : daemonZone), hourCycle: preferredHourCycle() }), [fixture, daemonZone, preferences]);
  const model = useMemo(() => createModel(input, { now, ...clock }, consoleReads), [input, now, clock, consoleReads]);
  const consoleSessions = useMemo(() => consoleSessionsOf(input), [input]);
  const inspector = useSettled(useMemo(() => ({ model, consoleReads }), [model, consoleReads]), range.mode === "historical" ? range.anchor : null, gesturing);

  return { input, model, connection: fixture ? fixture.connection || FIXTURE_CONNECTION : connection, refusals: fixture?.refusals || null, consoleSessions, inspector, now, pulse: tick + refresh.count };
}
