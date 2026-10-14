// The relation buses of the Timeline. The Messages and Spawns from one source Session in one
// absolute-clock bin share a dotted trunk that keeps a terminal per recipient, so a fan-out
// to many recipients reads as one vertical line instead of one line per record, and a Spawn
// beside the Message that follows it reads as one trunk instead of two. Only relationships
// that touch a lane in the vertical viewport are drawn; an off-screen end stops at the
// viewport edge under a chevron.

import { DAY, HOUR, MESSAGE_STUB_INSET, MINUTE, laneYFor } from "../model.ts";
import type { Focus, Lane, MessageRecord, SpawnAction } from "../types.ts";

const SECOND = 1000;

// Bins stay roughly eight screen pixels wide. The stepped durations keep a bundle stable
// while panning and change only at useful zoom thresholds.
export const RELATION_BIN_PIXEL_TARGET = 8;
export const RELATION_BIN_OPTIONS = [SECOND, 2 * SECOND, 5 * SECOND, 10 * SECOND, 15 * SECOND, 30 * SECOND, MINUTE, 2 * MINUTE, 5 * MINUTE, 10 * MINUTE, 15 * MINUTE, 30 * MINUTE, HOUR, 2 * HOUR, 6 * HOUR, 12 * HOUR, DAY];

export function relationBinDuration(windowDuration: number, plotWidth: number) {
  const desiredDuration = windowDuration * RELATION_BIN_PIXEL_TARGET / Math.max(plotWidth, 1);
  return RELATION_BIN_OPTIONS.find((duration) => duration >= desiredDuration)
    || RELATION_BIN_OPTIONS[RELATION_BIN_OPTIONS.length - 1];
}

// One Actor a bus reaches: the Messages it received and the Spawns that created a Session
// of it, in this bin from this source.
export interface BundleRecipient {
  actor: string;
  workspace: string;
  messages: MessageRecord[];
  spawns: SpawnAction[];
}

// Where the source of a folded bundle is: above or below the viewport, or not in the table.
export type FoldedSource = "up" | "down" | "hidden-up";

export interface RelationBundle {
  start: number;
  end: number;
  from: string;
  fromWorkspace: string | null;
  fromSessionId: string | null;
  // In accepted order.
  messages: MessageRecord[];
  // In spawn order.
  spawns: SpawnAction[];
  recipients: BundleRecipient[];
  foldedSource?: FoldedSource;
}

const byAccepted = (left: MessageRecord, right: MessageRecord) => left.accepted - right.accepted;
const byAt = (left: SpawnAction, right: SpawnAction) => left.at - right.at;

// What a bundle holds, in words: "×3 Messages", "×2 Spawns", or both.
export function relationCountLabel(bundle: Pick<RelationBundle, "messages" | "spawns">) {
  const parts: string[] = [];
  if (bundle.messages.length) parts.push(`×${bundle.messages.length} Message${bundle.messages.length === 1 ? "" : "s"}`);
  if (bundle.spawns.length) parts.push(`×${bundle.spawns.length} Spawn${bundle.spawns.length === 1 ? "" : "s"}`);
  return parts.join(" and ");
}

// The one record of a bus that holds a single one, which opens like that record.
export function relationSingle(bundle: Pick<RelationBundle, "messages" | "spawns">): { message: MessageRecord } | { spawn: SpawnAction } | null {
  if (bundle.messages.length + bundle.spawns.length !== 1) return null;
  return bundle.messages.length ? { message: bundle.messages[0] } : { spawn: bundle.spawns[0] };
}

// A bus is one source Session in one absolute-clock bin. Pair-only aggregation would leave
// a fan-out to many recipients as dense as the original records, so every recipient in the
// bin shares the same trunk and keeps only its terminal marker.
export function relationBundlesFor(messages: MessageRecord[], spawns: SpawnAction[], binDuration: number): RelationBundle[] {
  const grouped = new Map<string, { bundle: RelationBundle; recipients: Map<string, BundleRecipient> }>();
  function recipientOf(at: number, from: string, fromWorkspace: string | null, fromSessionId: string | null, to: string, toWorkspace: string) {
    const start = Math.floor(at / binDuration) * binDuration;
    const key = [fromWorkspace || "", from, fromSessionId || "", start].join("\u0000");
    let entry = grouped.get(key);
    if (!entry) {
      entry = {
        bundle: { start, end: start + binDuration, from, fromWorkspace, fromSessionId, messages: [], spawns: [], recipients: [] },
        recipients: new Map(),
      };
      grouped.set(key, entry);
    }
    const recipientKey = [toWorkspace || "", to].join("\u0000");
    let recipient = entry.recipients.get(recipientKey);
    if (!recipient) {
      recipient = { actor: to, workspace: toWorkspace, messages: [], spawns: [] };
      entry.recipients.set(recipientKey, recipient);
    }
    return { bundle: entry.bundle, recipient };
  }
  messages.forEach((message) => {
    const { bundle, recipient } = recipientOf(message.accepted, message.from, message.fromWorkspace, message.fromSessionId, message.to, message.toWorkspace);
    bundle.messages.push(message);
    recipient.messages.push(message);
  });
  spawns.forEach((action) => {
    const { bundle, recipient } = recipientOf(action.at, action.spawner, action.spawnerWorkspace, action.spawnerSessionId, action.target, action.targetWorkspace);
    bundle.spawns.push(action);
    recipient.spawns.push(action);
  });
  return [...grouped.values()].map(({ bundle, recipients }) => ({
    ...bundle,
    messages: bundle.messages.sort(byAccepted),
    spawns: bundle.spawns.sort(byAt),
    recipients: [...recipients.values()],
  })).sort((left, right) => left.start - right.start || left.from.localeCompare(right.from));
}

// The row a bundle leaves from: the source's Session current at the time; null when the
// source has no lane (the Admin, or an Actor the table does not show).
export function senderYOf(bundle: RelationBundle, lanes: Map<string, Lane>) {
  const lane = lanes.get(bundle.from);
  return lane ? laneYFor(lane, bundle.fromSessionId) : null;
}

const within = (y: number | null, top: number, bottom: number): y is number => y !== null && y >= top && y <= bottom;
const firstInstant = (bundle: RelationBundle) => Math.min(bundle.messages[0]?.accepted ?? Infinity, bundle.spawns[0]?.at ?? Infinity);

// Once a source is above or below the viewport, its identity no longer needs a vertical
// line of its own. Records entering the same visible recipient from the same edge and bin
// become one folded inbound bundle; the tooltip still lists every original source.
export function foldIncomingBundles(bundles: RelationBundle[], lanes: Map<string, Lane>, top: number, bottom: number): RelationBundle[] {
  const visible: RelationBundle[] = [];
  const folded = new Map<string, RelationBundle>();
  bundles.forEach((bundle) => {
    const senderY = senderYOf(bundle, lanes);
    if (within(senderY, top, bottom)) {
      visible.push(bundle);
      return;
    }
    const edge: FoldedSource = senderY === null ? "hidden-up" : senderY < top ? "up" : "down";
    bundle.recipients.forEach((recipient) => {
      const toLane = lanes.get(recipient.actor);
      if (!toLane || !within(toLane.mainY, top, bottom)) return;
      const key = [bundle.start, edge, recipient.workspace || "", recipient.actor].join("\u0000");
      let aggregate = folded.get(key);
      if (!aggregate) {
        aggregate = { ...bundle, messages: [], spawns: [], recipients: [{ actor: recipient.actor, workspace: recipient.workspace, messages: [], spawns: [] }], foldedSource: edge };
        folded.set(key, aggregate);
      }
      aggregate.messages.push(...recipient.messages);
      aggregate.spawns.push(...recipient.spawns);
      aggregate.recipients[0].messages.push(...recipient.messages);
      aggregate.recipients[0].spawns.push(...recipient.spawns);
    });
  });
  return visible.concat([...folded.values()].map((bundle) => ({ ...bundle, messages: bundle.messages.sort(byAccepted), spawns: bundle.spawns.sort(byAt) })))
    .sort((left, right) => left.start - right.start || firstInstant(left) - firstInstant(right));
}

// How a mark relates to the selected Actor or Session: drawn as is with nothing selected,
// accented when it is related, receding otherwise.
export type Emphasis = "" | "related" | "muted";

export interface BusTerminal {
  // The terminal points from `fromY` to `y`, the recipient's main line.
  fromY: number;
  y: number;
  // A waiting Message ends in an open tray; a read one, or a Spawn, in an arrowhead.
  waiting: boolean;
  emphasis: Emphasis;
}

export interface BusEdge {
  y: number;
  direction: "up" | "down";
  // The other end is not in the table rather than scrolled away.
  hidden: boolean;
  emphasis: Emphasis;
}

export interface BusGeometry {
  fromY: number;
  toY: number;
  trunk: Emphasis;
  stub: boolean;
  // The span of the trunk that the selection touches, accented over a muted shared trunk.
  overlay: { fromY: number; toY: number } | null;
  terminals: BusTerminal[];
  edges: BusEdge[];
}

export const emphasisOf = (focus: Focus | null, related: boolean): Emphasis => (!focus ? "" : related ? "related" : "muted");
const clip = (y: number, top: number, bottom: number) => Math.max(top, Math.min(bottom, y));

type Records = Pick<RelationBundle, "messages" | "spawns">;
const anyRelated = (focus: Focus | null, records: Records) => !focus || records.messages.some((message) => focus.messages.has(message.id)) || records.spawns.some((action) => focus.actions.has(action.id));
const allRelated = (focus: Focus | null, records: Records) => !focus || (records.messages.every((message) => focus.messages.has(message.id)) && records.spawns.every((action) => focus.actions.has(action.id)));

// The trunk, terminals, and edge chevrons of one bundle inside the viewport [top, bottom];
// null when no part of it is drawn. A bus keeps a terminal per recipient, so an intermediate
// lane is never mistaken for a destination.
export function relationBusGeometry(bundle: RelationBundle, lanes: Map<string, Lane>, top: number, bottom: number, focus: Focus | null): BusGeometry | null {
  const fromLane = bundle.foldedSource ? null : lanes.get(bundle.from);
  const senderY = fromLane ? laneYFor(fromLane, bundle.fromSessionId) : null;
  const senderVisible = within(senderY, top, bottom);
  const ys: number[] = [];
  const relatedYs: number[] = [];
  const terminals: BusTerminal[] = [];
  let stub = false;
  // An edge is related when any relationship through it is.
  const sourceEdges = new Map<string, boolean>();
  const targetEdges = new Map<string, boolean>();
  const addEdge = (edges: Map<string, boolean>, edge: string, related: boolean) => edges.set(edge, (edges.get(edge) || false) || related);
  const span = (fromY: number, toY: number, related: boolean) => {
    ys.push(fromY, toY);
    if (related) relatedYs.push(fromY, toY);
  };

  bundle.recipients.forEach((recipient) => {
    const toLane = lanes.get(recipient.actor);
    const recipientY = toLane ? toLane.mainY : null;
    const recipientVisible = within(recipientY, top, bottom);
    const related = anyRelated(focus, recipient);
    const terminal = (fromY: number, y: number) => terminals.push({ fromY, y, waiting: recipient.messages.some((message) => message.read === null), emphasis: emphasisOf(focus, related) });

    if ((bundle.foldedSource === "up" || bundle.foldedSource === "down") && recipientVisible) {
      const edgeY = bundle.foldedSource === "up" ? top : bottom;
      span(edgeY, recipientY, related);
      addEdge(sourceEdges, bundle.foldedSource, related);
      terminal(edgeY, recipientY);
      return;
    }

    if (fromLane && toLane) {
      // A relationship whose two Actors are both outside the viewport contributes no line to
      // the visible rows. If either end is visible, the other is clipped to an edge.
      if (!senderVisible && !recipientVisible) return;
      const clippedSenderY = clip(senderY!, top, bottom);
      const clippedRecipientY = clip(recipientY!, top, bottom);
      span(clippedSenderY, clippedRecipientY, related);
      if (!senderVisible) addEdge(sourceEdges, senderY! < top ? "up" : "down", related);
      if (!recipientVisible) {
        addEdge(targetEdges, recipientY! < top ? "up" : "down", related);
        return;
      }
      terminal(clippedSenderY, recipientY!);
      return;
    }

    // An Actor filtered out of the table keeps the local stub: unlike a scrolled Actor, it
    // has no top or bottom position to preserve.
    stub = true;
    if (fromLane && senderVisible) {
      const stubY = Math.min(bottom, fromLane.top + fromLane.height - MESSAGE_STUB_INSET);
      span(senderY, stubY, related);
      addEdge(targetEdges, "hidden-down", related);
    } else if (toLane && recipientVisible) {
      const stubY = Math.max(top, toLane.top + MESSAGE_STUB_INSET);
      span(stubY, recipientY, related);
      addEdge(sourceEdges, "hidden-up", related);
      terminal(stubY, recipientY);
    }
  });

  if (!ys.length) return null;
  const fromY = Math.min(...ys);
  const toY = Math.max(...ys);
  const edges = [...sourceEdges, ...targetEdges].map(([edge, related]): BusEdge => {
    const direction = edge.includes("up") ? "up" : "down";
    const hidden = edge.startsWith("hidden");
    const y = hidden ? (direction === "up" ? fromY : toY) : (direction === "up" ? top : bottom);
    return { y, direction, hidden, emphasis: emphasisOf(focus, related) };
  });
  // A shared bus can mix selected and unrelated recipients: its full extent recedes and only
  // the selected relationships are accented.
  const overlay = focus && anyRelated(focus, bundle) && relatedYs.length ? { fromY: Math.min(...relatedYs), toY: Math.max(...relatedYs) } : null;
  return { fromY, toY, trunk: emphasisOf(focus, allRelated(focus, bundle)), stub, overlay, terminals, edges };
}

// The chevron at a viewport edge, pointing the way the relationship continues.
export function edgeChevronPath(x: number, y: number, direction: "up" | "down") {
  const pointY = direction === "up" ? y + 1 : y - 1;
  const wingY = direction === "up" ? y + 7 : y - 7;
  return `M ${(x - 4).toFixed(1)} ${wingY.toFixed(1)} L ${x.toFixed(1)} ${pointY.toFixed(1)} L ${(x + 4).toFixed(1)} ${wingY.toFixed(1)}`;
}
