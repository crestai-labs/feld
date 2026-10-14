// The resize handles: between the Actors and Metrics panes, between the bands and the
// inspector, and at the right edge of every column of the Actor table. Each is dragged with
// the pointer and stepped with the arrow keys; the size is the store's, measured and
// clamped here.

import { useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { ACTOR_WIDTH_LIMITS, actorColumnOption, actorColumnWidth, clampPaneHeight, inspectorBounds, RESIZE_CLASSES } from "./model.ts";
import { useActorsStore } from "./store.ts";
import type { AgentsState } from "./types.ts";
import { timelineMinWidth } from "./charts/interaction.ts";

export type ResizeKind = keyof typeof RESIZE_CLASSES;

interface Gesture {
  pointerId: number;
  startPosition: number;
  startSize: number;
}

// A pointer drag of one handle: the start size is measured when it begins, and every move
// applies the start size plus the pointer's travel along `axis`.
function useResizeGesture(kind: ResizeKind, onResizing: (kind: ResizeKind | null) => void, { axis, startSize, apply, end }: { axis: "x" | "y"; startSize: (handle: HTMLElement) => number; apply: (handle: HTMLElement, delta: number, start: number) => void; end?: () => void }) {
  const gesture = useRef<Gesture | null>(null);
  const [active, setActive] = useState(false);
  const position = (event: PointerEvent) => (axis === "y" ? event.clientY : event.clientX);
  return {
    active,
    handlers: {
      onPointerDown(event: PointerEvent<HTMLElement>) {
        if (event.button !== 0) return;
        gesture.current = { pointerId: event.pointerId, startPosition: position(event), startSize: startSize(event.currentTarget) };
        event.currentTarget.setPointerCapture?.(event.pointerId);
        setActive(true);
        onResizing(kind);
        event.preventDefault();
      },
      onPointerMove(event: PointerEvent<HTMLElement>) {
        const current = gesture.current;
        if (!current || event.pointerId !== current.pointerId) return;
        apply(event.currentTarget, position(event) - current.startPosition, current.startSize);
      },
      onPointerUp: finish,
      onPointerCancel: finish,
    },
  };

  function finish(event: PointerEvent<HTMLElement>) {
    const current = gesture.current;
    if (!current || event.pointerId !== current.pointerId) return;
    gesture.current = null;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    setActive(false);
    onResizing(null);
    end?.();
  }
}

// ---- Actors and Metrics panes ----------------------------------------------------------

function bandsScrollOf(handle: HTMLElement) {
  return handle.closest(".feld-agents__bands")?.querySelector<HTMLElement>(".feld-agents__bands-scroll") || null;
}

// The pane handle moves the Actors pane down to what leaves the Metrics pane its room
// under the Actors header.
function applyPaneHeight(handle: HTMLElement, height: number) {
  const bands = handle.closest<HTMLElement>(".feld-agents__bands");
  const head = bands?.querySelector<HTMLElement>(".feld-agents__actors-head");
  useActorsStore.getState().setAgentsPaneHeight(clampPaneHeight(height, bands ? bands.clientHeight - (head?.offsetHeight || 0) : null));
}

export function PaneResizer({ onResizing }: { onResizing: (kind: ResizeKind | null) => void }) {
  const height = useActorsStore((state) => state.agentsPaneHeight);
  const handle = useRef<HTMLDivElement>(null);
  // Until the reader sets a height, the handle reports the height the layout gave the pane.
  const [measured, setMeasured] = useState<number | null>(null);
  useLayoutEffect(() => {
    if (height !== null || !handle.current) return;
    const pane = bandsScrollOf(handle.current);
    if (pane && pane.clientHeight !== measured) setMeasured(pane.clientHeight);
  });
  const gesture = useResizeGesture("pane", onResizing, {
    // The pane handle is horizontal, so it is dragged vertically.
    axis: "y",
    startSize: (target) => bandsScrollOf(target)?.clientHeight || 240,
    apply: (target, delta, start) => applyPaneHeight(target, start + delta),
  });
  function step(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const current = bandsScrollOf(event.currentTarget)?.clientHeight || 240;
    applyPaneHeight(event.currentTarget, current + (event.key === "ArrowUp" ? -16 : 16));
  }
  return (
    <div className={`feld-agents__pane-resizer${gesture.active ? " is-active" : ""}`} data-pane-resize="" role="separator" tabIndex={0} aria-label="Resize Actors and Metrics tables" aria-orientation="horizontal" aria-valuemin={96} aria-valuemax={520} aria-valuenow={height ?? measured ?? 310} ref={handle} onKeyDown={step} {...gesture.handlers}>
      <span aria-hidden="true"></span>
    </div>
  );
}

// ---- Bands and inspector ---------------------------------------------------------------

function workspaceOf(handle: HTMLElement) {
  return handle.closest<HTMLElement>(".feld-agents__workspace");
}

// A Session Console needs more room than the composer, so the card's limits depend on
// which one the workspace holds.
function boundsOf(workspace: HTMLElement | null) {
  return inspectorBounds(Boolean(workspace?.classList.contains("has-session-console")), workspace?.clientWidth || 900);
}

function applyInspectorWidth(handle: HTMLElement, width: number) {
  const { minimum, maximum } = boundsOf(workspaceOf(handle));
  useActorsStore.getState().setInspectorWidth(Math.max(minimum, Math.min(maximum, width)));
}

function inspectorWidthOf(handle: HTMLElement) {
  return workspaceOf(handle)?.querySelector(".feld-agents__inspector")?.getBoundingClientRect().width || useActorsStore.getState().inspectorWidth || 620;
}

export function InspectorResizer({ onResizing }: { onResizing: (kind: ResizeKind | null) => void }) {
  const width = useActorsStore((state) => state.inspectorWidth);
  const handle = useRef<HTMLDivElement>(null);
  // The handle reports the limits of the card it sizes and the width it was given.
  const [aria, setAria] = useState({ min: 240, max: 2000, now: width || 620 });
  useLayoutEffect(() => {
    const workspace = handle.current && workspaceOf(handle.current);
    const inspector = workspace?.querySelector(".feld-agents__inspector");
    if (!workspace || !inspector) return;
    const { minimum, maximum } = boundsOf(workspace);
    const now = Math.round(inspector.getBoundingClientRect().width);
    if (aria.min !== minimum || aria.max !== maximum || aria.now !== now) setAria({ min: minimum, max: maximum, now });
  });
  const gesture = useResizeGesture("inspector", onResizing, {
    axis: "x",
    startSize: inspectorWidthOf,
    // The card sits right of the handle, so moving right narrows it.
    apply: (target, delta, start) => applyInspectorWidth(target, start - delta),
  });
  function step(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    applyInspectorWidth(event.currentTarget, inspectorWidthOf(event.currentTarget) + (event.key === "ArrowLeft" ? 16 : -16));
  }
  return (
    <div className={`feld-agents__inspector-resizer${gesture.active ? " is-active" : ""}`} data-inspector-resize="" role="separator" tabIndex={0} aria-label="Resize inspector" aria-orientation="vertical" aria-valuemin={aria.min} aria-valuemax={aria.max} aria-valuenow={aria.now} ref={handle} onKeyDown={step} {...gesture.handlers}>
      <span aria-hidden="true"></span>
    </div>
  );
}

// ---- Actor table columns ---------------------------------------------------------------

// `column` is "name" or an Actor table column id.
function widthOf(state: AgentsState, column: string) {
  return column === "name" ? state.actorWidth : actorColumnWidth(state, column);
}

function columnWidthOf(column: string) {
  return widthOf(useActorsStore.getState(), column);
}

function setColumnWidth(column: string, width: number, persist = false) {
  const store = useActorsStore.getState();
  if (column === "name") store.setActorWidth(width, persist);
  else store.setActorColumnWidth(column, width, persist);
}

// The widest a column may grow: what the Timeline has beyond its minimum width. The last
// column's handle is the boundary between the table and the Timeline, and every column
// widens the table.
function roomOf(handle: HTMLElement, width: number) {
  const timeline = handle.closest(".feld-agents__bands-head")?.querySelector<HTMLElement>(".feld-agents__timeline-head");
  return timeline ? width + Math.max(0, Math.floor(timeline.getBoundingClientRect().width - timelineMinWidth(timeline))) : Number.POSITIVE_INFINITY;
}

export function ColumnResizer({ column, label, onResizing }: { column: string; label: string; onResizing: (kind: ResizeKind | null) => void }) {
  const width = useActorsStore((state) => widthOf(state, column));
  const limits = column === "name" ? ACTOR_WIDTH_LIMITS : actorColumnOption(column) || ACTOR_WIDTH_LIMITS;
  const room = useRef(Number.POSITIVE_INFINITY);
  const gesture = useResizeGesture("actor", onResizing, {
    axis: "x",
    startSize: (target) => {
      const start = columnWidthOf(column);
      room.current = roomOf(target, start);
      return start;
    },
    apply: (_target, delta, start) => setColumnWidth(column, Math.min(room.current, start + delta)),
    // The width is stored once the drag ends.
    end: () => setColumnWidth(column, columnWidthOf(column), true),
  });
  function step(event: KeyboardEvent<HTMLSpanElement>) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    setColumnWidth(column, Math.min(roomOf(event.currentTarget, width), width + (event.key === "ArrowLeft" ? -16 : 16)), true);
  }
  return <span className={`feld-agents__actor-resizer${gesture.active ? " is-active" : ""}`} data-actor-resize={column} role="separator" tabIndex={0} aria-label={`Resize ${label} column`} aria-orientation="vertical" aria-valuemin={limits.min} aria-valuemax={limits.max} aria-valuenow={width} onKeyDown={step} {...gesture.handlers}></span>;
}
