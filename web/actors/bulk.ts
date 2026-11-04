// The Bulk actions bar's rules: which of the chosen Actors each lifecycle primitive applies
// to, by the same facts as an Actor's Actions menu (a current Session, its ownership, a
// launchSpec), and how each Actor's part in a request is reported.

import { ownershipOf, type ActorsModel } from "./model.ts";
import type { ActorRecord, BulkAction, BulkResult } from "./types.ts";

export const BULK_ACTIONS: readonly BulkAction[] = Object.freeze(["start", "stop", "kill"]);

export const BULK_LABELS: Readonly<Record<BulkAction, string>> = Object.freeze({ start: "Start", stop: "Stop", kill: "Kill" });

export const BULK_NOTES: Readonly<Record<BulkAction, string>> = Object.freeze({
  start: "feld actor start · runs each chosen Actor's launchSpec now",
  stop: "feld session stop · asks each chosen Actor's current Session to end",
  kill: "feld session kill · ends each chosen Actor's current Session without asking",
});

export const BULK_OUTCOMES: Readonly<Record<BulkResult["outcome"], string>> = Object.freeze({
  pending: "Waiting for the daemon",
  accepted: "Accepted",
  refused: "Refused",
  skipped: "Skipped",
});

// One chosen Actor and what the action sends for it: the Session for Stop and Kill, or the
// reason it is skipped.
export interface BulkTarget {
  agent: ActorRecord;
  sessionId: string | null;
  skip: string | null;
}

export function bulkTargetFor(model: Pick<ActorsModel, "latestSessionFor">, action: BulkAction, agent: ActorRecord): BulkTarget {
  const skip = (reason: string): BulkTarget => ({ agent, sessionId: null, skip: reason });
  if (agent.deletedAt) return skip("deleted");
  const session = model.latestSessionFor(agent.id);
  const current = session && session.end === null ? session : null;
  if (action === "start") {
    if (current) return skip(`${current.id} is running`);
    if (!agent.launchSpec) return skip("no launchSpec: created by attach");
    return { agent, sessionId: null, skip: null };
  }
  if (!current) return skip("no running Session");
  if (ownershipOf(current) === "attached") return skip(`${current.id} is attached: the person who started it ends it`);
  return { agent, sessionId: current.id, skip: null };
}

// The chosen Actors among those drawn (`shown`, in their order): the bar acts only on rows
// the reader can see, so a search or Workspace change narrows what it sends.
export function chosenAgents(shown: readonly ActorRecord[], chosen: ReadonlySet<string>) {
  return shown.filter((agent) => chosen.has(agent.id));
}

export function bulkTargetsFor(model: Pick<ActorsModel, "latestSessionFor">, action: BulkAction, agents: readonly ActorRecord[]) {
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
