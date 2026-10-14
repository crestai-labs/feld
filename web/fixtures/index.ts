// Fixture mode: under the dev server (`npm run dev`), `?scenario=<name>` draws the fixture
// instead of the daemon's reads. The data and its conversion are imported only when `import.meta.env.DEV`,
// so a production build carries neither.

import { useCallback, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate, useSearchParams } from "react-router";
import type { ActorsInput } from "../actors/types.ts";
import type { DaemonRead, DaemonSource } from "../daemon/source.ts";
import { showPairing } from "../api/auth.ts";
import { ACTORS_STATES, AUTH_STATES, DEFAULT_SCENARIO, daemonStateOf, type ConsoleFixture, type FixtureActorsState, type FixtureAuthState, type FixtureDaemonState, type FixtureRange } from "./convert.ts";
import type { MockRefusals } from "./mock.ts";

export type { ConsoleFixture, FixtureActorsState, FixtureAuthState, FixtureDaemonState, FixtureRange, MockRefusals };

// The instant the fixture was observed at, which the page takes as now, and the timezone
// the fixture shows instants in.
export interface FixtureClock {
  now: number;
  timeZone: string;
}

export interface Fixture {
  scenario: string;
  clock: FixtureClock;
  // The Actors page's range when it opens.
  range: FixtureRange;
  // Whether the Jobs keep their default pin when the page opens.
  pinJobs: boolean;
  actors: ActorsInput;
  console: ConsoleFixture;
  daemon(state: FixtureDaemonState): DaemonRead;
  // The daemon's message of a failed read, and of each request it refuses.
  readFailure: string;
  refusals: MockRefusals;
  // POST /v1/ui/pair against the fixture's pairing codes: whether the code was taken.
  pair(code: string): boolean;
}

export type FixtureState =
  | { status: "loading" }
  | { status: "error"; error: Error }
  | { status: "ready"; fixture: Fixture };

export const FIXTURE_KEY = "fixture";

// The scenario the URL asks for, or null outside fixture mode. An unknown name opens the
// default scenario.
export function fixtureScenarioOf(search: URLSearchParams): string | null {
  if (!import.meta.env.DEV) return null;
  return search.get("scenario");
}

// The Actors page's `?state=` in fixture mode; null when
// the reads succeed and the daemon accepts every request.
export function fixtureActorsStateOf(search: URLSearchParams): FixtureActorsState | null {
  const value = search.get("state");
  return ACTORS_STATES.find((state) => state === value) || null;
}

// The Settings page is in fixture mode with `?scenario=` or `?state=`; a missing or unknown state is `running`.
export function fixtureDaemonStateOf(search: URLSearchParams): FixtureDaemonState | null {
  if (!import.meta.env.DEV || !(search.has("scenario") || search.has("state"))) return null;
  return daemonStateOf(search.get("state"));
}

// Whether the page is in fixture mode at all: the pairing screen then trades the fixture's
// codes instead of the daemon's.
export function fixtureModeOf(search: URLSearchParams) {
  return import.meta.env.DEV && (search.has("scenario") || search.has("state"));
}

// The `?state=` that puts the pairing screen in place of any page; null outside
// fixture mode.
export function fixtureAuthStateOf(search: URLSearchParams): FixtureAuthState | null {
  if (!import.meta.env.DEV) return null;
  const value = search.get("state");
  return AUTH_STATES.find((state) => state === value) || null;
}

// How long the fixture's daemon takes to answer POST /v1/ui/pair.
const PAIRING_ANSWER_DELAY = 600;

// The pairing screen's request in fixture mode, against the fixture's codes; null outside it.
// One fixture daemon holds the codes for every scenario until the page loads again.
export function fixturePairingOf(search: URLSearchParams): ((code: string) => Promise<boolean>) | null {
  if (!fixtureModeOf(search)) return null;
  const scenario = search.get("scenario") || DEFAULT_SCENARIO;
  return async (code) => {
    await new Promise((resolve) => window.setTimeout(resolve, PAIRING_ANSWER_DELAY));
    if (!import.meta.env.DEV) return false;
    const { loadFixture } = await import("./load.ts");
    return loadFixture(scenario).pair(code);
  };
}

// The scenario's fixture, or null outside fixture mode (the scenario is null).
export function useFixture(scenario: string | null): FixtureState | null {
  const enabled = import.meta.env.DEV && scenario !== null;
  const query = useQuery({
    queryKey: [FIXTURE_KEY, scenario],
    queryFn: () => (import.meta.env.DEV
      ? import("./load.ts").then(({ loadFixture }) => loadFixture(scenario!))
      : Promise.reject(new Error("Fixture mode runs only under the dev server"))),
    enabled,
    staleTime: Infinity,
    gcTime: Infinity,
    retry: false,
    // The fixture is read once and answered as the same objects; comparing its samples
    // deeply on every answer would only cost time.
    structuralSharing: false,
  });
  const { data, error } = query;
  return useMemo((): FixtureState | null => {
    if (!enabled) return null;
    if (data) return { status: "ready", fixture: data };
    if (error) return { status: "error", error };
    return { status: "loading" };
  }, [enabled, data, error]);
}

const READING: DaemonRead = Object.freeze({
  phase: "reading",
  status: null,
  lastRead: null,
  workspaces: null,
  defaultWorkspace: "default",
  actionError: null,
  restartedAt: null,
  now: 0,
});

// The fixture's daemon forgets this browser at once.
async function removeDevice() {
  showPairing("removed");
}

// The Settings page's daemon in fixture mode, or null outside it. A
// restart comes back running with its uptime started over, a stop leaves the daemon
// unreachable, a refusing daemon refuses both, and a retry finds it running again; the
// state is kept in `?state=`.
export function useFixtureDaemonSource(): DaemonSource | null {
  const [search] = useSearchParams();
  const navigate = useNavigate();
  const state = fixtureDaemonStateOf(search);
  const fixture = useFixture(state === null ? null : search.get("scenario") || DEFAULT_SCENARIO);
  const [restartedAt, setRestartedAt] = useState<number | null>(null);
  const [actionError, setActionError] = useState<DaemonRead["actionError"]>(null);
  const ready = fixture?.status === "ready" ? fixture.fixture : null;
  const now = ready ? ready.clock.now : 0;
  // The page keeps the section its hash names (the page sets the hash itself).
  const moveTo = useCallback((next: FixtureDaemonState) => {
    const updated = new URLSearchParams(location.search);
    updated.set("state", next);
    setActionError(null);
    navigate({ search: `?${updated}`, hash: location.hash }, { replace: true });
  }, [navigate]);
  const refusing = state === "refusing" && ready !== null;
  const retry = useCallback(() => moveTo("running"), [moveTo]);
  const restart = useCallback(async () => {
    if (refusing) setActionError({ action: "restart", message: ready.refusals.restart });
    else setRestartedAt(now);
  }, [refusing, ready, now]);
  const stop = useCallback(async () => {
    if (refusing) {
      setActionError({ action: "stop", message: ready.refusals.shutdown });
      return;
    }
    setRestartedAt(null);
    moveTo("unreachable");
  }, [refusing, ready, moveTo]);
  return useMemo(() => {
    if (state === null || !fixture) return null;
    const read = fixture.status === "ready" ? fixture.fixture.daemon(state) : READING;
    const restarted = read.phase === "reachable" ? restartedAt : null;
    // A restart starts the process again now.
    const status = restarted !== null && read.status ? { ...read.status, startedAt: new Date(restarted).toISOString() } : read.status;
    return { ...read, status, restartedAt: restarted, actionError, retry, restart, stop, removeDevice };
  }, [state, fixture, restartedAt, actionError, retry, restart, stop]);
}
