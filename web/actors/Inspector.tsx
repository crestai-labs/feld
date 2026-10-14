// The card over the Timeline: a Session's Console, the Actor card, or the Message composer
// for an Actor's mailbox.

import { memo, useMemo, type FormEvent } from "react";
import { useShallow } from "zustand/react/shallow";
import { ConsoleTerminal } from "../console/ConsoleTerminal.tsx";
import type { ConsoleReads } from "../console/reads.ts";
import { commandLine } from "../lib/command-line.ts";
import { useActorsModel } from "./context.ts";
import { COMMAND_HELP, ENV_HELP, FormError, FormField, TimezoneSelect } from "./Dialogs.tsx";
import {
  BINDING_PROVENANCE,
  contextDetailFor,
  contextValueFor,
  actorNameOf,
  kindLabel,
  metricTitleFor,
  modelLabel,
  executionDetailFor,
  formatDuration,
  kindRules,
  ownershipOf,
  placementRows,
  runtimeLabel,
  usageDetailFor,
  usageValueFor,
  runtimeName,
  editDraftOf,
  SECRET_MASK,
  settingsText,
} from "./model.ts";
import { saveActorEdit, sendComposedMessage, setScheduleEnabled } from "./mutations.ts";
import { useActorsStore } from "./store.ts";
import type { ActorRecord, AgentSession, CardView, EditDraft, LaunchOptions, ScheduleRecord, ScheduleRun } from "./types.ts";
import { useActorsTools, usePageActions } from "./ui.tsx";

// The Session Console, the Actor card, or the composer, whichever the card holds. None is
// drawn again while a historical range pans: they read the page where the pan settles
// (see useSettled).
export const Inspector = memo(function Inspector({ consoleReads }: { consoleReads: ConsoleReads }) {
  const model = useActorsModel();
  const inspectedSessionId = useActorsStore((state) => state.inspectedSessionId);
  const actorCard = useActorsStore((state) => state.actorCard);
  const composeAgentId = useActorsStore((state) => state.composeAgentId);
  const session = model.sessionById(inspectedSessionId);
  return (
    <>
      {session ? (
        <aside className="feld-agents__inspector is-console" aria-label="Session Console">
          <ConsolePanel session={session} consoleReads={consoleReads} />
        </aside>
      ) : null}
      {actorCard ? <ActorCardPanel agentId={actorCard.agentId} sessionId={actorCard.sessionId} /> : null}
      {composeAgentId ? <Composer agentId={composeAgentId} /> : null}
    </>
  );
});

// ---- Console -------------------------------------------------------------------------------

// Console view of the card for the exact selected Session. One Console is open at a
// time: opening another Session replaces it.
function ConsolePanel({ session, consoleReads }: { session: AgentSession; consoleReads: ConsoleReads }) {
  const model = useActorsModel();
  const { openConsole, openJob, openActorCard, closeCard } = usePageActions();
  const workspace = useActorsStore((state) => state.workspace);
  const consoleFocus = useActorsStore((state) => state.consoleFocus);
  const consoleMark = useActorsStore((state) => state.consoleMark);
  const options = useMemo(() => model.consoleOptionsFor(session, { workspace }), [model, session, workspace]);
  const listed = model.consoleSessionFor(session.id);
  const consoleSession = useMemo(() => (listed ? consoleReads.consoleSessionFor(listed) : null), [listed, consoleReads]);

  const controls = (
    <div className="feld-agents__console-controls">
      <button className="feld-agents__details-button" type="button" data-action="open-actor" data-agent-id={session.actor} data-session-id={session.id} aria-haspopup="dialog" onClick={() => openActorCard(session.actor, session.id)}>
        <svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6"></circle><path d="M8 7.1v4M8 4.7v.1"></path></svg>
        <span>Details</span>
      </button>
      <button className="feld-agents__details-button feld-agents__console-close" type="button" data-action="close-inspector" aria-label="Close Console" title="Close Console" onClick={closeCard}>×</button>
    </div>
  );

  return (
    <div className="feld-agents__console" data-session-id={session.id}>
      <ConsoleTerminal
        session={consoleSession}
        controls={controls}
        {...options}
        focus={consoleFocus?.sessionId === session.id ? consoleFocus.recordId : null}
        onFocused={() => useActorsStore.getState().consumeConsoleFocus()}
        mark={consoleMark?.sessionId === session.id ? consoleMark.recordId : null}
        onOpenConsole={(sessionId, recordId) => openConsole(sessionId, recordId || null)}
        onOpenJob={(agentId) => openJob(agentId)}
      />
    </div>
  );
}

// ---- Actor card ----------------------------------------------------------------------------

// The Actor card: opened from an Actor name, it overlays the Timeline from the right like the
// Console. It holds the Actor's own facts and its Session table, and one step deeper the
// facts of a single Session with the way into its Console.
function ActorCardPanel({ agentId, sessionId }: { agentId: string; sessionId: string | null }) {
  const model = useActorsModel();
  const { openActorCard, closeCard } = usePageActions();
  const workspace = useActorsStore((state) => state.workspace);
  const agent = model.input.actors.find((candidate) => candidate.id === agentId);
  if (!agent) return null;
  const session = sessionId ? model.sessionById(sessionId) : null;
  return (
    <aside className="feld-agents__inspector is-actor" role="dialog" aria-labelledby={session ? "feld-agents-session-details-title" : "feld-agents-actor-card-title"} tabIndex={-1}>
      <div className="feld-agents__inspector-top">
        <div className="feld-agents__inspector-eyebrow">
          {session ? <button type="button" className="feld-agents__card-back" data-action="close-actor-session" aria-label={`Back to ${model.actorLabel(agent.id, { workspace })}`} onClick={() => openActorCard(agent.id)}><span aria-hidden="true">←</span> {model.actorLabel(agent.id, { workspace })}</button> : "Actor"}
        </div>
        <button type="button" data-action="close-inspector" aria-label={`Close ${session ? "Session details" : "Actor card"}`} title="Close" onClick={closeCard}>×</button>
      </div>
      {session ? <SessionFacts session={session} /> : <ActorOverview agent={agent} />}
    </aside>
  );
}

function ActorOverview({ agent }: { agent: ActorRecord }) {
  const model = useActorsModel();
  const { openConsole, openComposer } = usePageActions();
  const workspace = useActorsStore((state) => state.workspace);
  const session = model.latestSessionFor(agent.id);
  const actorState = model.actorStateFor(agent);
  const schedule = model.scheduleFor(agent.id);
  const waiting = model.waitingMessagesFor(agent.id);
  const sessions = model.allSessionsFor(agent.id);
  return (
    <>
      <div className="feld-agents__inspector-title">
        <div>
          <h2 id="feld-agents-actor-card-title">{model.actorLabel(agent.id, { workspace })}</h2>
          <span className="feld-agents__inspector-agent">{kindLabel(agent.kind)} · Workspace {agent.workspace}{agent.deletedAt ? ` · deleted ${model.nearTime(agent.deletedAt)}` : ""}</span>
        </div>
        <span className={`feld-agents__session-state is-${actorState.key}`} title={actorState.title}>{actorState.label}</span>
      </div>
      <dl className="feld-agents__inspector-facts">
        <div><dt>Kind</dt><dd className="is-multiline"><strong>{kindLabel(agent.kind)}</strong><span>{kindRules(agent.kind).summary} · Control Plane fact declared at creation</span></dd></div>
        <LaunchSpecRows agent={agent} />
        <GrantsRow agent={agent} />
        <div><dt>Mailbox</dt><dd><strong>{waiting.length} waiting</strong><span>{waiting.length ? `oldest accepted ${model.nearTime(waiting[0].accepted)}` : "no Message is waiting for a read"}</span></dd></div>
        <div><dt>Sessions</dt><dd><strong>{sessions.length}</strong><span>{session ? `latest ${session.id}` : "none recorded"}</span></dd></div>
      </dl>
      <div className="feld-agents__card-actions">
        {session ? <button className="feld-agents__button" type="button" data-action="open-console" data-session-id={session.id} onClick={() => openConsole(session.id)}>Open latest Console</button> : null}
        {agent.deletedAt ? null : <button className="feld-agents__button" type="button" data-action="send-message" data-agent-id={agent.id} onClick={() => openComposer(agent.id)}>Send message</button>}
      </div>
      {agent.kind === "job" ? <JobSection agent={agent} schedule={schedule} /> : <AgentSection agent={agent} />}
      <h3 className="feld-agents__inspector-section">{schedule ? "Sessions and skipped ticks" : "Sessions"} · Control Plane fact</h3>
      <ActorSessions agent={agent} schedule={schedule} />
    </>
  );
}

// The launch spec of an Actor, as its facts show it: the execution, where it runs, and
// the names of its environment variables.
function LaunchSpecRows({ agent }: { agent: ActorRecord }) {
  const model = useActorsModel();
  const launchSpec = agent.launchSpec;
  if (!launchSpec) return <div><dt>Launch spec</dt><dd className="is-multiline"><strong>None</strong><span>Created by attach; Feld cannot start a Session for it until <code>feld actor update {agent.name} …</code> gives one</span></dd></div>;
  const execution = launchSpec.runtime ? `${runtimeName(launchSpec.runtime, model.input.launchOptions.runtimes)} (--runtime ${launchSpec.runtime})` : commandLine(launchSpec.command);
  const envNames = Object.keys(launchSpec.env || {});
  return (
    <>
      <div><dt>Launch spec</dt><dd className="is-multiline"><strong>{execution}</strong><span>{launchSpec.workdir} · {launchSpec.provider} · {launchSpec.node} · the Actor's launch recipe · Control Plane fact; each start copies it into the Session's execution and decides the Placement then</span></dd></div>
      {envNames.length ? <div><dt>Environment</dt><dd className="is-multiline"><strong>{envNames.join(", ")}</strong><span>Names only; the values can be secrets · set beneath the variables Feld sets</span></dd></div> : null}
      {launchSpec.runtime ? <div><dt>Trust workdir</dt><dd className="is-multiline"><strong>{launchSpec.trustWorkdir ? "Yes" : "No"}</strong><span>{launchSpec.trustWorkdir ? "The Runtime reads the settings the workdir carries without asking" : "The Runtime asks before it reads the settings the workdir carries"}</span></dd></div> : null}
      {launchSpec.providerOptions ? <div><dt>Provider options</dt><dd className="is-multiline"><code className="feld-agents__settings" data-provider-options="">{settingsText(launchSpec.providerOptions)}</code><span>The {launchSpec.provider} Provider's own choices, as it resolved them · secret-like values read as {SECRET_MASK}</span></dd></div> : null}
      {launchSpec.runtime ? <div><dt>Runtime settings</dt><dd className="is-multiline">{launchSpec.runtimeSettings ? <code className="feld-agents__settings" data-runtime-settings="">{settingsText(launchSpec.runtimeSettings)}</code> : <strong>None</strong>}<span>Passed to the Runtime as declared · values under key, token, secret, password, or credential keys read as {SECRET_MASK}</span></dd></div> : null}
    </>
  );
}

// What the Admin lets the Actor's Sessions do to the other Actors of the Workspace.
function GrantsRow({ agent }: { agent: ActorRecord }) {
  const grants = agent.grants || [];
  return <div><dt>Grants</dt><dd className="is-multiline"><strong>{grants.length ? grants.join(", ") : "None"}</strong><span>Given by the Admin alone · <code>feld actor update {agent.name} --grant …</code> replaces them</span></dd></div>;
}

// The Job section of the Actor card: the Schedule's own fields and the derived next run,
// with Edit and Pause / Resume, or the statement that the Job has no Schedule, with Edit.
// In edit, the form over the name, cron, timezone and the launch spec takes the section's
// place.
function JobSection({ agent, schedule }: { agent: ActorRecord; schedule: ScheduleRecord | null }) {
  const model = useActorsModel();
  const { client } = useActorsTools();
  const cardView = useActorsStore((state) => (state.cardView?.agentId === agent.id ? state.cardView : null));
  const edit = useEditAction(agent, schedule);
  if (cardView?.editing && cardView.draft) return <ActorEditForm agent={agent} schedule={schedule} cardView={cardView} />;
  // A deleted Actor keeps its history and takes no more changes.
  const deleted = Boolean(agent.deletedAt);
  if (!schedule) {
    return (
      <>
        <h3 className="feld-agents__inspector-section" data-job-schedule="">Schedule</h3>
        <dl className="feld-agents__inspector-facts">
          <div><dt>Schedule</dt><dd className="is-multiline"><strong>None</strong><span>Feld decides no start time for this Job: a Session starts when the Admin or another Session spawns or starts it · <code>feld actor update {agent.name} --cron … --timezone …</code> declares one</span></dd></div>
        </dl>
        {cardView?.error ? <FormError message={cardView.error} /> : null}
        {cardView?.result ? <p className="feld-agents__composer-result" role="status">{cardView.result}</p> : null}
        <div className="feld-agents__card-actions">
          <button className="feld-agents__button" type="button" data-action="edit-actor" disabled={deleted} onClick={edit}>Edit…</button>
        </div>
        <p className="feld-agents__form-note">Without a Schedule there is nothing to pause: the band follows the range rule alone. Delete actor in the Actions menu retires the Job and keeps its history.</p>
      </>
    );
  }
  const facts = model.scheduleFactsFor(schedule);
  return (
    <>
      <h3 className="feld-agents__inspector-section" data-job-schedule="">Schedule · Control Plane fact</h3>
      <dl className="feld-agents__inspector-facts">
        <div><dt>Enabled</dt><dd className="is-multiline"><strong>{schedule.enabled ? "Yes" : "No"}</strong><span>{facts.enabled}</span></dd></div>
        <div><dt>Cron</dt><dd className="is-multiline"><strong>{schedule.cron}</strong><span>{schedule.timezone} · current Schedule definition</span></dd></div>
        <div><dt>Next run</dt><dd className="is-multiline" title="Derived at query time from the cron expression and the current time; Feld stores no next-run value"><strong>{facts.nextRun}</strong><span>{schedule.enabled ? "Derived from the cron expression, its timezone, and now · not a stored fact" : "Paused · no tick fires until resumed"}</span></dd></div>
      </dl>
      {cardView?.error ? <FormError message={cardView.error} /> : null}
      {cardView?.result ? <p className="feld-agents__composer-result" role="status">{cardView.result}</p> : null}
      <div className="feld-agents__card-actions">
        <button className="feld-agents__button" type="button" data-action="edit-actor" disabled={deleted} onClick={edit}>Edit…</button>
        {schedule.enabled
          ? <button className="feld-agents__button" type="button" data-action="pause-job" data-agent-id={agent.id} disabled={deleted} onClick={() => setScheduleEnabled(client, model, agent.id, false)}>Pause schedule</button>
          : <button className="feld-agents__button" type="button" data-action="resume-job" data-agent-id={agent.id} disabled={deleted} onClick={() => setScheduleEnabled(client, model, agent.id, true)}>Resume schedule</button>}
      </div>
      <p className="feld-agents__form-note">Pausing stops the ticks: no tick fires, a running Session is not touched, and the band leaves Live mode once its last run leaves the range. Delete actor in the Actions menu retires the Job and keeps its history.</p>
    </>
  );
}

// Opens the card's edit form on the Actor as it is declared.
function useEditAction(agent: ActorRecord, schedule: ScheduleRecord | null) {
  const model = useActorsModel();
  const { focus } = usePageActions();
  return () => {
    useActorsStore.getState().editActor(agent.id, editDraftOf(agent, schedule, model.input.launchOptions));
    focus('[data-edit-form] [name="name"]:not(:disabled), [data-edit-form] [name="cron"], [data-edit-form] [name="workdir"]');
  };
}

// The Agent section of the Actor card: how the launch spec differs from what the latest
// Session was started with, and Edit. In edit, the form over the name and the launch spec
// takes the section's place.
function AgentSection({ agent }: { agent: ActorRecord }) {
  const model = useActorsModel();
  const cardView = useActorsStore((state) => (state.cardView?.agentId === agent.id ? state.cardView : null));
  const edit = useEditAction(agent, null);
  if (cardView?.editing && cardView.draft) return <ActorEditForm agent={agent} schedule={null} cardView={cardView} />;
  return (
    <>
      <h3 className="feld-agents__inspector-section" data-agent-section="">Agent · Control Plane fact</h3>
      <dl className="feld-agents__inspector-facts">
        <div><dt>Latest Session</dt><dd className="is-multiline" data-launch-drift=""><LaunchDrift agent={agent} /></dd></div>
      </dl>
      {cardView?.error ? <FormError message={cardView.error} /> : null}
      {cardView?.result ? <p className="feld-agents__composer-result" role="status">{cardView.result}</p> : null}
      <div className="feld-agents__card-actions">
        <button className="feld-agents__button" type="button" data-action="edit-actor" disabled={Boolean(agent.deletedAt)} onClick={edit}>Edit…</button>
      </div>
      <p className="feld-agents__form-note">A change to the launch spec takes effect at the next start; the current Session keeps what it was started with.</p>
    </>
  );
}

function launchSpecLineOf(agent: ActorRecord) {
  return `${agent.launchSpec?.runtime ? "runtime" : "command"}, workdir, Provider, and Node`;
}

// The latest Session against the launch spec, as far as the Session recorded what it ran.
function LaunchDrift({ agent }: { agent: ActorRecord }) {
  const model = useActorsModel();
  const drift = model.launchSpecDriftFor(agent);
  if (!drift) return <><strong>None</strong><span>{agent.launchSpec ? "No Session has been started from the launch spec yet" : "No launch spec to compare"}</span></>;
  if (!drift.comparable) return <><strong>Not comparable</strong><span>{drift.session.id} is attached: the person started it, not the launch spec</span></>;
  const unrecorded = drift.unrecorded.length ? ` · ${drift.unrecorded.join(", ")} not recorded with it` : "";
  const notRecorded = " · env, runtime settings, trust workdir, and Provider options are not recorded with a Session";
  return drift.changes.length
    ? <><strong>{drift.changes.map((change) => `${change.field} ${change.recorded} → ${change.declared}`).join(" · ")}</strong><span>{drift.session.id} ran with the left side; the next start uses the right{unrecorded}{notRecorded}</span></>
    : <><strong>{drift.unrecorded.length ? "No difference recorded" : "Same launch spec"}</strong><span>{drift.session.id} ran with the {launchSpecLineOf(agent)} declared now{unrecorded}{notRecorded}</span></>;
}

// The Actor card's edit form changes up to three records, one request each: a Job's
// Schedule (cron, timezone), the Actor's launchSpec (runtime and its settings or the
// command, working directory, env), and its name. Each `feld actor update` sends only the
// fields it was given; the rest keep their values.
function ActorEditForm({ agent, schedule, cardView }: { agent: ActorRecord; schedule: ScheduleRecord | null; cardView: CardView }) {
  const model = useActorsModel();
  const { client } = useActorsTools();
  const { focus } = usePageActions();
  const store = useActorsStore.getState;
  const launchSpec = agent.launchSpec;
  const draft = cardView.draft || editDraftOf(agent, schedule, model.input.launchOptions);
  const runtime = launchSpec ? Boolean(launchSpec.runtime) : agent.kind === "agent";
  // A choice the daemon no longer offers stays selectable while it is the declared one.
  const offered = (options: LaunchOptions["runtimes"], value: string) => options.some(([choice]) => choice === value) || !value ? options : [[value, value] as const, ...options];
  const set = (field: keyof EditDraft) => (value: string) => store().setEditDraftField(field, value);
  // The daemon refuses a rename while the current Session runs.
  const running = model.runningSessionOf(agent);

  // The options belong to the Provider: another Provider starts from none and fills its
  // own, and the declared one gets its options back.
  function changeProvider(provider: string) {
    set("provider")(provider);
    set("providerOptions")(provider === launchSpec?.provider ? settingsText(launchSpec.providerOptions) : "");
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    saveActorEdit(client, model).then(() => {
      if (!store().cardView?.editing) focus('[data-action="edit-actor"]');
    });
  }

  function cancel() {
    store().cancelActorEdit();
    focus('[data-action="edit-actor"]');
  }

  return (
    <>
      <h3 className="feld-agents__inspector-section" data-job-schedule={schedule ? "" : undefined} data-agent-section={schedule ? undefined : ""}>Edit {kindLabel(agent.kind)}</h3>
      <form className="feld-agents__form feld-agents__job-form" data-edit-form="" data-schedule-form={schedule ? "" : undefined} data-agent-id={agent.id} onSubmit={submit}>
        <fieldset className="feld-agents__form-group">
          <legend>Name · <code>feld actor update {agent.name} --name …</code></legend>
          <FormField label="Name" help={running ? `${running.id} is running; stop it before the rename.` : "Unique in the Workspace; lowercase letters, digits, and hyphens. The id, Sessions, and mailbox stay."}>
            <input type="text" name="name" value={draft.name} required pattern="[a-z0-9][a-z0-9\-]*" spellCheck="false" disabled={Boolean(running)} data-edit-field="name" onChange={(event) => set("name")(event.target.value)} />
          </FormField>
          {running ? <p className="feld-agents__form-note" data-rename-blocked="">{running.id} is running: the name changes once it has ended.</p> : null}
        </fieldset>
        {schedule ? (
          <fieldset className="feld-agents__form-group">
            <legend>Schedule · <code>feld actor update {agent.name} --cron … --timezone …</code></legend>
            <FormField label="Cron expression" help="Minute, hour, day of month, month, day of week.">
              <input type="text" name="cron" value={draft.cron} required spellCheck="false" placeholder="13,43 * * * *" data-schedule-field="cron" onChange={(event) => set("cron")(event.target.value)} />
            </FormField>
            <FormField label="Timezone">
              <TimezoneSelect selected={draft.timezone} onChange={set("timezone")} data-schedule-field="timezone" />
            </FormField>
          </fieldset>
        ) : null}
        <fieldset className="feld-agents__form-group">
          <legend>Launch spec · <code>feld actor update {agent.name} --workdir … --env … --provider … --node … {runtime ? "--runtime … --runtime-settings … --trust-workdir" : "-- …"}</code></legend>
          {launchSpec ? null : <p className="feld-agents__form-note" data-launch-spec-absent="">Created by attach, so there is no launch spec yet. Filling the working directory gives one, and Feld can then start a Session for it.</p>}
          {runtime ? (
            <FormField label="Runtime">
              <select name="runtime" data-schedule-field="runtime" value={draft.runtime} onChange={(event) => set("runtime")(event.target.value)}>
                {offered(model.input.launchOptions.runtimes, draft.runtime).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
              </select>
            </FormField>
          ) : (
            <FormField label="Command" help={COMMAND_HELP}>
              <input type="text" name="command" value={draft.command} required={Boolean(launchSpec)} spellCheck="false" data-schedule-field="command" onChange={(event) => set("command")(event.target.value)} />
            </FormField>
          )}
          <FormField label="Working directory" help="An absolute path on the Node that runs it." required={Boolean(launchSpec)}>
            <input type="text" name="workdir" value={draft.workdir} required={Boolean(launchSpec)} spellCheck="false" placeholder="Example: /absolute/path/to/project" data-schedule-field="workdir" onChange={(event) => set("workdir")(event.target.value)} />
          </FormField>
          <FormField label="Environment" help={`${ENV_HELP} A value shown as ${SECRET_MASK} keeps its declared value while the mask stays.`} required={false}>
            <textarea name="env" value={draft.env} rows={3} spellCheck="false" autoComplete="off" placeholder="GH_CONFIG_DIR=/home/me/.config/gh-agent" data-schedule-field="env" onChange={(event) => set("env")(event.target.value)}></textarea>
          </FormField>
          {runtime ? (
            <>
              <FormField label="Runtime settings (JSON)" help={`Replaced as a whole; empty removes them. A value shown as ${SECRET_MASK} keeps its declared value while the mask stays.`} required={false}>
                <textarea name="settings" value={draft.settings} rows={5} spellCheck="false" autoComplete="off" placeholder={'{"permissions":{"allow":["Bash(feld:*)"]}}'} data-schedule-field="settings" onChange={(event) => set("settings")(event.target.value)}></textarea>
              </FormField>
              <label className="feld-agents__form-check">
                <input type="checkbox" name="trustWorkdir" checked={Boolean(draft.trustWorkdir)} data-schedule-field="trustWorkdir" onChange={(event) => set("trustWorkdir")(event.target.checked ? "true" : "")} />
                <span>Trust workdir · let the Runtime read the settings and hooks the workdir carries without asking</span>
              </label>
            </>
          ) : null}
          <FormField label="Provider" required={false}>
            <select name="provider" data-schedule-field="provider" value={draft.provider} onChange={(event) => changeProvider(event.target.value)}>
              {offered(model.input.launchOptions.providers, draft.provider).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
          </FormField>
          <FormField label="Node" required={false}>
            <select name="node" data-schedule-field="node" value={draft.node} onChange={(event) => set("node")(event.target.value)}>
              {offered(model.input.launchOptions.nodes, draft.node).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
          </FormField>
          <FormField label="Provider options (JSON)" help={`The Provider's own choices; replaced as a whole, and the Provider fills what is left out. Choosing another Provider empties them. A value shown as ${SECRET_MASK} keeps its declared value while the mask stays.`} required={false}>
            <textarea name="providerOptions" value={draft.providerOptions} rows={3} spellCheck="false" autoComplete="off" placeholder={'{"socket":"feld"}'} data-schedule-field="providerOptions" onChange={(event) => set("providerOptions")(event.target.value)}></textarea>
          </FormField>
        </fieldset>
        {cardView.result ? <p className="feld-agents__composer-result" role="status">{cardView.result}</p> : null}
        {cardView.error ? <FormError message={cardView.error} /> : null}
        {cardView.busy ? <p className="feld-agents__form-note" role="status">Sending the update to the daemon…</p> : null}
        <div className="feld-agents__form-actions">
          <button className="feld-agents__button feld-agents__button--quiet" type="button" data-action="cancel-actor-edit" onClick={cancel}>Cancel</button>
          <button className="feld-agents__button feld-agents__button--primary" type="submit" disabled={cardView.busy} title={`feld actor update: only the changed groups are sent, the name last.${schedule ? " The Schedule keeps its enabled state and the next run is computed from the new expression and now;" : ""} The launchSpec takes effect at the next start${schedule ? " or tick" : ""}.`}>Save changes</button>
        </div>
      </form>
    </>
  );
}

// The Session table of the Actor card: the Actor's Sessions, newest first, with the Sub Agent
// Sessions the Runtime recorded inside them and, for a scheduled Job, the ticks its
// Schedule skipped. A Session row opens that Session's facts, and from there its Console; a
// skipped tick left no Session, and the one the card was opened at is highlighted.
function ActorSessions({ agent, schedule }: { agent: ActorRecord; schedule: ScheduleRecord | null }) {
  const model = useActorsModel();
  const { openActorCard } = usePageActions();
  const highlightSkipId = useActorsStore((state) => (state.cardView?.agentId === agent.id ? state.cardView.skipId : null));
  const sessions = model.allSessionsFor(agent.id);
  const skips: ScheduleRun[] = schedule ? model.scheduleRunsFor(schedule).filter((entry) => entry.kind === "skip") : [];
  const rows = [
    ...sessions.map((session) => ({ at: session.start, session, skip: null })),
    ...skips.map((skip) => ({ at: skip.at, session: null, skip })),
  ].sort((left, right) => right.at - left.at);
  if (!rows.length) return <p className="feld-agents__card-empty">No Session has ever been recorded for this Actor.</p>;
  return (
    <ul className="feld-agents__runs">
      {rows.map((row) => {
        if (row.skip) {
          const skipRow = model.scheduleRunRowFor(row.skip, highlightSkipId);
          if (skipRow.kind !== "skip") return null;
          return (
            <li key={`skip-${skipRow.id}`} className={`feld-agents__run is-skip${skipRow.highlighted ? " is-highlighted" : ""}`} data-skip-row={skipRow.id}>
              <span className="feld-agents__run-time">{skipRow.time}</span>
              <span className="feld-agents__run-id">—</span>
              <span className="feld-agents__run-outcome"><strong>Skipped</strong><small>{skipRow.detail}</small></span>
              <span className="feld-agents__run-arrow"></span>
            </li>
          );
        }
        const session = row.session!;
        const outcome = model.outcomeFor(session);
        const detail = session.end === null
          ? `running since ${model.timeOnly.format(session.start)}`
          : `${formatDuration(model.sessionEnd(session) - session.start)} · ended ${model.timeOnly.format(session.end)}`;
        return (
          <li key={session.id}>
            <button type="button" className="feld-agents__run feld-agents__session-row" data-action="open-actor-session" data-session-id={session.id} aria-label={`Session details of ${session.id}`} onClick={() => openActorCard(agent.id, session.id)}>
              <span className="feld-agents__run-time">{model.dateTime.format(session.start)}</span>
              <span className="feld-agents__run-id">{session.parentSession ? `↳ ${session.subAgent || session.id}` : session.id}</span>
              <span className={`feld-agents__run-outcome is-${outcome.key}`}><strong>{outcome.label}</strong><small>{detail}</small></span>
              <span className="feld-agents__run-arrow" aria-hidden="true">→</span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

// Session facts, shown in the Actor card once a Session of the Actor is selected.
function SessionFacts({ session }: { session: AgentSession }) {
  const model = useActorsModel();
  const { openConsole } = usePageActions();
  const workspace = useActorsStore((state) => state.workspace);
  const state = { workspace };
  const actor = model.actorFor(session.actor);
  const details = model.sessionDetailsFor(session);
  const outcome = details.outcome;
  const lifecycle = model.lifecycleEvidence(session);
  const observed = model.observationEvidence(session);
  const label = session.subAgent ? `${model.actorLabel(session.actor, state)} / ${session.subAgent}` : model.actorLabel(session.actor, state);

  return (
    <>
      <div className="feld-agents__inspector-title">
        <div>
          <h2 id="feld-agents-session-details-title">{session.id}</h2>
          <span className="feld-agents__inspector-agent">{label} · {kindLabel(actor.kind)}</span>
        </div>
        <span className={`feld-agents__session-state is-${outcome.key}`} title={`${outcome.label}: ${outcome.detail}`}>{outcome.label}</span>
      </div>
      <div className="feld-agents__card-actions">
        <button className="feld-agents__button feld-agents__button--primary" type="button" data-action="open-console" data-session-id={session.id} onClick={() => openConsole(session.id)}>Open Console</button>
      </div>
      <dl className="feld-agents__inspector-facts">
        <div><dt>Execution</dt><dd className="is-multiline"><strong>{session.execution.label}</strong><span>{executionDetailFor(session)}</span></dd></div>
        <div><dt>Model</dt><dd><strong>{modelLabel(session)}</strong><span>{runtimeLabel(session)}</span></dd></div>
        <div><dt>Tokens</dt><dd className="is-multiline" title={metricTitleFor(session.metrics.tokens, "Token usage")}><strong>{usageValueFor(session)}</strong><span>{usageDetailFor(session)}</span></dd></div>
        <div><dt>Context</dt><dd className="is-multiline" title={metricTitleFor(session.metrics.context, "Context window")}><strong>{contextValueFor(session)}</strong><span>{contextDetailFor(session)}</span></dd></div>
        <div><dt>Started</dt><dd><strong>{details.started}</strong><span>{model.ZONE}</span></dd></div>
        <div><dt>Ended</dt><dd><strong>{details.ended}</strong><span>{details.endedDetail}</span></dd></div>
        <div><dt>{session.parentSession ? "Parent Session" : "Start kind"}</dt><dd className={session.startKind === "schedule" ? "is-multiline" : undefined}><strong>{session.parentSession || session.startKind}</strong><span>{details.startKindDetail}</span></dd></div>
        {session.parentSession ? null : <div><dt>Ownership</dt><dd><strong>{ownershipOf(session)}</strong><span>{ownershipOf(session) === "attached" ? "Feld attached to a running execution; it neither started it nor can stop it" : "Feld started this execution and can stop or kill it"} · Control Plane fact</span></dd></div>}
        {session.end === null || session.parentSession ? null : (
          <div><dt>Outcome</dt><dd className="is-multiline" title={details.outcomeTitle}><strong>{outcome.label}</strong><span>{outcome.detail} · {details.basis.short}</span></dd></div>
        )}
      </dl>
      <NowSection session={session} />
      <RelatedActivity session={session} />
      <h3 className="feld-agents__inspector-section" title={BINDING_PROVENANCE}>Placement · Control Plane fact</h3>
      <Placement session={session} />
      <h3 className="feld-agents__inspector-section">Session evidence</h3>
      <dl className="feld-agents__inspector-facts">
        <div><dt>Lifecycle</dt><dd title={lifecycle.title}><strong>{lifecycle.value}</strong><span>Control Plane fact · {lifecycle.detail}</span></dd></div>
        <div><dt>Observed</dt><dd title={observed.title}><strong>{observed.value}</strong><span>Provider observation · {observed.detail}</span></dd></div>
      </dl>
    </>
  );
}

// The Now section of Session facts: facts from different sources about a current Session,
// one row each with what the row means. It orders nothing, applies no age threshold, and
// concludes nothing about progress.
function NowSection({ session }: { session: AgentSession }) {
  const model = useActorsModel();
  const { openConsole } = usePageActions();
  const rows = model.nowSectionFor(session);
  if (!rows) return null;
  // A Stop row is one per end request, so rows share a label.
  return (
    <>
      <h3 className="feld-agents__inspector-section">Now</h3>
      <dl className="feld-agents__inspector-facts feld-agents__now">
        {rows.map((row, index) => (
          <div key={`${row.label}-${index}`}><dt>{row.label}</dt><dd className="is-multiline"><strong>{row.value}</strong><span>{row.meaning}</span>{row.openRecordId ? (
            <button type="button" className="feld-agents__run-open feld-agents__now-open" data-action="open-console" data-session-id={session.id} data-record-id={row.openRecordId} onClick={() => openConsole(session.id, row.openRecordId)}>Open in Console</button>
          ) : null}</dd></div>
        ))}
      </dl>
    </>
  );
}

function RelatedActivity({ session }: { session: AgentSession }) {
  const model = useActorsModel();
  const { openJob, openActorCard, openRecordConsole } = usePageActions();
  const workspace = useActorsStore((state) => state.workspace);
  const rows = model.relatedActivityFor(session, { workspace });

  function open(kind: string, id: string) {
    if (kind === "schedule") openJob(id);
    else if (kind === "session") openActorCard(model.sessionById(id)?.actor, id);
    else openRecordConsole(kind === "action" ? "action" : "message", id);
  }

  return (
    <>
      <h3 className="feld-agents__inspector-section">Related activity</h3>
      {rows.length ? (
        <ul className="feld-agents__related" aria-label={`Activity related to ${session.id}`}>
          {rows.map((row) => (
            <li key={`${row.kind}-${row.id}`}><button type="button" className="feld-agents__related-item" data-action={`inspect-${row.kind}`} data-record-id={row.id} title={`Open ${row.id}`} onClick={() => open(row.kind, row.id)}>
              <span className="feld-agents__related-glyph" aria-hidden="true">{row.glyph}</span>
              <span><strong>{row.label}</strong><small>{row.detail}</small></span>
            </button></li>
          ))}
        </ul>
      ) : <p className="feld-agents__related-empty">No Spawn, Message, or Sub Agent recorded for this Session.</p>}
    </>
  );
}

function Placement({ session }: { session: AgentSession }) {
  if (session.parentSession) {
    return (
      <dl className="feld-agents__inspector-facts">
        <div><dt>Placement</dt><dd className="is-multiline" title="Feld records no Execution Binding for a Sub Agent: it runs inside the parent Session's Runtime process"><strong>Not tracked</strong><span>Inside the parent Session {session.parentSession} · transcript entry</span></dd></div>
      </dl>
    );
  }
  return (
    <dl className="feld-agents__inspector-facts">
      {placementRows(session).map((row) => (
        <div key={row.label}><dt>{row.label}</dt><dd className="is-multiline" title={row.title}><strong>{row.value}</strong><span>{row.detail}</span></dd></div>
      ))}
    </dl>
  );
}

// ---- Composer --------------------------------------------------------------------------------

// Addressed to the Actor, not to a Session: the Message waits in the durable inbox until a
// Session performs an authenticated mailbox read.
function Composer({ agentId }: { agentId: string }) {
  const model = useActorsModel();
  const { client, afterRender } = useActorsTools();
  const { closeCard, cancelComposer } = usePageActions();
  const compose = useActorsStore(useShallow((state) => ({ workspace: state.workspace, draft: state.composeDrafts[agentId]?.text ?? "", sending: Boolean(state.composeSending[agentId]), result: state.composeResult, error: state.composeError })));
  const store = useActorsStore.getState;
  const { session, current, waiting } = model.composerFor(agentId);

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    sendComposedMessage(client, model).then(() => {
      if (store().composeResult) afterRender((root) => root.querySelector<HTMLElement>("[data-composer] textarea")?.focus());
    });
  }

  return (
    <aside className="feld-agents__inspector" aria-label="Message composer">
      <div className="feld-agents__inspector-top">
        <div className="feld-agents__inspector-eyebrow">Message composer</div>
        <button type="button" data-action="close-inspector" aria-label="Close Message composer" title="Close composer" onClick={closeCard}>×</button>
      </div>
      <div className="feld-agents__inspector-title">
        <div><h2>{model.actorLabel(agentId, compose)}</h2><span className="feld-agents__inspector-agent">Durable Actor mailbox</span></div>
        <span className="feld-agents__session-state">{current ? "current session" : "no session"}</span>
      </div>
      <p className="feld-agents__composer-hint">Addressed to the Actor, not to a Session. Feld stores the Message in the inbox before acknowledging it; it waits there until {current ? `${session!.id} or a later Session` : "a future Session"} performs an authenticated mailbox read. Nothing is typed into a console.{waiting.length ? ` ${waiting.length} Message${waiting.length === 1 ? " is" : "s are"} already waiting ahead of it in FIFO order.` : ""}</p>
      <form className="feld-agents__composer" data-composer="" onSubmit={submit}>
        <label>Message
          <textarea name="body" rows={4} placeholder={`Message for ${model.actorLabel(agentId, compose)}`} aria-label={`Message for ${model.actorLabel(agentId, compose)}`} data-composer-body="" required value={compose.draft} onChange={(event) => store().setComposeDraft(event.target.value)}></textarea>
        </label>
        <div className="feld-agents__composer-actions">
          <button className="feld-agents__button" type="button" data-action="close-inspector" onClick={cancelComposer}>Cancel</button>
          <button className="feld-agents__button feld-agents__button--primary" type="submit" disabled={compose.sending}>{compose.sending ? "Sending…" : "Send"}</button>
        </div>
      </form>
      {compose.error ? <p className="feld-agents__form-error" role="alert">{compose.error}</p> : null}
      {compose.result ? <p className="feld-agents__composer-result" role="status">Accepted. {compose.result.id} now waits in {actorNameOf(agentId)}'s mailbox until a Session reads it: "{compose.result.body}"</p> : null}
    </aside>
  );
}
