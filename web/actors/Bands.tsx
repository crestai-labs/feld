// The bands pane: the Actors section (its header, the table of the label column, the
// Timeline, and the Actions column, and the Timeline's time axis) and the Metrics section
// under it (its header, the lanes, and their time axis). The two tables scroll sideways
// together.

import { Fragment, memo, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type DragEvent, type MouseEvent, type PointerEvent, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { useActorsModel } from "./context.ts";
import { actorColumns, actorColumnsWidth, COMPACT_LAYOUT_QUERY, isPinned, kindLabel, kindRules } from "./model.ts";
import { ColumnResizer, PaneResizer, type ResizeKind } from "./resize.tsx";
import { useShallow } from "zustand/react/shallow";
import { markPointerFocusedLane } from "./charts/interaction.ts";
import { useActorsStore } from "./store.ts";
import type { ActorRecord, AgentsState, Focus } from "./types.ts";
import { ClockGlyph, GearGlyph, useActorsTools, usePageActions } from "./ui.tsx";
import { deleteActorFromMenu, killActorSession, refuseMenuAction, setScheduleEnabled, startActorSession, stopActorSession } from "./mutations.ts";
import { MetricsWindowControl, TimeControls } from "./Heading.tsx";
import { TimelineSvg } from "./charts/Timeline.tsx";
import { MetricLabels, MetricsSummary, MetricsSvg } from "./charts/Metrics.tsx";
import { AxisRow } from "./charts/Axis.tsx";
import { KIND_GLYPHS, actorStateGlyph } from "./charts/glyphs.tsx";
import { actorStateTooltip, useChartTooltip } from "./charts/hover.tsx";

// What the bands pane reads from the page state. The composer, the dialogs' drafts, and the
// widths dragged outside the pane stay out, so typing or resizing there leaves it as drawn.
type BandsView = Pick<AgentsState,
  | "workspace" | "query" | "mode" | "anchor" | "duration" | "metricsDuration" | "actorOrder" | "actorPins"
  | "showAgentsOutsideRange" | "showSubAgents" | "frozenBandIds" | "actorsCollapsed" | "metricsCollapsed"
  | "actorWidth" | "visibleActorDetails" | "actorColumnWidths" | "agentsPaneHeight" | "visibleMetricIds"
  | "timelineGuideOpen" | "agentDisplayMenuOpen" | "metricsMenuOpen" | "actionMenuAgent"
  | "inspectedSessionId" | "actorCard" | "focusedActorId">;

function selectBandsView(state: AgentsState): BandsView {
  return {
    workspace: state.workspace,
    query: state.query,
    mode: state.mode,
    anchor: state.anchor,
    duration: state.duration,
    metricsDuration: state.metricsDuration,
    actorOrder: state.actorOrder,
    actorPins: state.actorPins,
    showAgentsOutsideRange: state.showAgentsOutsideRange,
    showSubAgents: state.showSubAgents,
    frozenBandIds: state.frozenBandIds,
    actorsCollapsed: state.actorsCollapsed,
    metricsCollapsed: state.metricsCollapsed,
    actorWidth: state.actorWidth,
    visibleActorDetails: state.visibleActorDetails,
    actorColumnWidths: state.actorColumnWidths,
    agentsPaneHeight: state.agentsPaneHeight,
    visibleMetricIds: state.visibleMetricIds,
    timelineGuideOpen: state.timelineGuideOpen,
    agentDisplayMenuOpen: state.agentDisplayMenuOpen,
    metricsMenuOpen: state.metricsMenuOpen,
    actionMenuAgent: state.actionMenuAgent,
    inspectedSessionId: state.inspectedSessionId,
    actorCard: state.actorCard,
    focusedActorId: state.focusedActorId,
  };
}

export const Bands = memo(function Bands({ onResizing }: { onResizing: (kind: ResizeKind | null) => void }) {
  const model = useActorsModel();
  const state = useActorsStore(useShallow(selectBandsView));
  const agents = model.bandAgents(state);
  const range = model.rangeFor(state);
  const bandsScroll = useRef<HTMLDivElement>(null);
  const metricsScroll = useRef<HTMLDivElement>(null);
  const syncing = useRef(false);
  const store = useActorsStore.getState;
  const hasMetricLanes = model.visibleMetrics(state).length > 0;

  // The Metrics table follows the Actors table sideways and the other way round.
  function follow(source: HTMLDivElement | null, counterpart: HTMLDivElement | null) {
    if (!source || !counterpart || syncing.current || counterpart.scrollLeft === source.scrollLeft) return;
    syncing.current = true;
    counterpart.scrollLeft = source.scrollLeft;
    syncing.current = false;
  }

  // An unfolded Metrics table starts where the Actors table is scrolled to.
  useLayoutEffect(() => {
    if (!state.metricsCollapsed) follow(bandsScroll.current, metricsScroll.current);
  }, [state.metricsCollapsed]);

  function openGuide() {
    store().openTimelineGuide();
  }

  const style = {
    "--agents-identity-width": `${state.actorWidth}px`,
    "--agents-details-width": `${actorColumnsWidth(state)}px`,
    ...(state.agentsPaneHeight === null ? {} : { "--agents-pane-height": `${state.agentsPaneHeight}px`, "--metrics-pane-height": "1fr" }),
  } as CSSProperties;

  return (
    <div className={`feld-agents__bands${state.metricsCollapsed ? " is-metrics-collapsed" : ""}${state.actorsCollapsed ? " is-actors-collapsed" : ""}`} style={style}>
      <ActorsHead state={state} agents={agents} />
      <div className="feld-agents__bands-scroll" ref={bandsScroll} onScroll={() => follow(bandsScroll.current, metricsScroll.current)}>
        <div className="feld-agents__bands-content">
          <div className="feld-agents__bands-head">
            <div className="feld-agents__agent-head">
              <div className="feld-agents__agent-head-identity">
                <span>Name</span>
              </div>
              <ColumnResizer column="name" label="Name" onResizing={onResizing} />
            </div>
            <ActorDetailHead state={state} onResizing={onResizing} />
            <div className="feld-agents__timeline-head">
              <span>Timeline</span>
              <button className="feld-agents__timeline-guide" type="button" data-action="open-timeline-guide" aria-haspopup="dialog" aria-expanded={state.timelineGuideOpen} title="Read what every Timeline mark means" onClick={openGuide}>
                <span aria-hidden="true">?</span>Guide
              </button>
            </div>
            <div className="feld-agents__actions-head"></div>
          </div>
          <div className={`feld-agents__body${agents.length ? "" : " is-empty"}`}>
            <BandLabels state={state} agents={agents} />
            <ActorDetailRows state={state} agents={agents} />
            <div className="feld-agents__chart">
              <TimelineSvg agents={agents} />
              {agents.length ? null : <EmptyBands state={state} range={range} />}
            </div>
            <div className="feld-agents__actions">
              {agents.map((agent) => <ActionCell key={agent.id} agent={agent} state={state} />)}
            </div>
          </div>
          <AxisRow range={range} attribute="data-timeline-axis" />
        </div>
      </div>
      <PaneResizer onResizing={onResizing} />
      <section className="feld-agents__metrics" aria-label="Usage metrics">
        <MetricsHead state={state} />
        {state.metricsCollapsed ? null : (
          <div className="feld-agents__metrics-scroll" ref={metricsScroll} onScroll={() => follow(metricsScroll.current, bandsScroll.current)}>
            <div className="feld-agents__metrics-content">
              <div className={`feld-agents__metrics-body${hasMetricLanes ? "" : " is-empty"}`}>
                {hasMetricLanes ? (
                  <>
                    <div className="feld-agents__metric-labels"><MetricLabels /></div>
                    <div className="feld-agents__metrics-chart">
                      <MetricsSvg />
                    </div>
                  </>
                ) : <div className="feld-agents__metrics-empty">{state.visibleMetricIds.size ? "No metrics match the search." : "No metrics selected. Use the settings button to add a lane."}</div>}
                <div className="feld-agents__metric-spacer"></div>
              </div>
              {hasMetricLanes ? <AxisRow range={model.metricsRangeFor(state)} attribute="data-metrics-axis" focus={model.hasMetricsContext(state) ? range : null} /> : null}
            </div>
          </div>
        )}
      </section>
    </div>
  );
});

const TOGGLE_GLYPH = <svg viewBox="0 0 12 12" aria-hidden="true"><path d="m4 2.5 4 3.5-4 3.5"></path></svg>;

// The Actors section's own header, above the table and outside its scrolling: the fold,
// how many Actors the table shows (or, folded, how many are running), and the time
// controls, which set the range of the Timeline and, through it, of Metrics.
function ActorsHead({ state, agents }: { state: BandsView; agents: ActorRecord[] }) {
  const model = useActorsModel();
  const { focus } = usePageActions();
  const store = useActorsStore.getState;
  const all = model.workspaceAgents(state);
  const running = all.filter((agent) => model.currentSession(agent.id));
  const runningAgents = running.filter((agent) => agent.kind === "agent").length;
  const runningJobs = running.length - runningAgents;
  function toggleDisplayMenu() {
    store().toggleAgentDisplayMenu();
    focus(store().agentDisplayMenuOpen ? ".feld-agents__display-menu-close" : '[data-action="toggle-agent-display-menu"]');
  }
  return (
    <div className="feld-agents__section-head feld-agents__actors-head">
      <button type="button" className="feld-agents__section-toggle" data-action="toggle-actors" aria-expanded={!state.actorsCollapsed} title={state.actorsCollapsed ? "Show the Actor table" : "Fold Actors to its header"} onClick={() => { store().toggleActorsCollapsed(); focus('[data-action="toggle-actors"]'); }}>
        {TOGGLE_GLYPH}
        Actors
      </button>
      {state.actorsCollapsed ? (
        <span className="feld-agents__section-summary">
          <i className="feld-agents__running-dot" aria-hidden="true"></i>
          {`${runningAgents} ${runningAgents === 1 ? "Agent" : "Agents"} running${runningJobs ? ` · ${runningJobs} ${runningJobs === 1 ? "Job" : "Jobs"} running` : ""}`}
        </span>
      ) : (
        <>
          <small className="feld-agents__section-count" title={model.agentCountTitle(state, agents)}>{agents.length} of {all.length} shown</small>
          <HiddenBands state={state} agents={agents} />
        </>
      )}
      <div className="feld-agents__section-tools">
        <TimeControls />
        <button type="button" className="feld-agents__section-settings" data-action="toggle-agent-display-menu" aria-label="Configure Actor timeline display" aria-expanded={state.agentDisplayMenuOpen} title="Timeline display" onClick={toggleDisplayMenu}>
          <GearGlyph />
        </button>
      </div>
    </div>
  );
}

// The Metrics section's own header, in the same form as the Actors one: the fold, then at
// the end the Metrics window and the lane settings; folded, the latest reading of every
// visible lane.
function MetricsHead({ state }: { state: BandsView }) {
  const { focus } = usePageActions();
  const store = useActorsStore.getState;
  function toggleMetricsMenu() {
    store().toggleMetricsMenu();
    focus(store().metricsMenuOpen ? ".feld-agents__display-menu.is-metrics .feld-agents__display-menu-close" : '[data-action="toggle-metrics-menu"]');
  }
  return (
    <div className="feld-agents__section-head feld-agents__metrics-head">
      <button type="button" className="feld-agents__section-toggle" data-action="toggle-metrics" aria-expanded={!state.metricsCollapsed} title={state.metricsCollapsed ? "Show the metric lanes" : "Fold Metrics to its header"} onClick={() => { store().toggleMetricsCollapsed(); focus('[data-action="toggle-metrics"]'); }}>
        {TOGGLE_GLYPH}
        Metrics
      </button>
      <div className="feld-agents__section-tools">
        <MetricsWindowControl />
        <button type="button" className="feld-agents__section-settings" data-action="toggle-metrics-menu" aria-label="Configure visible metrics" aria-expanded={state.metricsMenuOpen} title="Metrics display" onClick={toggleMetricsMenu}>
          <GearGlyph />
        </button>
      </div>
      {state.metricsCollapsed ? <div className="feld-agents__metrics-summary"><MetricsSummary /></div> : null}
    </div>
  );
}

// ---- Actors column head ------------------------------------------------------------------

// The Actors the range holds back, shown or hidden again from the Actors header.
function HiddenBands({ state, agents }: { state: BandsView; agents: ActorRecord[] }) {
  const model = useActorsModel();
  const { focus } = usePageActions();
  const hidden = model.hiddenBandsFor(state, agents);
  if (!hidden) return null;
  const action = hidden.shown ? "hide-agents-outside-range" : "show-agents-outside-range";
  const next = hidden.shown ? "show-agents-outside-range" : "hide-agents-outside-range";
  return (
    <button className={`feld-agents__hidden-bands${hidden.shown ? " is-shown" : ""}`} type="button" data-action={action} title={hidden.title} onClick={() => { useActorsStore.getState().setAgentsOutsideRange(!hidden.shown); focus(`[data-action="${next}"]`, { preventScroll: true }); }}>
      {hidden.label}
    </button>
  );
}

// The Timeline with no band drawn. A Workspace that holds no Actor yet offers attach
// first and the declaration after it; the other two name the filter that emptied it.
function EmptyBands({ state, range }: { state: BandsView; range: FeldMetricRange }) {
  const model = useActorsModel();
  const { openNewActor } = usePageActions();
  const reason = model.emptyBandsReason(state);
  if (reason === "no-actors") {
    return (
      <div className="feld-agents__empty">
        <p>{state.workspace ? "This Workspace has no Actors yet" : "No Workspace has an Actor yet"}.</p>
        <p>Attach a Claude Code you already run: type <code>/feld attach</code> in it.</p>
        <p>Or start an Agent or a Job from Feld.</p>
        <button className="feld-agents__button" type="button" data-action="new-actor" aria-haspopup="dialog" onClick={openNewActor}>New actor</button>
      </div>
    );
  }
  if (reason === "no-match") return <div className="feld-agents__empty">No actors match the search query. The empty Timeline remains available for time navigation.</div>;
  return <div className="feld-agents__empty">No Actor has a Session in {model.formatRange(range)}. Drag or horizontally scroll this empty Timeline to move through time, or keep Actors without Sessions in range from the display settings.</div>;
}

// ---- Label column ---------------------------------------------------------------------

// The drop a drag over a row makes: above or below that row, by the pointer's half.
interface DropTarget {
  id: string;
  after: boolean;
}

// Rows reorder by dragging a row's name cell onto another row. Pinned rows stay above the
// rest, so a drop among the pinned rows pins the dragged one and a drop among the unpinned
// rows unpins it.
function BandLabels({ state, agents }: { state: BandsView; agents: ActorRecord[] }) {
  const model = useActorsModel();
  const focus = model.focusFor(state);
  const [dragged, setDragged] = useState<string | null>(null);
  const [drop, setDrop] = useState<DropTarget | null>(null);

  // Off the rows (the Timeline, the page heading) a drop moves nothing, so no row is marked.
  useEffect(() => {
    if (!dragged) return;
    const clear = (event: globalThis.DragEvent) => {
      if (!(event.target instanceof Element) || !event.target.closest("[data-drag-agent]")) setDrop(null);
    };
    document.addEventListener("dragover", clear);
    return () => document.removeEventListener("dragover", clear);
  }, [dragged]);

  function dropTargetFor(event: DragEvent<HTMLDivElement>, agent: ActorRecord): DropTarget | null {
    if (!dragged || agent.id === dragged) return null;
    const rect = event.currentTarget.getBoundingClientRect();
    return { id: agent.id, after: event.clientY > rect.top + rect.height / 2 };
  }

  const drag: RowDrag = {
    start(event, agent) {
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/plain", agent.id);
      setDragged(agent.id);
    },
    over(event, agent) {
      const target = dropTargetFor(event, agent);
      if (target?.id !== drop?.id || target?.after !== drop?.after) setDrop(target);
      if (!target) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
    },
    drop(event, agent) {
      const target = dropTargetFor(event, agent);
      setDrop(null);
      if (!target) return;
      event.preventDefault();
      const moved = agents.find((candidate) => candidate.id === dragged);
      if (moved) useActorsStore.getState().moveActor(agents, moved, agent, target.after);
    },
    end() {
      setDragged(null);
      setDrop(null);
    },
  };

  return (
    <div className="feld-agents__band-labels">
      {agents.map((agent) => <BandLabel key={agent.id} agent={agent} state={state} focus={focus} drag={drag} dragging={dragged === agent.id} drop={drop?.id === agent.id ? drop : null} />)}
    </div>
  );
}

interface RowDrag {
  start(event: DragEvent<HTMLDivElement>, agent: ActorRecord): void;
  over(event: DragEvent<HTMLDivElement>, agent: ActorRecord): void;
  drop(event: DragEvent<HTMLDivElement>, agent: ActorRecord): void;
  end(): void;
}

// A click on an Actor's row, apart from its controls, selects the Actor's relationships on
// the Timeline; a second click clears them. Focus moves to the Actor's lane, so the Escape
// that clears the selection reaches the page. The lane is marked as focused by the pointer,
// so it draws no focus outline until the keyboard moves the focus (see interaction.ts).
export function selectActorFrom(event: Pick<MouseEvent<Element>, "target" | "currentTarget">, agentId: string) {
  if ((event.target as Element).closest("button, a, input, [data-action]")) return;
  useActorsStore.getState().toggleActorFocus(agentId);
  const lanes = event.currentTarget.closest(".feld-agents__bands")?.querySelectorAll<SVGElement>(".feld-agents__lane-hit[data-focus-actor]") || [];
  const lane = [...lanes].find((candidate) => candidate.getAttribute("data-focus-actor") === agentId);
  if (!lane) return;
  markPointerFocusedLane(lane);
  lane.focus({ preventScroll: true });
}

const PIN_MARK = (
  <svg className="feld-agents__pin-mark" viewBox="0 0 16 16" aria-label="Pinned" role="img">
    <title>Pinned: kept above the unpinned rows, whatever the range</title>
    <path d="M6 2.5h4l-.6 4 2.1 2.1H4.5l2.1-2.1zM8 8.6v4.9"></path>
  </svg>
);

function BandLabel({ agent, state, focus, drag, dragging, drop }: { agent: ActorRecord; state: BandsView; focus: Focus | null; drag: RowDrag; dragging: boolean; drop: DropTarget | null }) {
  const model = useActorsModel();
  const { openActorCard } = usePageActions();
  const label = model.actorLabel(agent.id, state);
  const pinned = isPinned(state, agent);
  const classes = [
    "feld-agents__band-label",
    focus?.agents.has(agent.id) ? "is-focused" : "",
    state.focusedActorId === agent.id ? "is-selected" : "",
    pinned ? "is-pinned" : "",
    dragging ? "is-dragging" : "",
    drop ? (drop.after ? "is-drop-after" : "is-drop-before") : "",
  ].filter(Boolean).join(" ");
  return (
    <div className={classes} style={{ height: `${model.laneHeightFor(agent.id, state)}px` }} draggable data-drag-agent={agent.id} data-focus-actor={agent.id} title="Drag to reorder · click to select relationships" onClick={(event) => selectActorFrom(event, agent.id)} onDragStart={(event) => drag.start(event, agent)} onDragOver={(event) => drag.over(event, agent)} onDrop={(event) => drag.drop(event, agent)} onDragEnd={drag.end}>
      <div className="feld-agents__band-identity">
        <ActorStateMark agent={agent} />
        <KindGlyph agent={agent} />
        <span><button type="button" className="feld-agents__actor-name" data-action="open-actor" data-agent-id={agent.id} aria-haspopup="dialog" aria-expanded={state.actorCard?.agentId === agent.id} title={`${label} · ${kindLabel(agent.kind)} (Actor kind declared at creation) · Workspace ${agent.workspace} · opens the Actor card`} onClick={() => openActorCard(agent.id)}>{label}{pinned ? PIN_MARK : null}</button><BandMarks agent={agent} state={state} /></span>
      </div>
    </div>
  );
}

// The state glyph beside an Actor's name: its current Session inside, its Schedule as the
// outer ring. Hovering it shows both in the chart tooltip; its empty title keeps the row's
// "Drag to reorder" from showing over it.
function ActorStateMark({ agent }: { agent: ActorRecord }) {
  const model = useActorsModel();
  const actorState = model.actorStateFor(agent);
  const onPointer = (event: PointerEvent<HTMLSpanElement>) => {
    if (event.pointerType === "touch") return;
    useChartTooltip.getState().show(actorStateTooltip(actorState), event.clientX, event.clientY);
  };
  const hide = () => useChartTooltip.getState().hide();
  return (
    <span className={`feld-agents__state is-${actorState.key}`} role="img" title="" aria-label={`${actorState.lines.join(". ")}. ${actorState.title}`} onPointerEnter={onPointer} onPointerMove={onPointer} onPointerLeave={hide}>
      <svg viewBox="0 0 16 16" aria-hidden="true">{actorStateGlyph(actorState.key, actorState.schedule)}</svg>
    </span>
  );
}

// The kind glyph beside an Actor's name, with a popover (hover and keyboard focus) that
// names the kind and the one rule it changes.
function KindGlyph({ agent }: { agent: ActorRecord }) {
  const rules = kindRules(agent.kind);
  const id = `feld-agents-kind-${agent.id}`;
  return (
    <span className="feld-agents__kind">
      <button type="button" className={`feld-agents__info feld-agents__kind-glyph is-${agent.kind}`} aria-label={`${rules.label} · Actor kind declared at creation`} aria-describedby={id}>
        <svg viewBox="0 0 12 12" aria-hidden="true">{KIND_GLYPHS[agent.kind] || KIND_GLYPHS.agent}</svg>
      </button>
      <span className="feld-agents__info-popover feld-agents__kind-popover" role="tooltip" id={id}>
        <strong>{rules.label}</strong>
        <span>{rules.summary} Declared at creation; Feld never infers or changes it.</span>
        <span className="feld-agents__kind-rule">{rules.rule} An attached Session ends without any request, whatever the kind.</span>
      </span>
    </span>
  );
}

// ---- Actor table ------------------------------------------------------------------------

// Each column head ends in the handle that sizes the column; the last one shown is the
// boundary between the table and the Timeline.
function ActorDetailHead({ state, onResizing }: { state: BandsView; onResizing: (kind: ResizeKind | null) => void }) {
  return (
    <div className="feld-agents__detail-head">
      {actorColumns(state).map((option) => (
        <Fragment key={option.id}>
          <span className="feld-agents__detail-cell" style={{ width: `${option.width}px` }} title={option.detail}><span>{option.label}</span></span>
          <ColumnResizer column={option.id} label={option.label} onResizing={onResizing} />
        </Fragment>
      ))}
    </div>
  );
}

function ActorDetailRows({ state, agents }: { state: BandsView; agents: ActorRecord[] }) {
  const model = useActorsModel();
  const columns = actorColumns(state);
  return (
    <div className="feld-agents__band-details">
      {agents.map((agent) => (
        <div key={agent.id} className={`feld-agents__band-detail${state.focusedActorId === agent.id ? " is-focused" : ""}`} style={{ height: `${model.laneHeightFor(agent.id, state)}px` }} data-focus-actor={agent.id} title={`Select ${model.actorLabel(agent.id, state)} relationships`} onClick={(event) => selectActorFrom(event, agent.id)}>
          {columns.map((option) => <span key={option.id} className="feld-agents__detail-cell" style={{ width: `${option.width}px` }}><ActorDetailCell agent={agent} detailId={option.id} /></span>)}
        </div>
      ))}
    </div>
  );
}

const EMPTY_CELL = <span className="feld-agents__detail-empty" aria-hidden="true">—</span>;

// One cell of the Actor table, read from the latest top-level Session; an Agent has no
// Schedule and a program no Coding Agent, so their cells are empty.
function ActorDetailCell({ agent, detailId }: { agent: ActorRecord; detailId: string }) {
  const model = useActorsModel();
  if (detailId === "schedule") return <ScheduleCell agent={agent} />;
  const cell = model.actorDetailCellFor(agent, detailId);
  if (!cell) return EMPTY_CELL;
  if (cell.kind === "none") return <span className="feld-agents__session-state" title={cell.title}>No Session</span>;
  if (cell.kind === "text") return <span title={cell.title}>{cell.value}</span>;
  return (
    <span className="feld-agents__detail-status" title={cell.title}>
      <span className={`feld-agents__session-state is-${cell.key}`}>{cell.label}</span>
      <small>{cell.when}</small>
    </span>
  );
}

// The Schedule cell. "Next run" is derived at render time from the cron expression and
// now; "Paused since" is the accepted pause request. Clicking opens the Actor card at its
// Job section. Skipped ticks are a separate mark on the marks line under the Actor name.
function ScheduleCell({ agent }: { agent: ActorRecord }) {
  const model = useActorsModel();
  const { openJob } = usePageActions();
  const workspace = useActorsStore((state) => state.workspace);
  const cell = model.scheduleCellFor(agent, { workspace });
  if (!cell) return EMPTY_CELL;
  return (
    <button type="button" className={`feld-agents__schedule${cell.paused ? " is-paused" : ""}`} data-action="open-job" data-agent-id={agent.id} title={cell.title} aria-label={cell.ariaLabel} onClick={() => openJob(agent.id)}>
      <ClockGlyph />
      <span>{cell.summary}</span>
    </button>
  );
}

// A chain with its links apart: the Runtime has not linked to its Session.
const UNLINKED_GLYPH = <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M5 3.5 6.2 2.3a2 2 0 0 1 2.8 2.8L7.8 6.3M4.2 5.7 3 6.9a2 2 0 0 0 2.8 2.8L7 8.5"></path></svg>;

const MAIL_GLYPH = <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 5.5V10.5h7V5.5M6 .5v1.6M6 4v1.6"></path></svg>;

// The marks line under an Actor's name holds the marks that exist only while a current
// Session runs: skipped ticks, an unlinked Runtime, and unread mail (or, with no current
// Session, the neutral waiting count). One line, however many are present.
function BandMarks({ agent, state }: { agent: ActorRecord; state: BandsView }) {
  const model = useActorsModel();
  const { openJob, openConsole } = usePageActions();
  const skip = model.skipMarkFor(agent);
  const unread = model.unreadMarkFor(agent, state);
  const unlinked = model.unlinkedMarkFor(agent);
  const deleted = agent.deletedAt ? `Deleted ${model.dateTime.format(agent.deletedAt)} ${model.ZONE} (Control Plane fact) · its Sessions and Messages stay` : null;
  if (!skip && !unread && !unlinked && !deleted) return null;
  return (
    <span className="feld-agents__marks">
      {deleted ? <span className="feld-agents__unread is-idle" title={deleted} aria-label={deleted}><span>deleted</span></span> : null}
      {skip ? (
        <button type="button" className="feld-agents__unread is-skip" data-action="open-job" data-agent-id={agent.id} data-skip-id={skip.skipId} title={skip.title} aria-label={skip.title} onClick={() => openJob(agent.id, skip.skipId)}>
          <svg viewBox="0 0 12 12" aria-hidden="true"><circle cx="6" cy="6" r="4.4"></circle><path d="M3.8 8.2 8.2 3.8"></path></svg>
          <span>{skip.count} skipped</span>
        </button>
      ) : null}
      {unlinked ? (
        <span className="feld-agents__unread is-unlinked" title={unlinked.title} aria-label={unlinked.title}>
          {UNLINKED_GLYPH}
          <span>unlinked</span>
        </span>
      ) : null}
      {unread?.idle ? <span className="feld-agents__unread is-idle" title={unread.title} aria-label={unread.title}>{MAIL_GLYPH}<span>{unread.count} waiting</span></span> : null}
      {unread && !unread.idle ? (
        <button type="button" className="feld-agents__unread" data-action="open-console" data-session-id={unread.sessionId} data-record-id={unread.recordId} title={unread.title} aria-label={unread.title} onClick={() => openConsole(unread.sessionId, unread.recordId)}>
          {MAIL_GLYPH}
          <span>{unread.count} waiting</span>
        </button>
      ) : null}
    </span>
  );
}

// ---- Actions column --------------------------------------------------------------------

// The open Actions menu is drawn at the component's top level, outside the scrolling table
// and its sticky columns, so it draws over the page while ActionMenu anchors it to its row.
function ActionCell({ agent, state }: { agent: ActorRecord; state: BandsView }) {
  const model = useActorsModel();
  const open = state.actionMenuAgent === agent.id;
  const cell = useRef<HTMLDivElement>(null);
  const [host, setHost] = useState<Element | null>(null);
  useLayoutEffect(() => setHost(cell.current?.closest(".feld-agents") ?? null), []);
  return (
    <div className="feld-agents__action-cell" style={{ height: `${model.laneHeightFor(agent.id, state)}px` }} ref={cell}>
      <button className="feld-agents__action-button" type="button" data-action="toggle-agent-menu" data-agent-id={agent.id} aria-label={`More actions for ${model.actorLabel(agent.id, state)}`} aria-expanded={open} title="More actions" onClick={() => useActorsStore.getState().toggleAgentMenu(agent.id)}>⋮</button>
      {open && host ? createPortal(<ActionMenu agent={agent} state={state} anchor={cell} />, host) : null}
    </div>
  );
}

// Agent Actions expose only the primitives the product contract names:
// `feld session stop`, `feld session kill`, `feld actor start`, `feld actor delete`, and
// `feld message send`. The Actions menu's requests the fixture's daemon refuses.
const LIFECYCLE_REQUESTS: Readonly<Record<string, "start" | "stop" | "kill" | "delete">> = Object.freeze({ "start-replacement": "start", "stop-session": "stop", "terminate-session": "kill", "delete-actor": "delete" });

// Kill and Delete send on their second choice for the same target (Kill's Session, Delete's
// Actor): the first turns the item into its confirmation, and moving to another item,
// closing the menu, or a new target turns it back.
const CONFIRMATIONS: Readonly<Record<string, { label: string; note: (target: string) => string }>> = Object.freeze({
  "terminate-session": { label: "Confirm kill", note: (sessionId: string) => `Choose again to kill ${sessionId} · it cannot be undone` },
  "delete-actor": { label: "Confirm delete", note: (actor: string) => `Choose again to delete ${actor} · its Sessions and Messages stay` },
});

// Whether Kill or Delete stands confirmed: the target it was confirmed for is still the menu's.
export function targetConfirmed(confirmedTarget: string | null, menuTarget: string | null) {
  return confirmedTarget !== null && confirmedTarget === menuTarget;
}

// While a request of this menu is in flight every item is disabled, and a refusal stays
// under them until the reader picks something else. The menu opens below its row, or above
// it when only the space above holds it; otherwise it keeps to the viewport and scrolls. On
// a phone it is a bottom sheet placed by CSS. It follows its row while the page scrolls and
// closes once the row leaves the visible part of the bands pane.
function ActionMenu({ agent, state, anchor }: { agent: ActorRecord; state: BandsView; anchor: RefObject<HTMLDivElement | null> }) {
  const model = useActorsModel();
  const { client, refusals } = useActorsTools();
  const { openConsole, openJob, openActorCard, openComposer } = usePageActions();
  const actionRequest = useActorsStore((current) => current.actionRequest);
  const menu = model.agentMenuFor(agent, { actorPins: state.actorPins, actionRequest });
  const element = useRef<HTMLDivElement>(null);
  const [confirmed, setConfirmed] = useState<{ action: string; target: string | null } | null>(null);

  useLayoutEffect(() => {
    function place() {
      const current = element.current;
      const cell = anchor.current;
      if (!current || !cell) return;
      if (window.matchMedia(COMPACT_LAYOUT_QUERY).matches) {
        current.style.top = current.style.left = current.style.maxHeight = "";
        return;
      }
      const cellRect = cell.getBoundingClientRect();
      const viewport = document.documentElement.clientHeight;
      const maxHeight = viewport - 16;
      const height = Math.min(current.scrollHeight + current.offsetHeight - current.clientHeight, maxHeight);
      const below = cellRect.top + 48;
      const above = cellRect.bottom - 48 - height;
      const top = below + height <= viewport - 8 ? below : above >= 8 ? above : Math.max(8, viewport - 8 - height);
      current.style.top = `${top}px`;
      current.style.left = `${cellRect.right - 10 - current.offsetWidth}px`;
      current.style.maxHeight = `${maxHeight}px`;
    }
    function follow(event: Event) {
      if (element.current?.contains(event.target as Node)) return;
      const cell = anchor.current;
      const pane = cell?.closest(".feld-agents__bands-scroll");
      if (cell && pane && !window.matchMedia(COMPACT_LAYOUT_QUERY).matches) {
        const paneRect = pane.getBoundingClientRect();
        const cellRect = cell.getBoundingClientRect();
        const headHeight = pane.querySelector<HTMLElement>(".feld-agents__bands-head")?.offsetHeight || 0;
        if (cellRect.bottom <= paneRect.top + headHeight || cellRect.top >= paneRect.bottom) {
          useActorsStore.getState().dismissAgentMenu();
          return;
        }
      }
      place();
    }
    place();
    window.addEventListener("scroll", follow, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", follow, true);
      window.removeEventListener("resize", place);
    };
  });

  const targetOf = (action: string) => action === "delete-actor" ? agent.id : menu.sessionId;
  const confirming = (action: string) => Boolean(CONFIRMATIONS[action]) && confirmed?.action === action && targetConfirmed(confirmed.target, targetOf(action));
  function leave(action: string) {
    if (action !== confirmed?.action) setConfirmed(null);
  }

  function run(action: string) {
    const sessionId = menu.sessionId;
    if (CONFIRMATIONS[action] && !confirming(action)) {
      setConfirmed({ action, target: targetOf(action) });
      return;
    }
    setConfirmed(null);
    const request = LIFECYCLE_REQUESTS[action];
    if (request && refusals) {
      refuseMenuAction(agent.id, request, refusals[request].replace("{actor}", agent.name).replace("{session}", sessionId || ""));
      return;
    }
    switch (action) {
      case "toggle-pin": useActorsStore.getState().setPinned(agent, !isPinned(state, agent)); break;
      case "open-console": openConsole(sessionId); break;
      case "stop-session": if (sessionId) stopActorSession(client, agent.id, sessionId); break;
      case "terminate-session": if (sessionId) killActorSession(client, agent.id, sessionId); break;
      case "start-replacement": startActorSession(client, agent); break;
      case "pause-job": setScheduleEnabled(client, model, agent.id, false); break;
      case "resume-job": setScheduleEnabled(client, model, agent.id, true); break;
      case "open-agent": openActorCard(agent.id); break;
      case "open-job": openJob(agent.id); break;
      case "send-message": openComposer(agent.id); break;
      case "delete-actor": deleteActorFromMenu(client, agent); break;
    }
  }

  return (
    <div className="feld-agents__action-menu" role="menu" aria-label={`Actions for ${model.actorLabel(agent.id, state)}`} ref={element}>
      {menu.items.map((item, index) => item.kind === "rule"
        ? <div key={`rule-${index}`} className="feld-agents__action-menu-rule" role="separator"></div>
        : (
          <button key={item.action} type="button" role="menuitem" className={confirming(item.action) ? "is-confirming" : undefined} data-action={item.action} data-agent-id={agent.id} data-session-id={menu.sessionId ?? undefined} disabled={item.disabled} onClick={() => run(item.action)} onFocus={() => leave(item.action)} onPointerEnter={() => leave(item.action)}>
            <strong>{confirming(item.action) ? CONFIRMATIONS[item.action].label : item.label}</strong><small>{confirming(item.action) ? CONFIRMATIONS[item.action].note(item.action === "delete-actor" ? model.actorLabel(agent.id, state) : menu.sessionId || "") : item.note}</small>
          </button>
        ))}
    </div>
  );
}
