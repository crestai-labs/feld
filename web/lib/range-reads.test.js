import assert from "node:assert/strict";
import { test } from "vitest";
import { overlaps, compose, bucketSecondsFor, subAgentsInRange } from "./range-reads.ts";

test("range-reads", () => {
  const plain = (value) => JSON.parse(JSON.stringify(value));

  const T = (minutes) => Date.UTC(2026, 8, 19, 12, minutes);
  const iso = (instant) => new Date(instant).toISOString();
  const live = { workspace: "default", start: T(0), end: null, duration: 60 * 60_000 };
  const past = { workspace: "default", start: T(0), end: T(60), duration: 60 * 60_000 };

  // The range is half-open, as the daemon reads it: an interval ending at its start or
  // starting at its end is outside, and one without an end is still going.
  assert.equal(overlaps(past, iso(T(-10)), iso(T(0))), false);
  assert.equal(overlaps(past, iso(T(60)), null), false);
  assert.equal(overlaps(past, iso(T(59)), iso(T(90))), true);
  assert.equal(overlaps(live, iso(T(-600)), null), true);
  assert.equal(overlaps(live, iso(T(-600)), iso(T(-1))), false);
  // A Sub Agent whose start the transcript does not show is placed nowhere.
  assert.equal(overlaps(live, null, null), false);

  assert.equal(bucketSecondsFor(60 * 60_000), 60);
  assert.equal(bucketSecondsFor(7 * 24 * 60 * 60_000), 5040);

  const actor = (workspace, name) => ({ workspace, name });
  const session = (id, workspace, actorName, start, end = null) => ({ id, workspace, actor: actorName, start: iso(start), end: end === null ? null : iso(end) });
  const subAgent = (parent, name, start, end = null) => ({ ...parent, id: `${parent.id}_subagent_${name}`, parentSession: parent.id, subAgent: name, start: start === null ? null : iso(start), end: end === null ? null : iso(end) });
  const parent = session("s1", "default", "dev", T(-30));
  const later = session("s0", "default", "dev", T(10));
  const gone = session("s2", "default", "deleted", T(5));
  const held = {
    actors: new Map([
      ["default", [actor("default", "dev"), actor("default", "ops")]],
      ["other", [actor("other", "dev")]],
    ]),
    sessions: new Map([[parent.id, parent], [later.id, later], [gone.id, gone]]),
    subAgents: new Map([[parent.id, [
      subAgent(parent, "inside", T(-5)),
      subAgent(parent, "before", T(-120), T(-60)),
      subAgent(parent, "across", T(-40), T(-28)),
      subAgent(parent, "unplaced", null),
    ]]]),
    metrics: new Map([
      [parent.id, { session: parent.id, metrics: [] }],
      ["s1_subagent_inside", { session: "s1_subagent_inside", metrics: [{ metric: "token_usage", status: "n/a" }] }],
    ]),
    records: [
      { sequence: 1, actor: actor("default", "dev") },
      { sequence: 2, actor: actor("default", "deleted") },
    ],
    messages: [
      { id: "m1", to: actor("default", "ops") },
      { id: "m2", to: actor("default", "deleted") },
    ],
    runtimeMetrics: null,
  };

  const snapshot = plain(compose(live, ["default", "other"], held));
  // One Workspace shows its own live Actors only.
  assert.deepEqual(snapshot.actors.map((item) => `${item.workspace}/${item.name}`), ["default/dev", "default/ops"]);
  // Parents run in start order, each followed by its Sub Agents in the range; a deleted
  // Actor's Session, record, and Message leave with it.
  assert.deepEqual(snapshot.sessions.map((item) => item.id), ["s1", "s1_subagent_inside", "s0"]);
  assert.deepEqual(snapshot.records.map((record) => record.sequence), [1]);
  assert.deepEqual(snapshot.messages.map((message) => message.id), ["m1"]);
  // A Session and a Sub Agent Session have the metrics their reads answered, or none until then.
  assert.deepEqual(snapshot.metrics, [held.metrics.get("s1"), held.metrics.get("s1_subagent_inside")]);
  const unanswered = plain(compose(live, ["default"], { ...held, metrics: new Map([[parent.id, held.metrics.get(parent.id)]]) }));
  assert.deepEqual(unanswered.sessions.map((item) => item.id), ["s1", "s1_subagent_inside", "s0"]);
  assert.deepEqual(unanswered.metrics.map((entry) => entry.session), ["s1"]);
  assert.equal(snapshot.runtimeMetrics, null);

  // All Workspaces show every Workspace's live Actors.
  const everywhere = plain(compose({ ...live, workspace: null }, ["default", "other"], held));
  assert.deepEqual(everywhere.actors.map((item) => `${item.workspace}/${item.name}`), ["default/dev", "default/ops", "other/dev"]);
  assert.deepEqual(everywhere.workspaces, ["default", "other"]);

  // A Sub Agent counts from its parent's start in a past range as well: one from before
  // the parent's start is in no range, and one across it is in a range after that start only.
  const ids = (start, end) => plain(compose({ ...past, start: T(start), end: T(end) }, ["default"], held)).sessions.map((item) => item.id);
  assert.deepEqual(ids(-90, -70), ["s1", "s0"]);
  assert.deepEqual(ids(-45, -32), ["s1", "s0"]);
  assert.deepEqual(ids(-35, -25), ["s1", "s1_subagent_across", "s0"]);
  // The Sub Agent Sessions whose metrics are read are the ones the range shows.
  assert.deepEqual(subAgentsInRange(live, parent, held.subAgents.get(parent.id)).map((item) => item.id), ["s1_subagent_inside"]);
});
