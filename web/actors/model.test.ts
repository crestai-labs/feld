import { expect, test } from "vitest";
import type { ConsoleReads } from "../console/reads.ts";
import { DAY, HOUR, MINUTE, actorColumnWidthsOf, actorColumns, actorColumnsWidth, clampActorColumnWidth, axisLabels, axisTicks, consoleSessionOf, createModel, metricsQueryOf, metricsRangeOf, withAccountTones, laneSeriesName, listedSeriesOrder, cronError, draftLaunchSpec, emptyActorDraft, followsLive, gridLines, isMidnight, orderWithMove, pinsWith, executionDetailFor, formatDuration, launchChoice, metricSampleTimes, metricValueAt, seriesSampledAt, nextCronInstant, placementRows, quotaReporterAt, readingMarkerAt, validateActorDraft, validateEditDraft, envText, launchSpecPatchOf, maskSecrets, parseEnv, editDraftOf, SECRET_MASK } from "./model.ts";
import { createInitialState } from "./store.ts";
import type { ActorRecord, ActorsInput, AgentSession, AgentsState, MessageRecord, MetricLane, MetricSeries, ScheduleRecord } from "./types.ts";

// The rules the Actors page derives by: outcome, attention, next run, turn, and the Now
// section (docs/spec/concepts/glossary.md, docs/guide/webui.md).

const NOW = Date.parse("2026-09-25T00:30:00Z");

function actor(name: string, kind: FeldApi.ActorKind = "agent"): ActorRecord {
  return { id: `default/${name}`, name, workspace: "default", kind, launchSpec: { runtime: "claude-code", workdir: "/work", provider: "tmux", node: "local" } };
}

function session(fields: Partial<AgentSession> & { id: string; actor: string }): AgentSession {
  return {
    workspace: "default",
    start: NOW - HOUR,
    end: null,
    planned: null,
    startKind: "start",
    ownership: "owned",
    endKind: fields.end ? "exited" : "running",
    exitCode: null,
    execution: { type: "agent-runtime", label: "Claude Code", command: "claude", ai: true },
    model: ["Claude Code", "claude-opus"],
    linked: true,
    placement: { provider: "tmux", node: "local", workdir: "/work", summary: "feld:1", attachCommand: null },
    lifecycle: { startSettled: NOW - HOUR + 1000 },
    observation: { kind: fields.end ? "gone" : "running", at: NOW - MINUTE },
    metrics: { tokens: { status: "n/a", source: "test" }, context: { status: "n/a", source: "test" } },
    laneMetrics: {},
    ...fields,
  };
}

function input(fields: Partial<ActorsInput>): ActorsInput {
  return { workspaces: ["default"], defaultWorkspace: "default", actors: [], sessions: [], schedules: [], scheduleSkips: [], actions: [], messages: [], metrics: [], launchOptions: { runtimes: [], providers: [], nodes: [] }, ...fields };
}

function reads(fields: Partial<ConsoleReads> = {}): ConsoleReads {
  return {
    recordingFor: () => null,
    runtimeRecordFor: () => null,
    lastRuntimeRecordFor: () => null,
    lastLogLineFor: () => null,
    feldRecordsFor: () => null,
    consoleSessionFor: (session) => session,
    ...fields,
  };
}

function modelOf(fields: Partial<ActorsInput>, consoleReads = reads(), timeZone = "UTC") {
  return createModel(input(fields), { now: NOW, timeZone }, consoleReads);
}

function state(fields: Partial<AgentsState> = {}): AgentsState {
  return { ...createInitialState(undefined, NOW), ...fields };
}

// ---- outcome -------------------------------------------------------------------------

test("a Session with no end is running", () => {
  const model = modelOf({ actors: [actor("dev")] });
  const outcome = model.outcomeFor(session({ id: "s1", actor: "default/dev" }));
  expect(outcome.key).toBe("running");
  expect(outcome.attention).toBe(false);
});

test("an end after a stop or kill request is stopped whatever the kind and the exit code", () => {
  const model = modelOf({ actors: [actor("job", "job")] });
  const outcome = model.outcomeFor(session({ id: "s1", actor: "default/job", end: NOW, endKind: "stopped", exitCode: 143, lifecycle: { stop: { accepted: NOW - 1000, requesterSessionId: null, attempt: "SIGTERM", observed: "gone" } } }));
  expect(outcome.key).toBe("stopped");
  expect(outcome.attention).toBe(false);
});

test("an owned Job ending on its own is succeeded on exit 0 and failed otherwise", () => {
  const model = modelOf({ actors: [actor("job", "job")] });
  const succeeded = model.outcomeFor(session({ id: "s1", actor: "default/job", end: NOW, exitCode: 0 }));
  const failed = model.outcomeFor(session({ id: "s2", actor: "default/job", end: NOW, exitCode: 2 }));
  expect([succeeded.key, succeeded.attention]).toEqual(["succeeded", false]);
  expect([failed.key, failed.attention]).toEqual(["failed", true]);
});

test("an owned Agent ending without a request is exited, even with exit 0", () => {
  const model = modelOf({ actors: [actor("dev")] });
  const outcome = model.outcomeFor(session({ id: "s1", actor: "default/dev", end: NOW, exitCode: 0 }));
  expect(outcome.key).toBe("exited");
  expect(outcome.attention).toBe(true);
});

test("an attached Session or a Sub Agent that ended is ended whatever the kind", () => {
  const model = modelOf({ actors: [actor("dev"), actor("job", "job")] });
  const attachedJob = model.outcomeFor(session({ id: "s1", actor: "default/job", ownership: "attached", startKind: "attach", end: NOW, exitCode: 1 }));
  const attachedAgent = model.outcomeFor(session({ id: "s2", actor: "default/dev", ownership: "attached", startKind: "attach", end: NOW, exitCode: 0 }));
  const subAgent = model.outcomeFor(session({ id: "s3", actor: "default/dev", parentSession: "s2", subAgent: "Explore", startKind: "sub-agent", ownership: null, endKind: null, end: NOW }));
  expect([attachedJob.key, attachedAgent.key, subAgent.key]).toEqual(["ended", "ended", "ended"]);
  expect([attachedJob.attention, attachedAgent.attention, subAgent.attention]).toEqual([false, false, false]);
});

test("the Session line is dashed only from the instant Feld lost sight of the execution", () => {
  const model = modelOf({ actors: [actor("dev"), actor("job", "job")] });
  const lostOpen = session({ id: "s1", actor: "default/dev", observation: { kind: "lost", since: NOW - 10 * MINUTE, at: NOW - MINUTE } });
  const closedUnknown = session({ id: "s2", actor: "default/dev", end: NOW, endKind: "unknown", observation: { kind: "mismatched", since: NOW - 20 * MINUTE, at: NOW - MINUTE } });
  const exitedWithoutCode = session({ id: "s3", actor: "default/job", end: NOW });
  const subAgent = session({ id: "s4", actor: "default/dev", parentSession: "s1", subAgent: "Explore", startKind: "sub-agent", ownership: null, endKind: null, observation: null });
  expect(model.lostSince(lostOpen)).toBe(NOW - 10 * MINUTE);
  expect(model.lostSince(closedUnknown)).toBe(NOW - 20 * MINUTE);
  expect(model.outcomeFor(exitedWithoutCode).key).toBe("unknown");
  expect(model.lostSince(exitedWithoutCode)).toBeNull();
  expect(model.lostSince(subAgent)).toBeNull();
});

// ---- execution ------------------------------------------------------------------------

test("a Session with no recorded command is read by its ownership, not as attached", () => {
  const codex: AgentSession["execution"] = { type: "agent-runtime", label: "Codex", command: null, ai: true };
  const owned = session({ id: "s1", actor: "default/dev", execution: codex });
  const attached = session({ id: "s2", actor: "default/dev", ownership: "attached", startKind: "attach", execution: codex });
  const subAgent = session({ id: "s3", actor: "default/dev", parentSession: "s2", subAgent: "Explore", startKind: "sub-agent", ownership: null, endKind: null, execution: codex });
  expect(executionDetailFor(owned)).toBe("- · owned: Feld started the Runtime via the Codex Adapter; command is Adapter-managed");
  expect(executionDetailFor(attached)).toBe("- · attached: Feld did not start it and records no command");
  expect(executionDetailFor(subAgent)).not.toContain("attached");
  expect(executionDetailFor(session({ id: "s4", actor: "default/dev" }))).toBe("claude");
  // The start record's Execution fact agrees with its Ownership fact.
  const model = modelOf({ actors: [actor("dev")], sessions: [owned, attached] });
  const executionFact = (target: AgentSession) => model.feldRecordsFor(target, state()).find((row) => row.type === "session.started")!.facts.find((fact) => fact.label === "Execution")!;
  expect(executionFact(owned).detail).toBe("From the Actor's runtime; Feld started the execution via the Codex Adapter, which manages the command");
  expect(executionFact(attached).detail).toContain("Feld did not start the execution");
});

test("the Console names an agent-runtime Session's Runtime, and no Runtime for a program Session", () => {
  const codex: AgentSession["execution"] = { type: "agent-runtime", label: "Codex", command: null, ai: true };
  expect(consoleSessionOf(session({ id: "s1", actor: "default/dev", execution: codex })).runtimeName).toBe("Codex");
  expect(consoleSessionOf(session({ id: "s2", actor: "default/dev" })).runtimeName).toBe("Claude Code");
  const program: AgentSession["execution"] = { type: "program", label: "Program", command: "make", ai: false };
  expect(consoleSessionOf(session({ id: "s3", actor: "default/job", execution: program })).runtimeName).toBeUndefined();
});

test("a Spawn is on the Consoles of both the spawner's Session and the Session it created, each opening the other", () => {
  const parent = session({ id: "s1", actor: "default/dispatcher" });
  const child = session({ id: "s2", actor: "default/dev", start: NOW - 30 * MINUTE });
  const action = { id: "action_s2", spawner: "default/dispatcher", spawnerWorkspace: "default", spawnerSessionId: "s1", target: "default/dev", targetWorkspace: "default", session: "s2", at: NOW - 30 * MINUTE };
  const model = modelOf({ actors: [actor("dispatcher"), actor("dev")], sessions: [parent, child], actions: [action] });
  const spawns = (target: AgentSession) => model.feldRecordsFor(target, state()).filter((row) => row.type === "spawned").map((row) => [row.title, row.links.map((link) => `${link.sessionId}#${link.recordId}`)]);
  expect(spawns(parent)).toEqual([["Spawned dev", ["s2#action_s2"]]]);
  expect(spawns(child)).toEqual([["Spawned by dispatcher", ["s1#action_s2"]]]);
  expect(model.recordConsoleTarget("action", "action_s2")).toEqual({ sessionId: "s1", recordId: "action_s2" });

  // A Spawn outside the range read comes from the spawner Session's own records.
  const record = { sequence: 4, at: new Date(NOW - 2 * HOUR).toISOString(), kind: "spawned", actor: { id: "a2", workspace: "default", name: "dev" }, sessionId: "s0", data: { spawner: { type: "actor", workspace: "default", name: "dispatcher" }, spawnerSessionId: "s1" } } as unknown as FeldApi.FeldRecord;
  const reading = modelOf({ actors: [actor("dispatcher"), actor("dev")], sessions: [parent, child], actions: [action] }, reads({ feldRecordsFor: (id) => id === "s1" ? [record] : null }));
  expect(reading.feldRecordsFor(parent, state()).filter((row) => row.type === "spawned").map((row) => row.id)).toEqual(["action_s0", "action_s2"]);
});

test("an unproven end, an owned Job with no exit code, and a failed start are unknown", () => {
  const model = modelOf({ actors: [actor("dev"), actor("job", "job")] });
  const unproven = model.outcomeFor(session({ id: "s1", actor: "default/dev", ownership: "attached", end: NOW, endKind: "unknown" }));
  const noCode = model.outcomeFor(session({ id: "s2", actor: "default/job", end: NOW, exitCode: null }));
  const failedStart = model.outcomeFor(session({ id: "s3", actor: "default/job", end: NOW, lifecycle: { startFailed: NOW - HOUR }, observation: { kind: "gone", at: NOW - HOUR, detail: "tmux: no server" } }));
  expect([unproven.key, noCode.key, failedStart.key]).toEqual(["unknown", "unknown", "unknown"]);
  expect([unproven.attention, noCode.attention, failedStart.attention]).toEqual([true, true, true]);
  // A start that never began is shown as Start failed, with the observation's detail.
  expect(failedStart.label).toBe("Start failed");
  expect(failedStart.detail).toBe("tmux: no server");
  // The end marker's tooltip names why in a line: the Provider's detail, or the exit code.
  expect([unproven.reason, noCode.reason, failedStart.reason]).toEqual(["the Provider could not prove the end", "no exit code reported", "tmux: no server"]);
});

// ---- attention -------------------------------------------------------------------------

test("attention reads the Actor's latest top-level Session only", () => {
  const recovered = modelOf({
    actors: [actor("job", "job")],
    sessions: [
      session({ id: "s1", actor: "default/job", start: NOW - 3 * HOUR, end: NOW - 2 * HOUR, exitCode: 1 }),
      session({ id: "s2", actor: "default/job", start: NOW - HOUR, end: NOW - 30 * MINUTE, exitCode: 0 }),
    ],
  });
  expect(recovered.attentionReasons("default/job")).toEqual([]);
  const failing = modelOf({
    actors: [actor("job", "job")],
    sessions: [
      session({ id: "s1", actor: "default/job", start: NOW - 3 * HOUR, end: NOW - 2 * HOUR, exitCode: 0 }),
      session({ id: "s2", actor: "default/job", start: NOW - HOUR, end: NOW - 30 * MINUTE, exitCode: 1 }),
    ],
  });
  expect(failing.attentionReasons("default/job")).toHaveLength(1);
  expect(failing.attentionReasons("default/job")[0]).toMatch(/^s2 failed/);
});

test("a Sub Agent Session does not stand for its Actor in attention", () => {
  const model = modelOf({
    actors: [actor("dev")],
    sessions: [
      session({ id: "s1", actor: "default/dev" }),
      session({ id: "s1-sub", actor: "default/dev", parentSession: "s1", subAgent: "Explore", startKind: "sub-agent", ownership: null, endKind: null, start: NOW - 10 * MINUTE, end: NOW - MINUTE, observation: null }),
    ],
  });
  expect(model.latestSessionFor("default/dev")?.id).toBe("s1");
  expect(model.attentionReasons("default/dev")).toEqual([]);
});

test("a current Session observed lost or mismatched needs attention; running does not", () => {
  for (const kind of ["lost", "mismatched"] as const) {
    const model = modelOf({ actors: [actor("dev")], sessions: [session({ id: "s1", actor: "default/dev", observation: { kind, at: NOW - MINUTE } })] });
    expect(model.attentionReasons("default/dev")).toEqual([`s1 observed ${kind} (Provider observation)`]);
  }
  const running = modelOf({ actors: [actor("dev")], sessions: [session({ id: "s1", actor: "default/dev" })] });
  expect(running.attentionReasons("default/dev")).toEqual([]);
});

test("an ended Session's last observation is not an attention reason", () => {
  const model = modelOf({ actors: [actor("dev")], sessions: [session({ id: "s1", actor: "default/dev", end: NOW, endKind: "stopped", observation: { kind: "lost", at: NOW }, lifecycle: { stop: { accepted: NOW - 1000, requesterSessionId: null, attempt: "SIGTERM", observed: "lost" } } })] });
  expect(model.attentionReasons("default/dev")).toEqual([]);
});

test("unread mail and skipped ticks are marks, not attention", () => {
  const message: MessageRecord = { id: "m1", from: "Admin", fromWorkspace: null, fromSessionId: null, to: "default/job", toWorkspace: "default", toSessionId: "s1", accepted: NOW - 10 * MINUTE, read: null, anchor: null, preview: "hi", body: null };
  const schedule: ScheduleRecord = { actor: "default/job", cron: "*/5 * * * *", timezone: "UTC", enabled: true, nextRun: null };
  const model = modelOf({
    actors: [actor("job", "job")],
    sessions: [session({ id: "s1", actor: "default/job" })],
    messages: [message],
    schedules: [schedule],
    scheduleSkips: [{ id: "skip_1", actor: "default/job", planned: NOW - 5 * MINUTE, reason: "previous session current", previousSession: "s1" }],
  });
  expect(model.unreadMarkFor(actor("job", "job"), state())?.count).toBe(1);
  expect(model.skipMarkFor(actor("job", "job"))?.count).toBe(1);
  expect(model.attentionReasons("default/job")).toEqual([]);
});

test("an owned Runtime with no identity yet is marked unlinked, not attention", () => {
  const attach = { provider: "tmux", node: "local", workdir: "/work", summary: "feld:1", attachCommand: "tmux -L feld attach-session -t $0" };
  const model = modelOf({
    actors: [actor("dev"), actor("linked"), actor("attached"), actor("ended")],
    sessions: [
      session({ id: "s1", actor: "default/dev", model: ["Claude Code", null], linked: false, placement: attach }),
      session({ id: "s2", actor: "default/linked" }),
      session({ id: "s3", actor: "default/attached", ownership: "attached", linked: false }),
      session({ id: "s4", actor: "default/ended", end: NOW, linked: false }),
    ],
  });
  expect(model.unlinkedMarkFor(actor("dev"))?.title).toContain("answer it there: tmux -L feld attach-session -t $0");
  expect(model.attentionReasons("default/dev")).toEqual([]);
  expect(model.unlinkedMarkFor(actor("linked"))).toBeNull();
  expect(model.unlinkedMarkFor(actor("attached"))).toBeNull();
  expect(model.unlinkedMarkFor(actor("ended"))).toBeNull();
});

// ---- next run --------------------------------------------------------------------------

test("a paused Schedule has no next run", () => {
  const model = modelOf({});
  expect(model.nextRunFor({ actor: "default/job", cron: "* * * * *", timezone: "UTC", enabled: false, nextRun: NOW + MINUTE })).toBeNull();
  expect(model.nextRunFor(null)).toBeNull();
});

test("the daemon's next run is taken where the record carries one", () => {
  const model = modelOf({});
  const daemonAnswer = NOW + 7 * MINUTE;
  expect(model.nextRunFor({ actor: "default/job", cron: "0 0 * * *", timezone: "UTC", enabled: true, nextRun: daemonAnswer })).toBe(daemonAnswer);
});

test("without the daemon's answer the cron is evaluated in the Schedule's timezone from now", () => {
  const model = modelOf({});
  // 00:30 UTC is 09:30 in Tokyo, so the next 09:00 there is tomorrow's.
  expect(model.nextRunFor({ actor: "default/job", cron: "0 9 * * *", timezone: "Asia/Tokyo", enabled: true, nextRun: null })).toBe(Date.parse("2026-09-26T00:00:00Z"));
  expect(model.nextRunFor({ actor: "default/job", cron: "13,43 * * * *", timezone: "UTC", enabled: true, nextRun: null })).toBe(Date.parse("2026-09-25T00:43:00Z"));
  // The first tick is strictly after now.
  expect(nextCronInstant("30 0 * * *", "UTC", NOW)).toBe(Date.parse("2026-09-26T00:30:00Z"));
});

test("the cron reads 7 as Sunday and the month and day-of-week names as the daemon does", () => {
  // NOW is Friday 2026-09-25.
  const sunday = Date.parse("2026-09-27T09:00:00Z");
  expect(nextCronInstant("0 9 * * 7", "UTC", NOW)).toBe(sunday);
  expect(nextCronInstant("0 9 * * SUN", "UTC", NOW)).toBe(sunday);
  expect(nextCronInstant("0 9 * * MON", "UTC", NOW)).toBe(Date.parse("2026-09-28T09:00:00Z"));
  expect(nextCronInstant("0 9 * * tues,Thursday", "UTC", NOW)).toBe(Date.parse("2026-09-29T09:00:00Z"));
  // `N/step` runs to 7: 1/2 is Monday, Wednesday, Friday, and Sunday.
  expect(nextCronInstant("0 9 * * 1/2", "UTC", Date.parse("2026-09-26T09:00:00Z"))).toBe(sunday);
  expect(nextCronInstant("0 9 1 JAN *", "UTC", NOW)).toBe(Date.parse("2027-01-01T09:00:00Z"));
  expect(nextCronInstant("0 9 1 feb-mar *", "UTC", NOW)).toBe(Date.parse("2027-02-01T09:00:00Z"));
  // A month name stands alone or bounds a range of two names.
  for (const cron of ["0 9 * * 8", "0 9 * * FOO", "0 9 1 FEB/2 *", "0 9 1 1-MAR *"]) expect(nextCronInstant(cron, "UTC", NOW)).toBeNull();
});

test("the cron accepts and refuses what the daemon does", () => {
  // NOW is Friday 2026-09-25. `?` is `*` in the day fields.
  const nine = Date.parse("2026-09-25T09:00:00Z");
  expect(nextCronInstant("0 9 * * ?", "UTC", NOW)).toBe(nine);
  expect(nextCronInstant("0 9 ? * *", "UTC", NOW)).toBe(nine);
  expect(nextCronInstant("0 9 ?/10 * *", "UTC", NOW)).toBe(Date.parse("2026-10-01T09:00:00Z"));
  // A step runs up to the field's maximum, except in day of week.
  expect(nextCronInstant("*/59 * * * *", "UTC", NOW)).toBe(Date.parse("2026-09-25T00:59:00Z"));
  expect(nextCronInstant("0 9 * * */8", "UTC", NOW)).toBe(Date.parse("2026-09-27T09:00:00Z"));
  for (const cron of ["0 9 * * *-3", "0 9 ?-3 * *", "0 9 * * ?/2", "0 9 * * 1,?", "0 ? * * *", "*/60 * * * *", "0 */24 * * *", "0 9 */32 * *", "0 9 1 */13 *"]) expect(nextCronInstant(cron, "UTC", NOW)).toBeNull();
});

test("the next run is searched as far as the daemon accepts an expression", () => {
  expect(nextCronInstant("0 0 29 FEB */7", "UTC", NOW)).toBe(Date.parse("2032-02-29T00:00:00Z"));
  expect(nextCronInstant("30 2 29 2 */7", "America/New_York", NOW)).toBe(Date.parse("2032-02-29T07:30:00Z"));
});

test("a cron restricting both day fields fires on days either allows", () => {
  // NOW is Friday 2026-09-25.
  expect(nextCronInstant("0 9 1 * MON", "UTC", NOW)).toBe(Date.parse("2026-09-28T09:00:00Z"));
  expect(nextCronInstant("0 9 1 * MON", "UTC", Date.parse("2026-09-28T10:00:00Z"))).toBe(Date.parse("2026-10-01T09:00:00Z"));
  expect(nextCronInstant("0 9 31 FEB MON", "UTC", NOW)).toBe(Date.parse("2027-02-01T09:00:00Z"));
  // A day field starting with `*` leaves both to be met.
  expect(nextCronInstant("0 9 */10 * MON", "UTC", NOW)).toBe(Date.parse("2026-12-21T09:00:00Z"));
});

test("the next run holds across a half-hour DST shift inside a UTC hour", () => {
  // Lord Howe goes from 02:00 LHST (+10:30) to 02:30 LHDT (+11) at 15:30Z.
  const shift = Date.parse("2026-10-03T15:30:00Z");
  expect(nextCronInstant("30 2 * * *", "Australia/Lord_Howe", Date.parse("2026-10-03T14:30:00Z"))).toBe(shift);
  expect(nextCronInstant("31 2 * * *", "Australia/Lord_Howe", Date.parse("2026-10-03T15:10:00Z"))).toBe(shift + MINUTE);
  expect(nextCronInstant("29,31 2 * * *", "Australia/Lord_Howe", Date.parse("2026-10-03T14:30:00Z"))).toBe(shift + MINUTE);
});

test("the cron error names the field the daemon refuses", () => {
  expect(cronError("0 9 * * 8")).toBe("Cron day of week field '8' must be 0-7 or SUN-SAT.");
  expect(cronError("*/60 * * * *")).toBe("Cron minute field '*/60' must be 0-59.");
  expect(cronError("0 9 * *")).toBe("Cron expression must have five fields (minute, hour, day of month, month, day of week).");
  expect(cronError("0 9 * * ?")).toBeNull();
});

test("a cron that never fires or cannot be read has no next run", () => {
  const model = modelOf({});
  expect(model.nextRunFor({ actor: "default/job", cron: "0 0 31 2 *", timezone: "UTC", enabled: true, nextRun: null })).toBeNull();
  expect(model.nextRunFor({ actor: "default/job", cron: "every hour", timezone: "UTC", enabled: true, nextRun: null })).toBeNull();
});

// ---- turn --------------------------------------------------------------------------------

function lastRecord(kind: FeldConsoleLastRecord["kind"]): FeldConsoleLastRecord {
  return { kind, label: kind, at: NOW - 30 * 1000, recordId: `runtime:${kind}` };
}

test("only a current agent-runtime Session that is not a Sub Agent has a turn", () => {
  const model = modelOf({ actors: [actor("dev")] });
  const waiting = { turn: "waiting-input" as const, lastEntryAt: NOW - 30 * 1000 };
  expect(model.turnFor(session({ id: "s1", actor: "default/dev", end: NOW, exitCode: 0, ...waiting }))).toBeNull();
  expect(model.turnFor(session({ id: "s2", actor: "default/dev", parentSession: "s3", subAgent: "Explore" }))).toBeNull();
  expect(model.turnFor(session({ id: "s4", actor: "default/dev", execution: { type: "program", label: "Program", command: "make", ai: false }, model: null }))).toBeNull();
  expect(model.turnFor(session({ id: "s5", actor: "default/dev", ...waiting }))).toBe("waiting-input");
});

test("the turn is the Session read's, whatever the Console has read", () => {
  // The Console's last record disagrees with the read: the read's turn stands.
  const model = modelOf({ actors: [actor("dev")] }, reads({ lastRuntimeRecordFor: () => lastRecord("response") }));
  expect(model.turnFor(session({ id: "s1", actor: "default/dev", turn: "responding", lastEntryAt: NOW - MINUTE }))).toBe("responding");
  // A current agent-runtime Session whose read carries no turn reads as unavailable.
  expect(model.turnFor(session({ id: "s2", actor: "default/dev" }))).toBe("unavailable");
});

test("the turn's instant is the Session read's, and the Console's record adds the tool when it is the same entry", () => {
  const call: FeldConsoleLastRecord = { kind: "tool-call", label: "Tool call", tool: "Bash", hasResult: false, at: NOW - 30 * 1000, recordId: "runtime:c1" };
  const model = modelOf({ actors: [actor("dev")] }, reads({ lastRuntimeRecordFor: () => call }));
  const responding = session({ id: "s1", actor: "default/dev", turn: "responding", lastEntryAt: NOW - 30 * 1000 });
  expect(model.currentStatusSummary(responding)).toBe("Running · tool call Bash, no result since 00:29");
  expect(model.turnMeaning(responding, "responding", model.turnRecordFor(responding))).toBe("The tool is still running, or the Runtime is stuck.");
  // Before the Console reads the entry the turn was read from, the turn stands on its own.
  const newer = session({ id: "s2", actor: "default/dev", turn: "responding", lastEntryAt: NOW - 10 * 1000 });
  expect(model.turnRecordFor(newer)).toBeNull();
  expect(model.currentStatusSummary(newer)).toBe("Running · responding, last entry 00:29");
  expect(model.turnMeaning(newer, "responding", null)).toBe("The Runtime is in the middle of a response: the model is working, or the Runtime is stuck.");
  const unread = modelOf({ actors: [actor("dev")] });
  expect(unread.currentStatusSummary(session({ id: "s3", actor: "default/dev", turn: "waiting-input", lastEntryAt: NOW - 5 * MINUTE }))).toBe("Running · waiting for input since 00:25");
  expect(unread.currentStatusSummary(session({ id: "s4", actor: "default/dev", turn: "unavailable", lastEntryAt: null }))).toBe("Running since 23:30");
});

test("an unavailable turn names what is known of the transcript file", () => {
  const model = modelOf({
    actors: [actor("dev")],
    consoleSessions: [{ id: "s1", actor: "dev", sourceUnavailable: "the container is gone" }, { id: "s2", actor: "dev", console: "missing" }],
  });
  expect(model.turnMeaning(session({ id: "s1", actor: "default/dev" }), "unavailable", null)).toBe("The transcript file cannot be read: the container is gone.");
  expect(model.turnMeaning(session({ id: "s2", actor: "default/dev" }), "unavailable", null)).toBe("No transcript file exists yet; the Runtime creates it at the first input.");
  expect(model.turnMeaning(session({ id: "s3", actor: "default/dev" }), "unavailable", null)).toBe("The Runtime Adapter has not delivered the transcript file's location.");
  // The Console's own read of the transcript file answers over the range reads'.
  const readByConsole = modelOf({ actors: [actor("dev")], sessions: [session({ id: "s4", actor: "default/dev" })] }, reads({ consoleSessionFor: (known) => ({ ...known, console: "missing" }) }));
  expect(readByConsole.turnMeaning(readByConsole.sessionById("s4")!, "unavailable", null)).toBe("No transcript file exists yet; the Runtime creates it at the first input.");
});

// ---- Now -----------------------------------------------------------------------------------

test("the Now section lists Process, Turn, Last entry, and Mailbox for a current Session", () => {
  const message: MessageRecord = { id: "m1", from: "Admin", fromWorkspace: null, fromSessionId: null, to: "default/dev", toWorkspace: "default", toSessionId: "s1", accepted: NOW - 10 * MINUTE, read: null, anchor: null, preview: "hi", body: null };
  const readMessage: MessageRecord = { ...message, id: "m0", accepted: NOW - 40 * MINUTE, read: { readId: "r1", sessionId: "s1", at: NOW - 35 * MINUTE, fifo: 1 } };
  const model = modelOf({ actors: [actor("dev")], messages: [message, readMessage] }, reads({ lastRuntimeRecordFor: () => ({ kind: "tool-call", label: "Tool call", tool: "Bash", hasResult: false, at: NOW - 30 * 1000, recordId: "runtime:c1" }) }));
  const rows = model.nowSectionFor(session({ id: "s1", actor: "default/dev", turn: "responding", lastEntryAt: NOW - 30 * 1000 }))!;
  expect(rows.map((row) => row.label)).toEqual(["Process", "Turn", "Last entry", "Mailbox"]);
  expect(rows[0].value).toMatch(/^running · observed /);
  expect(rows[1].value).toBe("responding");
  // A tool call with no result says so.
  expect(rows[2].value).toMatch(/^Tool call · no result · /);
  expect(rows[2].openRecordId).toBe("runtime:c1");
  // The waiting count, the oldest waiting acceptance, and the last read of the Actor's mail.
  expect(rows[3].value).toBe("1 waiting · oldest accepted 00:20 · last read 23:55");
  expect(rows.every((row) => row.meaning.length > 0)).toBe(true);
});

test("the Now section shows a Stop row only for an accepted stop or kill", () => {
  const model = modelOf({ actors: [actor("dev")] });
  const stopping = model.nowSectionFor(session({ id: "s1", actor: "default/dev", lifecycle: { stop: { accepted: NOW - 20 * 1000, requesterSessionId: null, attempt: "SIGTERM", observed: "running" } } }))!;
  expect(stopping.filter((row) => row.label === "Stop").map((row) => row.value)).toEqual(["feld session stop accepted 00:29:40"]);
  const quiet = model.nowSectionFor(session({ id: "s2", actor: "default/dev" }))!;
  expect(quiet.some((row) => row.label === "Stop")).toBe(false);
});

test("a program Session's Now section has no Turn and reads its last output line", () => {
  const model = modelOf({ actors: [actor("job", "job")] }, reads({ lastLogLineFor: () => ({ kind: "stdout", label: "stdout", at: NOW - 5000, recordId: "log:1" }) }));
  const rows = model.nowSectionFor(session({ id: "s1", actor: "default/job", execution: { type: "program", label: "Program", command: "make", ai: false }, model: null }))!;
  expect(rows.map((row) => row.label)).toEqual(["Process", "Last entry", "Mailbox"]);
  expect(rows[1].meaning).toBe("The last line the program wrote; a program can work without writing.");
});

test("a current Session the Provider has not observed yet reads as running, not observed", () => {
  const unobserved = session({ id: "s1", actor: "default/dev", lifecycle: {}, observation: null, turn: "responding", lastEntryAt: NOW - 5000 });
  const model = modelOf({ actors: [actor("dev")], sessions: [unobserved] }, reads({ lastRuntimeRecordFor: () => ({ kind: "input", label: "Input", at: NOW - 5000, recordId: "runtime:i1" }) }));
  expect(model.actorStateFor(actor("dev"))).toMatchObject({ key: "running", title: "Running: current Session s1 (Control Plane fact). No Provider observation yet" });
  expect(model.currentStatusSummary(unobserved)).toMatch(/^Running \(not observed yet\) · responding/);
  expect(model.observationEvidence(unobserved)).toMatchObject({ value: "not observed", detail: "no Provider observation yet" });
  const process = model.nowSectionFor(unobserved)!.find((row) => row.label === "Process")!;
  expect(process).toEqual({ label: "Process", value: "not observed yet", meaning: "The Provider has not observed the recorded resource yet." });
  expect(model.attentionReasons("default/dev")).toEqual([]);
});

test("a Session closed as unknown before any observation says it was never observed", () => {
  const closed = session({ id: "s1", actor: "default/dev", end: NOW - MINUTE, endKind: "unknown", lifecycle: {}, observation: null });
  const model = modelOf({ actors: [actor("dev")], sessions: [closed] });
  const end = model.feldRecordsFor(closed, state()).find((record) => record.type === "session.ended")!;
  expect(end.summary).toBe("closed as unknown when the next start request was admitted · never observed");
});

test("an ended Session and a Sub Agent have no Now section", () => {
  const model = modelOf({ actors: [actor("dev")] });
  expect(model.nowSectionFor(session({ id: "s1", actor: "default/dev", end: NOW, exitCode: 0 }))).toBeNull();
  expect(model.nowSectionFor(session({ id: "s2", actor: "default/dev", parentSession: "s1", subAgent: "Explore" }))).toBeNull();
});

// ---- Timeline: Sub Agent Sessions -------------------------------------------------------------

test("a Sub Agent Session is drawn from its parent's start, in the parent's band, Live and in History", () => {
  const parent = session({ id: "p1", actor: "default/dev", ownership: "attached", startKind: "attach", start: NOW - HOUR });
  const straddling = session({ id: "sub1", actor: "default/dev", parentSession: "p1", subAgent: "Plan", startKind: "sub-agent", ownership: null, endKind: null, start: NOW - 2 * HOUR, end: NOW - 50 * MINUTE, observation: null });
  const before = session({ id: "sub2", actor: "default/dev", parentSession: "p1", subAgent: "Explore", startKind: "sub-agent", ownership: null, endKind: null, start: NOW - 3 * HOUR, end: NOW - 2 * HOUR, observation: null });
  const model = modelOf({ actors: [actor("dev")], sessions: [parent, straddling, before] });
  expect(model.timelineStart(straddling)).toBe(parent.start);
  for (const range of [state({ mode: "live", duration: 3 * HOUR }), state({ mode: "historical", anchor: NOW - 90 * MINUTE, duration: 3 * HOUR })]) {
    const drawn = model.subAgentSessionsFor("default/dev", range).map((candidate) => candidate.id);
    expect(drawn).toEqual(["sub1"]);
    expect(model.overlaps(before, model.rangeFor(range))).toBe(false);
    const lane = model.laneLayoutFor([actor("dev")], range).agents.get("default/dev")!;
    expect(lane.sessionY.get("sub1")).toBeGreaterThan(lane.mainY);
    expect(lane.sessionY.has("sub2")).toBe(false);
  }
});

test("hidden Sub Agents take no row in the band", () => {
  const parent = session({ id: "p1", actor: "default/dev" });
  const sub = session({ id: "sub1", actor: "default/dev", parentSession: "p1", subAgent: "Plan", startKind: "sub-agent", ownership: null, endKind: null, start: NOW - 30 * MINUTE, end: NOW - 20 * MINUTE, observation: null });
  const model = modelOf({ actors: [actor("dev")], sessions: [parent, sub] });
  expect(model.bandRowsFor("default/dev", { showSubAgents: false }).count).toBe(1);
  expect(model.bandRowsFor("default/dev", { showSubAgents: true }).count).toBe(2);
});

// ---- The Actor table ---------------------------------------------------------------------------

test("the Actor table keeps the selected columns in their order, and its width is theirs", () => {
  const selected = { visibleActorDetails: new Set(["schedule", "status"]), actorColumnWidths: {} };
  expect(actorColumns(selected).map((option) => option.id)).toEqual(["status", "schedule"]);
  expect(actorColumnsWidth(selected)).toBe(176 + 196);
  expect(actorColumnsWidth({ visibleActorDetails: new Set(), actorColumnWidths: {} })).toBe(0);
});

test("a dragged column width counts only while its column is shown", () => {
  const actorColumnWidths = { status: 240, execution: 200 };
  expect(actorColumnsWidth({ visibleActorDetails: new Set(["status", "schedule"]), actorColumnWidths })).toBe(240 + 196);
  expect(actorColumnsWidth({ visibleActorDetails: new Set(["execution"]), actorColumnWidths })).toBe(200);
});

test("stored column widths keep only known columns within their limits", () => {
  expect(actorColumnWidthsOf({ status: 240, "coding-agent": 20, execution: "wide", schedule: 400, gone: 100 })).toEqual({ status: 240 });
  expect(actorColumnWidthsOf(null)).toEqual({});
  expect(actorColumnWidthsOf([180])).toEqual({});
  expect(clampActorColumnWidth("status", 1000)).toBe(320);
  expect(clampActorColumnWidth("execution", 10)).toBe(80);
});

test("an Actor table cell reads the latest top-level Session", () => {
  const model = modelOf({
    actors: [actor("dev"), actor("job", "job"), actor("idle")],
    sessions: [
      session({ id: "s1", actor: "default/dev" }),
      session({ id: "s2", actor: "default/job", execution: { type: "program", label: "Program", command: "make", ai: false }, model: null }),
    ],
  });
  const dev = actor("dev");
  expect(model.actorDetailCellFor(dev, "status")).toMatchObject({ kind: "status", key: "running", label: "Running", when: "since 23:30" });
  expect(model.actorDetailCellFor(dev, "coding-agent")).toMatchObject({ kind: "text", value: "Claude Code" });
  expect(model.actorDetailCellFor(dev, "execution")).toMatchObject({ kind: "text", value: "tmux" });
  expect(model.actorDetailCellFor(actor("job", "job"), "coding-agent")).toBeNull();
  expect(model.actorDetailCellFor(actor("idle"), "status")).toMatchObject({ kind: "none" });
  expect(model.actorDetailCellFor(actor("idle"), "execution")).toBeNull();
});

test("the Actor-name glyph is running, stopped, or no Session, from the latest Session", () => {
  const model = modelOf({
    actors: [actor("dev"), actor("done"), actor("idle")],
    sessions: [
      session({ id: "s1", actor: "default/dev" }),
      session({ id: "s2", actor: "default/done", end: NOW - MINUTE }),
    ],
  });
  expect(model.actorStateFor(actor("dev"))).toMatchObject({ key: "running", title: expect.stringMatching(/^Running: current Session s1 /) });
  expect(model.actorStateFor(actor("done"))).toMatchObject({ key: "stopped", title: expect.stringMatching(/^Stopped: no current Session .* Latest Session s2 /) });
  expect(model.actorStateFor(actor("idle"))).toMatchObject({ key: "none", label: "No Session", title: "No Session recorded (Control Plane fact)" });
});

test("the Actor-name glyph's outer ring and tooltip read the Schedule and the current state", () => {
  const model = modelOf({
    actors: [actor("hourly", "job"), actor("paused", "job"), actor("dev")],
    sessions: [
      session({ id: "s1", actor: "default/hourly" }),
      session({ id: "s2", actor: "default/dev", end: NOW - MINUTE, execution: { type: "program", label: "Program", command: "make", ai: false }, model: null, exitCode: 1 }),
    ],
    schedules: [
      { actor: "default/hourly", cron: "0 * * * *", timezone: "UTC", enabled: true, nextRun: null },
      { actor: "default/paused", cron: "0 * * * *", timezone: "UTC", enabled: false, pausedAt: NOW - 10 * MINUTE, nextRun: null },
    ],
  });
  expect(model.actorStateFor(actor("hourly", "job"))).toMatchObject({ key: "running", schedule: "scheduled", lines: ["Scheduled (0 * * * * UTC) · Next run 01:00", "Now: Running (since 23:30)"] });
  expect(model.actorStateFor(actor("paused", "job"))).toMatchObject({ key: "none", schedule: "paused", lines: ["Schedule paused (0 * * * * UTC) · Paused since 00:20", "Now: No Session"] });
  expect(model.actorStateFor(actor("dev"))).toMatchObject({ key: "stopped", schedule: null, lines: ["No Schedule", expect.stringMatching(/^Now: .+ \(since 00:29\)$/)] });
});

// ---- The page's timezone -----------------------------------------------------------------------

test("instants are shown in the clock's timezone", () => {
  const model = modelOf({}, reads(), "Asia/Tokyo");
  expect(model.timeOnly.format(NOW)).toBe("09:30");
  expect(model.ZONE).toBe("JST");
  expect(model.toDateInput(NOW)).toBe("2026-09-25");
  expect(model.instantOfLocal("2026-09-25", "09:30")).toBe(NOW);
});

test("displayed instants follow the hour cycle, and a time input's value stays 24-hour", () => {
  const model = createModel(input({}), { now: NOW + 6 * HOUR, timeZone: "Asia/Tokyo", hourCycle: "h12" }, reads());
  expect(model.timeOnly.format(NOW + 6 * HOUR)).toBe("03:30 PM");
  expect(model.timeInput.format(NOW + 6 * HOUR)).toBe("15:30");
  expect(model.instantOfLocal("2026-09-25", "15:30")).toBe(NOW + 6 * HOUR);
});

test("a launchSpec choice the daemon does not offer moves to the first one it does", () => {
  const offered = [["codex", "Codex"]] as const;
  expect(launchChoice("claude-code", offered)).toBe("codex");
  expect(launchChoice("codex", offered)).toBe("codex");
  expect(launchChoice("claude-code", [])).toBe("claude-code");
});

test("the Attach row is shown only while the Session's resource can still be attached to", () => {
  const attachable = { provider: "tmux", node: "local", workdir: "/work", summary: "feld:1", attachCommand: "tmux attach-session -t $5" };
  const labels = (fields: Partial<AgentSession>) => placementRows(session({ id: "s", actor: "a", placement: attachable, ...fields })).map((row) => row.label);
  expect(labels({})).toContain("Attach");
  expect(labels({ end: NOW })).not.toContain("Attach");
  expect(labels({ observation: { kind: "gone", at: NOW } })).not.toContain("Attach");
});

test("a duration under a minute is shown in seconds", () => {
  expect(formatDuration(3_000)).toBe("3s");
  expect(formatDuration(59_999)).toBe("59s");
  expect(formatDuration(MINUTE)).toBe("1m");
});

test("the time axis ticks at round times of the display zone and keeps clear of the edges", () => {
  // 10:07–11:07 in Tokyo over 600 px: a tick every 10 minutes, the ones at the edges dropped.
  const range = { start: Date.parse("2026-09-25T01:07:00Z"), end: Date.parse("2026-09-25T02:07:00Z") };
  const ticks = axisTicks(range, 600, "Asia/Tokyo");
  expect(ticks.map((tick) => new Date(tick).toISOString().slice(11, 16))).toEqual(["01:10", "01:20", "01:30", "01:40", "01:50", "02:00"]);
  // Each labelled tick is a major grid line, with minor ones at 5 minutes between them.
  const lines = gridLines(range, 600, "Asia/Tokyo");
  expect(lines.filter((line) => line.major).map((line) => line.instant)).toEqual(expect.arrayContaining(ticks));
  expect(lines.length).toBe(12);
});

test("a day-long axis names midnight in the display zone", () => {
  const midnight = Date.parse("2026-09-25T15:00:00Z");
  expect(isMidnight(midnight, "Asia/Tokyo")).toBe(true);
  expect(isMidnight(midnight, "UTC")).toBe(false);
  const ticks = axisTicks({ start: midnight - 12 * HOUR, end: midnight + 12 * HOUR }, 600, "Asia/Tokyo");
  expect(ticks).toContain(midnight);
  expect(ticks.every((tick) => (tick - midnight) % (3 * HOUR) === 0)).toBe(true);
  expect(DAY % (3 * HOUR)).toBe(0);
});

test("the axis labels are one line: the date on the first tick and on each new day, the time on every tick", () => {
  const format = { date: (instant: number) => new Date(instant).toISOString().slice(5, 10), time: (instant: number) => new Date(instant).toISOString().slice(11, 16) };
  const measure = (text: string) => text.length * 6;
  const at = (clock: string) => Date.parse(`2026-09-29T${clock}:00Z`);
  const ticks = [at("20:00"), at("22:00"), Date.parse("2026-09-30T00:00:00Z"), Date.parse("2026-09-30T02:00:00Z")];
  const x = (instant: number) => 20 + (instant - ticks[0]) / HOUR * 60;
  const labels = axisLabels(ticks, x, 600, format, measure);
  expect(labels.map(({ date, time }) => [date, time])).toEqual([["09-29", "20:00"], [null, "22:00"], ["09-30", "00:00"], [null, "02:00"]]);
  // A label near the left edge stays inside the axis.
  expect(labels[0].left).toBe(0);
  // Ticks a day apart name only their date.
  const days = [0, 1, 2].map((day) => Date.parse("2026-09-29T00:00:00Z") + day * DAY);
  expect(axisLabels(days, (instant) => 40 + (instant - days[0]) / DAY * 100, 600, format, measure).map(({ date, time }) => [date, time]))
    .toEqual([["09-29", null], ["09-30", null], ["10-01", null]]);
});

test("a time label that would overlap a dated one is dropped, never the dated one", () => {
  const format = { date: (instant: number) => new Date(instant).toISOString().slice(5, 10), time: (instant: number) => new Date(instant).toISOString().slice(11, 16) };
  const measure = (text: string) => text.length * 6;
  const midnight = Date.parse("2026-09-30T00:00:00Z");
  const ticks = [-2, -1, 0, 1, 2].map((step) => midnight + step * 30 * MINUTE);
  const x = (instant: number) => 300 + (instant - midnight) / MINUTE * 1.5;
  const labels = axisLabels(ticks, x, 600, format, measure);
  expect(labels.find((label) => label.instant === midnight)?.date).toBe("09-30");
  expect(labels.map((label) => label.instant)).not.toContain(midnight - 30 * MINUTE);
  expect(labels.map((label) => label.instant)).not.toContain(midnight + 30 * MINUTE);
  const widths = labels.map((label) => ({ left: label.left, right: label.left + ((label.date ? label.date.length + 1 : 0) + (label.time?.length ?? 0)) * 6 }));
  widths.slice(1).forEach((label, index) => expect(label.left).toBeGreaterThanOrEqual(widths[index].right));
});

test("a new day keeps its date when the first tick's dated label would cover it", () => {
  const format = { date: (instant: number) => new Date(instant).toISOString().slice(5, 10), time: (instant: number) => new Date(instant).toISOString().slice(11, 16) };
  const measure = (text: string) => text.length * 6;
  const midnight = Date.parse("2026-09-30T00:00:00Z");
  // 23:00, 00:00, 01:00, 02:00 at 70 px apart: "09-29 23:00" and "09-30 00:00" (66 px each)
  // cannot both fit, so the first tick keeps only its time.
  const ticks = [-1, 0, 1, 2].map((hour) => midnight + hour * HOUR);
  const x = (instant: number) => 100 + (instant - ticks[0]) / HOUR * 70;
  expect(axisLabels(ticks, x, 600, format, measure).map(({ date, time }) => [date, time]))
    .toEqual([[null, "23:00"], ["09-30", "00:00"], [null, "01:00"], [null, "02:00"]]);
  // Closer still, the first tick's time is dropped and the new day still names its date.
  const near = (instant: number) => 100 + (instant - ticks[0]) / HOUR * 40;
  expect(axisLabels(ticks, near, 600, format, measure).find((label) => label.instant === midnight)?.date).toBe("09-30");
});

test("only a Live range that is not paused follows the daemon", () => {
  expect(followsLive({ mode: "live", paused: false })).toBe(true);
  expect(followsLive({ mode: "live", paused: true })).toBe(false);
  expect(followsLive({ mode: "historical", paused: false })).toBe(false);
});

test("the range button names a Live range by its length and a past one by its dates", () => {
  const model = modelOf({});
  expect(model.rangeButtonLabel({ mode: "live", anchor: NOW, duration: 3 * HOUR })).toBe("Last 3 hours");
  expect(model.rangeButtonLabel({ mode: "historical", anchor: NOW - 2 * HOUR, duration: HOUR })).toBe("Sep 24, 22:00–23:00 UTC");
});

test("the Actors with no Session in range are shown and hidden again by what the button says", () => {
  const model = modelOf({
    actors: [actor("busy"), actor("idle")],
    sessions: [session({ id: "s1", actor: "default/busy" })],
  });
  const hidden = state();
  expect(model.hiddenBandsFor(hidden, model.bandAgents(hidden))?.label).toBe("Show 1 Actor with no Sessions in range");
  const shown = state({ showAgentsOutsideRange: true });
  expect(model.hiddenBandsFor(shown, model.bandAgents(shown))?.label).toBe("Hide 1 Actor with no Sessions in range");
});

test("a reading names its time only when its samples stop before the end of the range", () => {
  const range = { start: NOW - 3 * HOUR, end: NOW };
  // A 90-second bucket that started before the last minute of the range still covers its end.
  const lane = (end: number): MetricLane => ({
    id: "tokens", name: "Token usage", caption: "", unit: "tokens/min", maximum: 100, defaultVisible: true, derivation: "test",
    series: [{ provider: "Claude Code", tone: "claude", source: "test", samples: [{ at: end - 90 * 1000, end, value: 5 }] }],
  });
  const model = modelOf({});
  const running = lane(NOW + 30 * 1000);
  expect(model.metricReadingFor(running, running.series[0], range).sampleTime).toBeNull();
  const stopped = lane(NOW - 10 * MINUTE);
  expect(model.metricReadingFor(stopped, stopped.series[0], range).sampleTime).toBe(`at ${model.timeOnly.format(NOW - 10 * MINUTE - 90 * 1000)}`);
});

test("a pinned Actor keeps its row whatever the range and sits above the unpinned ones", () => {
  const model = modelOf({
    actors: [actor("idle"), actor("busy"), actor("nightly", "job")],
    sessions: [session({ id: "s1", actor: "default/busy" }), session({ id: "s2", actor: "default/idle", start: NOW - 3 * DAY, end: NOW - 2 * DAY })],
  });
  const ids = (fields: Partial<AgentsState>) => model.visibleAgents(state(fields)).map((agent) => agent.id);
  // A Job starts pinned; an Agent only when the reader pins it.
  expect(ids({})).toEqual(["default/nightly", "default/busy"]);
  expect(ids({ actorPins: { "default/idle": true, "default/nightly": false } })).toEqual(["default/idle", "default/busy"]);
  // Within the pinned and the unpinned group, the dragged order comes first.
  expect(ids({ actorPins: { "default/busy": true }, actorOrder: ["default/busy"] })).toEqual(["default/busy", "default/nightly"]);
});

test("Actors alike in band and activity are ordered by name across Workspaces", () => {
  const model = modelOf({
    workspaces: ["default", "home"],
    actors: [actor("b-agent"), { ...actor("a-agent"), id: "home/a-agent", workspace: "home" }],
    sessions: [session({ id: "s1", actor: "default/b-agent" }), session({ id: "s2", actor: "home/a-agent", workspace: "home" })],
  });
  expect(model.visibleAgents(state({ workspace: null })).map((agent) => agent.id)).toEqual(["home/a-agent", "default/b-agent"]);
});

test("the pins hold only the changes to the default", () => {
  const job = actor("nightly", "job");
  const agent = actor("lead");
  expect(pinsWith({}, agent, true)).toEqual({ "default/lead": true });
  expect(pinsWith({ "default/lead": true }, agent, false)).toEqual({});
  expect(pinsWith({}, job, false)).toEqual({ "default/nightly": false });
  expect(pinsWith({ "default/nightly": false }, job, true)).toEqual({});
});

test("a dropped row lands beside its target, and Actors not shown keep their place after", () => {
  expect(orderWithMove(["x", "c"], ["a", "b", "c"], "c", "a", false)).toEqual(["c", "a", "b", "x"]);
  expect(orderWithMove([], ["a", "b", "c"], "a", "b", true)).toEqual(["b", "a", "c"]);
});

test("the search keeps a whole lane by its name and otherwise the series of the Runtimes it names", () => {
  const series = (provider: string) => ({ provider, tone: provider, source: "test", samples: [] });
  const metrics: MetricLane[] = [
    { id: "tokens", name: "Token usage", caption: "", unit: "tokens/min", maximum: 100, defaultVisible: true, derivation: "test", series: [series("Claude Code"), series("Codex")] },
    { id: "cost", name: "Cost", caption: "", unit: "$", maximum: 100, defaultVisible: true, derivation: "test", series: [series("Codex")] },
  ];
  const model = modelOf({ metrics });
  const lanes = (query: string) => model.visibleMetrics({ visibleMetricIds: new Set(["tokens", "cost"]), query }).map((lane) => [lane.metric.id, lane.series.map((item) => item.provider)]);
  expect(lanes("")).toEqual([["tokens", ["Claude Code", "Codex"]], ["cost", ["Codex"]]]);
  expect(lanes("token")).toEqual([["tokens", ["Claude Code", "Codex"]]]);
  expect(lanes("claude")).toEqual([["tokens", ["Claude Code"]]]);
  expect(model.visibleMetrics({ visibleMetricIds: new Set(["tokens"]), query: "claude" })[0].metric.series).toHaveLength(2);
  expect(lanes("zzz")).toEqual([]);
});

test("the search keeps the series of the accounts it names", () => {
  const account = (name: string) => ({ provider: "Claude Code", account: name, tone: "claude", source: "test", samples: [] });
  const quota: MetricLane = { id: "quota-5h", name: "5-hour quota", caption: "", unit: "%", maximum: 100, kind: "quota", level: true, defaultVisible: true, derivation: "test", series: [account("team@example.com"), account("personal@example.com")] };
  const model = modelOf({ metrics: [quota] });
  const series = (query: string) => model.visibleMetrics({ visibleMetricIds: new Set(["quota-5h"]), query }).flatMap((lane) => lane.series.map((item) => item.account));
  expect(series("personal")).toEqual(["personal@example.com"]);
  expect(series("claude")).toEqual(["team@example.com", "personal@example.com"]);
});

test("the Metrics window is never narrower than the Timeline's, centred on it, and ends at now at the latest", () => {
  const timeline = { start: NOW - 3 * DAY - HOUR, end: NOW - 3 * DAY };
  expect(metricsRangeOf(timeline, HOUR, NOW)).toBe(timeline);
  expect(metricsRangeOf(timeline, DAY, NOW)).toEqual({ start: NOW - 3 * DAY - 30 * MINUTE - 12 * HOUR, end: NOW - 3 * DAY - 30 * MINUTE + 12 * HOUR });
  expect(metricsRangeOf({ start: NOW - HOUR, end: NOW }, 7 * DAY, NOW)).toEqual({ start: NOW - 7 * DAY, end: NOW });
  const model = modelOf({});
  // A choice narrower than the Timeline reads as the Timeline's.
  expect(model.metricsRangeFor(state({ duration: DAY, metricsDuration: HOUR }))).toEqual({ start: NOW - DAY, end: NOW });
  expect(model.metricsButtonLabel(state({ metricsDuration: 7 * DAY }))).toBe("Last 7 days");
});

test("a Metrics window that ends at now is read as a Live one, so its read keeps its key while the clock moves", () => {
  const past = { mode: "historical" as const, anchor: NOW - 2 * HOUR, duration: HOUR, metricsDuration: 7 * DAY };
  expect(metricsQueryOf(past, NOW)).toEqual({ workspace: null, start: NOW - 7 * DAY, end: null, duration: 7 * DAY });
  // A window placed wholly in the past keeps its own instants.
  const older = { ...past, anchor: NOW - 3 * DAY, metricsDuration: DAY };
  expect(metricsQueryOf(older, NOW)).toEqual({ workspace: null, start: NOW - 3 * DAY - 12 * HOUR, end: NOW - 3 * DAY + 12 * HOUR, duration: DAY });
  expect(metricsQueryOf({ ...older, mode: "live" }, NOW)).toEqual({ workspace: null, start: NOW - DAY, end: null, duration: DAY });
  expect(metricsQueryOf({ ...older, metricsDuration: HOUR, duration: 30 * MINUTE }, NOW).duration).toBe(HOUR);
});

test("a quota lane lists its accounts highest first and folds past three", () => {
  const account = (name: string, value: number) => ({ provider: "Claude Code", account: name, tone: "claude", source: "test", samples: [{ at: NOW - 10 * MINUTE, end: NOW - 10 * MINUTE, until: NOW + HOUR, value }] });
  const quota: MetricLane = {
    id: "quota-5h", name: "5-hour quota", caption: "", unit: "%", maximum: 100, kind: "quota", level: true, defaultVisible: true, derivation: "test",
    series: [account("a", 10), account("b", 70), account("c", 40), account("d", 90)],
  };
  const model = modelOf({ metrics: [quota] });
  const listed = (expanded: string[]) => model.metricLanesFor(state({ visibleMetricIds: new Set(["quota-5h"]), expandedMetricIds: new Set(expanded) }))[0];
  const folded = listed([]);
  expect([folded.series.map((series) => series.account), folded.hiddenCount, folded.height]).toEqual([["d", "b", "c"], 1, 88]);
  const expanded = listed(["quota-5h"]);
  expect([expanded.series.map((series) => series.account), expanded.hiddenCount, expanded.height]).toEqual([["d", "b", "c", "a"], 0, 104]);
  expect(model.metricReadingFor(quota, quota.series[0], model.metricsRangeFor(state())).resetTime).toBe(`resets ${model.timeOnly.format(NOW + HOUR)}`);
});

test("each account of a Runtime with several keeps one colour on every lane, and a single account keeps the Runtime's", () => {
  const series = (provider: string, account: string) => ({ provider, account, tone: provider, source: "test", samples: [{ at: NOW, end: NOW, value: 1 }] });
  const lane = (id: string, items: ReturnType<typeof series>[]): MetricLane => ({ id, name: id, caption: "", unit: "%", maximum: 100, kind: "quota", defaultVisible: true, derivation: "test", series: items });
  const lanes = withAccountTones([
    lane("5h", [series("claude", "team"), series("claude", "personal"), series("codex", "team")]),
    lane("7d", [series("claude", "personal"), series("claude", "team")]),
  ]);
  expect(lanes.map((item) => item.series.map((entry) => entry.accountTone ?? entry.tone))).toEqual([["account-1", "account-2", "codex"], ["account-2", "account-1"]]);
});

test("a reading's marker sits on its line: a rate reading at the end of its bin, a level one at its sample", () => {
  const range = { start: NOW - HOUR, end: NOW };
  const rate: MetricLane = { id: "cost", name: "Cost", caption: "", unit: "$", maximum: 10, defaultVisible: true, derivation: "test", series: [] };
  const series = { provider: "Codex", tone: "codex", source: "test", samples: [{ at: NOW - MINUTE, end: NOW, value: 3 }, { at: NOW, end: NOW + MINUTE, value: 5 }] };
  const markerAt = readingMarkerAt(rate, { at: NOW - MINUTE, end: NOW }, range);
  expect(markerAt).toBe(NOW - 1);
  expect(metricValueAt(rate, series, markerAt)).toBe(3);
  expect(metricSampleTimes(rate, series, range, 480, markerAt).segments.flat()).toContain(markerAt);
  expect(readingMarkerAt({ ...rate, level: true }, { at: NOW - 5 * MINUTE, end: NOW - 5 * MINUTE }, range)).toBe(NOW - 5 * MINUTE);
  expect(readingMarkerAt({ ...rate, level: true }, { at: NOW - 2 * HOUR, end: NOW - 2 * HOUR }, range)).toBe(range.start);
});

test("a rate series has no value outside its bins, and its line breaks at an empty bin", () => {
  const range = { start: NOW - HOUR, end: NOW };
  const rate: MetricLane = { id: "tokens", name: "Token usage", caption: "", unit: "tokens/min", maximum: 100, defaultVisible: true, derivation: "test", series: [] };
  const series = { provider: "Claude Code", tone: "claude", source: "test", samples: [{ at: NOW - 30 * MINUTE, end: NOW - 29 * MINUTE, value: 0 }, { at: NOW - 10 * MINUTE, end: NOW - 9 * MINUTE, value: 7 }] };
  expect([seriesSampledAt(rate, series, NOW - 45 * MINUTE), metricValueAt(rate, series, NOW - 45 * MINUTE)]).toEqual([false, null]);
  expect([seriesSampledAt(rate, series, NOW - 30 * MINUTE), metricValueAt(rate, series, NOW - 30 * MINUTE)]).toEqual([true, 0]);
  expect(seriesSampledAt(rate, { ...series, samples: [] }, NOW - 45 * MINUTE)).toBe(false);
  const { segments } = metricSampleTimes(rate, series, range, 480, NOW - 9 * MINUTE - 1);
  expect(segments.map((instants) => [instants[0], instants.at(-1)])).toEqual([[NOW - 30 * MINUTE, NOW - 29 * MINUTE - 1], [NOW - 10 * MINUTE, NOW - 9 * MINUTE - 1]]);
});

test("a quota reading names its latest reporter by Session id", () => {
  const series = { provider: "Claude Code", tone: "claude", source: "test", samples: [], segments: [{ start: NOW - HOUR, end: null, reporter: { session: "session_A", workspace: "default", actor: "worker" } }] };
  expect(quotaReporterAt(series, NOW)).toEqual({ id: "session_A" });
});

test("a context sample wider than a bin holds over every bin it spans", () => {
  const worker = session({ id: "session_A", actor: "default/worker", start: NOW - HOUR, metrics: { tokens: { status: "n/a", source: "test" }, context: { status: "reported", source: "test", size: 1000, samples: [
    { start: NOW - HOUR, end: NOW - 30 * MINUTE, value: 10 },
    { start: NOW - 30 * MINUTE, end: NOW, value: 40 },
  ] } } });
  const model = modelOf({ actors: [actor("worker")], sessions: [worker] });
  const bins = model.sessionMetricBins(worker, { start: NOW - 45 * MINUTE, end: NOW }, 5 * MINUTE, "context");
  expect(bins.map((bin) => [bin.start, bin.value])).toEqual(Array.from({ length: 9 }, (_unused, index) => [NOW - 45 * MINUTE + index * 5 * MINUTE, index < 3 ? 10 : 40]));
});

test("a context reading holds until the next one, across buckets with no reading, and the last until the Session's end", () => {
  const worker = session({ id: "session_A", actor: "default/worker", start: NOW - HOUR, metrics: { tokens: { status: "n/a", source: "test" }, context: { status: "reported", source: "test", size: 1000, samples: [
    { start: NOW - HOUR, end: NOW - 59 * MINUTE, value: 10 },
    { start: NOW - 30 * MINUTE, end: NOW - 29 * MINUTE, value: 40 },
  ] } } });
  const model = modelOf({ actors: [actor("worker")], sessions: [worker] });
  const bins = model.sessionMetricBins(worker, { start: NOW - HOUR, end: NOW }, 10 * MINUTE, "context");
  expect(bins.map((bin) => bin.value)).toEqual([10, 10, 10, 40, 40, 40]);
});

test("a lane with no Runtime to read is drawn without a search and when its name matches", () => {
  const empty: MetricLane = { id: "tokens", name: "Token usage", caption: "", unit: "tokens/min", maximum: 100, defaultVisible: true, derivation: "test", series: [] };
  const model = modelOf({ metrics: [empty] });
  expect(model.visibleMetrics({ visibleMetricIds: new Set(["tokens"]), query: "" }).map((lane) => lane.metric.id)).toEqual(["tokens"]);
  expect(model.visibleMetrics({ visibleMetricIds: new Set(["tokens"]), query: "claude" })).toEqual([]);
  expect(model.visibleMetrics({ visibleMetricIds: new Set(["tokens"]), query: "token" }).map((lane) => lane.metric.id)).toEqual(["tokens"]);
});

test("the Actors the range holds back leave out the pinned ones", () => {
  const model = modelOf({
    actors: [actor("idle"), actor("old"), actor("nightly", "job")],
    sessions: [session({ id: "s1", actor: "default/idle", start: NOW - 3 * DAY, end: NOW - 2 * DAY }), session({ id: "s2", actor: "default/old", start: NOW - 3 * DAY, end: NOW - 2 * DAY })],
  });
  const hidden = state();
  expect(model.hiddenBandsFor(hidden, model.bandAgents(hidden))?.label).toBe("Show 2 Actors with no Sessions in range");
  const pinned = state({ actorPins: { "default/idle": true } });
  expect(model.hiddenBandsFor(pinned, model.bandAgents(pinned))?.label).toBe("Show 1 Actor with no Sessions in range");
  const shown = state({ actorPins: { "default/idle": true }, showAgentsOutsideRange: true });
  expect(model.hiddenBandsFor(shown, model.bandAgents(shown))?.label).toBe("Hide 1 Actor with no Sessions in range");
});

test("a New actor draft is refused for its name's form, then its uniqueness, then its working directory", () => {
  const draft = { ...emptyActorDraft(1, "default", "UTC", { runtimes: [], providers: [], nodes: [] }), kind: "job", command: "echo hi" };
  expect(validateActorDraft({ ...draft, name: "Bad", workdir: "rel" }, ["bad"])).toMatch(/^Name must be/);
  expect(validateActorDraft({ ...draft, name: "dispatcher", workdir: "rel" }, ["dispatcher"])).toBe("An Actor named dispatcher already exists in this Workspace; names are unique.");
  expect(validateActorDraft({ ...draft, name: "fresh", workdir: "rel" }, ["dispatcher"])).toBe("Working directory must be an absolute path on the Node.");
  expect(validateActorDraft({ ...draft, name: "fresh", workdir: "/tmp" }, ["dispatcher"])).toBeNull();
  expect(validateActorDraft({ ...draft, name: "fresh", workdir: "/tmp", command: "sh -c 'echo hi" }, ["dispatcher"])).toBe("Command has an unclosed ' quote.");
});

test("a New actor draft carries a Runtime for an Agent and a command for a Job", () => {
  const draft = { ...emptyActorDraft(1, "default", "UTC", { runtimes: [["claude-code", "Claude Code"]], providers: [["tmux", "tmux"]], nodes: [["local", "Local"]] }), runtime: "claude-code", command: "make test", workdir: "/work" };
  expect(draftLaunchSpec({ ...draft, kind: "agent" })).toEqual({ runtime: "claude-code", workdir: "/work", provider: "tmux", node: "local" });
  expect(draftLaunchSpec({ ...draft, kind: "job" })).toEqual({ command: ["make", "test"], workdir: "/work", provider: "tmux", node: "local" });
});

test("a Job edit is refused for a Command with an unclosed quote", () => {
  const job = { kind: "job" as const, launchSpec: { command: ["echo", "hi"], workdir: "/work", provider: "tmux", node: "local" } };
  const schedule: ScheduleRecord = { actor: "default/nightly", cron: "0 9 * * *", timezone: "UTC", enabled: true, nextRun: null };
  const draft = { ...editDraftOf({ ...job, name: "nightly" }, schedule), command: "sh -c 'echo hi'" };
  expect(validateEditDraft(draft, job, schedule)).toBeNull();
  expect(validateEditDraft({ ...draft, command: "sh -c 'echo hi" }, job, schedule)).toBe("Command has an unclosed ' quote.");
  expect(validateEditDraft({ ...draft, command: "" }, job, schedule)).toBe("Command is required.");
  expect(validateEditDraft({ ...draft, name: "Nightly" }, job, schedule)).toMatch(/^Name must be lowercase/);
});

test("a New actor draft from All workspaces asks for its Workspace before anything else", () => {
  const draft = { ...emptyActorDraft(1, "", "UTC", { runtimes: [], providers: [], nodes: [] }), name: "fresh", command: "make", workdir: "/tmp" };
  // A fresh draft is a Job.
  expect(draft).toMatchObject({ workspace: "", chooseWorkspace: true, kind: "job" });
  expect(validateActorDraft(draft)).toBe("Choose the Workspace to declare the Actor in.");
  expect(validateActorDraft({ ...draft, workspace: "research" })).toBeNull();
  expect(emptyActorDraft(1, "default", "UTC", { runtimes: [], providers: [], nodes: [] }).chooseWorkspace).toBe(false);
});

test("a lookup by id finds the first record of that id, as a search of the list does", () => {
  const first = session({ id: "s1", actor: "default/dev" });
  const duplicate = session({ id: "s1", actor: "default/ops" });
  const message: MessageRecord = { id: "m1", from: "Admin", fromWorkspace: null, fromSessionId: null, to: "default/dev", toWorkspace: "default", toSessionId: "s1", accepted: NOW - 10 * MINUTE, read: null, anchor: null, preview: "first", body: null };
  const model = modelOf({ actors: [actor("dev"), { ...actor("dev"), kind: "job" }], sessions: [first, duplicate], messages: [message, { ...message, preview: "second" }] });
  expect(model.sessionById("s1")).toBe(first);
  expect(model.messageById("m1")).toBe(message);
  expect(model.kindOf("default/dev")).toBe("agent");
  expect(model.sessionsFor("default/ops").map((entry) => entry.id)).toEqual(["s1"]);
  expect(model.sessionById("missing")).toBeNull();
});

test("waiting mail is listed per Actor in accepted order, without the Messages already read", () => {
  const message = (id: string, to: string, accepted: number, read: MessageRecord["read"] = null): MessageRecord => ({ id, from: "Admin", fromWorkspace: null, fromSessionId: null, to, toWorkspace: "default", toSessionId: null, accepted, read, anchor: null, preview: id, body: null });
  const model = modelOf({
    actors: [actor("dev"), actor("ops")],
    messages: [
      message("m3", "default/dev", NOW - MINUTE),
      message("m1", "default/dev", NOW - 3 * MINUTE),
      message("m2", "default/ops", NOW - 2 * MINUTE),
      message("m0", "default/dev", NOW - 4 * MINUTE, { readId: "r1", sessionId: "s1", at: NOW - MINUTE, fifo: 1 }),
    ],
  });
  expect(model.waitingMessagesFor("default/dev").map((entry) => entry.id)).toEqual(["m1", "m3"]);
  expect(model.waitingMessagesFor("default/ops").map((entry) => entry.id)).toEqual(["m2"]);
  expect(model.waitingMessagesFor("default/none")).toEqual([]);
});

test("the Actor card lists every Session of its Actor while the Timeline keeps the range's", () => {
  const inRange = session({ id: "s2", actor: "default/dev" });
  const earlier = session({ id: "s1", actor: "default/dev", start: NOW - 3 * DAY, end: NOW - 2 * DAY });
  const model = modelOf({ actors: [actor("dev")], sessions: [inRange], actorSessions: [{ ...inRange, metrics: earlier.metrics }, earlier] });
  expect(model.sessionsFor("default/dev").map((entry) => entry.id)).toEqual(["s2"]);
  expect(model.allSessionsFor("default/dev").map((entry) => entry.id)).toEqual(["s2", "s1"]);
  // The range's copy of a Session is the one read with the range's metrics.
  expect(model.sessionById("s2")).toBe(inRange);
  expect(model.sessionById("s1")).toBe(earlier);
  expect(model.consoleSessionFor("s1")).toMatchObject({ id: "s1", actor: "dev" });
});

test("a Session measured over its lifetime is found by id with its lifetime metrics while the Timeline keeps the range's", () => {
  const straddling = session({ id: "s2", actor: "default/dev", start: NOW - 2 * DAY });
  const lifetime = { ...straddling, metrics: session({ id: "s1", actor: "default/dev" }).metrics };
  const model = modelOf({ actors: [actor("dev")], sessions: [straddling], actorSessions: [lifetime], measuredSession: "s2" });
  expect(model.sessionById("s2")).toBe(lifetime);
  expect(model.sessionsFor("default/dev")).toEqual([straddling]);
  expect(model.allSessionsFor("default/dev").map((entry) => entry.id)).toEqual(["s2"]);
});

test("a lane of several Runtimes names each series with its Runtime and lists every Runtime before folding", () => {
  const series = (provider: string, account: string | undefined, value: number) => ({ provider, account, tone: provider, source: "test", samples: [{ at: NOW, end: NOW, value }] });
  const lane = (kind?: "quota"): MetricLane => ({ id: "lane", name: "lane", caption: "", unit: "", maximum: 100, kind, defaultVisible: true, derivation: "test", series: [] });
  const items = [series("Claude Code", undefined, 1), series("Claude Code", "team", 5), series("Claude Code", "bot", 3), series("Codex", "team", 2)];
  const valueOf = (item: MetricSeries) => item.samples?.[0].value ?? -1;
  expect(listedSeriesOrder(lane(), items, valueOf).map((item) => laneSeriesName(lane(), item))).toEqual(["Claude Code · team", "Codex · team", "Claude Code · bot", "Claude Code"]);
  expect(listedSeriesOrder(lane("quota"), items.slice(0, 3), valueOf).map((item) => laneSeriesName(lane("quota"), item))).toEqual(["team", "bot", "Claude Code"]);
});

test("env is edited as NAME=VALUE lines and refused for a name the daemon refuses", () => {
  expect(parseEnv("GH_CONFIG_DIR=/home/me/gh\n\nTOKEN=a=b\n")).toEqual({ env: { GH_CONFIG_DIR: "/home/me/gh", TOKEN: "a=b" }, error: null });
  expect(parseEnv("  \n")).toEqual({ env: null, error: null });
  expect(parseEnv("1BAD=x").error).toBe("Environment name 1BAD must be letters, digits, and _, not starting with a digit.");
  expect(parseEnv("NO_VALUE").error).toBe("Environment line NO_VALUE must be NAME=VALUE.");
  expect(parseEnv("A=1\nA=2").error).toBe("Environment name A appears twice.");
  expect(envText({ A: "1", B: "x=y" })).toBe("A=1\nB=x=y");
  // A value with line breaks or backslashes comes back from the form as it went in.
  const lined = { X: "one\ntwo\r", Y: "C:\\path\\n" };
  expect(envText(lined)).toBe("X=one\\ntwo\\r\nY=C:\\\\path\\\\n");
  expect(parseEnv(envText(lined))).toEqual({ env: lined, error: null });
  expect(parseEnv("P=C:\\path").env).toEqual({ P: "C:\\path" });
  // Every name the daemon takes is kept, those an object inherits among them.
  const inherited = parseEnv("__proto__=x\nconstructor=y\ntoString=z").env!;
  expect(Object.entries(inherited)).toEqual([["__proto__", "x"], ["constructor", "y"], ["toString", "z"]]);
  expect(JSON.parse(JSON.stringify(inherited))).toEqual(JSON.parse('{"__proto__":"x","constructor":"y","toString":"z"}'));
  const draft = { ...emptyActorDraft(1, "default", "UTC", { runtimes: [], providers: [], nodes: [] }), kind: "job", name: "fresh", command: "echo hi", workdir: "/tmp" };
  expect(validateActorDraft({ ...draft, env: "bad-name=1" })).toMatch(/^Environment name bad-name/);
  expect(draftLaunchSpec({ ...draft, env: "A=1" })).toMatchObject({ env: { A: "1" } });
  expect(draftLaunchSpec(draft)).not.toHaveProperty("env");
});

test("a Job edit sends env as a whole map, null once emptied, and nothing while unchanged", () => {
  const schedule: ScheduleRecord = { actor: "default/nightly", cron: "0 9 * * *", timezone: "UTC", enabled: true, nextRun: null };
  const job = { kind: "job" as const, launchSpec: { command: ["echo", "hi"], workdir: "/work", env: { B: "2", A: "1" }, provider: "tmux", node: "local" } };
  const draft = editDraftOf({ ...job, name: "nightly" }, schedule);
  expect(draft.env).toBe("B=2\nA=1");
  expect(launchSpecPatchOf({ ...draft, env: "A=1\nB=2" }, job)).toBeNull();
  expect(launchSpecPatchOf({ ...draft, env: "A=1\nB=3" }, job)).toEqual({ env: { A: "1", B: "3" } });
  expect(launchSpecPatchOf({ ...draft, env: "" }, job)).toEqual({ env: null });
  expect(launchSpecPatchOf({ ...draft, env: "" }, { ...job, launchSpec: { ...job.launchSpec, env: undefined } })).toBeNull();
  expect(validateEditDraft({ ...draft, env: "9X=1" }, job, schedule)).toMatch(/^Environment name 9X/);
});

test("runtimeSettings, env, and Provider options show secret-like values masked, and a mask left in place sends nothing", () => {
  const runtimeSettings = { model: "opus", env: { API_KEY: "sk-1", REGION: "jp" }, credentials: { user: "me", pass: "pw" }, apiKeyHelper: "" };
  expect(maskSecrets(runtimeSettings)).toEqual({ model: "opus", env: { API_KEY: SECRET_MASK, REGION: "jp" }, credentials: { user: SECRET_MASK, pass: SECRET_MASK }, apiKeyHelper: "" });
  const agent = { kind: "agent" as const, launchSpec: { runtime: "claude-code", runtimeSettings, workdir: "/work", env: { GITHUB_TOKEN: "ghp_1", REGION: "jp" }, provider: "tmux", node: "local", providerOptions: { socket: "feld", authToken: "t-1" } } };
  const draft = editDraftOf({ ...agent, name: "dev" }, null);
  for (const text of [draft.settings, draft.env, draft.providerOptions]) expect(text).toContain(SECRET_MASK);
  expect(`${draft.settings}${draft.env}${draft.providerOptions}`).not.toMatch(/sk-1|ghp_1|t-1/);
  expect(launchSpecPatchOf(draft, agent)).toBeNull();
  // An edit beside a kept mask sends the declared secret back with it.
  const edited = JSON.stringify({ ...JSON.parse(draft.settings), model: "sonnet" });
  expect(launchSpecPatchOf({ ...draft, settings: edited }, agent)).toEqual({ runtimeSettings: { ...runtimeSettings, model: "sonnet" } });
  const replaced = draft.settings.replace(`"API_KEY": "${SECRET_MASK}"`, '"API_KEY": "sk-2"');
  expect(launchSpecPatchOf({ ...draft, settings: replaced }, agent)).toEqual({ runtimeSettings: { ...runtimeSettings, env: { API_KEY: "sk-2", REGION: "jp" } } });
  expect(launchSpecPatchOf({ ...draft, settings: "" }, agent)).toEqual({ runtimeSettings: null });
  expect(launchSpecPatchOf({ ...draft, env: `${draft.env}\nDEBUG=1` }, agent)).toEqual({ env: { GITHUB_TOKEN: "ghp_1", REGION: "jp", DEBUG: "1" } });
  expect(launchSpecPatchOf({ ...draft, providerOptions: '{"socket":"other","authToken":"' + SECRET_MASK + '"}' }, agent)).toEqual({ providerOptions: { socket: "other", authToken: "t-1" } });
  // A mask where nothing was declared is refused rather than sent.
  expect(validateEditDraft({ ...draft, settings: `{"token":"${SECRET_MASK}"}` }, agent, null)).toMatch(/where no value was declared/);
  expect(validateEditDraft({ ...draft, env: `API_TOKEN=${SECRET_MASK}` }, agent, null)).toMatch(/has no declared value/);
  expect(validateEditDraft({ ...draft, settings: "[1]" }, agent, null)).toBe("Runtime settings must be a JSON object.");
});

test("an Agent edit sends trustWorkdir, Provider, and Node, and gives an attached Agent a launch spec only once one is filled", () => {
  const agent = { kind: "agent" as const, launchSpec: { runtime: "claude-code", workdir: "/work", provider: "tmux", node: "local" } };
  const draft = editDraftOf({ ...agent, name: "dev" }, null);
  expect(launchSpecPatchOf({ ...draft, trustWorkdir: "true", node: "box" }, agent)).toEqual({ trustWorkdir: true, node: "box" });
  // Another Provider drops the options the old one resolved, unless the form gives new ones.
  const placed = { ...agent, launchSpec: { ...agent.launchSpec, providerOptions: { socket: "feld", unit: "session" } } };
  const placedDraft = editDraftOf({ ...placed, name: "dev" }, null);
  expect(launchSpecPatchOf({ ...placedDraft, provider: "herdr" }, placed)).toEqual({ provider: "herdr", providerOptions: null });
  expect(launchSpecPatchOf({ ...placedDraft, provider: "herdr", providerOptions: "" }, placed)).toEqual({ provider: "herdr", providerOptions: null });
  expect(launchSpecPatchOf({ ...placedDraft, provider: "herdr", providerOptions: '{"tab":"feld"}' }, placed)).toEqual({ provider: "herdr", providerOptions: { tab: "feld" } });
  const attached = { kind: "agent" as const, launchSpec: null };
  const options = { runtimes: [["claude-code", "Claude Code"]], providers: [["tmux", "tmux"]], nodes: [["local", "Local"]] } as const;
  const empty = editDraftOf({ ...attached, name: "manual" }, null, options);
  expect(launchSpecPatchOf(empty, attached)).toBeNull();
  expect(validateEditDraft({ ...empty, name: "manual-2" }, attached, null)).toBeNull();
  expect(validateEditDraft({ ...empty, workdir: "work" }, attached, null)).toMatch(/absolute path/);
  expect(launchSpecPatchOf({ ...empty, workdir: "/work" }, attached)).toEqual({ runtime: "claude-code", workdir: "/work", provider: "tmux", node: "local" });
});

test("the latest Session compares with the launch spec only when Feld started it, and names what it did not record", () => {
  const dev = { ...actor("dev"), launchSpec: { runtime: "claude-code", workdir: "/work", provider: "tmux", node: "local" } };
  const owned = session({ id: "s1", actor: dev.id, end: NOW - MINUTE });
  const drift = (sessions: ReturnType<typeof session>[]) => modelOf({ actors: [dev], sessions }).launchSpecDriftFor(dev);
  expect(drift([{ ...owned, ownership: "attached" }])).toMatchObject({ comparable: false, changes: [] });
  const moved = drift([{ ...owned, ownership: "owned", execution: { ...owned.execution, runtime: "codex" }, placement: { ...owned.placement!, workdir: "", provider: "", node: "" } }]);
  expect(moved).toMatchObject({ comparable: true, changes: [{ field: "runtime", recorded: "codex", declared: "claude-code" }], unrecorded: ["workdir", "provider", "node"] });
});

test("Delete actor waits for the current Session to end, and a deleted Actor is neither started nor addressed", () => {
  const dev = actor("dev");
  const gone = { ...actor("gone"), deletedAt: NOW - HOUR };
  const actions = (menu: { items: { kind: string; action?: string; disabled?: boolean }[] }) => menu.items.flatMap((entry) => entry.kind === "item" ? [`${entry.action}${entry.disabled ? ":disabled" : ""}`] : []);
  const running = modelOf({ actors: [dev], sessions: [session({ id: "s1", actor: dev.id })] });
  expect(actions(running.agentMenuFor(dev, state()))).toContain("delete-actor:disabled");
  expect(running.agentMenuFor(dev, state()).items.find((entry) => entry.kind === "item" && entry.action === "delete-actor")).toMatchObject({ note: expect.stringContaining("s1 is running") });
  const ended = modelOf({ actors: [dev], sessions: [session({ id: "s1", actor: dev.id, end: NOW - MINUTE })] });
  expect(actions(ended.agentMenuFor(dev, state()))).toContain("delete-actor");
  // The listing's current Session holds Delete back even when the range does not reach it.
  const listed = { ...dev, currentSession: { id: "s0", ownership: "attached" } };
  expect(actions(modelOf({ actors: [listed] }).agentMenuFor(listed, state()))).toContain("delete-actor:disabled");
  expect(modelOf({ actors: [listed] }).agentMenuFor(listed, state()).items.find((entry) => entry.kind === "item" && entry.action === "delete-actor")).toMatchObject({ note: expect.stringContaining("the person who started it ends it") });
  const idle = { ...dev, currentSession: null };
  expect(actions(modelOf({ actors: [idle] }).agentMenuFor(idle, state()))).toContain("delete-actor");
  const deleted = modelOf({ actors: [gone], sessions: [session({ id: "s2", actor: gone.id, end: NOW - 2 * HOUR })] });
  expect(actions(deleted.agentMenuFor(gone, state()))).toEqual(["toggle-pin", "deleted:disabled"]);
});

test("the running Session of an Actor follows the Session the page holds over the listing's current Session", () => {
  const dev = actor("dev");
  const listed = { ...dev, currentSession: { id: "s1", ownership: "owned" } };
  // The listing is read again on an Actor notification, which a Session's end does not send.
  const ended = modelOf({ actors: [listed], sessions: [session({ id: "s1", actor: dev.id, end: NOW - MINUTE })] });
  expect(ended.runningSessionOf(listed)).toBeNull();
  expect(ended.agentMenuFor(listed, state()).items).toContainEqual(expect.objectContaining({ action: "delete-actor", disabled: false }));
  const running = modelOf({ actors: [listed], sessions: [session({ id: "s1", actor: dev.id })] });
  expect(running.runningSessionOf(listed)).toEqual({ id: "s1", ownership: "owned" });
  expect(modelOf({ actors: [listed] }).runningSessionOf(listed)).toEqual({ id: "s1", ownership: "owned" });
  expect(modelOf({ actors: [{ ...dev, currentSession: null }], sessions: [session({ id: "s1", actor: dev.id })] }).runningSessionOf({ ...dev, currentSession: null })).toBeNull();
});

test("a deleted Job's Schedule keeps no band in Live mode", () => {
  const nightly = { ...actor("nightly", "job"), deletedAt: NOW - HOUR };
  const schedule: ScheduleRecord = { actor: nightly.id, cron: "0 9 * * *", timezone: "UTC", enabled: true, nextRun: NOW + HOUR };
  const model = modelOf({ actors: [nightly], schedules: [schedule] });
  expect(model.visibleAgents(state({ mode: "live", actorPins: { [nightly.id]: false } }))).toEqual([]);
});
