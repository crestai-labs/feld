import { expect, test, vi } from "vitest";
import { api, readSession } from "./client.ts";
import { readDeletedActorSessions } from "./queries.ts";

// A deleted Actor's card lists every Session it had and their Sub Agent Sessions.

vi.mock("./client.ts", () => ({
  api: vi.fn(),
  readSession: vi.fn(),
  isUnauthorized: () => false,
  readDaemonStatus: vi.fn(),
  listActors: vi.fn(),
}));

test("a deleted Actor's Sessions are read by its id, with each Session's Sub Agent Sessions", async () => {
  vi.mocked(api).mockResolvedValueOnce({ actor: { sessions: [{ id: "s2" }, { id: "s1" }] } });
  vi.mocked(readSession).mockImplementation(async (id) => ({ session: { id }, subAgents: id === "s1" ? [{ id: "s1_subagent_a", parentSession: "s1" }] : [] }) as never);
  const read = await readDeletedActorSessions("actor_old", new AbortController().signal);
  expect(api).toHaveBeenCalledWith("/v1/actors/actor_old", expect.anything());
  expect(read).toEqual({ sessions: [{ id: "s2" }, { id: "s1" }], subAgents: [{ id: "s1_subagent_a", parentSession: "s1" }] });
});
