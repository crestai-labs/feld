// The pointer, wheel, and key handling of the Actors charts: the pan of the timeline and
// the Metrics lanes, the hover that reads a value under the pointer, and the records a
// click on the timeline opens.

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type MouseEvent, type PointerEvent, type RefObject } from "react";
import { PLOT_PAD_LEFT, PLOT_PAD_RIGHT, TIMELINE_MIN_WIDTH, metricsDurationFor, type ActorsModel } from "../model.ts";
import { useActorsStore } from "../store.ts";
import { usePageActions } from "../ui.tsx";
import type { AgentsState, Instant } from "../types.ts";

// The latest value of a render, for handlers that outlive it.
export function useLatest<T>(value: T) {
  const ref = useRef(value);
  useLayoutEffect(() => {
    ref.current = value;
  });
  return ref;
}

// ---- Size --------------------------------------------------------------------------------

export function timelineMinWidth(element: Element) {
  const bands = element.closest(".feld-agents__bands");
  const declared = bands ? Number.parseFloat(getComputedStyle(bands).getPropertyValue("--agents-timeline-min-width")) : Number.NaN;
  return Number.isFinite(declared) && declared > 0 ? declared : TIMELINE_MIN_WIDTH;
}

export interface PlotBox {
  // The parent's client size, 0 before it is measured, and the table's timeline minimum.
  clientWidth: number;
  clientHeight: number;
  minWidth: number;
}

const UNMEASURED: PlotBox = Object.freeze({ clientWidth: 0, clientHeight: 0, minWidth: TIMELINE_MIN_WIDTH });

// A chart is drawn at its parent's size, measured after every render of the chart (a
// drawing can change the parent's height) and whenever the parent is resized.
export function useParentBox(ref: RefObject<Element | null>): PlotBox {
  const [box, setBox] = useState<PlotBox>(UNMEASURED);
  const measure = useCallback(() => {
    const parent = ref.current?.parentElement;
    if (!parent) return;
    const next = { clientWidth: parent.clientWidth, clientHeight: parent.clientHeight, minWidth: timelineMinWidth(parent) };
    setBox((current) => current.clientWidth === next.clientWidth && current.clientHeight === next.clientHeight && current.minWidth === next.minWidth ? current : next);
  }, [ref]);
  useLayoutEffect(measure);
  useEffect(() => {
    const parent = ref.current?.parentElement;
    if (!parent || typeof ResizeObserver !== "function") return undefined;
    const observer = new ResizeObserver(measure);
    observer.observe(parent);
    return () => observer.disconnect();
  }, [ref, measure]);
  return box;
}

// ---- Range --------------------------------------------------------------------------------

// Where a move of the range starts from: a Live range ends at now, so its anchor is its
// centre at the model's now rather than the anchor it was opened with.
function anchorOf(model: ActorsModel, state: AgentsState) {
  const range = model.rangeFor(state);
  return state.mode === "live" ? (range.start + range.end) / 2 : state.anchor;
}

// ---- Gestures -----------------------------------------------------------------------------

// The plot a gesture is on: the Timeline draws the range, Metrics its wider window, and a
// drag on either moves the Timeline by the time its own scale puts under the pointer.
export type Plot = "timeline" | "metrics";

function scaleOf(plot: Plot, state: AgentsState) {
  return plot === "metrics" ? metricsDurationFor(state) : state.duration;
}

interface PanGesture {
  pointerId: number;
  startX: number;
  anchor: Instant;
  width: number;
  scale: number;
  moved: boolean;
  target: Element;
  // The element the press was on, under the plot that captures the pointer.
  pressed: Element;
}

// The element a click on a plot reads. A press that may begin a pan captures the pointer on
// the plot, so the click that ends it lands on the plot itself; it reads the element the
// press was on.
export function plotClickTarget(target: Element, plot: Element, pressed: Element | null) {
  return target === plot && pressed ? pressed : target;
}

const HIT_SELECTOR = "[data-session], [data-action-link], [data-message-link], [data-relation-bundle], [data-skip]";
const LANE_HIT = ".feld-agents__lane-hit[data-focus-actor]";

// A lane the pointer is about to focus (a press on it, or a click on its Actor's row) is
// marked, so it draws no focus outline; the mark is lifted when the lane blurs or the
// keyboard moves the focus (see Timeline.tsx). Returns the lane marked, if any.
export function markPointerFocusedLane(target: Element | null) {
  const lane = target?.closest<SVGElement>(LANE_HIT) || null;
  lane?.setAttribute("data-pointer-focus", "");
  return lane;
}

// The pan of the plots. A gesture keeps the bands it started with until it ends, so the
// lanes under the pointer do not change mid-drag, and moves the range at most once a frame.
function usePan(model: ActorsModel, plot: Plot) {
  const modelRef = useLatest(model);
  const gesture = useRef<PanGesture | null>(null);
  const pending = useRef<{ frame: number; anchor: Instant } | null>(null);
  // A drag of a plot ends in a click on it, which must not open a record.
  const suppressClick = useRef(false);
  // A press that did not move ends in a click on the plot; it reads the element pressed.
  const pressed = useRef<Element | null>(null);

  const flush = useCallback(() => {
    const next = pending.current;
    if (!next) return;
    cancelAnimationFrame(next.frame);
    pending.current = null;
    if (gesture.current) useActorsStore.getState().panTo(next.anchor, modelRef.current.now());
  }, [modelRef]);

  useEffect(() => () => {
    if (pending.current) cancelAnimationFrame(pending.current.frame);
  }, []);

  const begin = useCallback((event: PointerEvent<Element>, target: Element, width: number) => {
    const store = useActorsStore.getState();
    const current = modelRef.current;
    store.beginGesture(current.bandAgents(store).map((agent) => agent.id));
    // Pressing a chart keeps the range: only a move takes it off Live.
    const anchor = anchorOf(current, store);
    gesture.current = { pointerId: event.pointerId, startX: event.clientX, anchor, width, scale: scaleOf(plot, store), moved: false, target, pressed: event.target as Element };
    target.setPointerCapture?.(event.pointerId);
  }, [modelRef, plot]);

  // Returns whether a gesture took the move.
  const move = useCallback((event: PointerEvent<Element>) => {
    const pan = gesture.current;
    if (!pan) return false;
    const deltaX = event.clientX - pan.startX;
    if (!pan.moved && Math.abs(deltaX) < 4) return true;
    pan.moved = true;
    pan.target.closest(".feld-agents")?.classList.add("is-panning");
    const state = useActorsStore.getState();
    const anchor = modelRef.current.anchorUpToNow(pan.anchor - deltaX / pan.width * pan.scale, state.duration);
    if (pending.current) {
      pending.current.anchor = anchor;
    } else {
      pending.current = { anchor, frame: requestAnimationFrame(flush) };
    }
    return true;
  }, [flush, modelRef]);

  const end = useCallback((event: PointerEvent<Element>) => {
    const pan = gesture.current;
    if (!pan || event.pointerId !== pan.pointerId) return;
    flush();
    gesture.current = null;
    pan.target.releasePointerCapture?.(event.pointerId);
    pan.target.closest(".feld-agents")?.classList.remove("is-panning");
    if (pan.moved) {
      suppressClick.current = true;
      setTimeout(() => { suppressClick.current = false; }, 0);
    } else {
      pressed.current = pan.pressed;
      setTimeout(() => { pressed.current = null; }, 0);
    }
    useActorsStore.getState().endGesture();
  }, [flush]);

  return { begin, move, end, suppressClick, pressed };
}

// Where a hovering pointer is; null when no pointer hovers the plot.
export interface PointerAt {
  clientX: number;
  clientY: number;
  // The element the pointer moved onto.
  target?: Element | null;
}

// The timeline and the Metrics lanes: a drag pans, a click on a record opens it, and a
// sideways wheel pans. A mouse or pen that hovers without a gesture reaches `hover`; a
// touch, a gesture, a wheel, and leaving the plot clear it. `pickSession` names the Session
// a click on overlapping Session hit areas opens.
export function usePlotHandlers(model: ActorsModel, svgRef: RefObject<SVGSVGElement | null>, plot: Plot, hover: (at: PointerAt | null) => void, pickSession?: (at: PointerAt) => string | null) {
  const modelRef = useLatest(model);
  const pan = usePan(model, plot);

  // Opening a record: a Session opens its Console, a Spawn or a Message the Console that
  // holds it at its row, and a skipped tick the Job modal at that tick.
  const actions = usePageActions();
  const open = useCallback((target: Element, picked?: string | null) => {
    const { session, skip, agentId, actionLink, messageLink } = (target as HTMLElement | SVGElement).dataset;
    if (skip) actions.openJob(agentId, skip);
    else if (session) actions.openConsole(picked || session);
    else if (actionLink) actions.openRecordConsole("action", actionLink);
    else if (messageLink) actions.openRecordConsole("message", messageLink);
  }, [actions]);

  const onPointerDown = useCallback((event: PointerEvent<SVGSVGElement>) => {
    if (event.button !== 0) return;
    markPointerFocusedLane(event.target as Element);
    if ((event.target as Element).closest(HIT_SELECTOR)) return;
    const rect = event.currentTarget.getBoundingClientRect();
    pan.begin(event, event.currentTarget, Math.max(rect.width - PLOT_PAD_LEFT - PLOT_PAD_RIGHT, 1));
  }, [pan]);

  const hoverRef = useLatest(hover);
  const onPointerMove = useCallback((event: PointerEvent<SVGSVGElement>) => {
    const panning = pan.move(event);
    hoverRef.current(panning || event.pointerType === "touch" ? null : { clientX: event.clientX, clientY: event.clientY, target: event.target as Element });
  }, [pan, hoverRef]);

  const onPointerLeave = useCallback(() => {
    hoverRef.current(null);
  }, [hoverRef]);

  // Clicking a Session opens its Console; Spawn and Message links open the Console that
  // holds them; a skipped tick opens its Schedule; a bundle of several Messages opens none.
  // Elsewhere on a lane, a click selects the lane's Actor or clears it. A click the plot
  // took is marked handled (defaultPrevented), so the page leaves its open menus as they are.
  const onClick = useCallback((event: MouseEvent<SVGSVGElement>) => {
    if (pan.suppressClick.current) {
      pan.suppressClick.current = false;
      event.preventDefault();
      return;
    }
    const target = plotClickTarget(event.target as Element, event.currentTarget, pan.pressed.current);
    pan.pressed.current = null;
    const hit = target.closest(HIT_SELECTOR);
    if (hit) {
      event.preventDefault();
      open(hit, pickSession?.({ clientX: event.clientX, clientY: event.clientY }));
      return;
    }
    const lane = target.closest<SVGElement>("[data-focus-actor]");
    if (lane) {
      event.preventDefault();
      useActorsStore.getState().toggleActorFocus(lane.dataset.focusActor!);
    }
  }, [open, pan, pickSession]);

  const onKeyDown = useCallback((event: KeyboardEvent<SVGSVGElement>) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    const lane = (event.target as Element).closest<SVGElement>(".feld-agents__lane-hit[data-focus-actor]");
    if (lane) {
      event.preventDefault();
      useActorsStore.getState().toggleActorFocus(lane.dataset.focusActor!);
      return;
    }
    const hit = (event.target as Element).closest(".feld-agents__session-hit, .feld-agents__relation-hit, .feld-agents__skip-hit");
    if (!hit) return;
    event.preventDefault();
    open(hit);
  }, [open]);

  // A sideways wheel (or a shifted one) pans the range. React's wheel listener is passive,
  // so this one is added to the element to keep the page from scrolling.
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return undefined;
    const onWheel = (event: WheelEvent) => {
      hoverRef.current(null);
      const horizontalIntent = Math.abs(event.deltaX) >= Math.abs(event.deltaY) || event.shiftKey;
      if (!horizontalIntent) return;
      event.preventDefault();
      const current = modelRef.current;
      const state = useActorsStore.getState();
      const start = anchorOf(current, state);
      const pixels = event.deltaX || event.deltaY;
      const width = Math.max(svg.getBoundingClientRect().width - PLOT_PAD_LEFT - PLOT_PAD_RIGHT, 1);
      const anchor = current.anchorUpToNow(start + pixels / width * scaleOf(plot, state), state.duration);
      if (anchor === start) return;
      state.panTo(anchor, current.now());
    };
    svg.addEventListener("wheel", onWheel, { passive: false });
    return () => svg.removeEventListener("wheel", onWheel);
  }, [modelRef, hoverRef, svgRef, plot]);

  return { onPointerDown, onPointerMove, onPointerLeave, onPointerUp: pan.end, onPointerCancel: pan.end, onClick, onKeyDown };
}
