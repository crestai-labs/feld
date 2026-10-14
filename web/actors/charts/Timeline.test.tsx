import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import type { ConsoleReads } from "../../console/reads.ts";
import { loadFixture } from "../../fixtures/load.ts";
import { HOUR, MINUTE, createModel } from "../model.ts";
import { createInitialState } from "../store.ts";
import type { AgentSession, AgentsState } from "../types.ts";
import { relationBundleTooltip, relationLayers, sessionTargetAt, timelineLayers, type SessionTarget, type TimelineViewport } from "./Timeline.tsx";

function target(id: string, startX: number, endX: number, y = 20, markerX: number | null = endX): SessionTarget {
  return { session: { id } as AgentSession, startX, endX, y, markerX, lostX: null, label: "Failed", reason: "exit 1" };
}

test("a Session that ended within seconds opens from the square around its end marker", () => {
  const short = target("s1", 100, 101);
  expect(sessionTargetAt([short], { x: 92, y: 12 })?.session.id).toBe("s1");
  expect(sessionTargetAt([short], { x: 110, y: 29 })?.session.id).toBe("s1");
  expect(sessionTargetAt([short], { x: 112, y: 20 })).toBeNull();
  // A line running on past the range has no end marker.
  expect(sessionTargetAt([target("s2", 100, 101, 20, null)], { x: 108, y: 20 })).toBeNull();
});

test("where the hit areas of neighbouring Sessions overlap, the nearer one wins", () => {
  const sessions = [target("s1", 100, 101), target("s2", 106, 107)];
  expect(sessionTargetAt(sessions, { x: 102, y: 20 })?.session.id).toBe("s1");
  expect(sessionTargetAt(sessions, { x: 105, y: 20 })?.session.id).toBe("s2");
  // Off every end marker, the line that passes nearer wins.
  expect(sessionTargetAt([target("long", 0, 300, 20, null), target("s3", 150, 152, 30)], { x: 100, y: 23 })?.session.id).toBe("long");
});

test("within the square around an end marker, the marker wins over the line of the next Session", () => {
  // session_RC ends where session_RC1 starts on the same lane.
  const sessions = [target("RC", 40, 100), target("RC1", 100, 140)];
  for (const point of [{ x: 101, y: 14 }, { x: 102, y: 20 }, { x: 105, y: 20 }, { x: 109, y: 20 }, { x: 97, y: 20 }]) {
    expect(sessionTargetAt(sessions, point)?.session.id).toBe("RC");
    expect(sessionTargetAt(sessions, point, true)?.session.id).toBe("RC");
  }
  expect(sessionTargetAt(sessions, { x: 111, y: 20 })?.session.id).toBe("RC1");
  // A marker on the line of a longer Session on the lane below.
  expect(sessionTargetAt([target("long", 0, 300, 20, null), target("s3", 150, 152, 30)], { x: 151, y: 23 })?.session.id).toBe("s3");
});

test("the hover reads only the end markers", () => {
  const long = target("long", 0, 300);
  expect(sessionTargetAt([long], { x: 150, y: 20 }, true)).toBeNull();
  expect(sessionTargetAt([long], { x: 295, y: 20 }, true)?.session.id).toBe("long");
});

// ---- Message buses over the density scenario (35 Actors, 136 Messages in 30 minutes) ----

const READS: ConsoleReads = {
  recordingFor: () => null,
  runtimeRecordFor: () => null,
  lastRuntimeRecordFor: () => null,
  lastLogLineFor: () => null,
  feldRecordsFor: () => null,
  consoleSessionFor: (session) => session,
};

// The width the Timeline column has at 1440 px beside the shell's rail.
const WIDTH = 467;

function density(fields: Partial<AgentsState> = {}, viewportOf?: (height: number) => TimelineViewport) {
  const fixture = loadFixture("message-density-30m");
  const model = createModel(fixture.actors, fixture.clock, READS);
  const jobs = Object.fromEntries(fixture.actors.actors.filter((actor) => actor.kind === "job").map((actor) => [actor.id, false]));
  const state: AgentsState = { ...createInitialState(undefined, fixture.clock.now), ...fixture.range, actorPins: jobs, ...fields };
  const agents = model.bandAgents(state);
  const layers = timelineLayers(model, state, agents, WIDTH, 0);
  const relations = relationLayers(model, state, agents, layers, viewportOf ? viewportOf(layers.height) : { top: 0, bottom: layers.height });
  const markup = renderToStaticMarkup(<svg>{layers.marks}{relations.marks}{layers.hits}{relations.hits}</svg>);
  const hits = [...markup.matchAll(/<line class="feld-agents__relation-hit"[^>]*data-relation-bundle[^>]*>/g)].map(([hit]) => hit);
  return { model, state, agents, layers, relations, markup, hits };
}

const count = (markup: string, pattern: RegExp) => [...markup.matchAll(pattern)].length;

test("the 136 Messages of the density scenario are drawn as a few buses", () => {
  const { model, state, agents, layers, relations, hits } = density();
  expect(agents).toHaveLength(35);
  expect(model.timelineRecordsFor(state, new Set(agents.map((agent) => agent.id)), layers.range).messages).toHaveLength(136);
  // With every lane in view, each Message is on exactly one bus.
  expect(relations.bundles.flatMap((bundle) => bundle.messages)).toHaveLength(136);
  expect(hits.length).toBeLessThan(40);
  expect(new Set(hits.map((hit) => hit.match(/· (\S+) bin/)![1]))).toEqual(new Set(["1m"]));
  // Zoomed out to 3 hours, the same Messages fill 5 minute bins.
  expect(new Set(density({ duration: 3 * HOUR }).hits.map((hit) => hit.match(/· (\S+) bin/)![1]))).toEqual(new Set(["5m"]));
});

test("the timeline has one Tab stop per lane, however many Sessions and bundles it draws", () => {
  const { agents, markup } = density();
  const stops = [...markup.matchAll(/<[^>]*tabindex="0"[^>]*>/g)].map(([hit]) => hit);
  expect(stops).toHaveLength(agents.length);
  for (const stop of stops) expect(stop).toMatch(/^<rect class="feld-agents__lane-hit/);
  expect(count(markup, /tabindex="-1"/g)).toBeGreaterThan(agents.length);
  // Every other hit names the lane the arrow keys reach it along.
  expect(count(markup, /tabindex="-1"/g)).toBe(count(markup, /data-lane="[^"]+" data-at="[\d.]+"[^>]*tabindex="-1"/g));
});

test("a single Message opens like a record; a bus of several opens none of them", () => {
  const { hits } = density();
  const singles = hits.filter((hit) => hit.includes("data-message-link="));
  const buses = hits.filter((hit) => !hit.includes("data-message-link="));
  expect(singles.length).toBeGreaterThan(0);
  expect(buses.length).toBeGreaterThan(0);
  for (const hit of singles) expect(hit).toMatch(/data-lane="[^"]+" data-at="[\d.]+" role="button" tabindex="-1"/);
  // A bus is reached with the arrow keys along its lane, where its focus shows the tooltip.
  for (const hit of buses) expect(hit).toMatch(/data-lane="[^"]+" data-at="[\d.]+" role="img" tabindex="-1"/);
});

test("a bus's tooltip counts its Messages and lists each accepted time and whether it was read", () => {
  const { model, state, relations } = density();
  const bundle = relations.bundles.find((candidate) => candidate.messages.length === 60)!;
  const wide = renderToStaticMarkup(<>{relationBundleTooltip(model, state, bundle, false)}</>);
  // The orchestrator's five Spawns of the same bin ride the same bus, each a row of its own.
  expect(wide).toContain("×60 Messages and ×5 Spawns · 20 recipients");
  expect(count(wide, /feld-agents__message-tooltip-row/g)).toBe(12);
  expect(wide).toContain("+13 more routes");
  // worker-01's third Message is still waiting in its mailbox.
  expect(wide).toMatch(/orchestrator → worker-01<\/strong><span>14:51:00\.000 → read 14:51:00\.040 · 14:51:00\.280 → read 14:51:00\.329 · 14:51:00\.560 → waiting<\/span><em class="is-waiting">×3 · 1 waiting<\/em>/);
  expect(wide).toContain("14:51:00.000–14:51:51.160 · 1m bin");
  const compact = renderToStaticMarkup(<>{relationBundleTooltip(model, state, bundle, true)}</>);
  expect(count(compact, /feld-agents__message-tooltip-row/g)).toBe(4);
  expect(compact).toContain("+21 more routes");
});

test("relationships stop at the edges of the vertical viewport", () => {
  // The first four lanes are in view.
  const { markup, hits } = density({}, () => ({ top: 0, bottom: 176 }));
  for (const hit of hits) {
    const [y1, y2] = [Number(hit.match(/y1="([\d.]+)"/)![1]), Number(hit.match(/y2="([\d.]+)"/)![1])];
    expect(y1).toBeGreaterThanOrEqual(8);
    expect(y2).toBeLessThanOrEqual(168);
  }
  expect(count(markup, /feld-agents__relation-edge/g)).toBeGreaterThan(0);
  // Scrolled to the middle, the orchestrator's buses come in from the top edge.
  const middle = density({}, () => ({ top: 600, bottom: 1000 }));
  expect(middle.hits.every((hit) => hit.includes('y1="608"'))).toBe(true);
});

test("a selected Actor accents its relationships and lane; a selected Session takes precedence", () => {
  const { model, markup, layers } = density({ focusedActorId: "default/worker-03" });
  expect(markup).toMatch(/<rect class="feld-agents__lane-hit is-selected"[^>]*data-focus-actor="default\/worker-03" role="button" tabindex="0" aria-pressed="true">/);
  expect(count(markup, /aria-pressed="false"/g)).toBe(34);
  expect(markup).toContain("feld-agents__actor-focus-band");
  expect([...layers.focus!.agents].sort()).toEqual(["default/orchestrator", "default/worker-03"]);
  // The orchestrator's shared buses recede and only worker-03's part is accented.
  expect(count(markup, /is-focus-overlay/g)).toBeGreaterThan(0);
  expect(markup).toMatch(/feld-agents__relation-link is-muted/);
  const session = model.focusFor({ inspectedSessionId: "session_density_worker-05", actorCard: null, focusedActorId: "default/worker-03" })!;
  expect(session.agents.has("default/worker-05")).toBe(true);
  expect(session.agents.has("default/worker-03")).toBe(false);
});

// ---- Spawns on the buses: the orchestrator spawns worker-06 to worker-10 in 14:51:02–14:51:12 ----

const WORKERS_06_TO_10 = ["worker-06", "worker-07", "worker-08", "worker-09", "worker-10"].map((worker) => `default/${worker}`);
const spawnBundleOf = (relations: { bundles: { spawns: unknown[] }[] }) => relations.bundles.findIndex((bundle) => bundle.spawns.length);
const groupOf = (markup: string, index: number) => markup.match(new RegExp(`<g class="feld-agents__relation-bundle-visual" data-relation-bundle-mark="${index}"[^]*?</g>`))![0];

test("a burst of Spawns from one Session rides the bus of the Messages it sent in the same bin: one trunk, an arrow per target", () => {
  const { relations, markup, hits } = density();
  const index = spawnBundleOf(relations);
  const bundle = relations.bundles[index];
  expect(bundle.spawns).toHaveLength(5);
  expect(bundle.messages).toHaveLength(60);
  expect(bundle.recipients.filter((recipient) => recipient.spawns.length).map((recipient) => recipient.actor)).toEqual(WORKERS_06_TO_10);
  // One trunk for the bin, where there used to be a wavy one beside the dotted one.
  expect(count(groupOf(markup, index), /class="feld-agents__relation-link"/g)).toBe(1);
  expect(markup).not.toContain("feld-agents__spawn-link");
  const hit = hits[index];
  expect(hit).toMatch(/role="img"[^>]*tabindex="-1" aria-label="×60 Messages and ×5 Spawns from orchestrator to worker-01, worker-02, worker-03 and 17 more · 1m bin"/);
  expect(hit).not.toContain("data-action-link");
  expect(hit).not.toContain("data-message-link");
});

test("the tooltip of a bus lists a Spawn target with its exact time and the Session it created", () => {
  const { model, state, relations } = density({ duration: MINUTE, anchor: Date.parse("2026-08-29T14:51:05+09:00") });
  const bundle = relations.bundles.find((candidate) => candidate.spawns.some((action) => action.id === "action_spawn_density_worker-10"))!;
  const wide = renderToStaticMarkup(<>{relationBundleTooltip(model, state, bundle, false)}</>);
  expect(wide).toContain("×1 Spawn · 1 recipient");
  expect(wide).toContain("<strong>orchestrator → worker-10</strong><span>14:51:12.400 · session_density_worker-10</span><em class=\"\">×1 Spawn</em>");
  expect(wide).toContain("14:51:12.400–14:51:12.400 · 2s bin");
});

test("Spawns follow the zoom into the bins; a bus of one Spawn opens it like a record", () => {
  expect(new Set(density({ duration: 3 * HOUR }).hits.map((hit) => hit.match(/· (\S+) bin/)![1]))).toEqual(new Set(["5m"]));
  // At a 2 s bin the Spawns at 14:51:09.8 and 14:51:12.4 fall between the Message waves.
  const zoomed = density({ duration: MINUTE, anchor: Date.parse("2026-08-29T14:51:05+09:00") });
  expect(zoomed.relations.bundles.filter((bundle) => bundle.spawns.length).map((bundle) => bundle.spawns.length)).toEqual([1, 1, 1, 1, 1]);
  const single = zoomed.hits.find((hit) => hit.includes('data-action-link="action_spawn_density_worker-10"'))!;
  expect(single).toMatch(/data-action-link="action_spawn_density_worker-10" data-lane="[^"]+" data-at="[\d.]+" role="button" tabindex="-1"/);
  expect(single).not.toContain("data-message-link");
});

test("a selected Actor accents its part of a shared bus, Spawn and Messages alike", () => {
  const { markup, relations } = density({ focusedActorId: "default/worker-07" });
  const group = groupOf(markup, spawnBundleOf(relations));
  expect(group).toContain("feld-agents__relation-link is-muted");
  expect(group).toContain("feld-agents__relation-link is-related is-focus-overlay");
  // worker-07's terminal is the one accented; the other 19 recede.
  expect(count(group, /feld-agents__(?:relation-arrow|mailbox-marker) is-related/g)).toBe(1);
  expect(count(group, /feld-agents__(?:relation-arrow|mailbox-marker) is-muted/g)).toBe(19);
});

test("a bus stops at the edges of the vertical viewport under chevrons", () => {
  // The first four lanes are in view: the orchestrator, and none of the targets.
  const top = density({}, () => ({ top: 0, bottom: 176 }));
  const index = spawnBundleOf(top.relations);
  expect(top.hits[index]).toMatch(/y2="168"/);
  const group = groupOf(top.markup, index);
  // The three workers in view keep their terminals; every other recipient is behind one chevron.
  expect(count(group, /feld-agents__(?:relation-arrow|mailbox-marker)/g)).toBe(3);
  expect(count(group, /feld-agents__relation-edge/g)).toBe(1);
  // Scrolled past every lane of the bus, nothing is drawn.
  expect(density({}, () => ({ top: 1000, bottom: 1400 })).relations.bundles.filter((bundle) => bundle.spawns.length)).toHaveLength(0);
});

test("a search that hides one end of a Spawn keeps it as a stub in the shown lane", () => {
  // Only the target is shown: the stub enters its lane from a hidden chevron and ends in its terminal.
  const child = density({ query: "worker-07" });
  const childIndex = spawnBundleOf(child.relations);
  expect(child.relations.bundles[childIndex].recipients.filter((recipient) => recipient.spawns.length).map((recipient) => recipient.actor)).toEqual(["default/worker-07"]);
  const childGroup = groupOf(child.markup, childIndex);
  expect(count(childGroup, /feld-agents__(?:relation-arrow|mailbox-marker)/g)).toBe(1);
  expect(count(childGroup, /feld-agents__relation-edge is-hidden/g)).toBe(1);
  // Only the spawner is shown: the burst keeps its bus, down to a hidden chevron in its lane.
  const parent = density({ query: "orchestrator" });
  const parentIndex = spawnBundleOf(parent.relations);
  expect(parent.relations.bundles[parentIndex].spawns).toHaveLength(5);
  const parentGroup = groupOf(parent.markup, parentIndex);
  expect(count(parentGroup, /feld-agents__relation-arrow/g)).toBe(0);
  expect(count(parentGroup, /feld-agents__relation-edge is-hidden/g)).toBe(1);
  // A hidden spawner folds into one inbound bus per shown target, each from a hidden
  // chevron at the top of its trunk.
  const fanIn = density({ query: "worker-0" });
  const inbound = fanIn.relations.bundles.map((bundle, index) => ({ bundle, index })).filter(({ bundle }) => bundle.spawns.length);
  expect(inbound.map(({ bundle }) => bundle.recipients.map((recipient) => recipient.actor))).toEqual(WORKERS_06_TO_10.slice(0, 4).map((worker) => [worker]));
  for (const { index } of inbound) {
    const group = groupOf(fanIn.markup, index);
    expect(count(group, /feld-agents__relation-edge is-hidden/g)).toBe(1);
    const trunkTop = Number(group.match(/class="feld-agents__relation-link[^"]*" x1="[\d.]+" y1="([\d.]+)"/)![1]);
    const chevronTip = Number(group.match(/class="feld-agents__relation-edge is-hidden" d="M [\d.]+ [\d.]+ L [\d.]+ ([\d.]+)/)![1]);
    expect(chevronTip).toBeCloseTo(trunkTop + 1, 5);
  }
});

// ---- Session lines ----------------------------------------------------------------------

test("a Session line is dashed only from the instant Feld lost sight of the execution", () => {
  const fixture = loadFixture("default");
  const model = createModel(fixture.actors, fixture.clock, READS);
  const lost = fixture.actors.sessions.find((session) => session.id === "session_RC")!;
  const state: AgentsState = { ...createInitialState(undefined, fixture.clock.now), mode: "historical", anchor: (lost.start + lost.end!) / 2, duration: 2 * (lost.end! - lost.start) };
  const agents = model.bandAgents(state).filter((agent) => agent.id === lost.actor);
  const markup = renderToStaticMarkup(<svg>{timelineLayers(model, state, agents, WIDTH, 0).marks}</svg>);
  const lines = [...markup.matchAll(/<line class="feld-agents__session([^"]*)" x1="([\d.]+)"[^>]*x2="([\d.]+)"[^>]*><title>session_RC: ([^<]*)</g)]
    .map(([, classes, x1, x2, title]) => ({ dashed: classes.includes("is-uncertain"), x1: Number(x1), x2: Number(x2), title }));
  expect(lines.map((line) => line.dashed)).toEqual([false, true]);
  expect(lines[0].x2).toBe(lines[1].x1);
  expect(lines[1].title).toMatch(/^observed lost since /);
  // The hover reads the dashed part from where it starts.
  const target = timelineLayers(model, state, agents, WIDTH, 0).sessions.find((session) => session.session.id === "session_RC")!;
  expect(target.lostX).toBeCloseTo(lines[1].x1, 1);
});

test("only a recorded end gets an end mark: a running line stops bare at now, and a line past the range ends in a chevron", () => {
  const fixture = loadFixture("default");
  const model = createModel(fixture.actors, fixture.clock, READS);
  const endsOf = (fields: Partial<AgentsState>) => {
    const state: AgentsState = { ...createInitialState(undefined, fixture.clock.now), ...fields };
    const layers = timelineLayers(model, state, model.bandAgents(state), WIDTH, 0);
    const markup = renderToStaticMarkup(<svg>{layers.marks}</svg>);
    return layers.sessions.map((target) => ({
      target,
      circle: markup.includes(`cx="${target.endX.toFixed(1)}" cy="${target.y}"`),
      chevron: markup.includes(`L ${target.endX} ${target.y} L`),
    }));
  };

  const live = endsOf({ mode: "live", duration: HOUR, anchor: fixture.clock.now - HOUR / 2 });
  const running = live.filter(({ target }) => target.session.end === null);
  expect(running.length).toBeGreaterThan(0);
  for (const end of running) {
    expect(end.circle).toBe(false);
    // The hover and the click still find the end.
    expect(end.target.markerX).toBe(end.target.endX);
  }

  const historical = endsOf(fixture.range);
  const stopped = historical.filter(({ target }) => target.label === "Stopped" && target.markerX !== null);
  expect(stopped.length).toBeGreaterThan(0);
  for (const end of stopped) expect(end.circle).toBe(true);
  const continuing = historical.filter(({ target }) => target.markerX === null);
  expect(continuing.length).toBeGreaterThan(0);
  for (const end of continuing) expect(end.chevron).toBe(true);
});
