// The Actors page as it is drawn: the heading, the bands and the card over them, and the
// page-level dialogs, with the page-wide handling of clicks and Escape.

import { useCallback, useEffect, useLayoutEffect, useMemo, useReducer, useRef, useState, type CSSProperties, type KeyboardEvent, type MouseEvent, type PointerEvent } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { ActorsModelContext } from "./context.ts";
import { Bands } from "./Bands.tsx";
import { AgentDisplayMenu, MetricsMenu, NewActor, NewWorkspace } from "./Dialogs.tsx";
import { Heading, MetricsPicker, Picker } from "./Heading.tsx";
import { useInputModality } from "./inputModality.ts";
import { Inspector } from "./Inspector.tsx";
import { RESIZE_CLASSES } from "./model.ts";
import { InspectorResizer, type ResizeKind } from "./resize.tsx";
import { useActorsStore, useActorsUrlSync } from "./store.ts";
import { Toasts } from "./Toasts.tsx";
import type { ActorsData } from "./useActorsData.ts";
import { ActorsToolsContext, focusIn, useActorsTools, usePageActions, type ActorsTools } from "./ui.tsx";
import { TimelineGuide } from "./charts/Guide.tsx";
import { ChartTooltip } from "./charts/hover.tsx";

export function ActorsView({ data, client }: { data: ActorsData; client: QueryClient | null }) {
  useActorsUrlSync();
  // The DOM steps actions ask for, run once the state they changed is drawn.
  const steps = useRef<((root: HTMLElement) => void)[]>([]);
  const [, drawn] = useReducer((count: number) => count + 1, 0);
  const afterRender = useCallback((step: (root: HTMLElement) => void) => {
    steps.current.push(step);
    drawn();
  }, []);
  // The page shows the lanes the reader chose and every other lane by its default. A quota
  // lane's default follows the windows its Runtime reports, so it is known once the read answers.
  const laneIds = data.input.metrics.map((lane) => `${lane.id}:${lane.defaultVisible}`).join(" ");
  useLayoutEffect(() => {
    useActorsStore.getState().showDefaultMetrics(data.input.metrics);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [laneIds]);
  const tools = useMemo<ActorsTools>(() => ({ client, connection: data.connection, refusals: data.refusals, afterRender }), [client, data.connection, data.refusals, afterRender]);
  return (
    <ActorsModelContext value={data.model}>
      <ActorsToolsContext value={tools}>
        <feld-agents>
          <Actors inspector={data.inspector} steps={steps} />
        </feld-agents>
      </ActorsToolsContext>
    </ActorsModelContext>
  );
}

// A click on anything but one of these closes the Timeline display and an Actions menu
// left open: they are the controls that act by themselves. A click the plot took (a record
// it opened, or the click that ends a pan) is marked handled and closes nothing either.
const SELF_CONTAINED_ACTIONS = new Set([
  "open-timeline-guide", "close-timeline-guide",
  "toggle-agent-display-menu", "close-agent-display-menu", "toggle-metrics-menu", "close-metrics-menu",
  "toggle-actors", "toggle-metrics", "refresh", "toggle-paused", "close-inspector",
  "open-actor", "open-actor-session", "close-actor-session",
  "open-console", "toggle-agent-menu", "open-job", "inspect-schedule", "close-job", "pause-job", "resume-job", "edit-actor", "cancel-actor-edit", "open-agent",
  "select-workspace", "all-workspaces", "new-workspace", "close-new-workspace", "show-agents-outside-range", "hide-agents-outside-range", "new-actor", "close-new-actor",
  "stop-session", "terminate-session", "start-replacement", "inspect-session", "inspect-action", "inspect-message", "send-message",
]);
// The Timeline marks that open what they stand for.
const TIMELINE_RECORDS = "[data-session], [data-action-link], [data-message-link], [data-skip]";

function Actors({ inspector, steps }: { inspector: ActorsData["inspector"]; steps: { current: ((root: HTMLElement) => void)[] } }) {
  const section = useRef<HTMLElement>(null);
  useInputModality();
  const inspectedSessionId = useActorsStore((state) => state.inspectedSessionId);
  const composeAgentId = useActorsStore((state) => state.composeAgentId);
  const actorCard = useActorsStore((state) => Boolean(state.actorCard));
  const inspectorWidth = useActorsStore((state) => state.inspectorWidth);
  const [resizing, setResizing] = useState<ResizeKind | null>(null);
  const store = useActorsStore.getState;
  const hasInspector = Boolean(inspectedSessionId || composeAgentId || actorCard);

  useLayoutEffect(() => {
    const root = section.current;
    if (!root || !steps.current.length) return;
    steps.current.splice(0).forEach((step) => step(root));
  });

  const { afterRender } = useActorsTools();
  const { closeCard, returnFocus } = usePageActions();
  const focus = (selector: string, options?: FocusOptions) => afterRender((root) => focusIn(root, selector, options));

  function dismissMenus(event: MouseEvent<HTMLElement>) {
    if (event.defaultPrevented) return;
    const target = event.target as Element;
    const action = target.closest("[data-action]")?.getAttribute("data-action");
    if ((action && SELF_CONTAINED_ACTIONS.has(action)) || target.closest(TIMELINE_RECORDS)) return;
    const state = store();
    if (state.agentDisplayMenuOpen && !target.closest(".feld-agents__display-menu")) state.closeAgentDisplayMenu();
    if (state.actionMenuAgent && !target.closest(".feld-agents__action-menu")) state.dismissAgentMenu();
  }

  // Escape closes one layer at a time: the page-level modals first, then an info popover,
  // then everything else at once. Focus returns to the control that
  // opened what closed. A modal takes Escape on the document, so it closes wherever the
  // focus fell, including on the body once a submit disabled the focused button.
  const modalOpen = useActorsStore((state) => Boolean(state.newWorkspace || state.newActor));
  useEffect(() => {
    if (!modalOpen) return;
    const focusAfter = (selector: string, options?: FocusOptions) => afterRender((root) => focusIn(root, selector, options));
    function escapeModal(event: globalThis.KeyboardEvent) {
      if (event.key !== "Escape") return;
      const escaped = store().escapeModal();
      if (escaped === "new-workspace") focusAfter('[data-action="toggle-workspace-menu"]', { preventScroll: true });
      else if (escaped === "new-actor") focusAfter('[data-action="new-actor"]', { preventScroll: true });
    }
    document.addEventListener("keydown", escapeModal);
    return () => document.removeEventListener("keydown", escapeModal);
  }, [modalOpen, afterRender, store]);

  // While a page-level dialog is open, everything else in the document is inert: the
  // siblings of the dialog and of each of its ancestors up to the body. Tab and Shift+Tab
  // stay inside the dialog.
  const openDialog = useActorsStore((state) =>
    state.agentDisplayMenuOpen ? "display" : state.timelineGuideOpen ? "guide" : state.metricsMenuOpen ? "metrics"
    : state.newWorkspace ? "new-workspace" : state.newActor ? "new-actor" : null);
  useLayoutEffect(() => {
    const dialog = section.current?.querySelector(".feld-agents__display-modal");
    if (!openDialog || !dialog) return;
    const behind: HTMLElement[] = [];
    for (let node: Element = dialog; node.parentElement && node !== document.body; node = node.parentElement) {
      for (const sibling of node.parentElement.children) {
        if (sibling !== node && sibling instanceof HTMLElement && !sibling.inert) behind.push(sibling);
      }
    }
    behind.forEach((element) => { element.inert = true; });
    return () => behind.forEach((element) => { element.inert = false; });
  }, [openDialog]);

  function escape(event: KeyboardEvent<HTMLElement>) {
    if (event.key !== "Escape" || modalOpen) return;
    const infoButton = (event.target as Element).closest<HTMLElement>(".feld-agents__info");
    if (infoButton) {
      // Close only the popover: blur ends the focus-shown state, and the dismissed class
      // keeps a still-hovering pointer from reopening it until it leaves.
      event.preventDefault();
      infoButton.classList.add("is-dismissed");
      infoButton.blur();
      return;
    }
    // The Actor card's edit form closes first, on its own, and the card stays.
    if (store().cardView?.editing) {
      store().cancelActorEdit();
      focus('[data-action="edit-actor"]', { preventScroll: true });
      return;
    }
    const restore = store().escapeAll();
    if (restore?.display) focus('[data-action="toggle-agent-display-menu"]');
    if (restore?.metrics) focus('[data-action="toggle-metrics-menu"]');
    if (restore?.guide) focus('[data-action="open-timeline-guide"]');
    if (restore?.card) returnFocus(restore.card);
  }

  function leaveInfo(event: PointerEvent<HTMLElement>) {
    (event.target as Element).closest?.(".feld-agents__info")?.classList.remove("is-dismissed");
  }

  const workspaceStyle = hasInspector && inspectorWidth !== null ? { "--inspector-width": `${inspectorWidth}px` } as CSSProperties : undefined;

  return (
    <section className={`feld-agents${resizing ? ` ${RESIZE_CLASSES[resizing]}` : ""}`} aria-label="Actor activity" ref={section} onClick={dismissMenus} onKeyDown={escape} onPointerOut={leaveInfo}>
      <Heading />
      <Picker />
      <MetricsPicker />
      <div className={`feld-agents__workspace${hasInspector ? " has-inspector" : ""}${inspectedSessionId ? " has-session-console" : ""}`} style={workspaceStyle}>
        <Bands onResizing={setResizing} />
        {hasInspector ? <InspectorResizer onResizing={setResizing} /> : null}
        {hasInspector ? <div className="feld-agents__scrim" data-action="close-inspector" aria-hidden="true" onClick={closeCard}></div> : null}
        <ActorsModelContext value={inspector.model}>
          <Inspector consoleReads={inspector.consoleReads} />
        </ActorsModelContext>
      </div>
      <AgentDisplayMenu />
      <TimelineGuide />
      <MetricsMenu />
      <NewActor />
      <NewWorkspace />
      <ChartTooltip />
      <Toasts />
    </section>
  );
}
