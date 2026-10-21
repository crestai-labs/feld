// The Actors page: the daemon's range reads, or under the dev server a fixture scenario
// (`?scenario=`), drawn by ActorsView.

import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useSearchParams } from "react-router";
import { ConsoleFixtureContext, readTails, useConsoleReads, type ConsoleSubject } from "../console/data.ts";
import { connectionOf } from "../api/queries.ts";
import { fixtureActorsStateOf, fixtureScenarioOf, useFixture, type Fixture } from "../fixtures/index.ts";
import { workspaceOfSearch } from "../shell/workspace-url.ts";
import { ActorsView } from "./ActorsView.tsx";
import { COMPACT_LAYOUT_QUERY, followsLive, selectedSessionId } from "./model.ts";
import { useActorsStore } from "./store.ts";
import { useActorsData, type ActorsFixture } from "./useActorsData.ts";

export function ActorsPage() {
  const [search] = useSearchParams();
  const scenario = fixtureScenarioOf(search);
  const fixture = useFixture(scenario);
  if (scenario === null) return <Fresh><LiveActors /></Fresh>;
  if (fixture?.status !== "ready") return <feld-agents></feld-agents>;
  return <Fresh fixture={fixture.fixture}><FixtureActors fixture={fixture.fixture} /></Fresh>;
}

// Every visit opens a fresh page, as a first visit does: the store is a module's, so it is
// set back before anything is drawn from it. A fixture opens on its scenario's range, at
// the instant it was observed.
function Fresh({ fixture = null, children }: { fixture?: Fixture | null; children: ReactNode }) {
  const [search] = useSearchParams();
  const [ready, setReady] = useState(false);
  useLayoutEffect(() => {
    const store = useActorsStore.getState();
    store.reset(workspaceOfSearch(search), fixture ? fixture.clock.now : Date.now());
    if (fixture) {
      const { mode, anchor, duration } = fixture.range;
      useActorsStore.setState({ mode, anchor, duration });
      if (!fixture.pinJobs) {
        useActorsStore.setState({ actorPins: Object.fromEntries(fixture.actors.actors.filter((actor) => actor.kind === "job").map((actor) => [actor.id, false])) });
      }
    }
    setReady(true);
    // Only a new page is set back; the URL moves the store from here on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fixture]);
  // The lanes follow the viewport across the compact breakpoint.
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return undefined;
    const query = window.matchMedia(COMPACT_LAYOUT_QUERY);
    const follow = () => useActorsStore.getState().setCompactLayout(query.matches);
    follow();
    query.addEventListener("change", follow);
    return () => query.removeEventListener("change", follow);
  }, []);
  return ready ? children : <feld-agents></feld-agents>;
}

function LiveActors() {
  const client = useQueryClient();
  // The Sessions the Console reads are the snapshot's, which is read with those reads in
  // hand; so they follow the snapshot one drawing behind. A current Session is read for its
  // Actor line's turn, and the selected one (in the Console or the Actor card) for its Now
  // section. A current Session's tail is followed on a Live range that is not paused, and
  // otherwise read with the range on Refresh.
  const [subjects, setSubjects] = useState<ConsoleSubject[]>([]);
  const consoleReads = useConsoleReads(subjects);
  const data = useActorsData({ consoleReads });
  const inspected = useActorsStore(selectedSessionId);
  const following = useActorsStore(followsLive);
  const current = useMemo(() => {
    const ids = new Set(data.input.sessions.filter((session) => session.end === null).map((session) => session.id));
    return data.consoleSessions.filter((session) => ids.has(session.id));
  }, [data.input.sessions, data.consoleSessions]);
  const next = useMemo(() => {
    const ids = new Set(current.map((session) => session.id));
    return data.consoleSessions.map((session): ConsoleSubject => ({ session, live: following && ids.has(session.id), read: ids.has(session.id) || session.id === inspected }));
  }, [current, data.consoleSessions, inspected, following]);
  useEffect(() => setSubjects(next), [next]);
  const tails = useRef({ following, current });
  tails.current = { following, current };
  useEffect(() => {
    if (!tails.current.following) readTails(client, tails.current.current);
  }, [client, data.pulse]);
  return <ActorsView data={data} client={client} />;
}

function FixtureActors({ fixture }: { fixture: Fixture }) {
  const consoleFixture = useMemo(() => ({
    recordings: Object.fromEntries(fixture.console.recordings),
    logs: Object.fromEntries(fixture.console.logs),
  }), [fixture]);
  return (
    <ConsoleFixtureContext value={consoleFixture}>
      <FixtureBody fixture={fixture} />
    </ConsoleFixtureContext>
  );
}

// A fixture reads nothing, so every Session is the Console's to answer from at once.
function FixtureBody({ fixture }: { fixture: Fixture }) {
  const subjects = useMemo(() => fixture.console.sessions.map((session): ConsoleSubject => ({ session, live: false })), [fixture]);
  const consoleReads = useConsoleReads(subjects);
  const [search] = useSearchParams();
  const state = fixtureActorsStateOf(search);
  const actorsFixture = useMemo<ActorsFixture>(() => ({
    input: fixture.actors,
    now: fixture.clock.now,
    timeZone: fixture.clock.timeZone,
    connection: state === null || state === "refusing" ? null : connectionOf(state, fixture.readFailure),
    refusals: state === "refusing" ? fixture.refusals : null,
  }), [fixture, state]);
  const data = useActorsData({ consoleReads, fixture: actorsFixture });
  return <ActorsView data={data} client={null} />;
}
