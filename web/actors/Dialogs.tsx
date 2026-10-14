// The page-level dialogs: Timeline display, Metrics display, New actor, and New workspace,
// with the form pieces the Actor card's Job edit shares.

import { useEffect, useId, useRef, type FormEvent, type ReactNode } from "react";
import { KIND_GLYPHS } from "./charts/glyphs.tsx";
import { useActorsModel } from "./context.ts";
import {
  ACTOR_DETAIL_OPTIONS,
  SESSION_METRIC_OPTIONS,
  TIMELINE_ELEMENT_OPTIONS,
  launchChoice,
  metricTitle,
  seriesSourceLine,
} from "./model.ts";
import { submitNewActor, submitNewWorkspace } from "./mutations.ts";
import { useShallow } from "zustand/react/shallow";
import { useActorsStore } from "./store.ts";
import type { ActorDraft, ActorDraftField, AgentsState } from "./types.ts";
import { useActorsTools, usePageActions } from "./ui.tsx";

// The shell shared by every page-level dialog (Timeline display, Metrics display, Job, New
// actor): a scrim that closes it and a titled panel. The scrim is for the pointer only, so the
// Tab order stays in the panel, whose close button closes it from the keyboard.
function DisplayModal({ modifier = "", closeAction, closeLabel, onClose, titleId, title, subtitle, children }: { modifier?: string; closeAction: string; closeLabel: string; onClose: () => void; titleId: string; title: string; subtitle: ReactNode; children: ReactNode }) {
  const menu = useRef<HTMLDivElement>(null);
  // A form's sticky title and actions wrap with the dialog's width; their heights keep a
  // focused field scrolled clear of them (see agents.css). The body changes between
  // renders (a form, then its result), so each render observes the actions it has.
  useEffect(() => {
    const panel = menu.current;
    if (!panel || typeof ResizeObserver !== "function") return undefined;
    const title = panel.querySelector<HTMLElement>(":scope > .feld-agents__display-menu-title");
    const actions = panel.querySelector<HTMLElement>(".feld-agents__form-actions");
    const observer = new ResizeObserver(() => {
      panel.style.setProperty("--display-menu-title-height", `${title?.offsetHeight ?? 0}px`);
      panel.style.setProperty("--form-actions-height", `${actions?.offsetHeight ?? 0}px`);
    });
    for (const element of [title, actions]) if (element) observer.observe(element);
    return () => observer.disconnect();
  });
  return (
    <div className="feld-agents__display-modal">
      <button className="feld-agents__display-scrim" type="button" tabIndex={-1} aria-hidden="true" data-action={closeAction} onClick={onClose}></button>
      <div className={`feld-agents__display-menu${modifier}`} role="dialog" aria-modal="true" aria-labelledby={titleId} ref={menu}>
        <div className="feld-agents__display-menu-title">
          <div>
            <strong id={titleId}>{title}</strong>
            <span>{subtitle}</span>
          </div>
          <button className="feld-agents__display-menu-close" type="button" data-action={closeAction} aria-label={closeLabel} onClick={onClose}>×</button>
        </div>
        {children}
      </div>
    </div>
  );
}

// ---- Display menus ---------------------------------------------------------------------------

// What Timeline display reads from the page state.
type AgentDisplayView = Pick<AgentsState,
  | "agentDisplayMenuOpen" | "inspectedSessionId" | "visibleActorDetails" | "sessionMetric"
  | "showSpawnLinks" | "showMessageLinks" | "showSubAgents" | "showScheduleSkips" | "showAgentsOutsideRange" | "showDeletedActors">;

function selectAgentDisplayView(state: AgentsState): AgentDisplayView {
  return {
    agentDisplayMenuOpen: state.agentDisplayMenuOpen,
    inspectedSessionId: state.inspectedSessionId,
    visibleActorDetails: state.visibleActorDetails,
    sessionMetric: state.sessionMetric,
    showSpawnLinks: state.showSpawnLinks,
    showMessageLinks: state.showMessageLinks,
    showSubAgents: state.showSubAgents,
    showScheduleSkips: state.showScheduleSkips,
    showAgentsOutsideRange: state.showAgentsOutsideRange,
    showDeletedActors: state.showDeletedActors,
  };
}

export function AgentDisplayMenu() {
  const model = useActorsModel();
  const state = useActorsStore(useShallow(selectAgentDisplayView));
  const { focus } = usePageActions();
  const store = useActorsStore.getState;
  if (!state.agentDisplayMenuOpen) return null;
  const inspectedIsSubAgent = Boolean(model.sessionById(state.inspectedSessionId)?.parentSession);
  return (
    <DisplayModal
      closeAction="close-agent-display-menu"
      closeLabel="Close Timeline display"
      onClose={() => { store().closeAgentDisplayMenu(); focus('[data-action="toggle-agent-display-menu"]'); }}
      titleId="feld-agents-display-title"
      title="Timeline display"
      subtitle="Choose the evidence and detail shown in the Actors timeline."
    >
      <fieldset>
        <legend>Actor details</legend>
        {ACTOR_DETAIL_OPTIONS.map((option) => (
          <label key={option.id}>
            <input type="checkbox" data-actor-detail={option.id} checked={state.visibleActorDetails.has(option.id)} onChange={(event) => store().setActorDetail(option.id, event.target.checked)} />
            <span><strong>{option.label}</strong><small>{option.detail}</small></span>
          </label>
        ))}
      </fieldset>
      <fieldset>
        <legend>Session metric</legend>
        {SESSION_METRIC_OPTIONS.map(([value, label, detail]) => (
          <label key={value}>
            <input type="radio" name="session-metric" value={value} data-agent-display-metric="" checked={state.sessionMetric === value} onChange={() => store().setSessionMetric(value)} />
            <span><strong>{label}</strong><small>{detail}</small></span>
          </label>
        ))}
      </fieldset>
      <fieldset>
        <legend>Timeline elements</legend>
        {TIMELINE_ELEMENT_OPTIONS.map(([key, label, detail]) => (
          <label key={key}>
            <input type="checkbox" data-agent-visibility={key} checked={state[key]} onChange={(event) => store().setVisibility(key, event.target.checked, inspectedIsSubAgent)} />
            <span><strong>{label}</strong><small>{detail}</small></span>
          </label>
        ))}
      </fieldset>
      <fieldset>
        <legend>Actor bands</legend>
        <label>
          <input type="checkbox" data-agent-visibility="showAgentsOutsideRange" checked={state.showAgentsOutsideRange} onChange={(event) => store().setVisibility("showAgentsOutsideRange", event.target.checked)} />
          <span><strong>Actors without Sessions in range</strong><small>Keep bands whose Sessions fall outside the visible range</small></span>
        </label>
        <label>
          <input type="checkbox" data-agent-visibility="showDeletedActors" checked={state.showDeletedActors} onChange={(event) => store().setVisibility("showDeletedActors", event.target.checked)} />
          <span><strong>Deleted Actors</strong><small>Read the deleted Actors with their history; nothing starts or addresses them</small></span>
        </label>
      </fieldset>
    </DisplayModal>
  );
}

export function MetricsMenu() {
  const model = useActorsModel();
  const open = useActorsStore((state) => state.metricsMenuOpen);
  const visible = useActorsStore((state) => state.visibleMetricIds);
  const { focus } = usePageActions();
  const store = useActorsStore.getState;
  if (!open) return null;
  const metrics = model.input.metrics;
  return (
    <DisplayModal
      modifier=" is-metrics"
      closeAction="close-metrics-menu"
      closeLabel="Close Metrics display"
      onClose={() => { store().closeMetricsMenu(); focus('[data-action="toggle-metrics-menu"]'); }}
      titleId="feld-agents-metrics-display-title"
      title="Display metrics"
      subtitle="Choose the Runtime telemetry shown below the Actors timeline."
    >
      <fieldset>
        <legend>Visible metrics · {visible.size} of {metrics.length}</legend>
        {metrics.map((metric) => (
          <label key={metric.id} title={metricTitle(metric)}>
            <input type="checkbox" data-metric-toggle={metric.id} checked={visible.has(metric.id)} onChange={(event) => store().setMetricVisible(metric.id, event.target.checked)} />
            <span>
              <strong>{metric.name}</strong>
              <small>{metric.caption}</small>
              {[...new Set(metric.series.map(seriesSourceLine))].map((line) => <small key={line} className="feld-agents__metric-source">{line}</small>)}
            </span>
          </label>
        ))}
      </fieldset>
    </DisplayModal>
  );
}

// ---- Form fields -----------------------------------------------------------------------------

// A help line belongs only where the value has a rule the placeholder cannot show.
// A field's name, with its help behind an info mark beside the name: shown on hover and
// on keyboard focus, so the form stays a list of fields. The mark is a span, not a button:
// a button would be the label's control, hovered along with the whole field.
export function FormField({ label, help, required = true, children }: { label: string; help?: ReactNode; required?: boolean; children: ReactNode }) {
  return (
    <label className={`feld-agents__field${required ? " is-required" : ""}`}>
      <span><span className="feld-agents__field-name">{label}</span>{help ? <FieldInfo label={label} help={help} /> : null}</span>
      {children}
    </label>
  );
}

function FieldInfo({ label, help }: { label: string; help: ReactNode }) {
  const id = useId();
  return (
    <span className="feld-agents__field-info">
      <span className="feld-agents__info" role="button" tabIndex={0} aria-label={`About ${label}`} aria-describedby={id}>
        <svg viewBox="0 0 12 12" aria-hidden="true"><circle cx="6" cy="6" r="4.6"></circle><path d="M6 5.5v3M6 3.5v.3"></path></svg>
      </span>
      <span className="feld-agents__info-popover feld-agents__field-popover" role="tooltip" id={id}>{help}</span>
    </span>
  );
}

// The Kind, chosen between two framed options with the kind's glyph; the Job first, as
// the form's default.
const KIND_CHOICES: [kind: "job" | "agent", label: string, tagline: string][] = [
  ["job", "Job", "A command that runs and ends on its own"],
  ["agent", "Agent", "A Coding Agent loop that runs until stopped"],
];

function KindChoice({ kind, onChange }: { kind: string; onChange: (kind: "job" | "agent") => void }) {
  return (
    <div className="feld-agents__kind-choice" role="radiogroup" aria-label="Kind">
      {KIND_CHOICES.map(([value, label, tagline]) => (
        <button key={value} type="button" role="radio" aria-checked={kind === value} className={`feld-agents__kind-option is-${value}${kind === value ? " is-selected" : ""}`} data-new-field="kind" data-kind={value} onClick={() => onChange(value)}>
          <svg viewBox="0 0 12 12" aria-hidden="true">{KIND_GLYPHS[value]}</svg>
          <span><strong>{label}</strong><small>{tagline}</small></span>
        </button>
      ))}
    </div>
  );
}

export const ENV_HELP = "One NAME=VALUE per line, with \\n for a line break and \\\\ for a backslash, set beneath the variables Feld sets. Values can be secrets: the launch spec shows only the names.";
const ENV_PLACEHOLDER = "GH_CONFIG_DIR=/home/me/.config/gh-agent";
export const COMMAND_HELP = <>Started as argv with no shell in between; write <code>bash -c '…'</code> or point at a script when a shell is needed.</>;

// A refused request comes into view below the fields and takes the focus the disabled
// buttons dropped, so it is read and Escape still reaches the page.
export function FormError({ message }: { message: string }) {
  const error = useRef<HTMLParagraphElement>(null);
  useEffect(() => {
    error.current?.scrollIntoView({ block: "nearest" });
    error.current?.focus({ preventScroll: true });
  }, [message]);
  return <p className="feld-agents__form-error" role="alert" tabIndex={-1} ref={error}>{message}</p>;
}

export function TimezoneSelect({ selected, onChange, ...data }: { selected: string; onChange: (value: string) => void } & Record<`data-${string}`, string>) {
  const model = useActorsModel();
  return (
    <select name="timezone" value={selected} onChange={(event) => onChange(event.target.value)} {...data}>
      {model.TIMEZONE_OPTIONS.map((zone) => <option key={zone} value={zone}>{zone}</option>)}
    </select>
  );
}

// ---- New workspace ----------------------------------------------------------------------------

// `feld workspace create <key>` from the Workspace switcher.
export function NewWorkspace() {
  const model = useActorsModel();
  const draft = useActorsStore((state) => state.newWorkspace);
  const { client } = useActorsTools();
  const { closeNewWorkspace, focus } = usePageActions();
  const store = useActorsStore.getState;
  if (!draft) return null;

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    submitNewWorkspace(client, model).then(() => {
      if (!store().newWorkspace) focus('[data-action="toggle-workspace-menu"]', { preventScroll: true });
    });
  }

  return (
    <DisplayModal
      modifier=" is-form"
      closeAction="close-new-workspace"
      closeLabel="Close New workspace"
      onClose={closeNewWorkspace}
      titleId="feld-agents-new-workspace-title"
      title="New workspace"
      subtitle="Create a Workspace for Actors and their records. The page switches to it once created."
    >
      <form className="feld-agents__form" data-new-workspace="" onSubmit={submit}>
        <FormField label="Key" help="Unique across Workspaces; lowercase letters, digits, and hyphens.">
          <input type="text" name="key" value={draft.key} required pattern="[a-z0-9][a-z0-9\-]*" spellCheck="false" placeholder="research" data-new-workspace-field="key" disabled={draft.submitting} onChange={(event) => store().setNewWorkspaceKey(event.target.value)} />
        </FormField>
        {draft.error ? <FormError message={draft.error} /> : null}
        <div className="feld-agents__form-actions">
          <button className="feld-agents__button feld-agents__button--quiet" type="button" data-action="close-new-workspace" onClick={closeNewWorkspace}>Cancel</button>
          <button className="feld-agents__button feld-agents__button--primary" type="submit" title="feld workspace create <key>" disabled={draft.submitting}>Create</button>
        </div>
      </form>
    </DisplayModal>
  );
}

// ---- New actor --------------------------------------------------------------------------------

// The form declares an Actor the way `feld actor create --kind agent` / `--kind job` would:
// Kind, then the launchSpec every created Actor has (a Runtime for an Agent, a command for
// a Job), and for a Job an optional Schedule. A Session the person runs themselves is
// registered from inside it with `/feld attach`, so attach is not offered here. The form is
// create-only: an existing name is refused here although the CLI and API treat a repeated
// `create` as a replacement of the declaration (a UI policy).
export function NewActor() {
  const draft = useActorsStore((state) => state.newActor);
  const { closeNewActor } = usePageActions();
  if (!draft) return null;
  return (
    <DisplayModal
      modifier=" is-form"
      closeAction="close-new-actor"
      closeLabel="Close New actor"
      onClose={closeNewActor}
      titleId="feld-agents-new-actor-title"
      title="New actor"
      subtitle={draft.chooseWorkspace && !draft.workspace
        ? "Choose the Workspace to declare an Agent or a Job in. A deleted Actor keeps its Sessions and Messages, and Timeline display can show it."
        : draft.chooseWorkspace
          ? <><b className="feld-agents__form-workspace">Workspace: {draft.workspace}</b> Declare an Agent or a Job. A deleted Actor keeps its Sessions and Messages, and Timeline display can show it.</>
          : `Declare an Agent or a Job in the ${draft.workspace} Workspace. A deleted Actor keeps its Sessions and Messages, and Timeline display can show it.`}
    >
      {draft.created ? <NewActorResult draft={draft} /> : <NewActorForm draft={draft} />}
    </DisplayModal>
  );
}

// What was declared, read back from the snapshot the daemon answered with.
function NewActorResult({ draft }: { draft: ActorDraft }) {
  const model = useActorsModel();
  const mode = useActorsStore((state) => state.mode);
  const { openConsole, openJob, closeNewActor } = usePageActions();
  const result = model.newActorResult(draft, draft.created!, { mode });
  const outcome = result.outcome;
  return (
    <>
      <p className={`feld-agents__composer-result${outcome ? ` is-${outcome.key}` : ""}`} role={outcome?.key === "failed" ? "alert" : "status"}>{result.text}{outcome ? <> <strong>{outcome.text}.</strong></> : null} {result.note}</p>
      <div className="feld-agents__form-actions">
        {result.sessionId ? <button className="feld-agents__button" type="button" data-action="open-console" data-session-id={result.sessionId} onClick={() => openConsole(result.sessionId)}>Open Console</button> : null}
        {result.job ? <button className="feld-agents__button" type="button" data-action="open-job" data-agent-id={result.actorKey} onClick={() => openJob(result.actorKey)}>Open job</button> : null}
        <button className="feld-agents__button feld-agents__button--primary" type="button" data-action="close-new-actor" onClick={closeNewActor}>Close</button>
      </div>
    </>
  );
}

function NewActorForm({ draft }: { draft: ActorDraft }) {
  const model = useActorsModel();
  const { client } = useActorsTools();
  const { closeNewActor, focus } = usePageActions();
  const store = useActorsStore.getState;
  const isAgent = draft.kind === "agent";
  // The daemon's choices can answer after the form opened: a draft value it does not
  // offer moves to the first one it does, as the select shows.
  const options = model.input.launchOptions;
  useEffect(() => {
    for (const [name, offered] of [["runtime", options.runtimes], ["node", options.nodes], ["provider", options.providers]] as const) {
      const chosen = launchChoice(draft[name], offered);
      if (chosen !== draft[name]) store().setNewActorField(name, chosen);
    }
  }, [options, draft.runtime, draft.node, draft.provider, store]);
  const kindHelp = isAgent
    ? "An Agent Loop that runs until stopped and takes its task from the mailbox. Started at creation by default."
    : "A bounded program that ends on its own; its exit code is the outcome. A cron expression gives it a Schedule; without one, it runs only when spawned or started. Created without a Session by default.";
  // Every field writes through to the draft, so a Kind change reshapes the form without
  // losing what was typed.
  const field = (name: ActorDraftField) => ({
    name,
    "data-new-field": name,
    value: draft[name],
    onChange: (event: { target: { value: string } }) => store().setNewActorField(name, event.target.value),
  });

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const submitter = (event.nativeEvent as SubmitEvent).submitter as HTMLButtonElement | null;
    submitNewActor(client, model, submitter?.value === "start" ? "start" : "create").then(() => {
      if (store().newActor?.created) focus('[data-action="close-new-actor"].feld-agents__button--primary');
    });
  }

  return (
    <form className="feld-agents__form" data-new-actor="" onSubmit={submit}>
      {draft.chooseWorkspace ? (
        <FormField label="Workspace" help="The page shows all Workspaces; the Actor is declared in the one chosen here.">
          <select {...field("workspace")} required data-new-rerender=""><option value="" disabled>Choose a Workspace</option>{model.input.workspaces.map((workspace) => <option key={workspace} value={workspace}>{workspace}</option>)}</select>
        </FormField>
      ) : null}
      <FormField label="Kind" help={`${kindHelp} Declared by you and never inferred or changed by Feld. A Session you run yourself is registered from inside it with /feld attach, so attach is not offered here.`}>
        <KindChoice kind={draft.kind} onChange={(kind) => store().setNewActorField("kind", kind)} />
      </FormField>
      <FormField label="Name" help="Unique in the Workspace; lowercase letters, digits, and hyphens.">
        <input type="text" {...field("name")} required pattern="[a-z0-9][a-z0-9\-]*" spellCheck="false" placeholder={isAgent ? "developer-02" : "issue-triage"} />
      </FormField>
      {isAgent ? (
        <FormField label="Runtime">
          <select {...field("runtime")}>{model.input.launchOptions.runtimes.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
        </FormField>
      ) : (
        <FormField label="Command" help={COMMAND_HELP}>
          <input type="text" {...field("command")} required spellCheck="false" placeholder="python3 triage.py --project 10" />
        </FormField>
      )}
      <FormField label="Working directory" help="An absolute path on the Node that runs it; each Node has its own filesystem.">
        <input type="text" {...field("workdir")} required spellCheck="false" placeholder="Example: /absolute/path/to/project" />
      </FormField>
      {isAgent ? null : (
        <>
          <FormField label="Cron expression" help="Optional. Minute, hour, day of month, month, day of week; a tick is skipped while the previous run is still current. Leave it empty for a Job without a Schedule." required={false}>
            <input type="text" {...field("cron")} spellCheck="false" placeholder="13,43 * * * *" />
          </FormField>
          <FormField label="Timezone" help="Stored with the Schedule; the daemon's local timezone when omitted from the CLI.">
            <TimezoneSelect selected={draft.timezone} onChange={(value) => store().setNewActorField("timezone", value)} data-new-field="timezone" />
          </FormField>
        </>
      )}
      <details className="feld-agents__advanced" data-new-advanced="" open={draft.advancedOpen} onToggle={(event) => store().setNewActorAdvancedOpen(event.currentTarget.open)}>
        <summary>Advanced</summary>
        <div className="feld-agents__advanced-fields">
        <FormField label="Provider" required={false}>
          <select {...field("provider")}>{model.input.launchOptions.providers.map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
        </FormField>
        <FormField label="Environment" help={ENV_HELP} required={false}>
          <textarea {...field("env")} rows={3} spellCheck="false" autoComplete="off" placeholder={ENV_PLACEHOLDER}></textarea>
        </FormField>
        {isAgent ? (
          <FormField label="Runtime settings (JSON)" help="Carried into the per-Session settings file as declared; Feld has no default and no opinion about it." required={false}>
            <textarea {...field("settings")} rows={3} spellCheck="false" placeholder={'{"permissions":{"allow":["Bash(feld:*)"]}}'}></textarea>
          </FormField>
        ) : null}
        </div>
      </details>
      {draft.error ? <FormError message={draft.error} /> : null}
      {draft.submitting ? <p className="feld-agents__form-note" role="status">{draft.settling ? "The start was accepted; waiting for the daemon to settle it…" : "Sending the declaration to the daemon…"}</p> : null}
      <div className="feld-agents__form-actions">
        <button className="feld-agents__button feld-agents__button--quiet" type="button" data-action="close-new-actor" onClick={closeNewActor}>Cancel</button>
        <ActorSubmitButtons draft={draft} />
      </div>
    </form>
  );
}

// The buttons state what submitting does, because the default depends on Kind: an Agent is
// started unless `--no-start`; a Job is created without a first Session unless `--start`.
// With a cron expression the Job gets a Schedule and its first run is the next tick;
// without one, Feld decides no start time and a Session starts only when spawned or
// started.
function ActorSubmitButtons({ draft }: { draft: ActorDraft }) {
  const busy = draft.submitting;
  if (draft.kind === "agent") {
    return (
      <>
        <button className="feld-agents__button" type="submit" name="intent" value="create" disabled={busy} title="feld actor create --kind agent … --no-start: the Actor is declared and no Session starts.">Create only</button>
        <button className="feld-agents__button feld-agents__button--primary" type="submit" name="intent" value="start" disabled={busy} title="feld actor create --kind agent …: the Actor and its first Session are created together (the default for an Agent).">Create and start</button>
      </>
    );
  }
  return (
    <>
      <button className="feld-agents__button" type="submit" name="intent" value="start" disabled={busy} title="feld actor create --kind job … --start: a Session starts now; with a cron expression the Schedule also starts later ticks.">Create and run now</button>
      <button className="feld-agents__button feld-agents__button--primary" type="submit" name="intent" value="create" disabled={busy} title="feld actor create --kind job …: no Session starts (the default for a Job). With a cron expression the first run is the next tick; without one, a Session starts on feld actor start.">Create</button>
    </>
  );
}
