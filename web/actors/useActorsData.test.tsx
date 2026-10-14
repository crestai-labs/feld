// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createQueryClient } from "../api/queries.ts";
import type { ConsoleReads } from "../console/reads.ts";
import { useActorsStore } from "./store.ts";
import { listedActorOf, useActorsData, type ActorsData } from "./useActorsData.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

test("the selected Session's Actor is read before the kept one", () => {
  expect(listedActorOf("default/a", "s-b", "default/b", "default/a")).toBe("default/a");
  // A Session of another Actor selected from the Timeline reads that Actor's Sessions.
  expect(listedActorOf(null, "s-b", "default/b", "default/a")).toBe("default/b");
  // A selected Session the range lacks keeps the Actor it was opened from.
  expect(listedActorOf(null, "s-old", null, "default/a")).toBe("default/a");
  expect(listedActorOf(null, null, null, "default/a")).toBe(null);
});

// A daemon with one running Session that started two days ago, across the start of the
// Live range. Its metrics read from its start answer `lifetime`, and any other `range`.
const SESSION_START = Date.now() - 2 * 24 * 60 * 60 * 1000;
const SESSION = {
  id: "s1", workspace: "default", actor: "dev", startKind: "manual", ownership: "owned", endKind: null, exitCode: null,
  start: new Date(SESSION_START).toISOString(), end: null,
  execution: { type: "program", label: "sh", command: ["sh"], ai: false },
  placement: { provider: "local", node: "local", workdir: "/tmp", placement: {} },
};
const ANSWERS: Record<string, unknown> = {
  "/v1/workspaces": { workspaces: [{ key: "default" }] },
  "/v1/workspaces/default/actors": { actors: [{ workspace: "default", name: "dev", kind: "job", launchSpec: { command: ["sh"], workdir: "/tmp", provider: "local", node: "local" } }] },
  "/v1/sessions": { sessions: [SESSION] },
  "/v1/sessions/s1": { session: SESSION, subAgents: [] },
  "/v1/records": { records: [] },
  "/v1/messages": { messages: [] },
  "/v1/runtimes": { runtimes: [] },
  "/v1/providers": { providers: [] },
  "/v1/daemon/status": { nodes: [] },
};

function daemon(path: string) {
  const url = new URL(path, "http://feld");
  const answer = url.pathname === "/v1/sessions/s1/metrics"
    ? { session: "s1", bucket: 60, metrics: [{ metric: "marker", read: Date.parse(url.searchParams.get("start")!) === SESSION_START ? "lifetime" : "range" }] }
    : ANSWERS[url.pathname];
  if (answer === undefined) return new Promise(() => {});
  return Promise.resolve(new Response(JSON.stringify(answer), { status: 200 }));
}

let root: ReturnType<typeof createRoot>;
let data: ActorsData;

function Probe() {
  data = useActorsData({ consoleReads: {} as ConsoleReads });
  return null;
}

beforeEach(async () => {
  vi.stubGlobal("fetch", daemon);
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<QueryClientProvider client={createQueryClient()}><Probe /></QueryClientProvider>));
});

afterEach(async () => {
  await act(async () => useActorsStore.getState().clearInspector());
  await act(async () => root.unmount());
  vi.unstubAllGlobals();
});

const markerOf = (session: { laneMetrics: Record<string, unknown> } | null | undefined) => (session?.laneMetrics.marker as { read: string } | undefined)?.read;

test("a selected Session across the range's start is measured over its lifetime while the Timeline draws the range's", async () => {
  await vi.waitFor(() => expect(markerOf(data.model.sessionsFor("default/dev")[0])).toBe("range"));
  await act(async () => useActorsStore.getState().openConsole("s1"));
  await vi.waitFor(() => expect(markerOf(data.model.sessionById("s1"))).toBe("lifetime"));
  expect(markerOf(data.model.sessionsFor("default/dev")[0])).toBe("range");
});
