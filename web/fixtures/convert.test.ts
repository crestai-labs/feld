import { expect, test } from "vitest";
import {
  actorsInputOf, consoleFixtureOf, contextMetricOf, daemonReadOf, daemonStateOf, generatedValueAt, metricLanesOf, pairingOf,
  minuteBins, rangeOf, tokenMetricOf,
} from "./convert.ts";
import type { MockActorsData, MockDaemonData, MockMetricLane, MockRecording, MockSession } from "./mock.ts";

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
const NOW = Date.parse("2026-09-02T15:00:00+09:00");
const clock = { now: NOW, timeZone: "Asia/Tokyo" };
const domain = { start: NOW - 2 * HOUR, end: NOW, step: MINUTE };

const program = { status: "n/a", source: "no Runtime Adapter" } as const;

function session(overrides: Partial<MockSession>): MockSession {
  return {
    id: "session_A",
    actor: "worker",
    start: NOW - HOUR,
    end: NOW - 30 * MINUTE,
    startKind: "start",
    ownership: "owned",
    endKind: "exited",
    exitCode: 0,
    execution: { type: "program", label: "Shell program", command: "bash run.sh", ai: false },
    model: null,
    placement: { provider: "tmux", node: "Local Node", workdir: "/work", summary: "session $1 (feld-1, created)", attachCommand: "tmux -L feld attach-session -t '$1'" },
    metrics: { tokens: program, context: program },
    observation: { kind: "gone", at: NOW - 30 * MINUTE },
    ...overrides,
  };
}

function actorsData(overrides: Partial<MockActorsData> = {}): MockActorsData {
  return {
    scenarios: {
      "three-days-ago-1h": { anchor: NOW - 3 * 24 * HOUR, duration: HOUR },
      "three-days-ago-1d": { anchor: NOW - 3 * 24 * HOUR, duration: 24 * HOUR },
    },
    workspaces: ["default", "research"],
    defaultWorkspace: "default",
    actors: [
      { id: "worker", workspace: "default", kind: "job", launchSpec: { command: ["bash", "run.sh"], workdir: "/work", provider: "tmux", node: "Local Node" } },
      { id: "researcher", workspace: "research", kind: "agent", launchSpec: null },
    ],
    sessions: [session({})],
    schedules: [{ actor: "worker", cron: "0 * * * *", timezone: "Asia/Tokyo", enabled: false, pausedAt: NOW - HOUR }],
    scheduleSkips: [{ id: "skip_1", actor: "worker", planned: NOW - 40 * MINUTE, reason: "previous session current", previousSession: "session_A" }],
    actions: [{ id: "action_1", spawner: "worker", spawnerWorkspace: "default", spawnerSessionId: "session_A", target: "researcher", targetWorkspace: "research", session: "session_R", at: NOW - 50 * MINUTE }],
    messages: [
      { id: "message_1", from: "Admin", fromWorkspace: null, fromSessionId: null, to: "worker", toWorkspace: "default", toSessionId: "session_A", accepted: NOW - 55 * MINUTE, read: null, anchor: null, body: "hello" },
      { id: "message_2", from: "worker", fromWorkspace: "default", fromSessionId: "session_A", to: "researcher", toWorkspace: "research", toSessionId: null, accepted: NOW - 45 * MINUTE, read: { readId: "read_1", sessionId: "session_R", at: NOW - 44 * MINUTE, fifo: 1 }, anchor: "msg_1", body: "A brief that runs past the forty-eight characters a list preview holds." },
    ],
    metricLanes: [],
    nodePrograms: ["bash"],
    ...overrides,
  };
}

test("an unknown scenario opens the default one, historical around its anchor", () => {
  const scenarios = actorsData().scenarios;
  expect(rangeOf(scenarios, "three-days-ago-1d")).toEqual({ mode: "historical", ...scenarios["three-days-ago-1d"] });
  expect(rangeOf(scenarios, "nope")).toEqual({ mode: "historical", ...scenarios["three-days-ago-1h"] });
  expect(rangeOf(scenarios, null)).toEqual({ mode: "historical", ...scenarios["three-days-ago-1h"] });
});

test("Actors are keyed by workspace/name and every reference follows its own Workspace", () => {
  const input = actorsInputOf(actorsData(), clock, domain);
  expect(input.actors.map((actor) => [actor.id, actor.name, actor.workspace])).toEqual([
    ["default/worker", "worker", "default"],
    ["research/researcher", "researcher", "research"],
  ]);
  expect(input.sessions[0]).toMatchObject({ id: "session_A", workspace: "default", actor: "default/worker", planned: null, lifecycle: {}, laneMetrics: {} });
  expect(input.schedules).toEqual([{ actor: "default/worker", cron: "0 * * * *", timezone: "Asia/Tokyo", enabled: false, pausedAt: NOW - HOUR, enabledAt: undefined, nextRun: null }]);
  expect(input.scheduleSkips[0].actor).toBe("default/worker");
  expect(input.actions[0]).toMatchObject({ spawner: "default/worker", spawnerWorkspace: "default", target: "research/researcher", targetWorkspace: "research" });
  expect(input.messages.map((message) => [message.from, message.fromWorkspace, message.to])).toEqual([
    ["Admin", null, "default/worker"],
    ["default/worker", "default", "research/researcher"],
  ]);
  // The list preview is the daemon's: 48 characters, then `…` when the body goes on.
  expect(input.messages.map((message) => [message.preview, message.body])).toEqual([
    ["hello", "hello"],
    ["A brief that runs past the forty-eight character…", "A brief that runs past the forty-eight characters a list preview holds."],
  ]);
  expect(input.consoleSessions).toBeUndefined();
});

test("a Session keeps the fixture-only facts and its Sub Agent fields", () => {
  const sub = session({ id: "session_A_S1", parentSession: "session_A", subAgent: "explore", startKind: "sub-agent", ownership: null, endKind: null, placement: null, observation: null });
  const fixture = session({ recordingOrigin: "2026-09-09T01:54:25.741Z", observations: [{ kind: "running", at: NOW - HOUR }], attachCommand: "feld actor attach worker", lifecycle: { startSettled: NOW - HOUR + 400 }, planned: NOW - HOUR });
  const [converted, child] = actorsInputOf(actorsData({ sessions: [fixture, sub] }), clock, domain).sessions;
  expect(converted).toMatchObject({ recordingOrigin: "2026-09-09T01:54:25.741Z", attachCommand: "feld actor attach worker", planned: NOW - HOUR, lifecycle: { startSettled: NOW - HOUR + 400 } });
  expect(converted.observations).toEqual([{ kind: "running", at: NOW - HOUR }]);
  expect(child).toMatchObject({ parentSession: "session_A", subAgent: "explore", ownership: null, endKind: null, placement: null });
});

test("minute bins interpolate the fixture samples and keep a summed total", () => {
  const bins = minuteBins(0, 4 * MINUTE, [10, 30], true);
  expect(bins.map((bin) => [bin.start, bin.end])).toEqual([[0, MINUTE], [MINUTE, 2 * MINUTE], [2 * MINUTE, 3 * MINUTE], [3 * MINUTE, 4 * MINUTE]]);
  expect(bins.reduce((sum, bin) => sum + bin.value, 0)).toBeCloseTo(40);
  expect(minuteBins(0, 2 * MINUTE, [10, 30], false).map((bin) => bin.value)).toEqual([10, 30]);
});

test("token bins keep every kind's total; a running Session's bins end at now", () => {
  const samples = [[10, 20, 30, 40], [50, 60, 70, 80], [5, 6, 7, 8]];
  const metric = tokenMetricOf({ status: "reported", source: "OTel", samples }, NOW - 10 * MINUTE, NOW);
  if (metric.status !== "reported") throw new Error("reported");
  expect(metric.samples).toHaveLength(10);
  expect(metric.samples.at(-1)!.end).toBe(NOW);
  [0, 1, 2, 3].forEach((kind) => {
    expect(metric.samples.reduce((sum, sample) => sum + sample.value[kind], 0)).toBeCloseTo(samples.reduce((sum, sample) => sum + sample[kind], 0));
  });
  expect(tokenMetricOf({ status: "unavailable", source: "none" }, 0, 1)).toEqual({ status: "unavailable", source: "none" });
});

test("context samples stay the fixture's, spread evenly over the Session", () => {
  const metric = contextMetricOf({ status: "reported", source: "status line", size: 200000, samples: [20, 60, 30] }, 0, 3 * MINUTE);
  expect(metric).toEqual({
    status: "reported", source: "status line", size: 200000,
    samples: [{ start: 0, end: MINUTE, value: 20 }, { start: MINUTE, end: 2 * MINUTE, value: 60 }, { start: 2 * MINUTE, end: 3 * MINUTE, value: 30 }],
  });
});

test("Sessions whose end is null take their metrics up to the fixture's now", () => {
  const running = session({ end: null, metrics: { tokens: { status: "reported", source: "OTel", samples: [[1, 1, 1, 1]] }, context: { status: "reported", source: "s", size: 1, samples: [5] } } });
  const [converted] = actorsInputOf(actorsData({ sessions: [running] }), clock, domain).sessions;
  expect(converted.end).toBeNull();
  if (converted.metrics.context.status !== "reported") throw new Error("reported");
  expect(converted.metrics.context.samples).toEqual([{ start: NOW - HOUR, end: NOW, value: 5 }]);
});

const QUOTA: MockMetricLane = {
  id: "quota-claude-5h", name: "Claude Code quota · 5h", caption: "Claude Code · 5-hour window · % used", unit: "%", maximum: 100, kind: "quota",
  defaultVisible: true, derivation: "per resets_at",
  series: [{ provider: "Claude Code", tone: "claude", source: "status line", period: 5 * HOUR, resetAnchor: NOW - 90 * MINUTE, minimum: 7, span: 78, variation: 5, phase: .2, showResets: true }],
};
const COST: MockMetricLane = {
  id: "estimated-cost", name: "Estimated cost", caption: "USD/hour", prefix: "$", unit: "/h", decimals: 2, maximum: 6, defaultVisible: true, derivation: "summed",
  series: [
    { provider: "Claude Code", tone: "claude", source: "OTel", period: 66 * MINUTE, resetAnchor: NOW, minimum: .8, span: 2.9, variation: .7, phase: 1.3 },
    { provider: "Codex", tone: "codex", source: "OTel", until: NOW - HOUR, period: 54 * MINUTE, resetAnchor: NOW, minimum: 1.1, span: 3.4, variation: .6, phase: 3.9 },
  ],
};
const ERRORS: MockMetricLane = {
  id: "context-utilization", name: "Context", caption: "%", unit: "%", maximum: 100, defaultVisible: false, derivation: "latest",
  series: [{ provider: "Codex", tone: "codex", status: "unavailable", source: "attached Codex" }],
};

const LANES = { now: NOW, accounts: new Map<string, string>() };

test("a rate lane reads the fixture's generator at the last instant of each step and stops at `until`", () => {
  const [lane] = metricLanesOf([COST], domain, [], LANES);
  expect(lane).toMatchObject({ id: "estimated-cost", prefix: "$", decimals: 2, defaultVisible: true });
  expect(lane.level).toBeUndefined();
  const [claude, codex] = lane.series;
  expect(claude.samples).toHaveLength(120);
  expect(claude.samples![0]).toEqual({ at: domain.start, end: domain.start + MINUTE, value: generatedValueAt(COST, COST.series[0], domain.start + MINUTE - 1) });
  expect(codex.samples!.at(-1)).toEqual({ at: NOW - HOUR, end: NOW - HOUR + MINUTE, value: generatedValueAt(COST, COST.series[1], NOW - HOUR) });
});

test("a rate lane has no bin inside the fixture's `gaps`", () => {
  const gapped = { ...COST, series: [{ ...COST.series[0], gaps: [[NOW - 30 * MINUTE, NOW - 20 * MINUTE]] as const }] };
  const [lane] = metricLanesOf([gapped], domain, [], LANES);
  const starts = lane.series[0].samples!.map((sample) => sample.at);
  expect(starts).toContain(NOW - 31 * MINUTE);
  expect(starts).not.toContain(NOW - 30 * MINUTE);
  expect(starts).not.toContain(NOW - 21 * MINUTE);
  expect(starts).toContain(NOW - 20 * MINUTE);
});

test("a quota lane is a level lane with its resets, the reset ending each reading, and its reporters", () => {
  const claude = session({ id: "session_C", actor: "worker", start: NOW - 3 * HOUR, end: null, model: ["Claude Code", "claude-opus"], metrics: { tokens: program, context: { status: "reported", source: "s", size: 1, samples: [1] } } });
  const sessions = actorsInputOf(actorsData({ sessions: [claude] }), clock, domain).sessions;
  const [lane] = metricLanesOf([QUOTA], domain, sessions, LANES);
  expect(lane).toMatchObject({ kind: "quota", level: true, window: "five_hour", showResets: true });
  const [series] = lane.series;
  expect(series.resets).toEqual([NOW - 90 * MINUTE]);
  expect(series.samples![0]).toMatchObject({ at: domain.start + MINUTE - 1, end: domain.start + MINUTE - 1, until: NOW - 90 * MINUTE });
  expect(series.samples!.at(-1)).toMatchObject({ at: NOW - 1, until: NOW - 90 * MINUTE + 5 * HOUR });
  expect(series.segments).toEqual([{ start: domain.start + MINUTE - 1, end: null, reporter: { session: "session_C", workspace: "default", actor: "worker" } }]);
  const wider = metricLanesOf([QUOTA], { start: NOW - 7 * HOUR, end: NOW, step: MINUTE }, sessions, LANES)[0].series[0];
  expect(wider.resets).toEqual([NOW - 390 * MINUTE, NOW - 90 * MINUTE]);
  expect(wider.segments!.map((segment) => segment.reporter?.session ?? null)).toEqual([null, "session_C"]);
  expect(wider.segments![0].end).toBe(NOW - 3 * HOUR + MINUTE - 1);
});

test("a quota account's reporter is a Session of that account", () => {
  const reported = { tokens: program, context: { status: "reported" as const, source: "s", size: 1, samples: [1] } };
  const personal = session({ id: "session_P", actor: "worker", start: NOW - 3 * HOUR, end: null, model: ["Claude Code", "claude-opus"], metrics: reported, account: "personal@example.com" });
  const team = session({ id: "session_T", actor: "worker", start: NOW - 2 * HOUR, end: null, model: ["Claude Code", "claude-opus"], metrics: reported, account: "team@example.com" });
  const input = actorsInputOf(actorsData({ sessions: [personal, team], metricLanes: [{ ...QUOTA, series: [{ ...QUOTA.series[0], account: "personal@example.com" }, { ...QUOTA.series[0], account: "team@example.com" }] }] }), clock, domain);
  const reporters = input.metrics[0].series.map((series) => series.segments!.at(-1)!.reporter?.session);
  expect(reporters).toEqual(["session_P", "session_T"]);
});

test("a series with a status keeps it; the context lane reads as a level", () => {
  const [lane] = metricLanesOf([ERRORS], domain, [], LANES);
  expect(lane.level).toBe(true);
  expect(lane.series).toEqual([{ provider: "Codex", tone: "codex", source: "attached Codex", status: "unavailable" }]);
});

const RECORDING: MockRecording = {
  id: "uuid-1", sourceName: "uuid-1.jsonl", startedAt: "2026-09-09T01:54:36.026Z", endedAt: "2026-09-09T01:58:59.738Z",
  records: [{ cursor: "c1", at: "2026-09-09T01:54:36.026Z", kind: "input", text: "hi", raw: {} }],
};
const SHARED: MockRecording = { ...RECORDING, id: "uuid-shared" };

test("each Console Session resolves to its recording, the one it names, the shared one, or its log", () => {
  const console = consoleFixtureOf([
    { id: "session_D2", actor: "developer-01", model: "claude-fable-5-1", placement: "docker · Build Node" },
    { id: "session_FP5", actor: "my-project-2", model: "claude-fable-5-1", placement: "tmux · Local Node", recording: "session_D2" },
    { id: "session_P0", actor: "planner-01", model: "claude-opus", placement: "tmux · Local Node" },
    { id: "session_RS", actor: "researcher", model: "claude-opus", placement: "docker · Sandbox Node", sourceUnavailable: "gone" },
    { id: "session_T0", actor: "test-runner", model: "codex-terra", placement: "tmux · Local Node", console: "none" },
    { id: "session_D0", actor: "dispatcher", model: "N/A", placement: "tmux · Local Node", console: "log" },
    { id: "session_SC", actor: "stale-checkout", model: "N/A", placement: "tmux · Local Node", console: "log" },
  ], { session_D2: RECORDING, shared_short: SHARED }, { session_D0: { source: "stdout", lines: [[400, "stdout", "hello"]] } });
  expect(console.sessions).toHaveLength(7);
  expect(console.recordings.get("session_D2")).toEqual({ id: "uuid-1", records: RECORDING.records, hasOlder: false, startedAt: RECORDING.startedAt });
  expect(console.recordings.get("session_FP5")!.id).toBe("uuid-1");
  expect(console.recordings.get("session_P0")!.id).toBe("uuid-shared");
  expect([...console.recordings.keys()]).toEqual(["session_D2", "session_FP5", "session_P0"]);
  expect([...console.logs.keys()]).toEqual(["session_D0"]);
  expect(console.logs.get("session_D0")).toEqual({ source: "stdout", lines: [[400, "stdout", "hello"]] });
});

test("a current agent-runtime Session carries the turn of its recording on the fixture clock", () => {
  const runtime = { type: "agent-runtime", label: "Claude Code", command: "claude", ai: true } as const;
  const records: MockRecording["records"] = [
    { cursor: "c1", at: "2026-09-09T01:54:36.026Z", kind: "input", text: "hi", raw: {} },
    { cursor: "c2", at: "2026-09-09T01:54:40.026Z", kind: "response", text: "hello", raw: {} },
    { cursor: "c3", at: "2026-09-09T01:54:45.026Z", kind: "tool-call", text: "Bash", raw: {}, subAgent: "explore" },
    { cursor: "c4", at: "2026-09-09T01:54:50.026Z", kind: "system", text: "", raw: {} },
  ];
  const sessions = [
    session({ id: "session_C", end: null, execution: runtime }),
    // The instant `recordingOrigin` names coincides with the start instead of the recording's.
    session({ id: "session_O", end: null, execution: runtime, recordingOrigin: "2026-09-09T01:54:30.026Z" }),
    session({ id: "session_N", end: null, execution: runtime }),
    session({ id: "session_C_S1", end: null, execution: runtime, parentSession: "session_C", subAgent: "explore" }),
    session({ id: "session_E", execution: runtime }),
    session({ id: "session_P", end: null }),
  ];
  const console = consoleFixtureOf(sessions.map((entry) => ({ id: entry.id, actor: "worker", ...(entry.id === "session_N" ? { console: "missing" as const } : {}) })), { session_C: { ...RECORDING, records }, session_O: { ...RECORDING, records }, shared_short: SHARED }, {});
  const converted = actorsInputOf(actorsData({ sessions }), clock, domain, console).sessions;
  const turns = converted.map((entry) => [entry.id, entry.turn, entry.lastEntryAt]);
  expect(turns).toEqual([
    ["session_C", "waiting-input", NOW - HOUR + 4000],
    ["session_O", "waiting-input", NOW - HOUR + 10000],
    ["session_N", "unavailable", null],
    ["session_C_S1", undefined, undefined],
    ["session_E", undefined, undefined],
    ["session_P", undefined, undefined],
  ]);
});

const STATUS: FeldApi.DaemonStatus = {
  state: "running", version: "0.6.0", instanceId: "dmn_1", pid: 1, home: "/h", api: "http://127.0.0.1:7381", startedAt: "2026-09-01T09:12:45+09:00",
  serverTime: "2026-09-02T15:00:02+09:00", historyStart: null, timezone: "Asia/Tokyo", runtimes: [], providers: [{ name: "tmux", label: "tmux" }],
  nodes: ["local"], webUi: null, allowedHosts: [], otlp: "http://127.0.0.1:7382", restored: { actors: 1, runningSessions: 1, schedules: 0 },
  scheduler: { wokeAt: "2026-09-02T15:00:00+09:00", planned: [], restarts: 0, lastRestart: null },
};
const REFUSALS = { restart: "cannot restart", shutdown: "cannot shut down", start: "no start", stop: "closed", kill: "closed", delete: "running" };
const DAEMON: MockDaemonData = { status: STATUS, degradedStatus: { ...STATUS, otlp: null }, lastRead: "2026-09-02T14:52:18+09:00", readFailure: "503", refusals: REFUSALS, pairing: { codes: ["K7QM-4XPD"] } };

test("the Settings page reads each fixture daemon state on the fixture's clock", () => {
  const actors = actorsData({ workspaces: ["default", "home"], defaultWorkspace: "default" });
  expect(daemonReadOf(DAEMON, actors, clock, "running")).toEqual({
    phase: "reachable", status: STATUS, lastRead: STATUS.serverTime, workspaces: ["default", "home"], defaultWorkspace: "default",
    actionError: null, restartedAt: null, now: NOW,
  });
  expect(daemonReadOf(DAEMON, actors, clock, "degraded")).toMatchObject({ phase: "reachable", status: { otlp: null } });
  expect(daemonReadOf(DAEMON, actors, clock, "unreachable")).toMatchObject({ phase: "unreachable", status: STATUS, lastRead: "2026-09-02T14:52:18+09:00" });
  // Before the first read answers, nothing is read.
  expect(daemonReadOf(DAEMON, actors, clock, "reading")).toMatchObject({ phase: "reading", status: null, lastRead: null, workspaces: null });
  expect(daemonReadOf(DAEMON, actors, clock, "refusing")).toMatchObject({ phase: "reachable", status: STATUS });
  expect(daemonStateOf("degraded")).toBe("degraded");
  expect(daemonStateOf("bogus")).toBe("running");
  expect(daemonStateOf(null)).toBe("running");
  expect(daemonStateOf("unauthorized")).toBe("running");
});

test("the fixture's daemon takes each pairing code once, whatever failed before", () => {
  const pair = pairingOf(DAEMON.pairing);
  for (let failure = 0; failure < 5; failure += 1) expect(pair("ABCD-EFGH")).toBe(false);
  expect(pair("k7qm4xpd")).toBe(true);
  expect(pair("K7QM-4XPD")).toBe(false);
});
