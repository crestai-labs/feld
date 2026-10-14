import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { onlineManager, QueryObserver, type QueryClient } from "@tanstack/react-query";

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("window", globalThis);
});

afterEach(() => {
  onlineManager.setOnline(true);
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

// The Actors page's range reads, one read the daemon fixes at its start (Runtimes,
// Providers, its status) and a Console read, each with the reads it made after their first
// answers.
async function observed(client: QueryClient) {
  const reads = { range: 0, runtimes: 0, providers: 0, status: 0, console: 0 };
  const observers = ([
    ["range", ["listing", "live|*|1800000"]],
    ["runtimes", ["runtimes"]],
    ["providers", ["providers"]],
    ["status", ["daemon-status"]],
    ["console", ["console", "log", "s"]],
  ] as const).map(([name, queryKey]) => new QueryObserver(client, { queryKey, queryFn: () => ++reads[name], staleTime: 0 }));
  client.mount();
  const unsubscribe = [...observers.map((observer) => observer.subscribe(() => {})), () => client.unmount()];
  await vi.runAllTimersAsync();
  return { reads, unsubscribe: () => unsubscribe.forEach((stop) => stop()) };
}

test("a held range waits for Refresh when the browser is back online", async () => {
  const { createQueryClient } = await import("./queries.ts");
  const { holdRangeReads } = await import("./events.ts");
  const client = createQueryClient();
  const { reads, unsubscribe } = await observed(client);
  holdRangeReads(true);
  onlineManager.setOnline(false);
  onlineManager.setOnline(true);
  await vi.runAllTimersAsync();
  expect(reads).toEqual({ range: 1, runtimes: 1, providers: 1, status: 1, console: 2 });
  client.invalidateQueries();
  await vi.runAllTimersAsync();
  expect(reads).toEqual({ range: 2, runtimes: 2, providers: 2, status: 2, console: 3 });
  holdRangeReads(false);
  onlineManager.setOnline(false);
  onlineManager.setOnline(true);
  await vi.runAllTimersAsync();
  expect(reads).toEqual({ range: 3, runtimes: 3, providers: 3, status: 3, console: 4 });
  unsubscribe();
});

test("a stream that reconnects marks a held range stale and reads the rest", async () => {
  const { createQueryClient } = await import("./queries.ts");
  const { holdRangeReads, invalidateMissed } = await import("./events.ts");
  const client = createQueryClient();
  const { reads, unsubscribe } = await observed(client);
  holdRangeReads(true);
  invalidateMissed(client);
  await vi.runAllTimersAsync();
  expect(reads).toEqual({ range: 1, runtimes: 1, providers: 1, status: 1, console: 2 });
  for (const queryKey of [["listing", "live|*|1800000"], ["runtimes"], ["providers"], ["daemon-status"]]) {
    expect(client.getQueryState(queryKey)?.isInvalidated).toBe(true);
  }
  holdRangeReads(false);
  invalidateMissed(client);
  await vi.runAllTimersAsync();
  expect(reads).toEqual({ range: 2, runtimes: 2, providers: 2, status: 2, console: 3 });
  unsubscribe();
});

test("a resync marks a held range stale and reads the rest", async () => {
  const { createQueryClient } = await import("./queries.ts");
  const { holdRangeReads, noteNotification } = await import("./events.ts");
  const { invalidationCollapser } = await import("../lib/invalidations.ts");
  const client = createQueryClient();
  const { reads, unsubscribe } = await observed(client);
  const collapse = invalidationCollapser(client, 1000);
  holdRangeReads(true);
  noteNotification(client, collapse, { resource: "resync", id: "*" });
  await vi.runAllTimersAsync();
  expect(reads).toEqual({ range: 1, runtimes: 1, providers: 1, status: 1, console: 2 });
  expect(client.getQueryState(["listing", "live|*|1800000"])?.isInvalidated).toBe(true);
  holdRangeReads(false);
  noteNotification(client, collapse, { resource: "resync", id: "default" });
  await vi.runAllTimersAsync();
  expect(reads).toEqual({ range: 2, runtimes: 2, providers: 2, status: 2, console: 3 });
  unsubscribe();
});

test("an Actor listing goes stale with its Actor and the Sessions it lists alone", async () => {
  const { QueryClient } = await import("@tanstack/react-query");
  const { actorListingsHolding, invalidatedBy } = await import("./events.ts");
  const { keys } = await import("./queries.ts");
  const client = new QueryClient();
  const dev = keys.actorListing({ workspace: "default", name: "dev" });
  client.setQueryData(dev, { sessions: [{ id: "s1" }], subAgents: [] });
  client.setQueryData(keys.actorListing({ workspace: "default", name: "ops" }), { sessions: [{ id: "s2" }], subAgents: [] });
  expect(actorListingsHolding(client, { resource: "session", id: "s1" })).toEqual([dev]);
  expect(actorListingsHolding(client, { resource: "session", id: "s3" })).toEqual([]);
  expect(actorListingsHolding(client, { resource: "log", id: "s1" })).toEqual([]);
  expect(invalidatedBy({ resource: "actor", id: "dev" })).toContainEqual(keys.actorListingsOf("dev"));
  // The range's listing is a separate read.
  expect(invalidatedBy({ resource: "session", id: "s2" }).some((key) => key[0] === "actor-listing")).toBe(false);
});
