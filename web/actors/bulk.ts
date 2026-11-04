// The Bulk actions bar's rules: which of the chosen Actors each lifecycle primitive applies
// to, by the same facts as an Actor's Actions menu (a current Session, its ownership, a
// launchSpec), and how each Actor's part in a request is reported. The current Session is
// the one the Actor listing answers whatever the range, so a past range acts on what runs now.

import { MENU_ACTION_NOTES, type ActorsModel } from "./model.ts";
import type { ActorRecord, BulkAction, BulkResult } from "./types.ts";

export const BULK_ACTIONS: readonly BulkAction[] = Object.freeze(["start", "restart", "stop", "kill"]);

export const BULK_LABELS: Readonly<Record<BulkAction, string>> = Object.freeze({ start: "Start", restart: "Restart", stop: "Stop", kill: "Kill" });

export const BULK_NOTES: Readonly<Record<BulkAction, string>> = Object.freeze({
  start: "feld actor start · runs each chosen Actor's launchSpec now",
  restart: "feld actor restart · asks each chosen Actor's current Session to end, then starts the next generation from its launchSpec",
  stop: "feld session stop · asks each chosen Actor's current Session to end",
  kill: "feld session kill · ends each chosen Actor's current Session without asking",
});

// What an accepted request did, as the Actions menu's toast says it.
export const BULK_ACCEPTED: Readonly<Record<BulkAction, string>> = Object.freeze({
  start: MENU_ACTION_NOTES.start,
  restart: "The current Session is asked to end; the next generation starts once it does.",
  stop: MENU_ACTION_NOTES.stop,
  kill: MENU_ACTION_NOTES.kill,
});

export const BULK_OUTCOMES: Readonly<Record<BulkResult["outcome"], string>> = Object.freeze({
  pending: "Waiting for the daemon",
  accepted: "Accepted",
  refused: "Refused",
  skipped: "Skipped",
});

// One chosen Actor and what the action sends for it: the Session for Restart, Stop, and
// Kill, or the reason it is skipped.
export interface BulkTarget {
  agent: ActorRecord;
  sessionId: string | null;
  skip: string | null;
}

export function bulkTargetFor(model: Pick<ActorsModel, "runningSessionOf">, action: BulkAction, agent: ActorRecord): BulkTarget {
  const skip = (reason: string): BulkTarget => ({ agent, sessionId: null, skip: reason });
  if (agent.deletedAt) return skip("deleted");
  const current = model.runningSessionOf(agent);
  if (action === "start") {
    if (current) return skip(`${current.id} is running`);
    if (!agent.launchSpec) return skip("no launchSpec: created by attach");
    return { agent, sessionId: null, skip: null };
  }
  if (!current) return skip("no running Session");
  if (current.ownership === "attached") return skip(`${current.id} is attached: the person who started it ends it`);
  return { agent, sessionId: current.id, skip: null };
}

// The chosen Actors among those drawn (`shown`, in their order): the bar acts only on rows
// the reader can see, so a search or Workspace change narrows what it sends.
export function chosenAgents(shown: readonly ActorRecord[], chosen: ReadonlySet<string>) {
  return shown.filter((agent) => chosen.has(agent.id));
}

export function bulkTargetsFor(model: Pick<ActorsModel, "runningSessionOf">, action: BulkAction, agents: readonly ActorRecord[]) {
  return agents.map((agent) => bulkTargetFor(model, action, agent));
}

// The bar's summary of a request: how many Actors each outcome holds, in a fixed order.
export function bulkSummary(results: readonly BulkResult[]) {
  return (["accepted", "refused", "skipped", "pending"] as const)
    .map((outcome) => [outcome, results.filter((result) => result.outcome === outcome).length] as const)
    .filter(([, count]) => count > 0)
    .map(([outcome, count]) => `${count} ${BULK_OUTCOMES[outcome].toLowerCase()}`)
    .join(" · ");
}
