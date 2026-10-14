import { expect, test } from "vitest";
import { HOUR, MINUTE } from "../model.ts";
import type { Focus, Lane, MessageRecord, SpawnAction } from "../types.ts";
import { foldIncomingBundles, relationBinDuration, relationBundlesFor, relationBusGeometry, relationCountLabel, relationSingle } from "./relationBus.ts";

const T0 = Date.parse("2026-08-29T06:00:00Z");

function message(id: string, from: string, to: string, accepted: number, fields: Partial<MessageRecord> = {}): MessageRecord {
  return {
    id,
    from,
    fromWorkspace: from === "Admin" ? null : "default",
    fromSessionId: from === "Admin" ? null : `s-${from}`,
    body: null,
    to,
    toWorkspace: "default",
    toSessionId: `s-${to}`,
    accepted,
    read: { readId: `r-${id}`, sessionId: `s-${to}`, at: accepted + 50, fifo: 1 },
    anchor: null,
    preview: "",
    ...fields,
  };
}

// Lanes of 44 px stacked from the top, the main line 30 px into each.
function lanesOf(...actors: string[]) {
  return new Map<string, Lane>(actors.map((actor, index) => [actor, { top: index * 44, height: 44, mainY: index * 44 + 30, sessionY: new Map() }]));
}

function focusOn(...messages: string[]): Focus {
  return { sessions: new Set(), actions: new Set(), messages: new Set(messages), agents: new Set() };
}

function spawn(id: string, spawner: string, target: string, at: number, fields: Partial<SpawnAction> = {}): SpawnAction {
  return { id, spawner, spawnerWorkspace: "default", spawnerSessionId: `s-${spawner}`, target, targetWorkspace: "default", session: `s-${target}-${id}`, at, ...fields };
}

test("the bin stays about eight pixels wide, in 1 / 2 / 5 steps, and follows the zoom", () => {
  expect(relationBinDuration(30 * MINUTE, 429)).toBe(MINUTE);
  expect(relationBinDuration(30 * MINUTE, 493)).toBe(30_000);
  expect(relationBinDuration(3 * HOUR, 429)).toBe(5 * MINUTE);
  expect(relationBinDuration(MINUTE, 2000)).toBe(1000);
  expect(relationBinDuration(365 * 24 * HOUR, 10)).toBe(24 * HOUR);
});

test("one sender Session in one absolute bin makes one bus with a recipient entry per Actor", () => {
  const messages = [
    message("a", "lead", "w1", T0 + 1000),
    message("b", "lead", "w2", T0 + 2000),
    message("c", "lead", "w1", T0 + 500),
    // The next bin, and another sender in the first bin.
    message("d", "lead", "w1", T0 + MINUTE),
    message("e", "w2", "lead", T0 + 3000),
  ];
  const bundles = relationBundlesFor(messages, [], MINUTE);
  expect(bundles.map((bundle) => [bundle.from, bundle.start, bundle.messages.map((entry) => entry.id)])).toEqual([
    ["lead", T0, ["c", "a", "b"]],
    ["w2", T0, ["e"]],
    ["lead", T0 + MINUTE, ["d"]],
  ]);
  expect(bundles[0].recipients.map((recipient) => [recipient.actor, recipient.messages.map((entry) => entry.id)])).toEqual([["w1", ["a", "c"]], ["w2", ["b"]]]);
  // Another Session of the same sender keeps a bus of its own.
  expect(relationBundlesFor([message("a", "lead", "w1", T0), message("b", "lead", "w1", T0, { fromSessionId: "s-lead-2" })], [], MINUTE)).toHaveLength(2);
});

test("a bus runs from the sender to its farthest recipient and ends in one terminal per recipient", () => {
  const lanes = lanesOf("lead", "w1", "w2", "w3");
  const [bundle] = relationBundlesFor([message("a", "lead", "w1", T0), message("b", "lead", "w3", T0, { read: null })], [], MINUTE);
  const bus = relationBusGeometry(bundle, lanes, 0, 1000, null)!;
  expect(bus).toMatchObject({ fromY: 30, toY: 162, trunk: "", stub: false, overlay: null, edges: [] });
  // w2 sits under the trunk without a terminal, so it does not read as a recipient.
  expect(bus.terminals).toEqual([
    { fromY: 30, y: 74, waiting: false, emphasis: "" },
    { fromY: 30, y: 162, waiting: true, emphasis: "" },
  ]);
});

test("an end scrolled out of the viewport stops at its edge under a chevron; a relationship with both ends out is left out", () => {
  const lanes = lanesOf("lead", "w1", "w2", "w3", "w4");
  // The viewport shows w1 to w3: lead is above it, w4 below.
  const [outward] = relationBundlesFor([message("a", "w1", "lead", T0), message("b", "w1", "w4", T0)], [], MINUTE);
  expect(relationBusGeometry(outward, lanes, 60, 170, null)).toMatchObject({
    fromY: 60,
    toY: 170,
    terminals: [],
    edges: [{ y: 60, direction: "up", hidden: false }, { y: 170, direction: "down", hidden: false }],
  });
  // From lead to w4 nothing crosses the viewport, so only the Message to w1 is drawn.
  const [inward] = relationBundlesFor([message("c", "lead", "w1", T0), message("d", "lead", "w4", T0)], [], MINUTE);
  expect(relationBusGeometry(inward, lanes, 60, 170, null)).toMatchObject({ fromY: 60, toY: 74, terminals: [{ fromY: 60, y: 74 }], edges: [{ y: 60, direction: "up" }] });
  const [between] = relationBundlesFor([message("e", "lead", "w4", T0)], [], MINUTE);
  expect(relationBusGeometry(between, lanes, 60, 170, null)).toBeNull();
});

test("an Actor the table does not show keeps a short stub inside the drawn lane", () => {
  const lanes = lanesOf("lead", "w1");
  const bundles = relationBundlesFor([message("a", "lead", "hidden", T0), message("b", "Admin", "w1", T0 + 1000)], [], MINUTE);
  const outgoing = bundles.find((bundle) => bundle.from === "lead")!;
  const incoming = bundles.find((bundle) => bundle.from === "Admin")!;
  expect(relationBusGeometry(outgoing, lanes, 0, 1000, null)).toMatchObject({ fromY: 30, toY: 38, stub: true, terminals: [], edges: [{ y: 38, direction: "down", hidden: true }] });
  expect(relationBusGeometry(incoming, lanes, 0, 1000, null)).toMatchObject({ fromY: 50, toY: 74, stub: true, terminals: [{ fromY: 50, y: 74 }], edges: [{ y: 50, direction: "up", hidden: true }] });
});

test("senders above the viewport fold into one inbound bus per edge, bin, and visible recipient", () => {
  const lanes = lanesOf("w1", "w2", "w3", "lead", "w4");
  const bundles = relationBundlesFor([
    message("a", "w1", "lead", T0),
    message("b", "w2", "lead", T0 + 1000),
    message("c", "w4", "lead", T0 + 2000),
    message("d", "w1", "lead", T0 + MINUTE),
  ], [], MINUTE);
  // The viewport shows w3 and lead.
  const folded = foldIncomingBundles(bundles, lanes, 100, 170);
  expect(folded.map((bundle) => [bundle.foldedSource, bundle.start, bundle.messages.map((entry) => entry.id)])).toEqual([
    ["up", T0, ["a", "b"]],
    ["down", T0, ["c"]],
    ["up", T0 + MINUTE, ["d"]],
  ]);
  expect(relationBusGeometry(folded[0], lanes, 100, 170, null)).toMatchObject({ fromY: 100, toY: 162, terminals: [{ fromY: 100, y: 162 }], edges: [{ y: 100, direction: "up" }] });
  // With the recipient off-screen as well, nothing is drawn.
  expect(foldIncomingBundles(bundles, lanes, 0, 10)).toEqual([]);
});

test("on a shared bus only the selected relationship is accented and the rest recedes", () => {
  const lanes = lanesOf("lead", "w1", "w2", "w3");
  const [bundle] = relationBundlesFor([message("a", "lead", "w1", T0), message("b", "lead", "w3", T0)], [], MINUTE);
  const bus = relationBusGeometry(bundle, lanes, 0, 1000, focusOn("a"))!;
  expect(bus.trunk).toBe("muted");
  expect(bus.overlay).toEqual({ fromY: 30, toY: 74 });
  expect(bus.terminals.map((terminal) => terminal.emphasis)).toEqual(["related", "muted"]);
  expect(relationBusGeometry(bundle, lanes, 0, 1000, focusOn("a", "b"))!.trunk).toBe("related");
  expect(relationBusGeometry(bundle, lanes, 0, 1000, focusOn())).toMatchObject({ trunk: "muted", overlay: null });
});

test("the Spawns of a Session join the bus of its Messages in the same bin, each target with its own terminal", () => {
  const bundles = relationBundlesFor(
    [message("a", "lead", "w1", T0 + 1000), message("b", "lead", "w2", T0 + 2000)],
    [spawn("p", "lead", "w2", T0 + 500), spawn("q", "lead", "w3", T0 + 1500), spawn("r", "lead", "w4", T0 + MINUTE), spawn("s", "w2", "w3", T0 + 3000, { spawnerSessionId: "s-w2" })],
    MINUTE,
  );
  expect(bundles.map((bundle) => [bundle.from, bundle.start, bundle.messages.map((entry) => entry.id), bundle.spawns.map((entry) => entry.id)])).toEqual([
    ["lead", T0, ["a", "b"], ["p", "q"]],
    ["w2", T0, [], ["s"]],
    ["lead", T0 + MINUTE, [], ["r"]],
  ]);
  // w2 gets its Message and its Spawn on one terminal; w3 is reached by a Spawn alone.
  expect(bundles[0].recipients.map((recipient) => [recipient.actor, recipient.messages.map((entry) => entry.id), recipient.spawns.map((entry) => entry.id)])).toEqual([["w1", ["a"], []], ["w2", ["b"], ["p"]], ["w3", [], ["q"]]]);
  expect(relationCountLabel(bundles[0])).toBe("×2 Messages and ×2 Spawns");
  expect(relationCountLabel(bundles[1])).toBe("×1 Spawn");
  // Only a bus of one record opens like that record.
  expect(relationSingle(bundles[0])).toBeNull();
  expect(relationSingle(bundles[1])).toEqual({ spawn: bundles[1].spawns[0] });
  expect(relationSingle(relationBundlesFor([message("z", "lead", "w1", T0)], [], MINUTE)[0])).toEqual({ message: expect.objectContaining({ id: "z" }) });

  const lanes = lanesOf("lead", "w1", "w2", "w3");
  const bus = relationBusGeometry(bundles[0], lanes, 0, 1000, null)!;
  expect(bus).toMatchObject({ fromY: 30, toY: 162, trunk: "", overlay: null, edges: [] });
  expect(bus.terminals).toEqual([
    { fromY: 30, y: 74, waiting: false, emphasis: "" },
    { fromY: 30, y: 118, waiting: false, emphasis: "" },
    { fromY: 30, y: 162, waiting: false, emphasis: "" },
  ]);
  // A selected Spawn accents its own span of the shared trunk, as a selected Message does.
  const focus: Focus = { sessions: new Set(), actions: new Set(["q"]), messages: new Set(), agents: new Set() };
  const accented = relationBusGeometry(bundles[0], lanes, 0, 1000, focus)!;
  expect(accented.trunk).toBe("muted");
  expect(accented.overlay).toEqual({ fromY: 30, toY: 162 });
  expect(accented.terminals.map((terminal) => terminal.emphasis)).toEqual(["muted", "muted", "related"]);
});

test("a spawner above the viewport folds into the inbound bus of its visible target, with the Messages of the same bin", () => {
  const lanes = lanesOf("lead", "w1", "w2");
  const bundles = relationBundlesFor([message("a", "lead", "w2", T0 + 1000)], [spawn("p", "lead", "w2", T0 + 500)], MINUTE);
  // The viewport shows w1 and w2.
  const folded = foldIncomingBundles(bundles, lanes, 44, 132);
  expect(folded.map((bundle) => [bundle.foldedSource, bundle.messages.map((entry) => entry.id), bundle.spawns.map((entry) => entry.id)])).toEqual([["up", ["a"], ["p"]]]);
  expect(relationBusGeometry(folded[0], lanes, 44, 132, null)).toMatchObject({ fromY: 44, toY: 118, terminals: [{ fromY: 44, y: 118 }], edges: [{ y: 44, direction: "up" }] });
});
