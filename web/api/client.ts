// The daemon API: one request, and the writes the pages offer. Reads go through TanStack
// Query (see queries.ts); everything here is a single request. The browser token rides in
// its cookie and key, and a 401 gives the page over to the pairing screen (see auth.ts).

import { keyHeaders, noteAnswered, noteUnauthorized } from "./auth.ts";

// A failed daemon request; `status` tells a rejected token from an absent daemon.
export class ApiError extends Error implements FeldApiError {
  status?: number;

  constructor(message: string, status?: number) {
    super(message);
    this.status = status;
  }
}

export function isUnauthorized(error: unknown) {
  return (error as FeldApiError | null)?.status === 401;
}

export async function api<T = any>(path: string, options?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...options,
    cache: "no-store",
    headers: { "Content-Type": "application/json", ...keyHeaders(), ...options?.headers },
  });
  if (response.status === 401) noteUnauthorized();
  else if (response.ok) noteAnswered();
  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    throw new ApiError(body.message || `${response.status} ${response.statusText}`, response.status);
  }
  return (response.status === 204 ? null : response.json()) as Promise<T>;
}

function workspacePath(workspace: string) {
  return `/v1/workspaces/${encodeURIComponent(workspace)}`;
}

function actorPath(workspace: string, actor: string) {
  return `${workspacePath(workspace)}/actors/${encodeURIComponent(actor)}`;
}

function sessionPath(sessionId: string, leaf = "") {
  return `/v1/sessions/${encodeURIComponent(sessionId)}${leaf ? `/${leaf}` : ""}`;
}

function write<T>(path: string, method: string, body?: unknown) {
  return api<T>(path, { method, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
}

// ---- Reads that stand outside the range --------------------------------------------------

export function readSession(sessionId: string, signal?: AbortSignal) {
  return api<FeldApi.SessionResponse>(sessionPath(sessionId), { signal });
}

// One Message with its whole body; the list carries a preview alone.
export function readMessage(messageId: string, signal?: AbortSignal) {
  return api<FeldApi.MessageResponse>(`/v1/messages/${encodeURIComponent(messageId)}`, { signal });
}

// `includeDeleted` lists the deleted Actors as well.
export function listActors(workspace: string, includeDeleted = false, signal?: AbortSignal) {
  return api<{ workspace: FeldApi.Workspace; actors: FeldApi.Actor[] }>(`${workspacePath(workspace)}/actors${includeDeleted ? "?includeDeleted" : ""}`, { signal });
}

// `cursor` is one of the three the daemon accepts: none for the newest page, `before` for
// the page before a sequence already held, and `after` for whatever was appended since.
function consoleQuery(cursor?: FeldConsoleCursor) {
  const parameters = new URLSearchParams();
  if (cursor?.before !== undefined) parameters.set("before", String(cursor.before));
  else if (cursor?.after !== undefined) parameters.set("after", String(cursor.after));
  else parameters.set("tail", "true");
  if (cursor?.limit) parameters.set("limit", String(cursor.limit));
  return parameters.toString();
}

export function readTranscript(sessionId: string, cursor?: FeldConsoleCursor, signal?: AbortSignal) {
  return api<FeldApi.TranscriptPage>(`${sessionPath(sessionId, "transcript")}?${consoleQuery(cursor)}`, { signal });
}

export function readOutput(sessionId: string, cursor?: FeldConsoleCursor, signal?: AbortSignal) {
  return api<FeldApi.OutputPage>(`${sessionPath(sessionId, "output")}?${consoleQuery(cursor)}`, { signal });
}

// Feld's own records of the Session, which the Console places among the Runtime's.
export function readSessionRecords(sessionId: string, signal?: AbortSignal) {
  return api<{ records: FeldApi.FeldRecord[]; nextAfter: number | null }>(`/v1/records?${new URLSearchParams({ session: sessionId })}`, { signal });
}

export function readDaemonStatus(signal?: AbortSignal) {
  return api<FeldApi.DaemonStatus>("/v1/daemon/status", { signal });
}

// ---- Writes ------------------------------------------------------------------------------
// Each returns what the daemon answered; the pages read again through the query cache.

export function createWorkspace(key: string) {
  return write<{ workspace: FeldApi.Workspace }>("/v1/workspaces", "POST", { key });
}

export function createActor(workspace: string, request: object) {
  return write<{ actor: FeldApi.Actor }>(`${workspacePath(workspace)}/actors`, "POST", request);
}

// A partial update: `launchSpec` and `schedule` each carry only the fields that change.
export function updateActor(workspace: string, actor: string, patch: object) {
  return write<{ actor: FeldApi.Actor }>(actorPath(workspace, actor), "PATCH", patch);
}

// Records the Actor's `deletedAt`; the daemon refuses while its current Session is running.
export function deleteActor(workspace: string, actor: string) {
  return write<{ actor: FeldApi.Actor }>(actorPath(workspace, actor), "DELETE");
}

// Starts a Session from the Actor's own launchSpec.
export function startActor(workspace: string, actor: string) {
  return write<FeldApi.StartResponse>(`${actorPath(workspace, actor)}/start`, "POST");
}

export function stopSession(sessionId: string) {
  return write<unknown>(sessionPath(sessionId, "stop"), "POST");
}

export function killSession(sessionId: string) {
  return write<unknown>(sessionPath(sessionId, "kill"), "POST");
}

export function sendMessage(workspace: string, actor: string, body: string) {
  return write<{ message: FeldApi.Message }>(`${actorPath(workspace, actor)}/messages`, "POST", { body });
}

// The daemon answers before it closes, then the event stream ends with
// `{"resource":"daemon","id":"shutdown"}`.
export function shutdownDaemon() {
  return write<{ accepted: boolean; pid: number }>("/v1/daemon/shutdown", "POST");
}

// The daemon answers before it stops and starts again as a new instance; the browser
// token outlives it, so this page reads on.
export function restartDaemon() {
  return write<{ accepted: boolean; pid: number; instanceId: string }>("/v1/daemon/restart", "POST");
}
