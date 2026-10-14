// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createQueryClient } from "../api/queries.ts";
import { ActorsPage } from "./ActorsPage.tsx";
import { useActorsStore } from "./store.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// The Console counts its drawings.
const terminal = vi.hoisted(() => ({ drawings: 0 }));
vi.mock("../console/ConsoleTerminal.tsx", () => ({
  ConsoleTerminal: () => {
    terminal.drawings += 1;
    return <div data-terminal=""></div>;
  },
}));

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
// What settles a pan (METRICS_SETTLE) and some.
const SETTLE = 600;

// A daemon with one Actor and one Session six hours ago. It answers until `answering` is
// cleared, and after that leaves every new read pending, as a pan's new range waits.
const SESSION_START = Date.now() - 6 * HOUR;
const SESSION = {
  id: "s1", workspace: "default", actor: "dev", startKind: "manual", ownership: "owned", endKind: "exited", exitCode: 0,
  start: new Date(SESSION_START).toISOString(), end: new Date(SESSION_START + 10 * MINUTE).toISOString(),
  execution: { type: "program", label: "sh", command: ["sh"], ai: false },
  placement: { provider: "local", node: "local", workdir: "/tmp", placement: {} },
};
const ANSWERS: Record<string, unknown> = {
  "/v1/workspaces": { workspaces: [{ key: "default" }] },
  "/v1/workspaces/default/actors": { actors: [{ workspace: "default", name: "dev", kind: "job", launchSpec: { command: ["sh"], workdir: "/tmp", provider: "local", node: "local" } }] },
  "/v1/sessions": { sessions: [SESSION] },
  "/v1/sessions/s1": { session: SESSION, subAgents: [] },
  "/v1/sessions/s1/metrics": { session: "s1", bucket: 60, metrics: [] },
  "/v1/records": { records: [] },
  "/v1/messages": { messages: [] },
  "/v1/runtimes": { runtimes: [] },
  "/v1/providers": { providers: [] },
  "/v1/daemon/status": { nodes: [] },
};
let answering = true;

function daemon(path: string) {
  const answer = ANSWERS[new URL(path, "http://feld").pathname];
  if (!answering || answer === undefined) return new Promise(() => {});
  return Promise.resolve(new Response(JSON.stringify(answer), { status: 200 }));
}

let root: ReturnType<typeof createRoot>;
let page: HTMLElement;

beforeEach(() => {
  answering = true;
  terminal.drawings = 0;
  vi.stubGlobal("fetch", daemon);
  page = document.createElement("div");
  document.body.append(page);
  root = createRoot(page);
});

afterEach(async () => {
  await act(async () => root.unmount());
  page.remove();
  vi.unstubAllGlobals();
});

function panTo(anchor: number) {
  return act(async () => useActorsStore.getState().panTo(anchor, Date.now()));
}

function settle() {
  return act(() => new Promise((resolve) => setTimeout(resolve, SETTLE)));
}

// The page on a historical range around the Session, with its Console open.
async function openConsole() {
  await act(async () => root.render(
    <QueryClientProvider client={createQueryClient()}>
      <MemoryRouter initialEntries={["/ui/"]}><ActorsPage /></MemoryRouter>
    </QueryClientProvider>,
  ));
  await panTo(SESSION_START);
  await vi.waitFor(() => expect(page.querySelector('[data-session="s1"]')).not.toBeNull());
  await act(async () => useActorsStore.getState().openConsole("s1"));
  await vi.waitFor(() => expect(page.querySelector("[data-terminal]")).not.toBeNull());
  await settle();
}

async function pan() {
  const before = terminal.drawings;
  for (let step = 1; step <= 10; step += 1) await panTo(SESSION_START - step * MINUTE);
  expect(useActorsStore.getState().mode).toBe("historical");
  expect(terminal.drawings).toBe(before);
  // Once the range settles, the Console reads the page where the pan stopped.
  await settle();
  expect(terminal.drawings).toBeGreaterThan(before);
}

test("a pan of a historical range whose new reads are pending leaves the Console as it is until it stops", async () => {
  await openConsole();
  answering = false;
  await pan();
});

test("a pan of a historical range whose new reads answer leaves the Console as it is until it stops", async () => {
  await openConsole();
  await pan();
});

test("the Console follows the range as it returns to Live", async () => {
  await openConsole();
  const before = terminal.drawings;
  await panTo(Date.now());
  expect(useActorsStore.getState().mode).toBe("live");
  expect(terminal.drawings).toBeGreaterThan(before);
});
