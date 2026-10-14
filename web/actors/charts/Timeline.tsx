// The Actors timeline: the vertical grid, one lane per band, the Sessions along it, and
// the records between lanes. The time axis is a row of its own under the last lane (see
// Axis.tsx). The drawing changes with the range, the bands, and the display settings.

import { Fragment, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type FocusEvent, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { useShallow } from "zustand/react/shallow";
import { useActorsModel } from "../context.ts";
import {
  AXIS_ROW_HEIGHT,
  COMPACT_LAYOUT_QUERY,
  LANE,
  PLOT_PAD_LEFT,
  PLOT_PAD_RIGHT,
  arrowheadPath,
  formatDuration,
  gridLines,
  laneYFor,
  mailboxMarkerPath,
  placementSummary,
  plotX,
  runsOn,
  selectedSessionId,
  tokenBinDuration,
  type ActorsModel,
} from "../model.ts";
import { useActorsStore } from "../store.ts";
import type { ActorRecord, AgentSession, AgentsState, Focus, Instant, Lane, MessageRecord } from "../types.ts";
import { histogramTooltip, markTooltip, useChartTooltip, viewPointFor, type HistogramHit } from "./hover.tsx";
import { useParentBox, usePlotHandlers, type PointerAt } from "./interaction.ts";
import { laneKeyTarget } from "./laneKeys.ts";
import { edgeChevronPath, foldIncomingBundles, relationBinDuration, relationBundlesFor, relationBusGeometry, relationCountLabel, relationSingle, type Emphasis, type RelationBundle } from "./relationBus.ts";

// What the timeline's lanes and records read from the page state.
export type TimelineView = Pick<AgentsState,
  | "workspace" | "mode" | "anchor" | "duration" | "showSubAgents" | "inspectedSessionId" | "actorCard" | "focusedActorId"
  | "sessionMetric" | "showScheduleSkips" | "showSpawnLinks" | "showMessageLinks">;

function selectTimelineView(state: AgentsState): TimelineView {
  return {
    workspace: state.workspace,
    mode: state.mode,
    anchor: state.anchor,
    duration: state.duration,
    showSubAgents: state.showSubAgents,
    inspectedSessionId: state.inspectedSessionId,
    actorCard: state.actorCard,
    focusedActorId: state.focusedActorId,
    sessionMetric: state.sessionMetric,
    showScheduleSkips: state.showScheduleSkips,
    showSpawnLinks: state.showSpawnLinks,
    showMessageLinks: state.showMessageLinks,
  };
}

// Half the side of the square around a Session's end marker that opens the Session and
// shows its end in a tooltip, whatever the length of the line.
const MARKER_HIT = 10;
// Half the width of the stroke that hits a Session line (see .feld-agents__session-hit).
const LINE_HIT = 14;

// A Session's line and end marker, where a click or a hover on the timeline reads it.
export interface SessionTarget {
  session: AgentSession;
  startX: number;
  endX: number;
  y: number;
  // The end marker's centre; null when the line runs on past the range.
  markerX: number | null;
  // Where the line turns dashed; null when it stays solid.
  lostX: number | null;
  label: string;
  reason: string;
}

function inMarker(target: SessionTarget, point: { x: number; y: number }) {
  return target.markerX !== null && Math.abs(point.x - target.markerX) <= MARKER_HIT && Math.abs(point.y - target.y) <= MARKER_HIT;
}

function distanceTo(target: SessionTarget, point: { x: number; y: number }) {
  const dx = point.x - Math.max(target.startX, Math.min(target.endX, point.x));
  return Math.hypot(dx, point.y - target.y);
}

// The Session a click or a hover at the point reads. Within the square around an end
// marker, the marker whose centre is nearest wins over any line, so the tooltip and the
// click name the same Session; elsewhere the nearest line whose hit area lies under the
// point wins. `markerOnly` reads only the end markers.
export function sessionTargetAt(targets: SessionTarget[], point: { x: number; y: number }, markerOnly = false) {
  let nearest: SessionTarget | null = null;
  let nearestDistance = Infinity;
  let nearestOnMarker = false;
  for (const target of targets) {
    const onMarker = inMarker(target, point);
    const onLine = !markerOnly && point.x >= target.startX && point.x <= target.endX && Math.abs(point.y - target.y) <= LINE_HIT;
    if (!onMarker && (!onLine || nearestOnMarker)) continue;
    const distance = onMarker ? Math.hypot(point.x - target.markerX!, point.y - target.y) : distanceTo(target, point);
    if ((onMarker && !nearestOnMarker) || distance < nearestDistance) {
      nearest = target;
      nearestDistance = distance;
      nearestOnMarker = onMarker;
    }
  }
  return nearest;
}

export interface TimelineLayers {
  width: number;
  height: number;
  range: FeldMetricRange;
  lanes: Map<string, Lane>;
  focus: Focus | null;
  // The marks, and the hit targets over them.
  marks: ReactNode;
  // The hit targets of the lanes, the Sessions, and the skipped ticks; the end markers'
  // squares lie over the Spawn and Message hit lines as well.
  hits: ReactNode;
  markerHits: ReactNode;
  // The histogram bars, by their `data-histogram-bin`.
  bins: HistogramHit[];
  sessions: SessionTarget[];
}

// The drawing of the bands `agents` at `width`. With no band it extends down to
// `parentHeight`, which keeps a surface to pan on.
export function timelineLayers(model: ActorsModel, view: TimelineView, agents: ActorRecord[], width: number, parentHeight: number): TimelineLayers {
  const range = model.rangeFor(view);
  const lanes = model.laneLayoutFor(agents, view);
  const plotHeight = lanes.height;
  // The lanes keep their natural height, so the axis row sits right under the last one.
  const height = agents.length ? plotHeight : Math.max(plotHeight, parentHeight || plotHeight);
  const usable = width - PLOT_PAD_LEFT - PLOT_PAD_RIGHT;
  const binDuration = tokenBinDuration(view.duration, usable);
  const x = (time: Instant) => plotX(range, width, time);
  const clampedX = (time: Instant) => Math.max(PLOT_PAD_LEFT, Math.min(width - PLOT_PAD_RIGHT, x(time)));
  const agentIds = new Set(agents.map((agent) => agent.id));
  const focus = model.focusFor(view);
  const muteClass = model.muteClassFor(focus);
  // timelineRecordsFor reads only the display switches of the view.
  const records = model.timelineRecordsFor(view as AgentsState, agentIds, range);
  // The vertical grid: a line at every labelled tick and a lighter one at each minor step
  // between them, so a mark reads against the scale however far it sits from the axis.
  const marks: ReactNode[] = usable > 0 ? gridLines(range, usable, model.TIME_ZONE).map(({ instant, major }) => (
    <line className={`feld-agents__grid-line${major ? " is-major" : ""}`} x1={x(instant).toFixed(1)} y1="0" x2={x(instant).toFixed(1)} y2={height} key={`grid:${instant}`} />
  )) : [];
  const sessionHits: ReactNode[] = [];
  const relationHits: ReactNode[] = [];
  const markerHits: ReactNode[] = [];
  const bins: HistogramHit[] = [];
  const sessionTargets: SessionTarget[] = [];

  agents.forEach((agent, index) => {
    if (index > 0) {
      const laneTop = lanes.agents.get(agent.id)!.top;
      marks.push(<line className="feld-agents__lane-separator" x1="0" y1={laneTop} x2={width} y2={laneTop} key={`separator:${agent.id}`} />);
    }
  });
  const selectedLane = view.focusedActorId ? lanes.agents.get(view.focusedActorId) : null;
  if (selectedLane) marks.push(<rect className="feld-agents__actor-focus-band" x="0" y={selectedLane.top} width={width} height={selectedLane.height} key="actor-focus" />);

  records.sessions.forEach((session) => {
    const lane = lanes.agents.get(session.actor)!;
    const y = laneYFor(lane, session.id);
    const start = model.timelineStart(session);
    const end = model.sessionEnd(session);
    const clippedStart = Math.max(start, range.start);
    const clippedEnd = Math.min(end, range.end);
    const startX = clampedX(clippedStart);
    const endX = clampedX(clippedEnd);
    const outcome = model.outcomeFor(session);
    const lost = model.lostSince(session);
    const inspected = session.id === selectedSessionId(view);
    const related = focus?.sessions.has(session.id);
    const muted = muteClass(related);
    const sessionClass = `feld-agents__session${session.parentSession ? " is-sub-agent" : ""}${inspected ? " is-inspected" : ""}${muted}`;
    const parts: ReactNode[] = [];
    let sessionPath: string | null = null;
    let horizontalStartX = startX;
    let dashedX: number | null = null;
    if (session.parentSession && start >= range.start) {
      // The branch leaves the parent Session's own row, the band's main line.
      const parentY = laneYFor(lane, session.parentSession);
      horizontalStartX = Math.min(endX, startX + 11);
      const bendY = y - Math.min(16, Math.abs(y - parentY) / 2);
      sessionPath = `M ${startX.toFixed(1)} ${parentY} C ${startX.toFixed(1)} ${bendY.toFixed(1)}, ${(startX + 3).toFixed(1)} ${y}, ${horizontalStartX.toFixed(1)} ${y} L ${endX.toFixed(1)} ${y}`;
      parts.push(<circle className={`feld-agents__sub-agent-fork${muted}`} cx={startX.toFixed(1)} cy={parentY} r="3" key="fork"><title>{`${session.subAgent} spawned from ${session.parentSession}`}</title></circle>);
    }
    if (sessionPath) {
      parts.push(<path className={sessionClass} d={sessionPath} key="line"><title>{`${session.id}: ${session.subAgent} · ${runsOn(session)}`}</title></path>);
    } else {
      // The line is solid while the execution is known to live and dashed from the
      // instant Feld lost sight of it.
      const lostX = lost === null || lost >= clippedEnd ? endX : clampedX(Math.max(lost, clippedStart));
      if (lostX > startX || lostX === endX) parts.push(<line className={sessionClass} x1={startX.toFixed(1)} y1={y} x2={lostX.toFixed(1)} y2={y} key="line"><title>{`${session.id}: ${runsOn(session)}`}</title></line>);
      if (lostX < endX) {
        dashedX = lostX;
        parts.push(<line className={`${sessionClass} is-uncertain`} x1={lostX.toFixed(1)} y1={y} x2={endX.toFixed(1)} y2={y} key="lost"><title>{`${session.id}: ${model.lostLabel(session)}`}</title></line>);
      }
    }
    const hitDescription = `${session.subAgent ? `${session.subAgent}, ` : ""}${runsOn(session)}`;
    const hitLabel = `Open ${session.id} Console: ${hitDescription}`;
    const hitTitle = <title>{`${hitLabel} / ${placementSummary(session)}`}</title>;
    sessionHits.push(sessionPath
      ? <path className="feld-agents__session-hit" d={sessionPath} data-session={session.id} data-lane={session.actor} data-at={startX.toFixed(1)} role="button" tabIndex={-1} aria-label={hitLabel} key={session.id}>{hitTitle}</path>
      : <line className="feld-agents__session-hit" x1={startX.toFixed(1)} y1={y} x2={endX.toFixed(1)} y2={y} data-session={session.id} data-lane={session.actor} data-at={startX.toFixed(1)} role="button" tabIndex={-1} aria-label={hitLabel} key={session.id}>{hitTitle}</line>);
    // The square around the end marker keeps a Session that ended within seconds as easy
    // to open as a long one; its tooltip comes from the chart tooltip. It lies over the
    // Spawn and Message lines, which stay open along the rest of their length.
    const markerX = end > range.end ? null : endX;
    if (markerX !== null) {
      markerHits.push(<rect className="feld-agents__session-marker-hit" x={(markerX - MARKER_HIT).toFixed(1)} y={y - MARKER_HIT} width={2 * MARKER_HIT} height={2 * MARKER_HIT} rx="4" data-session={session.id} aria-hidden="true" key={`marker:${session.id}`} />);
    }
    sessionTargets.push({
      session,
      startX: horizontalStartX,
      endX,
      y,
      markerX,
      lostX: dashedX,
      label: outcome.label,
      reason: outcome.reason,
    });

    if (view.sessionMetric !== "none") {
      const metricBins = model.sessionMetricBins(session, range, binDuration, view.sessionMetric);
      const maximum = view.sessionMetric === "context"
        ? 100
        : Math.max(...metricBins.map((bin) => bin.value), 1);
      metricBins.forEach((bin, index) => {
        const binStartX = clampedX(bin.start);
        const binEndX = clampedX(bin.end);
        const gap = Math.min(.8, Math.max(0, (binEndX - binStartX) * .08));
        const barHeight = LANE.barBase + bin.value / maximum * LANE.barRange;
        parts.push(
          <rect
            className={`feld-agents__token-bar${view.sessionMetric === "context" ? " is-context" : ""}${muteClass(related)}`}
            x={(binStartX + gap / 2).toFixed(1)}
            y={(y - barHeight - LANE.barBase).toFixed(1)}
            width={Math.max(.7, binEndX - binStartX - gap).toFixed(1)}
            height={barHeight.toFixed(1)}
            data-histogram-bin={bins.length}
            key={`bin:${index}`}
          />,
        );
        bins.push({ left: binStartX, right: binEndX, top: y - 2 * LANE.barBase - LANE.barRange, bottom: y - LANE.barBase, session, bin, binDuration });
      });
    }

    if (start < range.start) {
      parts.push(<path className={`feld-agents__continuation${muteClass(related)}`} d={`M ${PLOT_PAD_LEFT + 6} ${y - 6} L ${PLOT_PAD_LEFT} ${y} L ${PLOT_PAD_LEFT + 6} ${y + 6}`} key="start" />);
    } else if (session.parentSession) {
      // The fork node on the parent line is the Sub Agent's start marker.
    } else if (session.startKind === "replacement") {
      parts.push(<rect className={`feld-agents__event${muteClass(related)}`} x={(startX - 5).toFixed(1)} y={y - 5} width="10" height="10" transform={`rotate(45 ${startX.toFixed(1)} ${y})`} key="start"><title>Replacement session</title></rect>);
    } else if (session.startKind === "attach") {
      // A hollow start: Feld observed a Session it did not start.
      parts.push(<circle className={`feld-agents__event${muteClass(related)}`} cx={startX.toFixed(1)} cy={y} r="4" key="start"><title>Session attached · Feld did not start this process</title></circle>);
    } else {
      // A scheduled start keeps the filled circle: the periodicity is visible from the
      // spacing of the runs, and the tooltip and Details name the schedule.
      parts.push(<circle className={`feld-agents__event-fill${muteClass(related)}`} cx={startX.toFixed(1)} cy={y} r="4" key="start"><title>{session.startKind === "schedule" ? `Session started by schedule · planned ${model.timeOnly.format(session.planned!)}` : "Session started"}</title></circle>);
    }

    // End markers follow the outcome concluded from ownership and the kind rule, not the
    // raw exit:
    //   filled circle   stopped after a request, or an attached / Sub Agent Session that ended
    //   end bar         owned Job run completed on its own (succeeded)
    //   ×               needs attention: owned Job failed, or owned Agent exited without request
    //   hollow circle   outcome unknown
    //   none            running: the line stops at now, so a line without a mark goes on
    const outcomeTitle = `${outcome.label} · ${outcome.detail}`;
    if (end > range.end) {
      const edge = width - PLOT_PAD_RIGHT;
      parts.push(<path className={`feld-agents__continuation${muteClass(related)}`} d={`M ${edge - 6} ${y - 6} L ${edge} ${y} L ${edge - 6} ${y + 6}`} key="end" />);
    } else if (outcome.key === "failed" || outcome.key === "exited") {
      parts.push(<path className={`feld-agents__event is-attention is-${outcome.key}${muteClass(related)}`} d={`M ${endX - 5} ${y - 5} L ${endX + 5} ${y + 5} M ${endX + 5} ${y - 5} L ${endX - 5} ${y + 5}`} key="end"><title>{outcomeTitle}</title></path>);
    } else if (outcome.key === "succeeded") {
      parts.push(<path className={`feld-agents__event is-succeeded${muteClass(related)}`} d={`M ${endX.toFixed(1)} ${y - 6} L ${endX.toFixed(1)} ${y + 6}`} key="end"><title>{outcomeTitle}</title></path>);
    } else if (outcome.key === "unknown") {
      parts.push(<circle className={`feld-agents__event${muteClass(related)}`} cx={endX.toFixed(1)} cy={y} r="4" key="end"><title>{outcomeTitle}</title></circle>);
    } else if (session.end !== null) {
      parts.push(<circle className={`feld-agents__event-fill${muteClass(related)}`} cx={endX.toFixed(1)} cy={y} r="4" key="end"><title>{outcomeTitle}</title></circle>);
    }
    marks.push(<Fragment key={`session:${session.id}`}>{parts}</Fragment>);
  });

  // A skipped tick sits on the Actor's main line at its planned instant, on top of
  // the Session line that was still running there: the line continuing through the mark
  // is the reason the tick was skipped.
  records.skips.forEach((skip) => {
    const lane = lanes.agents.get(skip.actor)!;
    const skipX = clampedX(skip.planned);
    const y = lane.mainY;
    const muted = muteClass(!focus || focus.sessions.has(skip.previousSession));
    const title = `Schedule tick skipped · planned ${model.timeOnly.format(skip.planned)} · ${skip.previousSession} was still current · schedule.skipped record (Control Plane fact)`;
    marks.push(<g className={`feld-agents__skip${muted}`} key={`skip:${skip.id}`}><circle cx={skipX.toFixed(1)} cy={y} r="4.5" /><path d={`M ${(skipX - 3).toFixed(1)} ${y + 3} L ${(skipX + 3).toFixed(1)} ${y - 3}`} /><title>{title}</title></g>);
    relationHits.push(<circle className="feld-agents__skip-hit" cx={skipX.toFixed(1)} cy={y} r="9" data-skip={skip.id} data-agent-id={skip.actor} data-lane={skip.actor} data-at={skipX.toFixed(1)} role="button" tabIndex={-1} aria-label={`Open the Schedule of ${model.actorLabel(skip.actor, view)} at the skipped ${model.timeOnly.format(skip.planned)} tick`} key={`skip:${skip.id}`}><title>{`${title} · click to open the Schedule`}</title></circle>);
  });

  const laneHits = agents.map((agent) => {
    const lane = lanes.agents.get(agent.id)!;
    const selected = view.focusedActorId === agent.id;
    return <rect className={`feld-agents__lane-hit${selected ? " is-selected" : ""}`} x="0" y={lane.top} width={width} height={lane.height} data-focus-actor={agent.id} role="button" tabIndex={0} aria-pressed={selected} key={`lane:${agent.id}`}><title>{`${agent.id} · select relationships`}</title></rect>;
  });

  return {
    width,
    height,
    range,
    lanes: lanes.agents,
    focus,
    marks,
    hits: <>{laneHits}{sessionHits}{relationHits}</>,
    markerHits,
    bins,
    sessions: sessionTargets,
  };
}

// The vertically visible part of the timeline, in its coordinates: the rows between the
// sticky table head and the time axis that cover the scrolling pane's edges.
export interface TimelineViewport {
  top: number;
  bottom: number;
}

// The edges a relationship stops at sit this far inside the viewport, so a chevron is
// whole on screen.
const VIEWPORT_INSET = 8;

const EMPHASIS_CLASS: Record<Emphasis, string> = { "": "", related: " is-related", muted: " is-muted" };

export interface RelationLayers {
  marks: ReactNode;
  hits: ReactNode;
  // The bundles, by their `data-relation-bundle`.
  bundles: RelationBundle[];
}

// The relation buses over `layers`, clipped to the vertical viewport: a
// relationship whose other end is scrolled away stops at the viewport edge under a chevron,
// and one whose two ends are both scrolled away is left out until either enters.
export function relationLayers(model: ActorsModel, view: TimelineView, agents: ActorRecord[], layers: TimelineLayers, viewport: TimelineViewport): RelationLayers {
  const { width, range, lanes, focus } = layers;
  const usable = width - PLOT_PAD_LEFT - PLOT_PAD_RIGHT;
  const clampedX = (time: Instant) => Math.max(PLOT_PAD_LEFT, Math.min(width - PLOT_PAD_RIGHT, plotX(range, width, time)));
  const top = Math.min(viewport.bottom, viewport.top + VIEWPORT_INSET);
  const bottom = Math.max(top, viewport.bottom - VIEWPORT_INSET);
  const records = model.timelineRecordsFor(view as AgentsState, new Set(agents.map((agent) => agent.id)), range);
  const marks: ReactNode[] = [];
  const hits: ReactNode[] = [];

  // The Messages and Spawns of one source Session in one absolute-clock bin share a bus
  // (see relationBus.ts). A bus of one record opens like that record; a bundle of several
  // shows its records in the tooltip and opens none of them.
  const binDuration = relationBinDuration(view.duration, usable);
  const bundles = foldIncomingBundles(relationBundlesFor(records.messages, records.spawns, binDuration), lanes, top, bottom);
  const binX = (start: Instant) => clampedX(Math.max(range.start, Math.min(range.end, start + binDuration / 2)));
  const drawn: RelationBundle[] = [];
  bundles.forEach((bundle) => {
    const geometry = relationBusGeometry(bundle, lanes, top, bottom, focus);
    if (!geometry) return;
    const index = drawn.length;
    const bundleX = binX(bundle.start);
    const x = bundleX.toFixed(1);
    const key = `${bundle.foldedSource || "own"}:${bundle.start}:${bundle.fromWorkspace}:${bundle.from}:${bundle.fromSessionId}:${bundle.foldedSource ? bundle.recipients[0].actor : ""}`;
    const sourceLabels = [...new Set([
      ...bundle.messages.map((message) => (message.from === "Admin" ? "Admin" : model.actorLabel(message.from, view, message.fromWorkspace))),
      ...bundle.spawns.map((action) => (action.spawner === "Admin" ? "Admin" : model.actorLabel(action.spawner, view, action.spawnerWorkspace))),
    ])];
    const sourceLabel = sourceLabels.length === 1 ? sourceLabels[0] : `${sourceLabels.length} senders`;
    const recipientLabels = bundle.recipients.map((recipient) => model.actorLabel(recipient.actor, view, recipient.workspace));
    const recipientSummary = recipientLabels.length <= 4 ? recipientLabels.join(", ") : `${recipientLabels.slice(0, 3).join(", ")} and ${recipientLabels.length - 3} more`;
    const single = relationSingle(bundle);
    const title = `${relationCountLabel(bundle)} from ${sourceLabel} to ${recipientSummary} · ${formatDuration(binDuration)} bin`;
    marks.push(
      <g className="feld-agents__relation-bundle-visual" data-relation-bundle-mark={index} key={key}>
        <line className={`feld-agents__relation-link${EMPHASIS_CLASS[geometry.trunk]}${geometry.stub ? " is-stub" : ""}`} x1={x} y1={geometry.fromY} x2={x} y2={geometry.toY} />
        {geometry.overlay ? <line className="feld-agents__relation-link is-related is-focus-overlay" x1={x} y1={geometry.overlay.fromY} x2={x} y2={geometry.overlay.toY} /> : null}
        {geometry.terminals.map((terminal, at) => (terminal.waiting
          ? <path className={`feld-agents__mailbox-marker${EMPHASIS_CLASS[terminal.emphasis]}`} d={mailboxMarkerPath(bundleX, terminal.fromY, terminal.y)} key={`end:${at}`} />
          : <path className={`feld-agents__relation-arrow${EMPHASIS_CLASS[terminal.emphasis]}`} d={arrowheadPath(bundleX, terminal.fromY, terminal.y)} key={`end:${at}`} />))}
        {geometry.edges.map((edge, at) => <path className={`feld-agents__relation-edge${EMPHASIS_CLASS[edge.emphasis]}${edge.hidden ? " is-hidden" : ""}`} d={edgeChevronPath(bundleX, edge.y, edge.direction)} key={`edge:${at}`} />)}
      </g>,
    );
    hits.push(
      <line
        className="feld-agents__relation-hit"
        x1={x}
        y1={geometry.fromY}
        x2={x}
        y2={geometry.toY === geometry.fromY ? geometry.toY + 1 : geometry.toY}
        data-relation-bundle={index}
        data-message-link={single && "message" in single ? single.message.id : undefined}
        data-action-link={single && "spawn" in single ? single.spawn.id : undefined}
        data-lane={!bundle.foldedSource && lanes.has(bundle.from) ? bundle.from : bundle.recipients.find((recipient) => lanes.has(recipient.actor))?.actor}
        data-at={x}
        role={single ? "button" : "img"}
        tabIndex={-1}
        aria-label={title}
        key={key}
      >
        <title>{`${title} · ${single ? "click to inspect" : "hover or focus for exact times"}`}</title>
      </line>,
    );
    drawn.push(bundle);
  });
  return { marks, hits, bundles: drawn };
}

// A bundle's tooltip: what it holds and how many recipients, then one row per Message
// route with every accepted time and whether it was read, one per Spawn target with every
// exact time and the Session each Spawn created, and the bin it fills.
export function relationBundleTooltip(model: ActorsModel, view: Pick<AgentsState, "workspace">, bundle: RelationBundle, compact: boolean) {
  const sourceOf = (from: string, workspace: string | null) => (from === "Admin" ? "Admin" : model.actorLabel(from, view, workspace));
  const rows: { key: string; label: string; detail: string; note: string; waiting: boolean }[] = [];
  bundle.recipients.forEach((recipient) => {
    const recipientLabel = model.actorLabel(recipient.actor, view, recipient.workspace);
    const routes = new Map<string, MessageRecord[]>();
    recipient.messages.forEach((message) => {
      const key = [message.fromWorkspace || "", message.from].join("\u0000");
      if (!routes.has(key)) routes.set(key, []);
      routes.get(key)!.push(message);
    });
    routes.forEach((messages) => {
      const first = messages[0];
      const waiting = messages.filter((message) => message.read === null).length;
      rows.push({
        key: first.id,
        label: `${sourceOf(first.from, first.fromWorkspace)} → ${recipientLabel}`,
        detail: messages.map((message) => `${model.preciseTime(message.accepted)} → ${message.read ? `read ${model.preciseTime(message.read.at)}` : "waiting"}`).join(" · "),
        note: `×${messages.length} · ${waiting ? `${waiting} waiting` : "read"}`,
        waiting: waiting > 0,
      });
    });
    if (recipient.spawns.length) {
      const first = recipient.spawns[0];
      rows.push({
        key: `spawn:${first.id}`,
        label: `${sourceOf(first.spawner, first.spawnerWorkspace)} → ${recipientLabel}`,
        detail: recipient.spawns.map((action) => `${model.preciseTime(action.at)} · ${action.session}`).join(" · "),
        note: `×${recipient.spawns.length} Spawn${recipient.spawns.length === 1 ? "" : "s"}`,
        waiting: false,
      });
    }
  });
  const shown = rows.slice(0, compact ? 4 : 12);
  const recipientCount = bundle.recipients.length;
  const instants = [...bundle.messages.map((message) => message.accepted), ...bundle.spawns.map((action) => action.at)].sort((left, right) => left - right);
  return (
    <>
      <span className="feld-agents__chart-tooltip-title">{`${relationCountLabel(bundle)} · ${recipientCount} recipient${recipientCount === 1 ? "" : "s"}`}</span>
      {shown.map((row) => (
        <span className="feld-agents__message-tooltip-row" key={row.key}>
          <strong>{row.label}</strong>
          <span>{row.detail}</span>
          <em className={row.waiting ? "is-waiting" : ""}>{row.note}</em>
        </span>
      ))}
      {rows.length > shown.length ? <span className="feld-agents__chart-tooltip-meta">{`+${rows.length - shown.length} more routes · zoom in for every exact time`}</span> : null}
      <span className="feld-agents__chart-tooltip-meta">{`${model.preciseTime(instants[0])}–${model.preciseTime(instants.at(-1)!)} · ${formatDuration(bundle.end - bundle.start)} bin`}</span>
    </>
  );
}

// The viewport of the timeline `svg` of `height` inside its scrolling pane. Before the
// pane is laid out, or when it shows less than half a lane, the whole timeline counts.
export function timelineViewportFor(svg: SVGSVGElement, height: number): TimelineViewport {
  const whole = { top: 0, bottom: height };
  const pane = svg.closest(".feld-agents__bands-scroll");
  if (!pane || !height) return whole;
  const paneRect = pane.getBoundingClientRect();
  const svgRect = svg.getBoundingClientRect();
  if (!svgRect.height) return whole;
  const headHeight = pane.querySelector<HTMLElement>(".feld-agents__bands-head")?.offsetHeight || 0;
  const axisHeight = pane.querySelector("[data-timeline-axis]")?.closest<HTMLElement>(".feld-agents__axis-row")?.offsetHeight || AXIS_ROW_HEIGHT;
  const scale = height / svgRect.height;
  const top = Math.max(0, Math.min(height, (paneRect.top + headHeight - svgRect.top) * scale));
  const bottom = Math.max(top, Math.min(height, (paneRect.bottom - axisHeight - svgRect.top) * scale));
  return bottom - top < LANE.laneHeight / 2 ? whole : { top: Math.round(top), bottom: Math.round(bottom) };
}

// The viewport, measured after every drawing and again as the pane scrolls (once a frame)
// or is resized.
function useTimelineViewport(svgRef: RefObject<SVGSVGElement | null>, height: number): TimelineViewport {
  const [viewport, setViewport] = useState<TimelineViewport>({ top: 0, bottom: height });
  const measure = useCallback(() => {
    const svg = svgRef.current;
    if (!svg) return;
    const next = timelineViewportFor(svg, Number(svg.getAttribute("height")) || 0);
    setViewport((current) => (current.top === next.top && current.bottom === next.bottom ? current : next));
  }, [svgRef]);
  useLayoutEffect(measure);
  useEffect(() => {
    const pane = svgRef.current?.closest(".feld-agents__bands-scroll");
    if (!pane) return undefined;
    let frame: number | null = null;
    const onScroll = () => {
      if (frame !== null) return;
      frame = requestAnimationFrame(() => {
        frame = null;
        measure();
      });
    };
    pane.addEventListener("scroll", onScroll, { passive: true });
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(onScroll) : null;
    observer?.observe(pane);
    return () => {
      if (frame !== null) cancelAnimationFrame(frame);
      pane.removeEventListener("scroll", onScroll);
      observer?.disconnect();
    };
  }, [svgRef, measure]);
  return viewport;
}

// The hit lines of the relation bundles.
const BUNDLE_HIT = "[data-relation-bundle]";

// The outline around the hit the keyboard focused, this far outside its box and at least
// this tall and wide, so a line shows it as a band.
const FOCUS_PAD = 4;
const FOCUS_MIN = 16;

interface FocusRing {
  x: number;
  y: number;
  width: number;
  height: number;
}

// The outline of the hit the keyboard focused in `svg`; a lane hit draws its own, and a
// hit the pointer brought the focus to draws none (see ../inputModality.ts).
function focusRingFor(svg: SVGSVGElement): FocusRing | null {
  const hit = document.activeElement;
  if (!(hit instanceof SVGGraphicsElement) || hit === svg || !svg.contains(hit) || hit.matches(".feld-agents__lane-hit") || hit.hasAttribute("data-pointer-focus") || !hit.matches(":focus-visible")) return null;
  const box = hit.getBBox();
  const padX = Math.max(FOCUS_PAD, (FOCUS_MIN - box.width) / 2);
  const padY = Math.max(FOCUS_PAD, (FOCUS_MIN - box.height) / 2);
  return { x: box.x - padX, y: box.y - padY, width: box.width + 2 * padX, height: box.height + 2 * padY };
}

function sameRing(a: FocusRing | null, b: FocusRing | null) {
  return a === b || (!!a && !!b && a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height);
}

// `agents` are the bands the page draws, in its order.
// The page may hand a new array of the same bands; the drawing follows the bands.
function sameBands(previous: { agents: ActorRecord[] }, next: { agents: ActorRecord[] }) {
  return previous.agents.length === next.agents.length && previous.agents.every((agent, index) => agent === next.agents[index]);
}

export const TimelineSvg = memo(function TimelineSvg({ agents }: { agents: ActorRecord[] }) {
  const model = useActorsModel();
  const view = useActorsStore(useShallow(selectTimelineView));
  const svgRef = useRef<SVGSVGElement>(null);
  const box = useParentBox(svgRef);
  const width = Math.max(box.minWidth, box.clientWidth || box.minWidth);
  const layers = useMemo(() => timelineLayers(model, view, agents, width, box.clientHeight), [model, view, agents, width, box.clientHeight]);
  const { range, height } = layers;
  const viewport = useTimelineViewport(svgRef, height);
  const relations = useMemo(() => relationLayers(model, view, agents, layers, viewport), [model, view, agents, layers, viewport]);
  // The bar under the pointer is emphasized and its value shown in the tooltip; so are the
  // end marker and the relation bundle the pointer reads.
  const [hovered, setHovered] = useState<number | null>(null);
  const [hoveredMarker, setHoveredMarker] = useState<string | null>(null);
  // The selector of the hovered bundle's marks.
  const [hoveredBundle, setHoveredBundle] = useState<string | null>(null);
  const pointer = useRef<PointerAt | null>(null);
  // A bundle shows its Messages and Spawns beside the pointer, or beside its trunk when
  // focused.
  const showBundle = useCallback((hit: Element | null | undefined, clientX: number, clientY: number) => {
    if (!hit || !svgRef.current?.contains(hit)) return false;
    const { relationBundle } = (hit as SVGElement).dataset;
    const compact = window.matchMedia(COMPACT_LAYOUT_QUERY).matches;
    const bundle = relationBundle === undefined ? undefined : relations.bundles[Number(relationBundle)];
    if (!bundle) return false;
    setHovered(null);
    setHoveredMarker(null);
    setHoveredBundle(`[data-relation-bundle-mark="${relationBundle}"]`);
    useChartTooltip.getState().show(relationBundleTooltip(model, view, bundle, compact), clientX, clientY);
    return true;
  }, [model, view, relations]);
  const hover = useCallback((at: PointerAt | null) => {
    pointer.current = at;
    const svg = svgRef.current;
    // After a drawing the pointer may sit over another mark than the one it moved onto.
    const element = at ? (at.target?.isConnected ? at.target : document.elementFromPoint(at.clientX, at.clientY)) : null;
    if (at && showBundle(element?.closest(BUNDLE_HIT), at.clientX, at.clientY)) return;
    setHoveredBundle(null);
    const point = at && svg ? viewPointFor(svg, at.clientX, at.clientY) : null;
    const index = point ? layers.bins.findIndex((hit) => point.x >= hit.left && point.x < hit.right && point.y >= hit.top && point.y <= hit.bottom) : -1;
    setHovered(index < 0 ? null : index);
    if (index >= 0) {
      setHoveredMarker(null);
      useChartTooltip.getState().show(histogramTooltip(model, layers.bins[index]), at!.clientX, at!.clientY);
      return;
    }
    // An end marker tells how the Session ended, and why.
    const target = point ? sessionTargetAt(layers.sessions, point, true) : null;
    setHoveredMarker(target?.session.id ?? null);
    if (target) {
      useChartTooltip.getState().show(markTooltip(target.label, target.reason), at!.clientX, at!.clientY);
      return;
    }
    // The dashed part of a line tells since when Feld lost sight of the execution.
    const line = point ? sessionTargetAt(layers.sessions, point) : null;
    if (line?.lostX != null && point!.x >= line.lostX) useChartTooltip.getState().show(markTooltip(line.session.id, model.lostLabel(line.session)), at!.clientX, at!.clientY);
    else useChartTooltip.getState().hide();
  }, [model, layers, showBundle]);
  // A click where the hit areas of Sessions overlap opens the nearest one.
  const pickSession = useCallback((at: PointerAt) => {
    const svg = svgRef.current;
    return svg ? sessionTargetAt(layers.sessions, viewPointFor(svg, at.clientX, at.clientY))?.session.id ?? null : null;
  }, [layers]);
  // A new drawing reads the mark that is now under a pointer that stayed.
  useLayoutEffect(() => {
    if (pointer.current) hover(pointer.current);
  }, [hover]);
  useLayoutEffect(() => {
    svgRef.current?.querySelectorAll(".feld-agents__token-bar.is-hovered, .feld-agents__session-marker-hit.is-hovered, .feld-agents__relation-bundle-visual.is-hovered").forEach((hit) => hit.classList.remove("is-hovered"));
    if (hovered !== null) svgRef.current?.querySelector(`[data-histogram-bin="${hovered}"]`)?.classList.add("is-hovered");
    if (hoveredMarker !== null) svgRef.current?.querySelector(`.feld-agents__session-marker-hit[data-session="${hoveredMarker}"]`)?.classList.add("is-hovered");
    if (hoveredBundle !== null) svgRef.current?.querySelector(hoveredBundle)?.classList.add("is-hovered");
  });
  // The focused hit's outline follows a new drawing.
  const [ring, setRing] = useState<FocusRing | null>(null);
  const measureRing = useCallback(() => {
    const next = svgRef.current ? focusRingFor(svgRef.current) : null;
    setRing((current) => (sameRing(current, next) ? current : next));
  }, []);
  useLayoutEffect(measureRing);
  const onFocus = useCallback((event: FocusEvent<SVGSVGElement>) => {
    measureRing();
    const hit = (event.target as Element).closest(BUNDLE_HIT);
    if (!hit) return;
    const rect = hit.getBoundingClientRect();
    showBundle(hit, rect.left + rect.width / 2, rect.top + Math.min(16, rect.height / 2));
  }, [showBundle, measureRing]);
  const onBlur = useCallback((event: FocusEvent<SVGSVGElement>) => {
    setRing(null);
    (event.target as Element).removeAttribute("data-pointer-focus");
    if (!(event.target as Element).closest(BUNDLE_HIT)) return;
    setHoveredBundle(null);
    useChartTooltip.getState().hide();
  }, []);
  const handlers = usePlotHandlers(model, svgRef, "timeline", hover, pickSession);
  // The arrow keys and Tab move between the hits (see laneKeys.ts); Enter opens one.
  const openOnKey = handlers.onKeyDown;
  const onKeyDown = useCallback((event: KeyboardEvent<SVGSVGElement>) => {
    // A hit the pointer focused shows its outline once the keyboard takes over.
    (event.target as Element).removeAttribute("data-pointer-focus");
    measureRing();
    const next = laneKeyTarget(event.currentTarget, event.target as Element, event.key, event.shiftKey);
    if (!next) {
      openOnKey(event);
      return;
    }
    event.preventDefault();
    if (next !== event.target) (next as SVGElement).focus();
  }, [openOnKey, measureRing]);
  return (
    <svg role="group" aria-label={`Actor Sessions in ${model.formatRange(range)}`} data-timeline="" viewBox={`0 0 ${width} ${height}`} height={height} ref={svgRef} {...handlers} onKeyDown={onKeyDown} onFocus={onFocus} onBlur={onBlur}>
      {layers.marks}
      {relations.marks}
      {layers.hits}
      {relations.hits}
      {layers.markerHits}
      {ring ? <rect className="feld-agents__hit-focus" x={ring.x.toFixed(1)} y={ring.y.toFixed(1)} width={ring.width.toFixed(1)} height={ring.height.toFixed(1)} rx="4" aria-hidden="true" /> : null}
    </svg>
  );
}, sameBands);
