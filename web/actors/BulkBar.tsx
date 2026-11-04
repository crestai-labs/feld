// The Bulk actions bar under the Actors header: a box on each row drawn, a box choosing
// every row drawn (the Workspace and search decide which those are), and Start, Stop, and
// Kill for the chosen Actors, with each Actor's result under them.

import { useState } from "react";
import { useActorsModel } from "./context.ts";
import { BULK_ACTIONS, BULK_LABELS, BULK_NOTES, BULK_OUTCOMES, bulkSummary, bulkTargetsFor, chosenAgents } from "./bulk.ts";
import { runBulkAction } from "./mutations.ts";
import { useActorsStore } from "./store.ts";
import type { ActorRecord, AgentsState, BulkAction } from "./types.ts";
import { useActorsTools } from "./ui.tsx";

// The Actors header's switch for the bar.
export function BulkToggle({ open }: { open: boolean }) {
  const pending = useActorsStore((state) => Boolean(state.bulkRequest?.pending));
  return (
    <button type="button" className={`feld-agents__button feld-agents__bulk-toggle${open ? " is-active" : ""}`} data-action="toggle-bulk" aria-pressed={open} disabled={pending} title={open ? "Close Bulk actions and clear the choice" : "Choose several Actors and start, stop, or kill them together"} onClick={() => useActorsStore.getState().toggleBulk()}>
      Bulk actions
    </button>
  );
}

// A row's box while the bar is open.
export function BulkCheckbox({ agent, label, chosen }: { agent: ActorRecord; label: string; chosen: boolean }) {
  const pending = useActorsStore((state) => Boolean(state.bulkRequest?.pending));
  return (
    <input type="checkbox" className="feld-agents__bulk-check" data-action="choose-bulk" data-agent-id={agent.id} aria-label={`Choose ${label} for Bulk actions`} checked={chosen} disabled={pending} onChange={(event) => useActorsStore.getState().setBulkChosen([agent.id], event.target.checked)} />
  );
}

export function BulkBar({ agents, state }: { agents: ActorRecord[]; state: Pick<AgentsState, "bulkActorIds" | "workspace"> }) {
  const model = useActorsModel();
  const { client, refusals } = useActorsTools();
  const request = useActorsStore((current) => current.bulkRequest);
  const [confirmKill, setConfirmKill] = useState<string | null>(null);
  const pending = Boolean(request?.pending);
  const chosen = chosenAgents(agents, state.bulkActorIds);
  const everyShown = agents.length > 0 && chosen.length === agents.length;
  // Kill sends on its second press for the same choice; another choice turns it back.
  const choiceKey = chosen.map((agent) => agent.id).join(" ");

  function chooseAll(all: boolean) {
    setConfirmKill(null);
    useActorsStore.getState().setBulkChosen(agents.map((agent) => agent.id), all);
  }

  function run(action: BulkAction) {
    if (action === "kill" && confirmKill !== choiceKey) {
      setConfirmKill(choiceKey);
      return;
    }
    setConfirmKill(null);
    runBulkAction(client, model, action, chosen, refusals ? refusals[action] : null);
  }

  return (
    <div className="feld-agents__bulk" role="region" aria-label="Bulk actions">
      <div className="feld-agents__bulk-bar">
        <label className="feld-agents__bulk-all">
          <input type="checkbox" data-action="choose-bulk-all" checked={everyShown} ref={(box) => { if (box) box.indeterminate = chosen.length > 0 && !everyShown; }} disabled={pending || !agents.length} onChange={(event) => chooseAll(event.target.checked)} />
          All shown
        </label>
        <span className="feld-agents__bulk-count" aria-live="polite">{chosen.length} of {agents.length} chosen</span>
        <div className="feld-agents__bulk-actions">
          {BULK_ACTIONS.map((action) => {
            const applies = bulkTargetsFor(model, action, chosen).filter((target) => !target.skip).length;
            const confirming = action === "kill" && confirmKill === choiceKey && applies > 0;
            return (
              <button key={action} type="button" className={`feld-agents__button${confirming ? " is-confirming" : ""}`} data-action={`bulk-${action}`} disabled={pending || applies === 0} title={`${BULK_NOTES[action]} · applies to ${applies} of ${chosen.length} chosen`} onClick={() => run(action)}>
                {confirming ? `Confirm kill ${applies}` : `${BULK_LABELS[action]} ${applies}`}
              </button>
            );
          })}
        </div>
      </div>
      {request ? (
        <div className="feld-agents__bulk-results">
          <p className="feld-agents__bulk-summary" aria-live="polite">{BULK_LABELS[request.action]} · {bulkSummary(request.results)}</p>
          <ul>
            {request.results.map((result) => (
              <li key={result.agentId} className={`is-${result.outcome}`} data-bulk-result={result.agentId}>
                <strong>{model.actorLabel(result.agentId, state)}</strong>
                <span>{BULK_OUTCOMES[result.outcome]}</span>
                {result.detail ? <small>{result.detail}</small> : null}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
}
