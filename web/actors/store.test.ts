import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { readingPlaces, useConsoleStore } from "../console/store.ts";
import { storedActorColumnWidths, storedActorWidth, storedMetricChoices, useActorsStore } from "./store.ts";
import type { ActorRecord } from "./types.ts";

// The transitions of the Actors page's UI state.

const NOW = Date.parse("2026-09-25T00:30:00Z");
const store = () => useActorsStore.getState();

beforeEach(() => {
  store().reset(undefined, NOW);
});

test("a Console opens in the default reading, and keeps each Session's position until it closes", () => {
  store().openConsole("s1");
  useConsoleStore.getState().setView("s1", "events");
  readingPlaces.set("s1", { row: 3, offset: 12 });
  store().openConsole("s2");
  store().openConsole("s1");
  expect(useConsoleStore.getState().readings.s1).toBeUndefined();
  expect(readingPlaces.get("s1")).toEqual({ row: 3, offset: 12 });
  store().clearInspector();
  expect(readingPlaces.size).toBe(0);
});

test("a Console opened at a record marks it and scrolls to it once", () => {
  store().openConsole("s1", { recordId: "m1" });
  expect(store().consoleFocus).toEqual({ sessionId: "s1", recordId: "m1" });
  store().consumeConsoleFocus();
  expect(store().consoleFocus).toBeNull();
  expect(store().consoleMark).toEqual({ sessionId: "s1", recordId: "m1" });
});

test("the Actor card and the Console replace each other, and one Escape closes the card", () => {
  store().openConsole("s1");
  store().openActorCard("default/dev", "s1");
  expect(store()).toMatchObject({ inspectedSessionId: null, actorCard: { agentId: "default/dev", sessionId: "s1" } });
  store().openActorCard("default/dev");
  expect(store().actorCard).toEqual({ agentId: "default/dev", sessionId: null });
  store().openConsole("s1");
  expect(store()).toMatchObject({ inspectedSessionId: "s1", actorCard: null });
  store().openActorCard("default/dev", "s1");
  expect(store().escapeAll()).not.toBeNull();
  expect(store().actorCard).toBeNull();
});

test("each Actor's draft outlasts Escape and the other cards, and Cancel drops it", () => {
  store().openComposer("default/dev");
  store().setComposeDraft("a long Message");
  store().openComposer("default/rev");
  store().setComposeDraft("another");
  expect(store().escapeAll()).not.toBeNull();
  store().openActorCard("default/dev");
  store().openConsole("s1");
  store().openComposer("default/dev");
  expect(store().composeDrafts).toMatchObject({ "default/dev": { text: "a long Message" }, "default/rev": { text: "another" } });
  store().cancelComposer();
  expect(store().composeAgentId).toBeNull();
  expect(Object.keys(store().composeDrafts)).toEqual(["default/rev"]);
});

test("a Job opens its Actor card at the Job section; an edit survives a step into a Session and back", () => {
  store().openJob({ id: "default/dev", kind: "agent" });
  expect(store().actorCard).toEqual({ agentId: "default/dev", sessionId: null });
  expect(store().cardView).toBeNull();
  store().openJob({ id: "default/job", kind: "job" }, { skipId: "skip_1" });
  expect(store().actorCard).toEqual({ agentId: "default/job", sessionId: null });
  expect(store().cardView).toMatchObject({ agentId: "default/job", skipId: "skip_1", editing: false });
  const serial = store().cardView!.serial;
  store().editActor("default/job", { name: "job", cron: "0 * * * *", timezone: "UTC", runtime: "", command: "make", workdir: "/w", env: "", settings: "", provider: "", node: "", trustWorkdir: "", providerOptions: "" });
  expect(store().cardView).toMatchObject({ serial, editing: true });
  store().openActorCard("default/job", "s1");
  store().openActorCard("default/job");
  expect(store().cardView).toMatchObject({ serial, editing: true });
  // Another Actor's card drops it, and an answer for it lands nowhere.
  store().openActorCard("default/dev");
  expect(store().cardView).toBeNull();
  expect(store().patchCardView(serial, { result: "late" })).toBe(false);
  // Escape closes the card like any other.
  store().openJob({ id: "default/job", kind: "job" });
  expect(store().escapeModal()).toBeNull();
  expect(store().escapeAll()).toMatchObject({ card: { kind: "actor", agentId: "default/job" } });
  expect(store().actorCard).toBeNull();
  expect(store().cardView).toBeNull();
});
test("the range moves to History and ends at now at the latest", () => {
  store().panTo(NOW - 60 * 60 * 1000, NOW);
  expect(store().mode).toBe("historical");
  // A pan that reaches now is Live again.
  store().panTo(NOW - store().duration / 2, NOW);
  expect(store()).toMatchObject({ mode: "live", anchor: NOW - store().duration / 2 });
  store().panTo(NOW - 60 * 60 * 1000, NOW);
  store().applyPicker(NOW + 60 * 60 * 1000, 60 * 60 * 1000, NOW);
  expect(store().anchor).toBe(NOW - 30 * 60 * 1000);
  store().quickLive(3 * 60 * 60 * 1000, NOW);
  expect(store()).toMatchObject({ mode: "live", duration: 3 * 60 * 60 * 1000 });
});

test("Refresh counts up and pausing is the reader's", () => {
  expect(store().paused).toBe(false);
  store().setPaused(true);
  store().refresh();
  expect(store()).toMatchObject({ paused: true, refreshes: 1 });
});

test("selecting another Workspace closes the Console and the dialogs of the old one", () => {
  store().openConsole("s1");
  store().openNewActor("default", "UTC", { runtimes: [], providers: [], nodes: [] });
  store().selectWorkspace("research");
  expect(store()).toMatchObject({ workspace: "research", inspectedSessionId: null, newActor: null });
});

test("a selected Actor toggles, outlasts the cards, clears with the next Escape, and stays with its Workspace", () => {
  store().toggleActorFocus("default/dev");
  expect(store().focusedActorId).toBe("default/dev");
  store().toggleActorFocus("default/dev");
  expect(store().focusedActorId).toBeNull();
  // The Actor card selects its Actor; the first Escape closes the card and keeps the
  // selection, and the next clears it.
  store().openActorCard("default/dev");
  expect(store().focusedActorId).toBe("default/dev");
  store().openConsole("s1");
  expect(store()).toMatchObject({ inspectedSessionId: "s1", focusedActorId: "default/dev" });
  expect(store().escapeAll()).not.toBeNull();
  expect(store()).toMatchObject({ inspectedSessionId: null, focusedActorId: "default/dev" });
  expect(store().escapeAll()).toBeNull();
  expect(store().focusedActorId).toBeNull();
  store().toggleActorFocus("default/dev");
  store().selectWorkspace("research");
  expect(store().focusedActorId).toBeNull();
});

test("New actor starts on the first Runtime, Provider, and Node the daemon offers", () => {
  store().openNewActor("default", "UTC", { runtimes: [["codex", "Codex"]], providers: [["herdr", "herdr"]], nodes: [["local", "local"]] });
  expect(store().newActor).toMatchObject({ runtime: "codex", provider: "herdr", node: "local" });
});

function actor(name: string, kind: ActorRecord["kind"] = "agent"): ActorRecord {
  return { id: `default/${name}`, name, workspace: "default", kind, launchSpec: null };
}

test("a row dropped across the pinned and unpinned groups takes the pin of the row it lands beside", () => {
  useActorsStore.setState({ actorPins: {}, actorOrder: [] });
  const job = actor("nightly", "job");
  const lead = actor("lead");
  const idle = actor("idle");
  // An unpinned Agent dropped onto a pinned row is pinned.
  store().moveActor([job, lead, idle], idle, job, true);
  expect(store().actorPins).toEqual({ "default/idle": true });
  expect(store().actorOrder).toEqual(["default/nightly", "default/idle", "default/lead"]);
  // A pinned Job dropped onto an unpinned row is unpinned.
  store().moveActor([job, idle, lead], job, lead, true);
  expect(store().actorPins).toEqual({ "default/idle": true, "default/nightly": false });
  expect(store().actorOrder).toEqual(["default/idle", "default/lead", "default/nightly"]);
});

test("a row moved within its group keeps its pin", () => {
  useActorsStore.setState({ actorPins: {}, actorOrder: [] });
  const lead = actor("lead");
  const idle = actor("idle");
  store().moveActor([lead, idle], idle, lead, false);
  expect(store().actorPins).toEqual({});
  expect(store().actorOrder).toEqual(["default/idle", "default/lead"]);
});

// A browser's localStorage, for the stored preferences.
function stubStorage() {
  const values = new Map<string, string>();
  vi.stubGlobal("localStorage", { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => values.set(key, value) });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a column width is clamped to its limits and stored once the drag ends", () => {
  stubStorage();
  store().setActorColumnWidth("status", 1000);
  expect(store().actorColumnWidths).toEqual({ status: 320 });
  expect(storedActorColumnWidths()).toEqual({});
  store().setActorColumnWidth("status", 200.4, true);
  expect(storedActorColumnWidths()).toEqual({ status: 200 });
});

test("a stored width that cannot be read or lies outside its limits falls back to the default", () => {
  stubStorage();
  localStorage.setItem("feld-mock-actor-width", "900");
  expect(storedActorWidth()).toBe(340);
  localStorage.setItem("feld-mock-actor-width", "300");
  expect(storedActorWidth()).toBe(300);
  localStorage.setItem("feld-mock-actor-column-widths", "{not json");
  expect(storedActorColumnWidths()).toEqual({});
});

test("the lanes the reader chose are stored and kept, and every other lane follows its default", () => {
  stubStorage();
  store().reset(undefined, NOW);
  const lanes = (reported: boolean) => [
    { id: "quota-5h-claude-code", defaultVisible: reported },
    { id: "quota-7d-claude-code", defaultVisible: reported },
    { id: "api-errors", defaultVisible: true },
    { id: "tool-calls", defaultVisible: false },
  ];
  store().showDefaultMetrics(lanes(false));
  expect([...store().visibleMetricIds].sort()).toEqual(["api-errors"]);
  store().setMetricVisible("api-errors", false);
  store().setMetricVisible("tool-calls", true);
  store().showDefaultMetrics(lanes(true));
  expect([...store().visibleMetricIds].sort()).toEqual(["quota-5h-claude-code", "quota-7d-claude-code", "tool-calls"]);
  // A page opened again reads the choices back.
  store().reset(undefined, NOW);
  store().showDefaultMetrics(lanes(true));
  expect([...store().visibleMetricIds].sort()).toEqual(["quota-5h-claude-code", "quota-7d-claude-code", "tool-calls"]);
  expect(storedMetricChoices()).toEqual({ "api-errors": false, "tool-calls": true });
});

test("stored lane choices that cannot be read fall back to the defaults", () => {
  stubStorage();
  localStorage.setItem("feld-mock-metric-lanes", "{not json");
  expect(storedMetricChoices()).toEqual({});
  localStorage.setItem("feld-mock-metric-lanes", JSON.stringify({ "api-errors": "no", "tool-calls": true }));
  expect(storedMetricChoices()).toEqual({ "tool-calls": true });
  localStorage.setItem("feld-mock-metric-lanes", "[true]");
  store().reset(undefined, NOW);
  store().showDefaultMetrics([{ id: "api-errors", defaultVisible: true }, { id: "tool-calls", defaultVisible: false }]);
  expect([...store().visibleMetricIds]).toEqual(["api-errors"]);
});
