import assert from "node:assert/strict";
import { test } from "vitest";
import { readSlots } from "./read-slots.ts";

function deferred() {
  let resolve;
  const promise = new Promise((done) => (resolve = done));
  return { promise, resolve };
}

const settle = () => new Promise((done) => setTimeout(done, 0));

test("read-slots", async () => {
  const run = readSlots(2);
  const started = [];
  const reads = [0, 1, 2, 3].map(() => deferred());
  const answers = reads.map((read, index) => run(() => {
    started.push(index);
    return read.promise;
  }));
  await settle();
  // Two reads go at once; the rest wait in the order they asked.
  assert.deepEqual(started, [0, 1]);

  reads[1].resolve("one");
  assert.equal(await answers[1], "one");
  await settle();
  assert.deepEqual(started, [0, 1, 2]);

  // A waiting read whose signal aborts leaves the line without taking a slot.
  const controller = new AbortController();
  const aborted = run(() => {
    started.push("aborted");
    return Promise.resolve();
  }, controller.signal);
  controller.abort(new Error("aborted"));
  await assert.rejects(aborted, /aborted/);

  // A failed read gives its slot back as well.
  reads[0].resolve(Promise.reject(new Error("failed")));
  await assert.rejects(answers[0], /failed/);
  await settle();
  assert.deepEqual(started, [0, 1, 2, 3]);

  reads[2].resolve("two");
  reads[3].resolve("three");
  assert.deepEqual(await Promise.all([answers[2], answers[3]]), ["two", "three"]);
  // Every slot is free again.
  const free = [];
  await Promise.all([run(async () => free.push(1)), run(async () => free.push(2))]);
  assert.deepEqual(free, [1, 2]);
});
