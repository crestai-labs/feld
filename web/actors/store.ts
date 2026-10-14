// The Actors page's UI state and the transitions between its states. What the daemon
// answered lives in the query cache; this store holds only what the reader chose. Every
// action changes state and nothing else: moving focus, scrolling, and measuring stay with
// the components, and each action names the DOM step that follows it where there is one.

import { useEffect } from "react";
import { useLocation, useNavigate, useSearchParams } from "react-router";
import { create } from "zustand";
import { forgetConsoles, startReading } from "../console/store.ts";
import { storedValue, storeValue } from "../shell/storage.ts";
import {
  ACTOR_COLUMN_WIDTHS_STORAGE_KEY,
  ACTOR_ORDER_STORAGE_KEY,
  ACTOR_PINS_STORAGE_KEY,
  ACTOR_WIDTH,
  ACTOR_WIDTH_LIMITS,
  ACTOR_WIDTH_STORAGE_KEY,
  DEFAULT_ACTOR_DETAILS,
  DEFAULT_WINDOW,
  METRIC_LANES,
  METRIC_LANES_STORAGE_KEY,
  METRICS_WINDOWS,
  METRICS_WINDOW_STORAGE_KEY,
  actorColumnWidthsOf,
  clampActorColumnWidth,
  clampActorWidth,
  emptyActorDraft,
  isPinned,
  isWithin,
  orderWithMove,
  pinsWith,
  reachesNow,
} from "./model.ts";
import { DEFAULT_WORKSPACE_KEY } from "./snapshot.ts";
import { nextSearch, workspaceInSearch } from "./url.ts";
import type { ActorDraft, ActorDraftField, ActorRecord, AgentsState, ComposeDraft, Instant, LaunchOptions, MetricLaneSpec, EditDraft, CardView, VisibilityKey, WorkspaceDraft } from "./types.ts";

// ---- Stored preferences --------------------------------------------------------------

// A width that cannot be read or lies outside its limits falls back to the default.
export function storedActorWidth() {
  const stored = Number(storedValue(ACTOR_WIDTH_STORAGE_KEY, ""));
  return isWithin(stored, ACTOR_WIDTH_LIMITS) ? stored : ACTOR_WIDTH;
}

export function storeActorWidth(width: number) {
  storeValue(ACTOR_WIDTH_STORAGE_KEY, String(width));
}

export function storedMetricsDuration() {
  const stored = Number(storedValue(METRICS_WINDOW_STORAGE_KEY, ""));
  return METRICS_WINDOWS.includes(stored) ? stored : METRICS_WINDOWS[0];
}

function storedJson(key: string) {
  try {
    return JSON.parse(storedValue(key, "null"));
  } catch (_error) {
    return null;
  }
}

export function storedActorColumnWidths() {
  return actorColumnWidthsOf(storedJson(ACTOR_COLUMN_WIDTHS_STORAGE_KEY));
}

export function storedActorPins(): Record<string, boolean> {
  const stored = storedJson(ACTOR_PINS_STORAGE_KEY);
  if (!stored || typeof stored !== "object" || Array.isArray(stored)) return {};
  return Object.fromEntries(Object.entries(stored).filter(([, pinned]) => typeof pinned === "boolean")) as Record<string, boolean>;
}

// The lanes the reader showed (true) or hid (false); an unreadable value is no choice.
export function storedMetricChoices(): Record<string, boolean> {
  const stored = storedJson(METRIC_LANES_STORAGE_KEY);
  if (!stored || typeof stored !== "object" || Array.isArray(stored)) return {};
  return Object.fromEntries(Object.entries(stored).filter(([, visible]) => typeof visible === "boolean")) as Record<string, boolean>;
}

export function storedActorOrder(): string[] {
  const stored = storedJson(ACTOR_ORDER_STORAGE_KEY);
  return Array.isArray(stored) ? stored.filter((key) => typeof key === "string") : [];
}

// The range ends at now at the latest: every anchor change goes through this. It is the
// model's anchorUpToNow for the instant an action is taken at.
function anchorUpToNow(anchor: Instant, duration: number, now: Instant) {
  return Math.min(anchor, now - duration / 2);
}

// ---- State -------------------------------------------------------------------------------

// The page opens Live: the window ends at now and follows it.
export function createInitialState(workspace: string | null | undefined, now: Instant): AgentsState {
  return {
    // The Workspace the page is filtered to, or null for all Workspaces: the
    // `workspace` query of the range reads, present or omitted.
    workspace: workspace === undefined ? DEFAULT_WORKSPACE_KEY : workspace,
    mode: "live",
    anchor: now - DEFAULT_WINDOW / 2,
    duration: DEFAULT_WINDOW,
    query: "",
    pickerOpen: false,
    paused: false,
    refreshes: 0,
    actionMenuAgent: null,
    agentDisplayMenuOpen: false,
    // Whether the guide to every Timeline mark is open.
    timelineGuideOpen: false,
    sessionMetric: "total-tokens",
    showSpawnLinks: true,
    showMessageLinks: true,
    showSubAgents: true,
    showScheduleSkips: true,
    showAgentsOutsideRange: false,
    showDeletedActors: false,
    actorPins: storedActorPins(),
    actorOrder: storedActorOrder(),
    // The Actor card's own state: which Actor, a Job's skipped tick to highlight, and the
    // edit form's state (editing, the draft, the daemon's answer).
    cardView: null,
    // The lifecycle or Schedule request started from an Actor's Actions menu, while it
    // is in flight and while its refusal is shown.
    actionRequest: null,
    // The New actor form keeps its draft in state so a Kind or Schedule change can
    // re-render the fields without losing what was typed.
    newActor: null,
    newWorkspace: null,
    frozenBandIds: null,
    visibleActorDetails: new Set(DEFAULT_ACTOR_DETAILS),
    actorWidth: storedActorWidth(),
    actorColumnWidths: storedActorColumnWidths(),
    metricsMenuOpen: false,
    metricsDuration: storedMetricsDuration(),
    metricsPickerOpen: false,
    expandedMetricIds: new Set(),
    emphasizedSeries: null,
    // Actors and Metrics start open; folded, the Actors header keeps how many Actors run
    // and the Metrics header the latest reading of every visible lane.
    actorsCollapsed: false,
    metricsCollapsed: false,
    visibleMetricIds: new Set(METRIC_LANES.filter((metric) => metric.defaultVisible).map((metric) => metric.id)),
    metricChoices: storedMetricChoices(),
    agentsPaneHeight: null,
    // Session Console starts at half of the workspace. A pixel width is stored only
    // after the reader moves the resize handle, so the default follows the viewport.
    inspectorWidth: null,
    inspectedSessionId: null,
    // The Actor whose relationships the Timeline accents, from its row or lane. It is apart
    // from an open card; an inspected Session takes precedence over it (focusFor).
    focusedActorId: null,
    // The record a Console was opened at, until the Console has scrolled to it once.
    consoleFocus: null,
    // The record a reader was sent to, marked for as long as its Console is open.
    consoleMark: null,
    composeAgentId: null,
    composeSerial: 0,
    // The composer keeps each Actor's text in state until it is sent or cancelled, and holds
    // the accepted Message or the daemon's refusal after a send.
    composeDrafts: {},
    composeSending: {},
    composeResult: null,
    composeError: null,
    // The Actor card over the Timeline: the Actor's facts and Sessions, or one step deeper
    // the facts of one of its Sessions.
    actorCard: null,
  };
}

// The fields clearInspector resets: the Console, the Actor card, and the composer. The
// composer's drafts outlast it.
const CLEARED_INSPECTOR = Object.freeze({
  inspectedSessionId: null,
  cardView: null,
  consoleFocus: null,
  consoleMark: null,
  composeAgentId: null,
  composeResult: null,
  composeError: null,
  actorCard: null,
} satisfies Partial<AgentsState>);

// What Escape closed, so the component returns focus to the control that opened it.
export type EscapedModal = "new-workspace" | "new-actor";

// The card open over the Timeline, by what opened it, so the focus returns there once it closes.
export type OpenCard = { kind: "actor"; agentId: string } | { kind: "console"; sessionId: string } | { kind: "composer"; agentId: string };

export function openCard(state: AgentsState): OpenCard | null {
  if (state.actorCard) return { kind: "actor", agentId: state.actorCard.agentId };
  if (state.inspectedSessionId) return { kind: "console", sessionId: state.inspectedSessionId };
  if (state.composeAgentId) return { kind: "composer", agentId: state.composeAgentId };
  return null;
}

export interface ActorsActions {
  // A fresh page, as a first visit opens it.
  reset(workspace: string | null | undefined, now: Instant): void;

  // The Workspace the page shows, from the switcher, from a Workspace just created, or
  // from the URL. The range reads are keyed by it, so the range is read again.
  selectWorkspace(workspace: string | null): void;
  clearInspector(): void;

  // Opens a Session's Console in the card over the Timeline, at `recordId` when given; it
  // replaces the Console open before. The caller checks that the snapshot holds the
  // Session, except for a deep link, which waits for it.
  // DOM: focus the Console's transcript.
  openConsole(sessionId: string, options?: { recordId?: string | null }): void;
  // The Console scrolled to the record it was opened at.
  consumeConsoleFocus(): void;
  // The Actor card replaces whatever card is open, at one of the Actor's Sessions when
  // given. The caller checks that the snapshot holds the Actor and the Session.
  // DOM: focus `.feld-agents__inspector.is-actor`.
  openActorCard(agentId: string, sessionId?: string | null): void;
  // Selects the Actor's relationships on the Timeline, or clears them when it is selected.
  toggleActorFocus(agentId: string): void;

  // The Actor card at its Job section, for any Job, with or without a Schedule; for any
  // other Actor, the Actor card. DOM: scroll `[data-skip-row="<skipId>"]`, else
  // `[data-job-schedule]`, into view and focus the card.
  openJob(actor: Pick<ActorRecord, "id" | "kind">, options?: { skipId?: string | null }): void;
  // The draft is the model's editDraftOf for the Actor. DOM: focus `[data-edit-form] [name="name"]`.
  editActor(agentId: string, draft: EditDraft): void;
  // DOM: focus `[data-action="edit-actor"]`.
  cancelActorEdit(): void;
  setEditDraftField(field: keyof EditDraft, value: string): void;
  // A request's answer, applied only while the same opening of the card is shown.
  patchCardView(serial: number, patch: Partial<CardView>): boolean;
  // The card follows its Actor to the key of a new name, while the same opening is shown.
  rekeyActorCard(serial: number, agentId: string): void;

  // DOM: focus `[data-new-workspace] [name="key"]`.
  openNewWorkspace(): void;
  // Closing with a created key shows that Workspace, as selecting it from the menu does.
  // DOM: focus `[data-action="toggle-workspace-menu"]`.
  closeNewWorkspace(createdKey?: string | null): void;
  setNewWorkspaceKey(key: string): void;
  patchNewWorkspace(serial: number, patch: Partial<WorkspaceDraft>): boolean;

  // The draft starts in the page's Workspace, or the default one with all shown, and in
  // the page's timezone. DOM: focus `[data-new-actor] [name="name"]`.
  openNewActor(workspace: string, timeZone: string, launchOptions: LaunchOptions): void;
  // DOM: focus `[data-action="new-actor"]`.
  closeNewActor(): void;
  // Kind and Execution change the form's shape; they also clear a refusal shown for the
  // old shape. DOM: keep focus on the changed control.
  setNewActorField(field: ActorDraftField, value: string): void;
  setNewActorAdvancedOpen(open: boolean): void;
  patchNewActor(serial: number, patch: Partial<ActorDraft>): boolean;

  // DOM: focus the guide's close button.
  openTimelineGuide(): void;
  // DOM: focus `[data-action="open-timeline-guide"]`.
  closeTimelineGuide(): void;
  // DOM: focus the display menu's close button when it opened.
  toggleAgentDisplayMenu(): void;
  // DOM: focus `[data-action="toggle-agent-display-menu"]` when closed by its own button.
  closeAgentDisplayMenu(): void;
  // DOM: focus the Metrics display's close button when it opened.
  toggleMetricsMenu(): void;
  // DOM: focus `[data-action="toggle-metrics-menu"]`.
  closeMetricsMenu(): void;
  // DOM: focus `[data-action="toggle-actors"]`.
  toggleActorsCollapsed(): void;
  // DOM: focus `[data-action="toggle-metrics"]`.
  toggleMetricsCollapsed(): void;
  // A display setting. Hiding Sub Agents closes the Console when it shows one
  // (`inspectedIsSubAgent`). DOM: keep focus on the checkbox.
  setVisibility(key: VisibilityKey, value: boolean, inspectedIsSubAgent?: boolean): void;
  // The hidden-bands button, which shows or hides the Actors with no Session in range.
  // DOM: focus the button it becomes.
  setAgentsOutsideRange(shown: boolean): void;
  // The Actions menu's Pin to top / Unpin. It closes the menu.
  setPinned(agent: ActorRecord, pinned: boolean): void;
  // A row dropped above or below another of the rows shown (`shown`, in their order). A
  // drop among the pinned rows pins the dragged one, and among the unpinned rows unpins it.
  moveActor(shown: ActorRecord[], dragged: ActorRecord, onto: ActorRecord, after: boolean): void;
  setActorDetail(id: string, visible: boolean): void;
  setMetricVisible(id: string, visible: boolean): void;
  // The snapshot's lanes shown: each lane the reader chose by that choice, and every other
  // lane by its default.
  showDefaultMetrics(lanes: readonly Pick<MetricLaneSpec, "id" | "defaultVisible">[]): void;
  setSessionMetric(metric: string): void;
  setQuery(query: string): void;

  // The Actions menu. Neither toggles nor closes while a request of it is in flight.
  toggleAgentMenu(agentId: string): void;
  dismissAgentMenu(): void;

  // DOM: focus `[data-composer] textarea`.
  openComposer(agentId: string): void;
  // Of the Actor whose composer is open.
  setComposeDraft(text: string): void;
  // Closes the composer and drops its draft.
  cancelComposer(): void;

  togglePicker(): void;
  closePicker(): void;
  closeMetricsPicker(): void;
  // The Metrics window's picker, whose Quick ranges choose the window's length.
  toggleMetricsPicker(): void;
  setMetricsDuration(duration: number): void;
  // "+N more" and Show fewer of a lane. DOM: keep focus on the button.
  toggleMetricSeries(metricId: string): void;
  emphasizeSeries(key: string | null): void;
  // "Last …" in the picker: a live window of that duration.
  quickLive(duration: number, now: Instant): void;
  // A fixed range around the picker's instant, which the model's instantOfLocal reads.
  applyPicker(instant: Instant, duration: number, now: Instant): void;
  // A pan of the Timeline or the Metrics lanes to `anchor`: a range that reaches now is
  // Live again.
  panTo(anchor: Instant, now: Instant): void;
  setPaused(paused: boolean): void;
  // Refresh: the page reads the range again. DOM: turn the Refresh icon once.
  refresh(): void;
  // A pointer gesture keeps the bands it started with until it ends.
  beginGesture(bandIds: string[]): void;
  endGesture(): void;

  // Sizes the component measured and clamped (clampPaneHeight, inspectorBounds).
  setAgentsPaneHeight(height: number): void;
  setInspectorWidth(width: number): void;
  // Stored once the drag ends or on a keyboard step (`persist`).
  setActorWidth(width: number, persist?: boolean): void;
  setActorColumnWidth(id: string, width: number, persist?: boolean): void;

  // Escape closes the page-level modals before anything behind them. Returns what closed.
  // DOM: "new-workspace" and "new-actor" focus their close actions.
  escapeModal(): EscapedModal | null;
  // Everything else Escape closes at once. Returns which display dialog and card were open,
  // so the component returns focus to what opened them, or null when nothing was open.
  escapeAll(): { display: boolean; metrics: boolean; guide: boolean; card: OpenCard | null } | null;
}

export type ActorsStore = AgentsState & ActorsActions;

// Each opening of a dialog gets its own serial (see CardView).
let serials = 0;

function nextSerial() {
  serials += 1;
  return serials;
}

// The drafts with the Actor's text replaced by a new draft; an empty text drops the Actor's
// entry.
export function draftsWith(drafts: Record<string, ComposeDraft>, agentId: string, text: string) {
  return text ? { ...drafts, [agentId]: { text, serial: nextSerial() } } : without(drafts, agentId);
}

export function without<T>(record: Record<string, T>, key: string) {
  const { [key]: _removed, ...rest } = record;
  return rest;
}

export const useActorsStore = create<ActorsStore>()((set, get) => ({
  ...createInitialState(undefined, Date.now()),

  reset(workspace, now) {
    set(createInitialState(workspace, now));
  },

  selectWorkspace(workspace) {
    if (workspace === get().workspace) return;
    set({ workspace, ...CLEARED_INSPECTOR, focusedActorId: null, cardView: null, newActor: null });
  },

  clearInspector() {
    set(CLEARED_INSPECTOR);
  },

  openConsole(sessionId, { recordId = null } = {}) {
    const consoleFocus = recordId ? { sessionId, recordId } : null;
    // A Session line opens the Console afresh even when it is already open; a record link
    // keeps its reading and scrolls to the record.
    if (!recordId) startReading(sessionId);
    set({
      consoleFocus,
      consoleMark: consoleFocus,
      composeAgentId: null,
      composeResult: null,
      composeError: null,
      actorCard: null,
      // A Console opened from the New actor result replaces that dialog; the card over
      // the Timeline is where the Session is read.
      cardView: null,
      newActor: null,
      inspectedSessionId: sessionId,
      actionMenuAgent: null,
    });
  },

  consumeConsoleFocus() {
    if (get().consoleFocus) set({ consoleFocus: null });
  },

  openActorCard(agentId, sessionId = null) {
    // The card's edit survives a step into a Session's facts and back.
    const held = get().cardView;
    set({ ...CLEARED_INSPECTOR, actorCard: { agentId, sessionId }, cardView: held?.agentId === agentId ? held : null, focusedActorId: agentId, actionMenuAgent: null });
  },

  toggleActorFocus(agentId) {
    set({ focusedActorId: get().focusedActorId === agentId ? null : agentId });
  },

  openJob(actor, { skipId = null } = {}) {
    const held = get().cardView;
    const cardView = held?.agentId === actor.id
      ? { ...held, skipId }
      : actor.kind === "job"
        ? { serial: nextSerial(), agentId: actor.id, skipId, editing: false, draft: null, error: null, result: null, busy: false }
        : null;
    set({ ...CLEARED_INSPECTOR, actorCard: { agentId: actor.id, sessionId: null }, cardView, focusedActorId: actor.id, actionMenuAgent: null, newActor: null });
  },

  editActor(agentId, draft) {
    const held = get().cardView;
    const view = held?.agentId === agentId ? held : { serial: nextSerial(), agentId, skipId: null, busy: false };
    // The draft is kept in state from here on, so a snapshot's re-render does not
    // drop what is being typed.
    set({ cardView: { ...view, editing: true, draft, error: null, result: null } });
  },

  cancelActorEdit() {
    const modal = get().cardView;
    if (!modal) return;
    set({ cardView: { ...modal, editing: false, draft: null, error: null } });
  },

  setEditDraftField(field, value) {
    const modal = get().cardView;
    if (!modal?.draft) return;
    set({ cardView: { ...modal, draft: { ...modal.draft, [field]: value } } });
  },

  patchCardView(serial, patch) {
    const modal = get().cardView;
    if (!modal || modal.serial !== serial) return false;
    set({ cardView: { ...modal, ...patch } });
    return true;
  },

  rekeyActorCard(serial, agentId) {
    const { cardView, actorCard, focusedActorId } = get();
    if (!cardView || cardView.serial !== serial) return;
    set({
      cardView: { ...cardView, agentId },
      actorCard: actorCard?.agentId === cardView.agentId ? { ...actorCard, agentId } : actorCard,
      focusedActorId: focusedActorId === cardView.agentId ? agentId : focusedActorId,
    });
  },

  openNewWorkspace() {
    set({ newWorkspace: { serial: nextSerial(), key: "", error: null, submitting: false }, cardView: null, newActor: null, actionMenuAgent: null });
  },

  closeNewWorkspace(createdKey = null) {
    set({ newWorkspace: null });
    if (createdKey !== null) get().selectWorkspace(createdKey);
  },

  setNewWorkspaceKey(key) {
    const draft = get().newWorkspace;
    if (draft) set({ newWorkspace: { ...draft, key } });
  },

  patchNewWorkspace(serial, patch) {
    const draft = get().newWorkspace;
    if (!draft || draft.serial !== serial) return false;
    set({ newWorkspace: { ...draft, ...patch } });
    return true;
  },

  openNewActor(workspace, timeZone, launchOptions) {
    set({ newActor: emptyActorDraft(nextSerial(), workspace, timeZone, launchOptions), cardView: null, actionMenuAgent: null });
  },

  closeNewActor() {
    set({ newActor: null });
  },

  setNewActorField(field, value) {
    const draft = get().newActor;
    if (!draft) return;
    set({ newActor: { ...draft, [field]: value, ...(field === "kind" ? { error: null } : {}) } });
  },

  setNewActorAdvancedOpen(open) {
    const draft = get().newActor;
    if (draft && draft.advancedOpen !== open) set({ newActor: { ...draft, advancedOpen: open } });
  },

  patchNewActor(serial, patch) {
    const draft = get().newActor;
    if (!draft || draft.serial !== serial) return false;
    set({ newActor: { ...draft, ...patch } });
    return true;
  },

  openTimelineGuide() {
    set({ timelineGuideOpen: true, agentDisplayMenuOpen: false, metricsMenuOpen: false, actionMenuAgent: null });
  },

  closeTimelineGuide() {
    set({ timelineGuideOpen: false });
  },

  toggleAgentDisplayMenu() {
    set({ agentDisplayMenuOpen: !get().agentDisplayMenuOpen, metricsMenuOpen: false, actionMenuAgent: null });
  },

  closeAgentDisplayMenu() {
    set({ agentDisplayMenuOpen: false });
  },

  toggleMetricsMenu() {
    set({ metricsMenuOpen: !get().metricsMenuOpen, agentDisplayMenuOpen: false, actionMenuAgent: null });
  },

  closeMetricsMenu() {
    set({ metricsMenuOpen: false });
  },

  toggleActorsCollapsed() {
    set({ actorsCollapsed: !get().actorsCollapsed });
  },

  toggleMetricsCollapsed() {
    set({ metricsCollapsed: !get().metricsCollapsed });
  },

  setVisibility(key, value, inspectedIsSubAgent = false) {
    set({
      [key]: value,
      ...(key === "showSubAgents" && !value && inspectedIsSubAgent ? CLEARED_INSPECTOR : {}),
    });
  },

  setAgentsOutsideRange(shown) {
    set({ showAgentsOutsideRange: shown });
  },

  setPinned(agent, pinned) {
    const actorPins = pinsWith(get().actorPins, agent, pinned);
    storeValue(ACTOR_PINS_STORAGE_KEY, JSON.stringify(actorPins));
    set({ actorPins, actionMenuAgent: null });
  },

  moveActor(shown, dragged, onto, after) {
    const state = get();
    const pinned = isPinned(state, onto);
    if (isPinned(state, dragged) !== pinned) get().setPinned(dragged, pinned);
    const actorOrder = orderWithMove(state.actorOrder, shown.map((agent) => agent.id), dragged.id, onto.id, after);
    storeValue(ACTOR_ORDER_STORAGE_KEY, JSON.stringify(actorOrder));
    set({ actorOrder });
  },

  setActorDetail(id, visible) {
    const details = new Set(get().visibleActorDetails);
    if (visible) details.add(id);
    else details.delete(id);
    set({ visibleActorDetails: details });
  },

  setMetricVisible(id, visible) {
    const metrics = new Set(get().visibleMetricIds);
    if (visible) metrics.add(id);
    else metrics.delete(id);
    const metricChoices = { ...get().metricChoices, [id]: visible };
    storeValue(METRIC_LANES_STORAGE_KEY, JSON.stringify(metricChoices));
    set({ visibleMetricIds: metrics, metricChoices });
  },

  showDefaultMetrics(lanes) {
    const choices = get().metricChoices;
    set({ visibleMetricIds: new Set(lanes.filter((lane) => choices[lane.id] ?? lane.defaultVisible).map((lane) => lane.id)) });
  },

  setSessionMetric(metric) {
    set({ sessionMetric: metric });
  },

  setQuery(query) {
    set({ query });
  },

  toggleAgentMenu(agentId) {
    const state = get();
    if (state.actionRequest?.pending) return;
    set({ actionRequest: null, actionMenuAgent: state.actionMenuAgent === agentId ? null : agentId });
  },

  dismissAgentMenu() {
    const state = get();
    if (!state.actionMenuAgent || state.actionRequest?.pending) return;
    set({ actionRequest: null, actionMenuAgent: null });
  },

  openComposer(agentId) {
    set({ ...CLEARED_INSPECTOR, composeAgentId: agentId, composeSerial: nextSerial(), actionMenuAgent: null });
  },

  setComposeDraft(text) {
    const { composeAgentId, composeDrafts } = get();
    if (composeAgentId) set({ composeDrafts: draftsWith(composeDrafts, composeAgentId, text) });
  },

  cancelComposer() {
    const { composeAgentId, composeDrafts } = get();
    set({ ...CLEARED_INSPECTOR, composeDrafts: composeAgentId ? draftsWith(composeDrafts, composeAgentId, "") : composeDrafts });
  },

  togglePicker() {
    set({ pickerOpen: !get().pickerOpen, metricsPickerOpen: false });
  },

  toggleMetricsPicker() {
    set({ metricsPickerOpen: !get().metricsPickerOpen, pickerOpen: false });
  },

  setMetricsDuration(duration) {
    storeValue(METRICS_WINDOW_STORAGE_KEY, String(duration));
    set({ metricsDuration: duration, metricsPickerOpen: false });
  },

  emphasizeSeries(key) {
    if (get().emphasizedSeries !== key) set({ emphasizedSeries: key });
  },

  toggleMetricSeries(metricId) {
    const expanded = new Set(get().expandedMetricIds);
    if (!expanded.delete(metricId)) expanded.add(metricId);
    set({ expandedMetricIds: expanded });
  },

  closePicker() {
    set({ pickerOpen: false });
  },

  closeMetricsPicker() {
    set({ metricsPickerOpen: false });
  },

  quickLive(duration, now) {
    set({ mode: "live", duration, anchor: now - duration / 2, pickerOpen: false });
  },

  applyPicker(instant, duration, now) {
    set({ duration, anchor: anchorUpToNow(Math.min(instant, now), duration, now), mode: "historical", pickerOpen: false });
  },

  panTo(anchor, now) {
    const { duration } = get();
    if (reachesNow({ start: anchor - duration / 2, end: anchor + duration / 2 }, now)) {
      set({ mode: "live", anchor: now - duration / 2 });
    } else {
      set({ mode: "historical", anchor });
    }
  },

  setPaused(paused) {
    set({ paused });
  },

  refresh() {
    set({ refreshes: get().refreshes + 1 });
  },

  beginGesture(bandIds) {
    set({ frozenBandIds: bandIds });
  },

  endGesture() {
    if (get().frozenBandIds) set({ frozenBandIds: null });
  },

  setAgentsPaneHeight(height) {
    set({ agentsPaneHeight: Math.round(height) });
  },

  setInspectorWidth(width) {
    set({ inspectorWidth: Math.round(width) });
  },

  setActorWidth(width, persist = false) {
    const actorWidth = Math.round(clampActorWidth(width));
    set({ actorWidth });
    if (persist) storeActorWidth(actorWidth);
  },

  setActorColumnWidth(id, width, persist = false) {
    const actorColumnWidths = { ...get().actorColumnWidths, [id]: Math.round(clampActorColumnWidth(id, width)) };
    set({ actorColumnWidths });
    if (persist) storeValue(ACTOR_COLUMN_WIDTHS_STORAGE_KEY, JSON.stringify(actorColumnWidths));
  },

  escapeModal() {
    const state = get();
    if (state.newWorkspace) {
      get().closeNewWorkspace();
      return "new-workspace";
    }
    if (state.newActor) {
      get().closeNewActor();
      return "new-actor";
    }
    return null;
  },

  escapeAll() {
    const state = get();
    const open = state.pickerOpen || state.metricsPickerOpen || state.agentDisplayMenuOpen || state.metricsMenuOpen || state.timelineGuideOpen || state.actionMenuAgent || state.inspectedSessionId || state.composeAgentId || state.actorCard;
    // The selected Actor outlasts what is open and clears with the Escape after.
    if (!open) {
      if (state.focusedActorId) set({ focusedActorId: null });
      return null;
    }
    const restore = { display: state.agentDisplayMenuOpen, metrics: state.metricsMenuOpen, guide: state.timelineGuideOpen, card: openCard(state) };
    set({
      pickerOpen: false,
      metricsPickerOpen: false,
      agentDisplayMenuOpen: false,
      metricsMenuOpen: false,
      timelineGuideOpen: false,
      actionMenuAgent: null,
      ...CLEARED_INSPECTOR,
    });
    return restore;
  },
}));

// A Console opens in the viewer's default view with its tools closed, and at its tail the
// first time; the reading positions of the Sessions it showed last until it closes.
useActorsStore.subscribe((state, previous) => {
  if (state.inspectedSessionId === previous.inspectedSessionId) return;
  if (state.inspectedSessionId) startReading(state.inspectedSessionId);
  else forgetConsoles();
});

// ---- The URL ---------------------------------------------------------------------------
// The query names the Workspace shown (`?workspace=`) and the Session inspected
// (`?session=`), so a reload, a back, a forward, and a shared link land on the same
// picture. The URL is where the page opens and where history moves it: when the query
// changes, the store follows it. The store is where the reader moves it: when the store
// changes, the query follows it, a Workspace as a new history entry (as the switcher did)
// and the inspected Session in place, so reading Consoles does not fill the history.

export function useActorsUrlSync() {
  const [search] = useSearchParams();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const urlWorkspace = workspaceInSearch(search);
  const urlSession = search.get("session");
  const workspace = useActorsStore((state) => state.workspace);
  const inspectedSessionId = useActorsStore((state) => state.inspectedSessionId);

  // URL → store, on the first render and whenever history moves the query.
  useEffect(() => {
    const store = useActorsStore.getState();
    if (urlWorkspace !== store.workspace) store.selectWorkspace(urlWorkspace);
    if (urlSession !== useActorsStore.getState().inspectedSessionId) {
      if (urlSession) store.openConsole(urlSession);
      else store.clearInspector();
    }
  }, [urlWorkspace, urlSession]);

  // Store → URL. The store is read as it is now: the effect above may have just moved it
  // to the query, and then there is nothing to write. The page's own path is kept as it
  // is, so the address stays one a reload and a shared link open again.
  useEffect(() => {
    const state = useActorsStore.getState();
    const next = nextSearch(search, state.workspace, state.inspectedSessionId);
    if (next) navigate({ pathname, search: `?${next.search}` }, { replace: next.replace });
    // Only a change of the store's two values writes the query; a change of the query is
    // the effect above's.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace, inspectedSessionId]);
}
