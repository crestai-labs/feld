// The Actors page's writes. Each sends one or more requests through api/client.ts, keeps
// the store's account of the request (in flight, accepted, refused), and settles once the
// daemon accepted it, invalidating the queries that read what it changed (see
// invalidateWritten). A write that lands after its
// dialog was closed or replaced changes nothing on the page. With no QueryClient (fixture
// mode) there is no daemon to write to: a form still reports what it refuses, a menu
// closes, and nothing is written.

import type { QueryClient, QueryKey } from "@tanstack/react-query";
import { invalidateWritten } from "../api/events.ts";
import { commandLine } from "../lib/command-line.ts";
import { keys } from "../api/queries.ts";
import { createActor, createWorkspace, deleteActor, killSession, listActors, readSession, sendMessage, startActor, stopSession, updateActor } from "../api/client.ts";
import {
  MENU_ACTION_DONE,
  MENU_ACTION_LABELS,
  MENU_ACTION_NOTES,
  MENU_ACTION_REFUSED,
  START_SETTLE_BUDGET,
  START_SETTLE_POLL,
  actorKeyOf,
  draftLaunchSpec,
  instantOf,
  launchSpecPatchOf,
  startSettled,
  submittedEditDraft,
  validateActorDraft,
  validateEditDraft,
  type ActorsModel,
} from "./model.ts";
import { draftsWith, useActorsStore, without, type ActorsStore } from "./store.ts";
import { useToasts, type ToastKind } from "./toasts.ts";
import type { ActorDraft, ActorRecord, CreatedActor, EditDraft, ScheduleRecord } from "./types.ts";

function messageOf(failure: unknown) {
  return failure instanceof Error ? failure.message : String(failure);
}

// An operation started from an Actor's Actions menu. The menu stays open with its items
// disabled while the daemon answers and closes once it has; a toast at the page's corner
// says the request is in flight, then that it was accepted or refused. `written` names the
// queries it changes.
let menuRequests = 0;

export function runMenuAction(client: QueryClient | null, agentId: string, action: string, send: () => Promise<unknown>, written: QueryKey[], fixture = false) {
  const store = useActorsStore;
  if (store.getState().actionRequest?.pending) return;
  const name = agentId.slice(agentId.indexOf("/") + 1);
  const id = `menu-${menuRequests += 1}`;
  const toast = (kind: ToastKind, text: string, detail: string | null) => useToasts.getState().show({ id, kind, text: `${text} · ${name}`, detail });
  if (!client && !fixture) {
    store.setState({ actionMenuAgent: null });
    toast("done", MENU_ACTION_DONE[action], "Fixture mode: nothing is written.");
    return;
  }
  store.setState({ actionRequest: { agentId, action, pending: true, error: null } });
  toast("pending", MENU_ACTION_LABELS[action], "Waiting for the daemon to accept the request.");
  send()
    .then(() => {
      if (client) invalidateWritten(client, written);
      store.setState({ actionRequest: null, actionMenuAgent: null });
      toast("done", MENU_ACTION_DONE[action], MENU_ACTION_NOTES[action] ?? null);
    })
    .catch((failure) => {
      store.setState({ actionRequest: null, actionMenuAgent: null });
      toast("error", MENU_ACTION_REFUSED[action], messageOf(failure));
    });
}

// Fixture mode's refusing daemon: the request is in flight a moment, then refused with the
// fixture's message.
const FIXTURE_ANSWER_DELAY = 800;
export function refuseMenuAction(agentId: string, action: string, message: string) {
  runMenuAction(null, agentId, action, () => new Promise((_, reject) => setTimeout(() => reject(new Error(message)), FIXTURE_ANSWER_DELAY)), [], true);
}

// The three lifecycle primitives. Start runs the Actor's own launchSpec, so it needs no
// body; Stop and Kill name the Session the menu was drawn for. Each changes the Actors'
// state and the range's Sessions.
const LIFECYCLE_WRITTEN = [keys.actorsAll(), keys.listingAll()];

export function startActorSession(client: QueryClient | null, actor: ActorRecord) {
  runMenuAction(client, actor.id, "start", () => startActor(actor.workspace, actor.name), [...LIFECYCLE_WRITTEN, keys.actorListingsOf(actor.name)]);
}

export function stopActorSession(client: QueryClient | null, agentId: string, sessionId: string) {
  runMenuAction(client, agentId, "stop", () => stopSession(sessionId), [...LIFECYCLE_WRITTEN, keys.session(sessionId)]);
}

export function killActorSession(client: QueryClient | null, agentId: string, sessionId: string) {
  runMenuAction(client, agentId, "kill", () => killSession(sessionId), [...LIFECYCLE_WRITTEN, keys.session(sessionId)]);
}

// `feld actor delete`: the Actor leaves the live listing, so its band goes once the Actors
// are read again; the daemon refuses while its current Session is running.
export function deleteActorFromMenu(client: QueryClient | null, actor: ActorRecord) {
  runMenuAction(client, actor.id, "delete", () => deleteActor(actor.workspace, actor.name), [keys.actorsAll()]);
}

// Pause and Resume send the Schedule's `enabled`; `pausedAt`, `enabledAt`, and the next
// run come back from the daemon, which is also where the band reads them once the Actors
// are read again. The toast reports the request, from the Actions menu and from the
// Actor card alike.
export function setScheduleEnabled(client: QueryClient | null, model: ActorsModel, agentId: string, enabled: boolean) {
  const schedule = model.scheduleFor(agentId);
  const actor = model.actorFor(agentId);
  if (!schedule || schedule.enabled === enabled) return;
  runMenuAction(client, agentId, enabled ? "resume" : "pause", () => updateActor(actor.workspace, actor.name, { schedule: { enabled } }), [keys.actorsAll()]);
}

// The `feld actor update` options of a launchSpec patch. Values can be secrets, so env,
// runtimeSettings, and the Provider's options are named without them.
function launchTermsOf(patch: FeldApi.LaunchSpecPatch) {
  const terms: string[] = [];
  if (patch.runtime) terms.push(`--runtime ${patch.runtime}`);
  if (patch.command) terms.push(`-- ${commandLine(patch.command)}`);
  if (patch.workdir) terms.push(`--workdir ${patch.workdir}`);
  if (patch.env !== undefined) terms.push(patch.env ? Object.keys(patch.env).map((name) => `--env ${name}=…`).join(" ") : "(env removed)");
  if (patch.runtimeSettings !== undefined) terms.push(patch.runtimeSettings ? "--runtime-settings …" : "(runtime settings removed)");
  if (patch.trustWorkdir !== undefined && patch.trustWorkdir !== null) terms.push(patch.trustWorkdir ? "--trust-workdir" : "--no-trust-workdir");
  if (patch.provider) terms.push(`--provider ${patch.provider}`);
  if (patch.node) terms.push(`--node ${patch.node}`);
  if (patch.providerOptions !== undefined) terms.push(patch.providerOptions ? "(provider options …)" : patch.provider ? `(${patch.provider} fills its own options)` : "(provider options removed)");
  return terms.map((term) => ` ${term}`).join("");
}

// Each changed group of the Actor card's edit form is one `feld actor update` sending only
// its own fields: the Schedule keeps its enabled state and the daemon derives the next
// run from the new expression, and the launchSpec takes effect at the next start (or
// tick). The requests are sent in that order, the rename last so the others still name
// the Actor as it was, and a refusal names the group it belongs to without losing the
// change of a group sent before it.
async function sendActorEdit(schedule: ScheduleRecord | null, actor: ActorRecord, draft: EditDraft, notes: string[], model: ActorsModel) {
  if (schedule && (draft.cron !== schedule.cron || draft.timezone !== schedule.timezone)) {
    const response = await updateActor(actor.workspace, actor.name, { schedule: { cron: draft.cron, timezone: draft.timezone } });
    const next = instantOf(response?.actor?.schedule?.nextRun);
    notes.push(`feld actor update ${actor.name} --cron "${draft.cron}" --timezone ${draft.timezone}: enabled state kept${response?.actor?.schedule?.enabled === false ? "; still paused" : `; next run ${next === null ? "never" : `${model.dateTime.format(next)} ${model.ZONE}`}`}`);
  }
  const patch = launchSpecPatchOf(draft, actor);
  if (patch) {
    await updateActor(actor.workspace, actor.name, { launchSpec: patch });
    notes.push(`feld actor update ${actor.name}${launchTermsOf(patch)}: takes effect at the next ${schedule ? "start or tick" : "start"}`);
  }
  if (draft.name !== actor.name) {
    await updateActor(actor.workspace, actor.name, { name: draft.name });
    notes.push(`feld actor update ${actor.name} --name ${draft.name}: the id, Sessions, and mailbox stay`);
    return actorKeyOf(actor.workspace, draft.name);
  }
  return null;
}

// The Actor card's edit form submitted. DOM once it lands: focus `[data-action="edit-actor"]`
// (the returned promise resolves then).
export async function saveActorEdit(client: QueryClient | null, model: ActorsModel) {
  const store = useActorsStore.getState();
  const modal = store.cardView;
  if (!modal || !modal.draft || modal.busy) return;
  const schedule = model.scheduleFor(modal.agentId);
  const actor = model.actorFor(modal.agentId);
  const serial = modal.serial;
  const draft = submittedEditDraft(modal.draft, actor, schedule);
  const error = validateEditDraft(draft, actor, schedule);
  store.patchCardView(serial, { draft, error });
  if (error || !client) return;
  store.patchCardView(serial, { busy: true });
  // A group committed before the refusal keeps its line, so the reader sees what
  // the daemon did take alongside what it refused.
  const notes: string[] = [];
  let renamed: string | null = null;
  try {
    renamed = await sendActorEdit(schedule, actor, draft, notes, model);
    // A renamed Actor is keyed by its new name, which the card follows.
    if (renamed) useActorsStore.getState().rekeyActorCard(serial, renamed);
    useActorsStore.getState().patchCardView(serial, {
      busy: false,
      editing: false,
      draft: null,
      result: notes.length ? `${notes.join(". ")}.` : "Nothing changed; no request was sent.",
    });
  } catch (failure) {
    useActorsStore.getState().patchCardView(serial, {
      busy: false,
      result: notes.length ? `${notes.join(". ")}.` : null,
      error: messageOf(failure),
    });
  }
  // Each note is a request the daemon accepted, whatever the next one met.
  // The range's Sessions name their Actor, so a rename reads them again as well.
  if (notes.length) invalidateWritten(client, renamed ? [keys.actorsAll(), keys.listingAll()] : [keys.actorsAll()]);
}

// `feld workspace create <key>` from the Workspace switcher. Closing shows the Workspace,
// and the switcher lists it once its list is read again.
// DOM once it lands: focus `[data-action="toggle-workspace-menu"]`.
export async function submitNewWorkspace(client: QueryClient | null, model: ActorsModel) {
  const store = useActorsStore.getState();
  const draft = store.newWorkspace;
  if (!draft || draft.submitting) return;
  const serial = draft.serial;
  const key = draft.key.trim();
  const error = model.validateWorkspaceKey(key);
  store.patchNewWorkspace(serial, { error });
  if (error || !client) return;
  store.patchNewWorkspace(serial, { submitting: true });
  try {
    await createWorkspace(key);
    // The event stream is scoped to the Workspace shown and does not name the new one.
    invalidateWritten(client, [keys.workspaces()]);
    if (useActorsStore.getState().newWorkspace?.serial === serial) useActorsStore.getState().closeNewWorkspace(key);
  } catch (failure) {
    useActorsStore.getState().patchNewWorkspace(serial, { submitting: false, error: messageOf(failure) });
  }
}

// The Session read back until the daemon settled its start. A failed read ends the wait
// with what is already held, so the screen reports pending instead of a refusal: the
// start was accepted whatever the read did.
async function settleStart(session: FeldApi.Session) {
  const deadline = Date.now() + START_SETTLE_BUDGET;
  let latest = session;
  while (!startSettled(latest) && Date.now() < deadline) {
    await new Promise((resolve) => window.setTimeout(resolve, START_SETTLE_POLL));
    const read = await readSession(latest.id).catch(() => null);
    if (!read?.session) break;
    latest = read.session;
  }
  return latest;
}

// `feld actor create` creates first and starts afterwards, because only `POST …/actors`
// carries the Kind and the Schedule. `declared` is called once the declaration is committed.
async function createActorFromDraft(draft: ActorDraft, intent: "create" | "start", declared: () => void): Promise<CreatedActor> {
  const workspace = draft.workspace;
  const id = draft.name.trim();
  const listing = await listActors(workspace);
  if ((listing?.actors || []).some((actor) => actor.name === id)) {
    throw new Error(`An Actor named ${id} already exists in this Workspace; names are unique.`);
  }
  const launchSpec = draftLaunchSpec(draft);
  const cron = draft.kind === "job" ? draft.cron.trim() : "";
  await createActor(workspace, {
    name: id,
    kind: draft.kind,
    launchSpec,
    ...(cron ? { schedule: { cron, timezone: draft.timezone } } : {}),
  });
  declared();
  if (intent !== "start") return { id, workspace, cron, session: null };
  // The declaration is committed, so a refused start leaves the Actor behind.
  const started = await startActor(workspace, id).catch((failure) => {
    throw new Error(`${id} was declared, but no Session started: ${messageOf(failure)}`);
  });
  return { id, workspace, cron, session: started.session };
}

// The New actor form submitted with the button the reader pressed (`intent`). Once it
// lands the draft holds what was created, and the form shows the model's newActorResult.
// DOM once it lands: focus `[data-action="close-new-actor"].feld-agents__button--primary`.
export async function submitNewActor(client: QueryClient | null, model: ActorsModel, intent: "create" | "start") {
  const store = useActorsStore.getState();
  const draft = store.newActor;
  if (!draft || draft.submitting) return;
  const serial = draft.serial;
  const error = validateActorDraft(draft, model.workspaceAgents({ workspace: draft.workspace }).map((actor) => actor.name));
  store.patchNewActor(serial, { error });
  if (error || !client) return;
  store.patchNewActor(serial, { submitting: true });
  try {
    // A refused start leaves the declaration committed, which the page reads all the same.
    const created = await createActorFromDraft(draft, intent, () => invalidateWritten(client, [keys.actorsAll()]));
    // A start the daemon accepted is not yet a start that ran, so the Session is
    // read back until the daemon settled it and the result reports the outcome.
    if (created.session) {
      useActorsStore.getState().patchNewActor(serial, { settling: true });
      created.session = await settleStart(created.session);
      useActorsStore.getState().patchNewActor(serial, { settling: false });
    }
    // The page shows the started Session by reading the range again rather than by adding a
    // record of its own.
    if (created.session) invalidateWritten(client, [...LIFECYCLE_WRITTEN, keys.actorListingsOf(created.id)]);
    useActorsStore.getState().patchNewActor(serial, { submitting: false, created });
  } catch (failure) {
    useActorsStore.getState().patchNewActor(serial, { submitting: false, settling: false, error: messageOf(failure) });
  }
}

// The composer's Message, addressed to the Actor's mailbox.
// DOM once it lands: focus `[data-composer] textarea`.
export async function sendComposedMessage(client: QueryClient | null, model: ActorsModel) {
  const state = useActorsStore.getState();
  const agentId = state.composeAgentId;
  const serial = state.composeSerial;
  const body = agentId ? (state.composeDrafts[agentId]?.text ?? "").trim() : "";
  if (!body || !agentId || state.composeSending[agentId] || !client) return;
  const actor = model.actorFor(agentId);
  const composeDrafts = draftsWith(state.composeDrafts, agentId, body);
  const sent = composeDrafts[agentId].serial;
  useActorsStore.setState({ composeDrafts, composeSending: { ...state.composeSending, [agentId]: true }, composeError: null, composeResult: null });
  // The Actor's send ends whichever composer is open; only the opening it started from
  // reports it.
  const settle = (report: Partial<ActorsStore>) => {
    const now = useActorsStore.getState();
    const current = now.composeSerial === serial && now.composeAgentId === agentId;
    useActorsStore.setState({ composeSending: without(now.composeSending, agentId), ...(current ? report : {}) });
  };
  try {
    const response = await sendMessage(actor.workspace, actor.name, body);
    // A Message changes its addressee's waiting count as well.
    invalidateWritten(client, [keys.messagesAll(), keys.actorsAll()]);
    // The sent draft leaves; a draft written since stays.
    const drafts = useActorsStore.getState().composeDrafts;
    if (drafts[agentId]?.serial === sent) useActorsStore.setState({ composeDrafts: without(drafts, agentId) });
    settle({ composeResult: { id: response.message.id, body } });
  } catch (failure) {
    settle({ composeError: messageOf(failure) });
  }
}
