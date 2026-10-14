import assert from "node:assert/strict";
import { afterEach, beforeEach, test, vi } from "vitest";
import { QueryClient, QueryObserver } from "@tanstack/react-query";
import { invalidationCollapser } from "./invalidations.ts";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

// A query whose every read takes `duration` milliseconds, with the reads it started, the
// ones cut short and the ones that finished.
function slowQuery(client, duration) {
  const reads = { started: 0, aborted: 0, finished: 0 };
  const observer = new QueryObserver(client, {
    queryKey: ["listing", "range"],
    queryFn: ({ signal }) => {
      reads.started += 1;
      const read = reads.started;
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          reads.finished += 1;
          resolve(read);
        }, duration);
        signal.addEventListener("abort", () => {
          clearTimeout(timer);
          reads.aborted += 1;
          reject(signal.reason);
        });
      });
    },
    staleTime: 0,
  });
  const unsubscribe = observer.subscribe(() => {});
  return { reads, observer, unsubscribe };
}

test("a steady stream of notifications lets a slow read finish", async () => {
  const client = new QueryClient();
  const { reads, observer, unsubscribe } = slowQuery(client, 3000);
  await vi.advanceTimersByTimeAsync(3000);
  assert.equal(reads.finished, 1);
  const collapse = invalidationCollapser(client, 1000);
  for (let second = 0; second < 12; second += 1) {
    collapse.add([["listing"]]);
    await vi.advanceTimersByTimeAsync(1000);
  }
  assert.equal(reads.aborted, 0);
  assert.ok(reads.finished >= 4, `finished ${reads.finished}`);
  assert.equal(observer.getCurrentResult().data, reads.finished);
  collapse.cancel();
  unsubscribe();
});

test("a read in flight when its notification comes is followed by one more read", async () => {
  const client = new QueryClient();
  const { reads, unsubscribe } = slowQuery(client, 3000);
  const collapse = invalidationCollapser(client, 1000);
  await vi.advanceTimersByTimeAsync(500);
  collapse.add([["listing"], ["listing"]]);
  await vi.advanceTimersByTimeAsync(1000);
  assert.deepEqual(reads, { started: 1, aborted: 0, finished: 0 });
  await vi.advanceTimersByTimeAsync(2000);
  assert.deepEqual(reads, { started: 2, aborted: 0, finished: 1 });
  await vi.advanceTimersByTimeAsync(10000);
  assert.deepEqual(reads, { started: 2, aborted: 0, finished: 2 });
  collapse.cancel();
  unsubscribe();
});

test("a held query is marked stale and not read again", async () => {
  const client = new QueryClient();
  const { reads, unsubscribe } = slowQuery(client, 100);
  await vi.advanceTimersByTimeAsync(100);
  const collapse = invalidationCollapser(client, 1000, () => true);
  collapse.add([["listing"]]);
  await vi.advanceTimersByTimeAsync(2000);
  assert.equal(reads.started, 1);
  assert.equal(client.getQueryState(["listing", "range"]).isInvalidated, true);
  collapse.cancel();
  unsubscribe();
});
