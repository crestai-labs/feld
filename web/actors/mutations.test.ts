import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { QueryClient, QueryObserver, type QueryKey } from "@tanstack/react-query";
import { holdRangeReads } from "../api/events.ts";
import { keys } from "../api/queries.ts";
import { createActor, deleteActor, listActors, sendMessage, startActor, updateActor } from "../api/client.ts";
import { deleteActorFromMenu, refuseMenuAction, saveActorEdit, sendComposedMessage, stopActorSession, submitNewActor } from "./mutations.ts";
import { emptyActorDraft, type ActorsModel } from "./model.ts";
import { useActorsStore } from "./store.ts";
import { clearToasts, useToasts } from "./toasts.ts";

// A write settles once the daemon accepts it, and reads again only what it changed.

vi.mock("../api/client.ts", () => ({
  stopSession: vi.fn(async () => ({})),
  deleteActor: vi.fn(),
  sendMessage: vi.fn(async () => ({ message: { id: "message_1" } })),
  updateActor: vi.fn(),
  listActors: vi.fn(),
  createActor: vi.fn(),
  startActor: vi.fn(),
}));

const NOW = Date.parse("2026-09-25T00:30:00Z");
const store = () => useActorsStore.getState();

beforeEach(() => {
  store().reset(undefined, NOW);
  clearToasts();
});

const toasts = () => useToasts.getState().toasts.map((toast) => [toast.kind, toast.text, toast.detail]);

afterEach(() => {
  holdRangeReads(false);
});

// An observed query whose reads never finish, with the reads it started and the ones cut
// short.
function endlessQuery(client: QueryClient, queryKey: QueryKey) {
  const reads = { started: 0, aborted: 0 };
  const observer = new QueryObserver(client, {
    queryKey,
    queryFn: ({ signal }) => {
      reads.started += 1;
      return new Promise((_, reject) => signal.addEventListener("abort", () => {
        reads.aborted += 1;
        reject(signal.reason);
      }));
    },
    staleTime: Infinity,
  });
  observer.subscribe(() => {});
  return { reads, query: () => client.getQueryCache().find({ queryKey, exact: true })! };
}

// An observed query whose reads each wait for `finish`, with the reads it started.
function heldQuery(client: QueryClient, queryKey: QueryKey) {
  const reads = { started: 0 };
  const pending: ((value: number) => void)[] = [];
  const observer = new QueryObserver(client, {
    queryKey,
    queryFn: () => {
      reads.started += 1;
      return new Promise<number>((resolve) => pending.push(resolve));
    },
    staleTime: Infinity,
  });
  observer.subscribe(() => {});
  return { reads, finish: (value: number) => pending.shift()!(value), query: () => client.getQueryCache().find({ queryKey, exact: true })! };
}

// An observed query read once, which tells whether a write invalidated it.
async function readQuery(client: QueryClient, queryKey: QueryKey) {
  const reads = { started: 0 };
  const observer = new QueryObserver(client, {
    queryKey,
    queryFn: async () => {
      reads.started += 1;
      return reads.started;
    },
    staleTime: Infinity,
  });
  observer.subscribe(() => {});
  await vi.waitFor(() => expect(reads.started).toBe(1));
  return { reads, query: () => client.getQueryCache().find({ queryKey, exact: true })! };
}

test("a Live range's menu closes once Stop is accepted, while the range's read goes on", async () => {
  const client = new QueryClient();
  const listing = endlessQuery(client, keys.listing("live|*|3600000"));
  const records = await readQuery(client, keys.records("live|*|3600000"));
  useActorsStore.setState({ actionMenuAgent: "default/dev" });
  stopActorSession(client, "default/dev", "session_1");
  expect(store().actionRequest?.pending).toBe(true);
  // The toast says the request is in flight, then that it was accepted.
  expect(toasts()).toEqual([["pending", "Asking the execution to end · dev", "Waiting for the daemon to accept the request."]]);
  await vi.waitFor(() => expect(store().actionMenuAgent).toBeNull());
  expect(store().actionRequest).toBeNull();
  expect(toasts()).toEqual([["done", "Stop accepted · dev", "The execution is asked to end; the Session ends when it does."]]);
  // The read in flight is not cut short for another one.
  expect(listing.reads).toEqual({ started: 1, aborted: 0 });
  expect(listing.query().state.isInvalidated).toBe(true);
  // Stop leaves the records as they were.
  expect(records.query().state.isInvalidated).toBe(false);
  expect(records.reads.started).toBe(1);
});

test("a paused range's write marks what it changed stale without reading it", async () => {
  const client = new QueryClient();
  const actors = await readQuery(client, keys.actors("default"));
  holdRangeReads(true);
  stopActorSession(client, "default/dev", "session_1");
  await vi.waitFor(() => expect(store().actionRequest).toBeNull());
  expect(actors.query().state.isInvalidated).toBe(true);
  expect(actors.query().state.fetchStatus).toBe("idle");
  expect(actors.reads.started).toBe(1);
});

test("the composer reports the Message sent while the Messages' read goes on", async () => {
  const client = new QueryClient();
  const messages = endlessQuery(client, keys.messages("live|*|3600000"));
  const model = { actorFor: () => ({ workspace: "default", name: "dev" }) } as unknown as ActorsModel;
  store().openComposer("default/dev");
  store().setComposeDraft("hello");
  await sendComposedMessage(client, model);
  expect(store()).toMatchObject({ composeSending: {}, composeDrafts: {}, composeResult: { id: "message_1", body: "hello" } });
  expect(messages.reads).toEqual({ started: 1, aborted: 0 });
  expect(messages.query().state.isInvalidated).toBe(true);
});

// A send whose daemon answer waits for `accept`.
function heldSend() {
  let accept!: () => void;
  vi.mocked(sendMessage).mockImplementationOnce(() => new Promise((resolve) => { accept = () => resolve({ message: { id: "message_1" } } as never); }));
  return () => accept();
}

const devModel = { actorFor: () => ({ workspace: "default", name: "dev" }) } as unknown as ActorsModel;
const draftOf = (agentId: string) => store().composeDrafts[agentId]?.text;

test("a send that lands after its composer was reopened keeps the draft written since", async () => {
  const accept = heldSend();
  store().openComposer("default/dev");
  store().setComposeDraft("hello");
  const sent = sendComposedMessage(new QueryClient(), devModel);
  store().clearInspector();
  store().openComposer("default/dev");
  store().setComposeDraft("next");
  accept();
  await sent;
  expect(draftOf("default/dev")).toBe("next");
  expect(store()).toMatchObject({ composeAgentId: "default/dev", composeSending: {}, composeResult: null });
});

test("a send that lands after its composer was reopened drops the draft it sent", async () => {
  const accept = heldSend();
  store().openComposer("default/dev");
  store().setComposeDraft("hello");
  const sent = sendComposedMessage(new QueryClient(), devModel);
  store().clearInspector();
  store().openComposer("default/dev");
  accept();
  await sent;
  expect(store()).toMatchObject({ composeSending: {}, composeDrafts: {}, composeResult: null });
});

test("a draft cancelled and written again with the same text outlasts the earlier send", async () => {
  const accept = heldSend();
  store().openComposer("default/dev");
  store().setComposeDraft("hello");
  const sent = sendComposedMessage(new QueryClient(), devModel);
  store().cancelComposer();
  store().openComposer("default/dev");
  store().setComposeDraft("hello");
  accept();
  await sent;
  expect(draftOf("default/dev")).toBe("hello");
});

test("the Actor's send in flight holds Send in the composer opened again", async () => {
  const calls = vi.mocked(sendMessage).mock.calls.length;
  const accept = heldSend();
  store().openComposer("default/dev");
  store().setComposeDraft("hello");
  const sent = sendComposedMessage(new QueryClient(), devModel);
  store().clearInspector();
  store().openComposer("default/dev");
  expect(store().composeSending).toEqual({ "default/dev": true });
  await sendComposedMessage(new QueryClient(), devModel);
  expect(vi.mocked(sendMessage).mock.calls.length).toBe(calls + 1);
  accept();
  await sent;
  expect(store().composeSending).toEqual({});
});

test("a read in flight when the write lands is read again once it ends", async () => {
  const client = new QueryClient();
  const messages = heldQuery(client, keys.messages("live|*|3600000"));
  const model = { actorFor: () => ({ workspace: "default", name: "dev" }) } as unknown as ActorsModel;
  store().openComposer("default/dev");
  store().setComposeDraft("hello");
  await sendComposedMessage(client, model);
  expect(messages.reads.started).toBe(1);
  // The read left before the Message, and answers without it.
  messages.finish(1);
  await vi.waitFor(() => expect(messages.reads.started).toBe(2));
  messages.finish(2);
  await vi.waitFor(() => expect(messages.query().state.data).toBe(2));
  expect(messages.reads.started).toBe(2);
});

test("the Job edit's accepted Schedule is read again when its launchSpec is refused", async () => {
  const client = new QueryClient();
  const actors = await readQuery(client, keys.actors("default"));
  vi.mocked(updateActor)
    .mockResolvedValueOnce({ actor: { schedule: { enabled: true, nextRun: null } } } as never)
    .mockRejectedValueOnce(new Error("workdir refused"));
  const model = {
    scheduleFor: () => ({ cron: "0 9 * * *", timezone: "Asia/Tokyo", enabled: true }),
    actorFor: () => ({ workspace: "default", name: "nightly", launchSpec: { command: ["sleep", "1"], workdir: "/tmp" } }),
    dateTime: { format: () => "" },
    ZONE: "JST",
  } as unknown as ActorsModel;
  const draft = { name: "nightly", cron: "0 10 * * *", timezone: "Asia/Tokyo", runtime: "", command: "sleep 1", workdir: "/var/tmp", env: "", settings: "", provider: "", node: "", trustWorkdir: "", providerOptions: "" };
  useActorsStore.setState({ cardView: { serial: 1, agentId: "default/nightly", skipId: null, editing: true, draft, error: null, result: null, busy: false } });
  await saveActorEdit(client, model);
  expect(store().cardView).toMatchObject({ busy: false, error: "workdir refused" });
  await vi.waitFor(() => expect(actors.reads.started).toBe(2));
});

test("an Agent edit sends its launchSpec under the old name, renames last, and the card follows the new name", async () => {
  const client = new QueryClient();
  vi.mocked(updateActor).mockReset().mockResolvedValue({ actor: {} } as never);
  const model = {
    scheduleFor: () => null,
    actorFor: () => ({ workspace: "default", name: "dev", kind: "agent", launchSpec: { runtime: "claude-code", runtimeSettings: { apiKey: "sk-1" }, workdir: "/w", provider: "tmux", node: "local" } }),
  } as unknown as ActorsModel;
  const draft = { name: "dev-2", cron: "", timezone: "", runtime: "claude-code", command: "", workdir: "/w2", provider: "tmux", node: "local", env: "", settings: '{"apiKey":"••••••••"}', trustWorkdir: "", providerOptions: "" };
  useActorsStore.setState({ actorCard: { agentId: "default/dev", sessionId: null }, cardView: { serial: 1, agentId: "default/dev", skipId: null, editing: true, draft, error: null, result: null, busy: false } });
  await saveActorEdit(client, model);
  // The kept mask is not sent, and the secret is not either.
  expect(vi.mocked(updateActor).mock.calls).toEqual([
    ["default", "dev", { launchSpec: { workdir: "/w2" } }],
    ["default", "dev", { name: "dev-2" }],
  ]);
  expect(store().actorCard).toEqual({ agentId: "default/dev-2", sessionId: null });
  expect(store().cardView).toMatchObject({ agentId: "default/dev-2", editing: false });
  expect(store().cardView?.result).toBe("feld actor update dev --workdir /w2: takes effect at the next start. feld actor update dev --name dev-2: the id, Sessions, and mailbox stay.");
});

test("a New actor whose start is refused is read again as declared", async () => {
  const client = new QueryClient();
  const actors = await readQuery(client, keys.actors("default"));
  vi.mocked(listActors).mockResolvedValueOnce({ actors: [] } as never);
  vi.mocked(createActor).mockResolvedValueOnce({} as never);
  vi.mocked(startActor).mockRejectedValueOnce(new Error("no Provider"));
  const model = { workspaceAgents: () => [] } as unknown as ActorsModel;
  const draft = emptyActorDraft(1, "default", "Asia/Tokyo", { runtimes: [], nodes: [], providers: [] } as never);
  useActorsStore.setState({ newActor: { ...draft, kind: "job", name: "nightly", command: "sleep 1", workdir: "/tmp" } });
  await submitNewActor(client, model, "start");
  expect(store().newActor).toMatchObject({ submitting: false, error: "nightly was declared, but no Session started: no Provider" });
  await vi.waitFor(() => expect(actors.reads.started).toBe(2));
});

test("a paused range's read in flight when the write lands is marked stale again once it ends", async () => {
  const client = new QueryClient();
  const actors = heldQuery(client, keys.actors("default"));
  holdRangeReads(true);
  stopActorSession(client, "default/dev", "session_1");
  await vi.waitFor(() => expect(store().actionRequest).toBeNull());
  // The read left before Stop, and answers without it.
  actors.finish(1);
  await vi.waitFor(() => expect(actors.query().state.data).toBe(1));
  await vi.waitFor(() => expect(actors.query().state.isInvalidated).toBe(true));
  expect(actors.query().state.fetchStatus).toBe("idle");
  expect(actors.reads.started).toBe(1);
});

test("a refused menu request closes the menu and says so in a toast", async () => {
  vi.useFakeTimers();
  try {
    useActorsStore.setState({ actionMenuAgent: "default/nightly" });
    refuseMenuAction("default/nightly", "pause", "nightly has no Schedule");
    expect(toasts()).toEqual([["pending", "Pausing the Schedule · nightly", "Waiting for the daemon to accept the request."]]);
    await vi.advanceTimersByTimeAsync(1000);
    expect(store().actionMenuAgent).toBeNull();
    expect(store().actionRequest).toBeNull();
    expect(toasts()).toEqual([["error", "Schedule not paused · nightly", "nightly has no Schedule"]]);
  } finally {
    vi.useRealTimers();
  }
});

test("Delete reads the Actors again once accepted, and says the daemon's refusal in a toast", async () => {
  const client = new QueryClient();
  const actors = await readQuery(client, keys.actors("default"));
  const dev = { id: "default/dev", name: "dev", workspace: "default", kind: "agent" as const, launchSpec: null };
  vi.mocked(deleteActor).mockResolvedValueOnce({} as never);
  useActorsStore.setState({ actionMenuAgent: dev.id });
  deleteActorFromMenu(client, dev);
  await vi.waitFor(() => expect(store().actionMenuAgent).toBeNull());
  expect(deleteActor).toHaveBeenCalledWith("default", "dev");
  await vi.waitFor(() => expect(actors.reads.started).toBe(2));
  vi.mocked(deleteActor).mockRejectedValueOnce(new Error("Actor 'dev' has a running current Session 's1'; stop it before the delete"));
  useActorsStore.setState({ actionMenuAgent: dev.id });
  deleteActorFromMenu(client, dev);
  await vi.waitFor(() => expect(store().actionMenuAgent).toBeNull());
  expect(store().actionRequest).toBeNull();
  expect(toasts().at(-1)).toEqual(["error", "Actor not deleted · dev", "Actor 'dev' has a running current Session 's1'; stop it before the delete"]);
});
