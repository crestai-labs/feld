// Reads the fixture (data/*.js) and converts it.
// Only the dev server loads this module (see index.ts): the data scripts are classic
// scripts that register on `window`, so each one runs here against a window of its own
// instead of the page's.

import { DEFAULT_SCENARIO, actorsInputOf, consoleFixtureOf, daemonReadOf, pairingOf, pinsJobsOf, rangeOf, type LaneDomain } from "./convert.ts";
import type { Fixture } from "./index.ts";
import type { MockData } from "./mock.ts";

const DAY = 24 * 60 * 60 * 1000;
// The lanes are sampled once a minute over the week before the fixture's now, the widest
// range the page reads.
const LANE_HISTORY = 7 * DAY;
const LANE_STEP = 60 * 1000;

const sources = import.meta.glob<string>("./data/*.js", { query: "?raw", import: "default", eager: true });

// The clock goes first: the other fixtures read their instants from it.
function loadOrder(path: string) {
  return path.endsWith("/clock.js") ? 0 : 1;
}

export function evaluateDataScripts(scripts: Record<string, string>): MockData {
  const window: { FeldWebUI?: { data: MockData } } = {};
  const paths = Object.keys(scripts).sort((left, right) => loadOrder(left) - loadOrder(right) || left.localeCompare(right));
  for (const path of paths) new Function("window", scripts[path])(window);
  if (!window.FeldWebUI?.data) throw new Error("The data scripts registered no fixture");
  return window.FeldWebUI.data;
}

export type LoadedFixture = Omit<Fixture, "scenario" | "range" | "pinJobs">;

export function convertFixture(data: MockData): LoadedFixture & { scenarios: MockData["actors"]["scenarios"] } {
  const { clock } = data;
  const domain: LaneDomain = { start: clock.now - LANE_HISTORY, end: clock.now, step: LANE_STEP };
  const console = consoleFixtureOf(data.consoleSessions, data.sessionRecordings, data.sessionLogs);
  return {
    clock: { now: clock.now, timeZone: clock.timeZone },
    scenarios: data.actors.scenarios,
    actors: actorsInputOf(data.actors, clock, domain, console),
    console,
    daemon: (state) => daemonReadOf(data.daemon, data.actors, clock, state),
    readFailure: data.daemon.readFailure,
    refusals: data.daemon.refusals,
    pair: pairingOf(data.daemon.pairing),
  };
}

let loaded: ReturnType<typeof convertFixture> | null = null;
const byScenario = new Map<string, Fixture>();

// The fixture of one scenario. The data is read and converted once; a scenario only picks
// the range, and asking again answers the same objects.
export function loadFixture(scenario: string): Fixture {
  loaded ||= convertFixture(evaluateDataScripts(sources));
  const { scenarios, ...fixture } = loaded;
  const name = scenarios[scenario] ? scenario : DEFAULT_SCENARIO;
  if (!byScenario.has(name)) byScenario.set(name, { ...fixture, scenario: name, range: rangeOf(scenarios, name), pinJobs: pinsJobsOf(scenarios, name) });
  return byScenario.get(name)!;
}
