import { beforeEach, expect, test, vi } from "vitest";
import { QueryClient, QueryObserver } from "@tanstack/react-query";
import { keys } from "../api/queries.ts";
import { killSession, startActor, stopSession } from "../api/client.ts";
import { bulkSummary, bulkTargetsFor, chosenAgents } from "./bulk.ts";
import { runBulkAction } from "./mutations.ts";
import { useActorsStore } from "./store.ts";
import type { ActorRecord, AgentSession } from "./types.ts";

vi.mock("../api/client.ts", () => ({
  startActor: vi.fn(async () => ({ session: { id: "session_new" } })),
  stopSession: vi.fn(async () => ({})),
  killSession: vi.fn(async () => ({})),
}));

const store = () => useActorsStore.getState();

beforeEach(() => {
  store().reset(undefined, Date.parse("2026-09-25T00:30:00Z"));
  vi.clearAllMocks();
});

const launchSpec = { runtime: "claude-code" } as ActorRecord["launchSpec"];
const actor = (name: string, extra: Partial<ActorRecord> = {}): ActorRecord => ({ id: `default/${name}`, name, workspace: "default", kind: "agent", launchSpec, ...extra });
const session = (id: string, ownership: "owned" | "attached", end: number | null = null) => ({ id, end, ownership, execution: { ownership } }) as unknown as AgentSession;

// dev runs an owned Session, att an attached one, idle has ended, bare has no launchSpec,
// and gone is deleted.
const AGENTS = [actor("dev"), actor("att"), actor("idle"), actor("bare", { launchSpec: null }), actor("gone", { deletedAt: 1 })];
const SESSIONS: Record<string, AgentSession> = {
  "default/dev": session("session_dev", "owned"),
  "default/att": session("session_att", "attached"),
  "default/idle": session("session_idle", "owned", 1),
};
const model = { latestSessionFor: (agentId: string) => SESSIONS[agentId] ?? null };

test("each action applies to the chosen Actors the Actions menu offers it for, and names why it skips the rest", () => {
  const skips = (action: "start" | "stop" | "kill") => bulkTargetsFor(model, action, AGENTS).map((target) => [target.agent.name, target.sessionId, target.skip]);
  expect(skips("start")).toEqual([
    ["dev", null, "session_dev is running"],
    ["att", null, "session_att is running"],
    ["idle", null, null],
    ["bare", null, "no launchSpec: created by attach"],
    ["gone", null, "deleted"],
  ]);
  expect(skips("stop")).toEqual([
    ["dev", "session_dev", null],
    ["att", null, "session_att is attached: the person who started it ends it"],
    ["idle", null, "no running Session"],
    ["bare", null, "no running Session"],
    ["gone", null, "deleted"],
  ]);
  expect(skips("kill")).toEqual(skips("stop"));
});

test("only the chosen Actors among the rows drawn are acted on, in the rows' order", () => {
  expect(chosenAgents(AGENTS, new Set(["default/idle", "default/dev", "default/hidden"])).map((agent) => agent.name)).toEqual(["dev", "idle"]);
});

test("Stop is sent to each Actor it applies to, and one refusal leaves the others accepted", async () => {
  const client = new QueryClient();
  const observer = new QueryObserver(client, { queryKey: keys.actors("default"), queryFn: async () => 1, staleTime: Infinity });
  observer.subscribe(() => {});
  await vi.waitFor(() => expect(observer.getCurrentResult().data).toBe(1));
  const other = actor("other");
  const sessions: Record<string, AgentSession> = { ...SESSIONS, "default/other": session("session_other", "owned") };
  vi.mocked(stopSession).mockImplementation(async (sessionId: string) => {
    if (sessionId === "session_other") throw new Error("session_other has already ended");
    return {} as never;
  });
  const running = runBulkAction(client, { latestSessionFor: (agentId) => sessions[agentId] ?? null }, "stop", [AGENTS[0], AGENTS[1], other]);
  expect(store().bulkRequest).toMatchObject({ action: "stop", pending: true });
  await running;
  expect(vi.mocked(stopSession).mock.calls.map(([sessionId]) => sessionId)).toEqual(["session_dev", "session_other"]);
  expect(store().bulkRequest).toEqual({
    action: "stop",
    pending: false,
    results: [
      { agentId: "default/dev", outcome: "accepted", detail: "The execution is asked to end; the Session ends when it does." },
      { agentId: "default/att", outcome: "skipped", detail: "session_att is attached: the person who started it ends it" },
      { agentId: "default/other", outcome: "refused", detail: "session_other has already ended" },
    ],
  });
  expect(bulkSummary(store().bulkRequest!.results)).toBe("1 accepted · 1 refused · 1 skipped");
  // The accepted Stop changed the Actors' state, which is read again.
  expect(client.getQueryCache().find({ queryKey: keys.actors("default"), exact: true })!.state.isInvalidated).toBe(true);
});

test("Start and Kill send their own primitive, and nothing is read again when every Actor is skipped", async () => {
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  await runBulkAction(client, model, "start", AGENTS);
  expect(vi.mocked(startActor).mock.calls).toEqual([["default", "idle"]]);
  await runBulkAction(client, model, "kill", AGENTS);
  expect(vi.mocked(killSession).mock.calls).toEqual([["session_dev"]]);
  invalidate.mockClear();
  await runBulkAction(client, model, "kill", [AGENTS[2]]);
  expect(store().bulkRequest?.results).toEqual([{ agentId: "default/idle", outcome: "skipped", detail: "no running Session" }]);
  expect(invalidate).not.toHaveBeenCalled();
});

test("the choice and the bar hold while a request is in flight, and closing the bar clears both", async () => {
  store().toggleBulk();
  store().setBulkChosen(["default/dev", "default/idle"], true);
  store().setBulkChosen(["default/idle"], false);
  expect([...store().bulkActorIds]).toEqual(["default/dev"]);
  let accept!: () => void;
  vi.mocked(stopSession).mockImplementationOnce(() => new Promise((resolve) => { accept = () => resolve({} as never); }));
  const running = runBulkAction(new QueryClient(), model, "stop", [AGENTS[0]]);
  store().toggleBulk();
  store().setBulkChosen(["default/idle"], true);
  store().selectWorkspace("other");
  expect(store().bulkOpen).toBe(true);
  expect([...store().bulkActorIds]).toEqual(["default/dev"]);
  accept();
  await running;
  store().toggleBulk();
  expect(store()).toMatchObject({ bulkOpen: false, bulkRequest: null });
  expect(store().bulkActorIds.size).toBe(0);
});
