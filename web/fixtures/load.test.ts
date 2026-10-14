import { expect, test } from "vitest";
import { lastRuntimeRecordIn } from "../console/model.ts";
import { loadFixture } from "./load.ts";

// The data scripts, run and converted as the dev server does.
test("the fixture loads and converts for both scenarios", () => {
  const fixture = loadFixture("three-days-ago-1d");
  expect(fixture.scenario).toBe("three-days-ago-1d");
  expect(fixture.clock).toEqual({ now: Date.parse("2026-09-02T15:00:00+09:00"), timeZone: "Asia/Tokyo" });
  expect(fixture.range).toEqual({ mode: "historical", anchor: Date.parse("2026-08-30T15:00:00+09:00"), duration: 24 * 60 * 60 * 1000 });
  expect(loadFixture("unknown").scenario).toBe("three-days-ago-1h");
  expect(loadFixture("three-days-ago-1d")).toBe(fixture);
  // The density scenario opens with the Jobs unpinned; the others keep the default pins.
  expect(fixture.pinJobs).toBe(true);
  expect(loadFixture("message-density-30m").pinJobs).toBe(false);

  const { actors } = fixture;
  const keys = new Set(actors.actors.map((actor) => actor.id));
  expect(keys.has("research/researcher")).toBe(true);
  expect(actors.sessions.length).toBeGreaterThan(30);
  for (const session of actors.sessions) expect(keys.has(session.actor), session.id).toBe(true);
  for (const schedule of actors.schedules) expect(keys.has(schedule.actor)).toBe(true);
  for (const action of actors.actions) expect(keys.has(action.spawner) && keys.has(action.target)).toBe(true);
  for (const message of actors.messages) expect((message.from === "Admin" || keys.has(message.from)) && keys.has(message.to)).toBe(true);
  expect(actors.metrics.map((lane) => lane.id)).toContain("token-usage");
  expect(actors.consoleSessions).toBe(fixture.console.sessions);

  const d2 = actors.sessions.find((session) => session.id === "session_D2")!;
  if (d2.metrics.tokens.status !== "reported") throw new Error("session_D2 reports tokens");
  const total = d2.metrics.tokens.samples.reduce((sum, sample) => sum + sample.value.reduce((a, b) => a + b, 0), 0);
  expect(Math.round(total)).toBe([[33, 14, 64, 9], [86, 37, 163, 24], [72, 31, 137, 20], [114, 49, 215, 32], [50, 21, 95, 14]].flat().reduce((a, b) => a + b, 0));

  expect(fixture.console.recordings.get("session_FP5")!.id).toBe(fixture.console.recordings.get("session_D2")!.id);
  expect(fixture.console.logs.get("session_D0")!.lines.length).toBeGreaterThan(0);
  expect(fixture.daemon("degraded").status!.otlp).toBeNull();
  expect(fixture.daemon("unreachable").phase).toBe("unreachable");
});

// Every Session the Actors page draws opens a Console: the density scenario's synthetic
// Sessions included, which show Feld's own records.
test("every fixture Session has a Console", () => {
  const fixture = loadFixture("message-density-30m");
  const consoles = new Map(fixture.console.sessions.map((session) => [session.id, session]));
  for (const session of fixture.actors.sessions) expect(consoles.has(session.id), session.id).toBe(true);
  const density = fixture.actors.sessions.filter((session) => session.id.startsWith("session_density_"));
  expect(density).toHaveLength(35);
  for (const session of density) expect(consoles.get(session.id)!.console).toBe("none");
});

// The turn each current agent-runtime Session carries is the one its Console's last record
// gives, at the instant the Console shows that record.
test("every current agent-runtime Session's turn agrees with its Console", () => {
  const fixture = loadFixture("three-days-ago-1h");
  const current = fixture.actors.sessions.filter((session) => session.end === null && !session.parentSession && session.execution.type === "agent-runtime");
  expect(current.length).toBeGreaterThan(0);
  for (const session of current) {
    const recording = fixture.console.recordings.get(session.id) || null;
    const origin = session.recordingOrigin || recording?.startedAt;
    const last = lastRuntimeRecordIn(recording, origin ? { recordingInstant: Date.parse(origin), sessionInstant: session.start } : null);
    const turn = !last ? "unavailable" : last.kind === "response" ? "waiting-input" : "responding";
    expect([session.turn, session.lastEntryAt], session.id).toEqual([turn, last ? last.at : null]);
  }
  expect(current.some((session) => session.turn !== "unavailable")).toBe(true);
});
