// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { expect, test, vi } from "vitest";
import { api, readSession, readUiAccess } from "./client.ts";
import { createQueryClient, readDeletedActorSessions, useCanOperate } from "./queries.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// A deleted Actor's card lists every Session it had and their Sub Agent Sessions.

vi.mock("./client.ts", () => ({
  api: vi.fn(),
  readSession: vi.fn(),
  isUnauthorized: () => false,
  readDaemonStatus: vi.fn(),
  readUiAccess: vi.fn(),
  listActors: vi.fn(),
}));

test("a deleted Actor's Sessions are read by its id, with each Session's Sub Agent Sessions", async () => {
  vi.mocked(api).mockResolvedValueOnce({ actor: { sessions: [{ id: "s2" }, { id: "s1" }] } });
  vi.mocked(readSession).mockImplementation(async (id) => ({ session: { id }, subAgents: id === "s1" ? [{ id: "s1_subagent_a", parentSession: "s1" }] : [] }) as never);
  const read = await readDeletedActorSessions("actor_old", new AbortController().signal);
  expect(api).toHaveBeenCalledWith("/v1/actors/actor_old", expect.anything());
  expect(read).toEqual({ sessions: [{ id: "s2" }, { id: "s1" }], subAgents: [{ id: "s1_subagent_a", parentSession: "s1" }] });
});

// What useCanOperate answers on each drawing of a page at `url`.
async function canOperateAt(url: string) {
  const answers: boolean[] = [];
  function Probe() {
    answers.push(useCanOperate());
    return null;
  }
  const root = createRoot(document.createElement("div"));
  await act(async () => root.render(
    <QueryClientProvider client={createQueryClient()}>
      <MemoryRouter initialEntries={[url]}><Probe /></MemoryRouter>
    </QueryClientProvider>,
  ));
  // The daemon's answer lands.
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  await act(async () => root.unmount());
  return answers;
}

test("the page draws itself read-only until the daemon answers admin", async () => {
  vi.mocked(readUiAccess).mockResolvedValueOnce({ access: "admin" });
  const answers = await canOperateAt("/ui/");
  expect(answers[0]).toBe(false);
  expect(answers.at(-1)).toBe(true);
  expect(readUiAccess).toHaveBeenCalledTimes(1);
});

test("a caller the daemon answers read stays read-only", async () => {
  vi.mocked(readUiAccess).mockResolvedValueOnce({ access: "read" });
  expect(await canOperateAt("/ui/")).not.toContain(true);
});

test("fixture mode answers ?access= without the daemon, admin by default", async () => {
  vi.mocked(readUiAccess).mockClear();
  expect(await canOperateAt("/ui/?scenario=three-days-ago-1h")).toEqual([true]);
  expect(await canOperateAt("/ui/settings?state=running&access=read")).toEqual([false]);
  expect(readUiAccess).not.toHaveBeenCalled();
});
