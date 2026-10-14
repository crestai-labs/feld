// What the Actors page's components share: the page's tools (the query client its writes go
// through, the reads' connection, and the one DOM step an action takes once its result is
// drawn), and the actions that pair a store transition with that step.

import { createContext, useContext, useMemo } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { useActorsModel } from "./context.ts";
import { markPointerFocus } from "./inputModality.ts";
import { openCard, useActorsStore, type OpenCard } from "./store.ts";
import type { MenuRefusals } from "./types.ts";

export interface ActorsTools {
  // The client the page's writes read the page again through; null in fixture mode,
  // where there is no daemon to write to.
  client: QueryClient | null;
  connection: FeldLiveConnection;
  // Fixture mode's refusing daemon; null when the daemon answers the Actions menu.
  refusals: MenuRefusals | null;
  // Runs `step` on the page's root once the state the caller just changed is drawn: the
  // focus or scroll an action moves once, never a restoration after every drawing.
  afterRender(step: (root: HTMLElement) => void): void;
}

export const ActorsToolsContext = createContext<ActorsTools | null>(null);

export function useActorsTools() {
  const tools = useContext(ActorsToolsContext);
  if (!tools) throw new Error("The Actors page provides its tools");
  return tools;
}

export function focusIn(root: HTMLElement, selector: string, options?: FocusOptions) {
  root.querySelector<HTMLElement>(selector)?.focus(options);
}

// An Actor's name in the table, where the focus returns when its Actor card closes.
export function actorNameSelector(agentId: string) {
  return `.feld-agents__actor-name[data-agent-id="${CSS.escape(agentId)}"]`;
}

// The actions reached from more than one place on the page, each with the DOM step that
// follows it.
export function usePageActions() {
  const model = useActorsModel();
  const { afterRender } = useActorsTools();
  return useMemo(() => {
    const store = useActorsStore.getState;
    const focus = (selector: string, options?: FocusOptions) => afterRender((root) => focusIn(root, selector, options));

    // A Session's Console in the card over the Timeline, at a record when given. A Session
    // the snapshot does not hold has no Console to open. The Session opened again, with no
    // record to go to, closes its Console: the same click opens and closes it.
    function openConsole(sessionId: string | null | undefined, recordId: string | null = null) {
      if (!sessionId || !model.sessionById(sessionId)) return;
      if (!recordId && store().inspectedSessionId === sessionId) {
        closeCard();
        return;
      }
      store().openConsole(sessionId, { recordId });
      focus(".feld-agents__console .feld-console__transcript", { preventScroll: true });
    }

    // Opens the Console of the Session whose records hold a record, at that record's row.
    function openRecordConsole(kind: "action" | "message", recordId: string | undefined) {
      const target = model.recordConsoleTarget(kind, recordId);
      if (target) openConsole(target.sessionId, target.recordId);
    }

    // The Actor card at its Job section: from the band's Schedule line, the Actions menu,
    // a skipped tick, a Console record link, the Scheduled-run row in Session details,
    // or the New actor result. A skipped tick comes into view highlighted.
    function openJob(agentId: string | null | undefined, skipId: string | null = null) {
      if (!agentId || !model.input.actors.some((actor) => actor.id === agentId)) return;
      store().openJob(model.actorFor(agentId), { skipId });
      afterRender((root) => {
        const target = (skipId ? root.querySelector(`[data-skip-row="${CSS.escape(skipId)}"]`) : null) || root.querySelector("[data-job-schedule]");
        target?.scrollIntoView({ block: skipId ? "center" : "start" });
        focusIn(root, ".feld-agents__inspector.is-actor", { preventScroll: true });
      });
    }

    // The Actor card, at one of the Actor's Sessions when given: from an Actor name, a
    // Console's Details, a Session row of the card, or a Related activity row naming a
    // Session. It replaces whatever card is open; the Actor's name pressed again while its
    // card is open closes the card, and with it the selection the card put on the lane.
    function openActorCard(agentId: string | null | undefined, sessionId: string | null = null) {
      if (!agentId || !model.input.actors.some((actor) => actor.id === agentId)) return;
      if (!sessionId && store().actorCard?.agentId === agentId) {
        closeCard();
        if (store().focusedActorId === agentId) store().toggleActorFocus(agentId);
        return;
      }
      store().openActorCard(agentId, sessionId && model.sessionById(sessionId) ? sessionId : null);
      focus(".feld-agents__inspector.is-actor", { preventScroll: true });
    }

    // Returns the focus to what opened a card that closed: an Actor card's Actor name, a
    // Console's Session on the Timeline, the composer's Actions menu, or else the Actor's name.
    // A search that hides the Actor leaves the search field. Closed by a pointer (the ×),
    // the control takes the focus without its outline.
    function returnFocus(card: OpenCard | null) {
      if (!card) return;
      const agentId = card.kind === "console" ? model.sessionById(card.sessionId)?.actor : card.agentId;
      const selectors = [
        card.kind === "console" ? `.feld-agents__session-hit[data-session="${CSS.escape(card.sessionId)}"]` : null,
        card.kind === "composer" ? `[data-action="toggle-agent-menu"][data-agent-id="${CSS.escape(card.agentId)}"]` : null,
        agentId ? actorNameSelector(agentId) : null,
        '[data-action="search"]',
      ].filter((selector) => selector !== null);
      afterRender((root) => {
        const target = selectors.map((selector) => root.querySelector<HTMLElement>(selector)).find(Boolean);
        if (!target) return;
        markPointerFocus(target);
        target.focus({ preventScroll: true });
      });
    }

    function closeCard() {
      const card = openCard(store());
      store().clearInspector();
      returnFocus(card);
    }

    // Closes the composer and clears its draft.
    function cancelComposer() {
      const card = openCard(store());
      store().cancelComposer();
      returnFocus(card);
    }

    function openNewActor() {
      const state = store();
      state.openNewActor(state.workspace || "", model.TIME_ZONE, model.input.launchOptions);
      focus(`[data-new-actor] [name="${store().newActor?.chooseWorkspace ? "workspace" : "name"}"]`);
    }

    function closeNewActor() {
      store().closeNewActor();
      focus('[data-action="new-actor"]', { preventScroll: true });
    }

    function openNewWorkspace() {
      store().openNewWorkspace();
      focus('[data-new-workspace] [name="key"]');
    }

    function closeNewWorkspace() {
      store().closeNewWorkspace();
      focus('[data-action="toggle-workspace-menu"]', { preventScroll: true });
    }

    function openComposer(agentId: string) {
      store().openComposer(agentId);
      focus("[data-composer] textarea");
    }

    return { focus, openConsole, openRecordConsole, openJob, openActorCard, returnFocus, closeCard, openNewActor, closeNewActor, openNewWorkspace, closeNewWorkspace, openComposer, cancelComposer };
  }, [model, afterRender]);
}

// ---- Glyphs -------------------------------------------------------------------------------

export function GearGlyph() {
  return <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="10" cy="10" r="2.6"></circle><path d="M10 2.6v2M10 15.4v2M2.6 10h2M15.4 10h2M4.8 4.8l1.4 1.4M13.8 13.8l1.4 1.4M4.8 15.2l1.4-1.4M13.8 6.2l1.4-1.4"></path><circle cx="10" cy="10" r="5.4"></circle></svg>;
}

export function ClockGlyph() {
  return <svg viewBox="0 0 12 12" aria-hidden="true"><circle cx="6" cy="6" r="4.6"></circle><path d="M6 3.4V6l1.9 1.3"></path></svg>;
}
