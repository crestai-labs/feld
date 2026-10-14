import { expect, test } from "vitest";
import { consoleSessionsOf, toActorsInput } from "./snapshot.ts";

// The range reads as the Actors page reads them.

function apiSession(fields: Partial<FeldApi.Session> & { id: string; actor: string }): FeldApi.Session {
  return {
    workspace: "default",
    actorId: `actor_${fields.actor}`,
    actorKind: "agent",
    generation: 1,
    start: "2026-09-25T00:00:00Z",
    end: null,
    startKind: "start",
    ownership: "owned",
    endKind: "running",
    exitCode: null,
    planned: null,
    execution: { type: "agent-runtime", runtime: "claude-code", label: "Claude Code", command: ["claude"], ai: true },
    placement: { provider: "tmux", node: "local", workdir: "/work", placement: { summary: "feld:1", attachCommand: "tmux attach -t feld:1" } },
    lifecycle: { startSettled: "2026-09-25T00:00:01Z", startFailed: null, stop: null, kill: null },
    observation: { kind: "running", at: "2026-09-25T00:00:01Z" },
    outcome: "running",
    ...fields,
  };
}

function apiActor(name: string, fields: Partial<FeldApi.Actor> = {}): FeldApi.Actor {
  return {
    id: `actor_${name}`, name, workspace: "default", kind: "agent", launchSpec: { runtime: "claude-code", workdir: "/work", provider: "tmux", node: "local" }, schedule: null, grants: [], acceptFrom: null,
    createdAt: "2026-09-01T00:00:00Z", updatedAt: "2026-09-01T00:00:00Z", deletedAt: null, currentSession: null, latestSession: null, sessions: [],
    attention: false, waitingCount: 0, unreadMail: false, skipMark: false, unlinkedMark: false, ...fields,
  };
}

function data(fields: Partial<FeldLiveData>): FeldLiveData {
  return { workspaces: ["default"], actors: [], sessions: [], metrics: [], records: [], messages: [], runtimeMetrics: null, runtimes: [], providers: [], nodes: [], ...fields };
}

test("a Session keeps its lifecycle facts as the daemon answered them", () => {
  const input = toActorsInput(data({
    actors: [apiActor("job", { kind: "job" })],
    sessions: [
      apiSession({ id: "s1", actor: "job", end: "2026-09-25T00:10:00Z", endKind: "exited", exitCode: 2 }),
      apiSession({ id: "s2", actor: "job", end: "2026-09-25T00:20:00Z", endKind: "stopped", exitCode: 143, ownership: "attached", startKind: "attach", lifecycle: { startSettled: null, startFailed: null, stop: { accepted: "2026-09-25T00:19:59Z", requesterSessionId: "s9", attempt: "SIGTERM", observed: { kind: "gone", at: "2026-09-25T00:20:00Z" } }, kill: null } }),
      apiSession({ id: "s3", actor: "job", endKind: "running", exitCode: undefined as unknown as null }),
    ],
  }));
  const [exited, stopped, running] = input.sessions;
  expect(exited).toMatchObject({ actor: "default/job", endKind: "exited", exitCode: 2, ownership: "owned", end: Date.parse("2026-09-25T00:10:00Z") });
  expect(stopped).toMatchObject({ endKind: "stopped", exitCode: 143, ownership: "attached", startKind: "attach" });
  expect(stopped.lifecycle.stop).toEqual({ accepted: Date.parse("2026-09-25T00:19:59Z"), requesterSessionId: "s9", attempt: "SIGTERM", observed: "gone" });
  // An exit code the daemon left out is one it did not report.
  expect(running.exitCode).toBeNull();
  expect(running.end).toBeNull();
  expect(running.placement).toEqual({ provider: "tmux", node: "local", workdir: "/work", summary: "feld:1", attachCommand: "tmux attach -t feld:1" });
  expect(running.execution).toEqual({ type: "agent-runtime", runtime: "claude-code", label: "Claude Code", command: "claude", ai: true });
});

test("a Session's model is its Runtime until the Runtime Adapter names the model; a program has none", () => {
  const input = toActorsInput(data({
    actors: [apiActor("dev"), apiActor("job", { kind: "job" })],
    sessions: [
      apiSession({ id: "s1", actor: "dev" }),
      apiSession({ id: "s2", actor: "dev", model: ["Claude Code", "claude-opus-4"] }),
      apiSession({ id: "s3", actor: "job", execution: { type: "program", label: "make", command: ["make", "test"], ai: false } }),
    ],
  }));
  expect(input.sessions.map((session) => session.model)).toEqual([["Claude Code", null], ["Claude Code", "claude-opus-4"], null]);
  // Only an arrived Runtime identity links the Session; the Runtime name alone does not.
  expect(input.sessions.map((session) => session.linked)).toEqual([false, true, false]);
  expect(input.sessions[2].execution.command).toBe("make test");
});

test("a Sub Agent Session follows its parent with no lifecycle of its own", () => {
  const subAgent: FeldApi.SubAgentSession = {
    id: "s1:agent-1", workspace: "default", actor: "dev", subAgent: "Explore", parentSession: "s1",
    start: "2026-09-25T00:05:00Z", end: "2026-09-25T00:06:00Z", startKind: "sub-agent", ownership: null, endKind: null, exitCode: null,
    execution: { type: "agent-runtime", label: "Claude Code", command: null, ai: true }, model: ["Claude Code", "claude-haiku"], placement: null, observation: null,
    outcome: "ended", consoleState: "lost", consoleDetail: "the transcript file was removed",
  };
  const input = toActorsInput(data({ actors: [apiActor("dev")], sessions: [apiSession({ id: "s1", actor: "dev" }), subAgent] }));
  const mapped = input.sessions[1];
  expect(mapped).toMatchObject({ id: "s1:agent-1", actor: "default/dev", parentSession: "s1", subAgent: "Explore", startKind: "sub-agent", ownership: null, endKind: null, placement: null, observation: null, consoleState: "lost" });
  expect(mapped.execution.command).toBeNull();
  // The Console names it under its parent's Actor and says why its transcript cannot be read.
  const known = consoleSessionsOf(input).find((session) => session.id === "s1:agent-1");
  expect(known).toMatchObject({ actor: "dev / Explore", placement: "Not tracked · inside s1", sourceUnavailable: "the transcript file was removed" });
});

test("a Session's metrics read as not answered, reported, or as the projection's status", () => {
  const projection = (session: string, metrics: FeldApi.SessionMetrics["metrics"]): FeldApi.SessionMetrics => ({ session, start: null, end: null, bucket: 60, metrics });
  const input = toActorsInput(data({
    actors: [apiActor("dev")],
    sessions: [apiSession({ id: "s1", actor: "dev" }), apiSession({ id: "s2", actor: "dev" }), apiSession({ id: "s3", actor: "dev" })],
    metrics: [
      projection("s2", [{ metric: "token_usage", status: "reported", source: "OTLP", series: [{ start: "2026-09-25T00:01:00Z", end: null, value: { input: 10, output: 5, cache_read: 3, cache_creation: 1 } }] }]),
      projection("s3", [{ metric: "token_usage", status: "n/a", source: "Program" }]),
    ],
  }));
  const [unread, reported, notApplicable] = input.sessions;
  expect(unread.metrics.tokens.status).toBe("reading");
  expect(reported.metrics.tokens).toEqual({ status: "reported", source: "OTLP", samples: [{ start: Date.parse("2026-09-25T00:01:00Z"), end: Date.parse("2026-09-25T00:02:00Z"), value: [10, 5, 3, 1] }] });
  // A metric the projection does not carry is unavailable, never zero.
  expect(reported.metrics.context.status).toBe("unavailable");
  expect(notApplicable.metrics.tokens).toEqual({ status: "n/a", source: "Program" });
});

test("a context window bin without a used percentage is no reading, and a Session with none has no usage record", () => {
  const projection = (session: string, series: FeldApi.MetricBin[]): FeldApi.SessionMetrics => ({
    session, start: null, end: null, bucket: 60,
    metrics: [{ metric: "context_window_utilization", status: "reported", source: "status line", series }],
  });
  const input = toActorsInput(data({
    actors: [apiActor("dev")],
    sessions: [apiSession({ id: "s1", actor: "dev" }), apiSession({ id: "s2", actor: "dev" })],
    metrics: [
      projection("s1", [
        { start: "2026-09-25T00:01:00Z", end: null, value: { size: 200000, used_percent: 20 } },
        { start: "2026-09-25T00:02:00Z", end: null, value: { size: 200000 } },
        { start: "2026-09-25T00:03:00Z", end: null, value: { size: 200000, used_percent: 30 } },
      ]),
      projection("s2", [{ start: "2026-09-25T00:01:00Z", end: null, value: { size: 200000 } }]),
    ],
  }));
  const [readings, none] = input.sessions;
  expect(readings.metrics.context).toMatchObject({ status: "reported", size: 200000 });
  expect(readings.metrics.context.status === "reported" && readings.metrics.context.samples.map((sample) => sample.value)).toEqual([20, 30]);
  expect(none.metrics.context).toEqual({ status: "unavailable", source: "No correlated Runtime usage record" });
});

test("a Runtime's context window lane leaves out a bin without a used percentage", () => {
  const at = (minute: number) => `2026-09-25T00:${String(minute).padStart(2, "0")}:00Z`;
  const input = toActorsInput(data({
    runtimes: [{ name: "claude-code", label: "Claude Code" }],
    runtimeMetrics: [{
      runtime: "claude-code",
      start: at(0),
      end: at(3),
      bucket: 60,
      metrics: [{ metric: "context_window_utilization", status: "reported", source: "status line", sessions: { reported: 1, unavailable: 0, "n/a": 0 }, accounts: [{ series: [
        { start: at(0), end: at(1), value: { size: 200000, used_percent: 20 } },
        { start: at(1), end: at(2), value: { size: 200000 } },
        { start: at(2), end: at(3), value: { size: 200000, used_percent: 30 } },
      ] }] }],
    }],
  }));
  const context = input.metrics.find((lane) => lane.id === "context-utilization")!;
  expect(context.series[0].samples?.map((sample) => sample.value)).toEqual([20, 30]);
});

test("a Runtime's prompt cache hit rate lane leaves out a bin whose denominator is 0, and holds no value past its bin", () => {
  const at = (minute: number) => `2026-09-25T00:${String(minute).padStart(2, "0")}:00Z`;
  const input = toActorsInput(data({
    runtimes: [{ name: "claude-code", label: "Claude Code" }],
    runtimeMetrics: [{
      runtime: "claude-code",
      start: at(0),
      end: at(3),
      bucket: 60,
      metrics: [{ metric: "token_usage", status: "reported", source: "OTLP", sessions: { reported: 1, unavailable: 0, "n/a": 0 }, accounts: [{ series: [
        { start: at(0), end: at(1), value: { input: 1, output: 5, cache_read: 3, cache_creation: 0 } },
        { start: at(1), end: at(2), value: { input: 0, output: 5, cache_read: 0, cache_creation: 0 } },
        { start: at(2), end: at(3), value: { input: 3, output: 5, cache_read: 1, cache_creation: 0 } },
      ] }] }],
    }],
  }));
  const cache = input.metrics.find((lane) => lane.id === "cache-hit-rate")!;
  expect(cache.level).toBeFalsy();
  expect(cache.series[0].samples?.map((sample) => [sample.at, sample.value])).toEqual([
    [Date.parse(at(0)), 75],
    [Date.parse(at(2)), 25],
  ]);
});

test("records become Schedules, skipped ticks, Spawns, and Messages keyed by workspace/name", () => {
  const input = toActorsInput(data({
    workspaces: ["research"],
    actors: [apiActor("lead", { workspace: "research" }), apiActor("triage", { workspace: "research", kind: "job", schedule: { cron: "*/30 * * * *", timezone: "UTC", enabled: false, createdAt: "2026-09-01T00:00:00Z", pausedAt: "2026-09-24T00:00:00Z", enabledAt: null, nextRun: null, skippedTicks: 1, overdueTick: null } })],
    sessions: [
      apiSession({ id: "s1", actor: "lead", workspace: "research" }),
      apiSession({ id: "s2", actor: "triage", workspace: "research", spawnedBy: { spawner: { type: "actor", id: "actor_lead", workspace: "research", name: "lead" }, spawnerSessionId: "s1" } }),
    ],
    records: [{ sequence: 7, at: "2026-09-25T00:30:00Z", kind: "schedule.skipped", actor: { id: "actor_triage", workspace: "research", name: "triage" }, sessionId: "s2", data: { planned: "2026-09-25T00:30:00Z", currentSessionId: "s2" } }],
    messages: [{ id: "m1", fifo: 1, from: { type: "admin" }, fromSessionId: null, to: { id: "actor_lead", workspace: "research", name: "lead" }, toSessionId: "s1", accepted: "2026-09-25T00:15:00Z", label: null, queued: null, read: null, preview: "go", anchor: "", waiting: true, deletedAt: null }],
  }));
  // A daemon without the default Workspace opens on its first one.
  expect(input.defaultWorkspace).toBe("research");
  expect(input.schedules).toEqual([{ actor: "research/triage", cron: "*/30 * * * *", timezone: "UTC", enabled: false, pausedAt: Date.parse("2026-09-24T00:00:00Z"), enabledAt: undefined, nextRun: null }]);
  expect(input.scheduleSkips).toEqual([{ id: "skip_7", actor: "research/triage", planned: Date.parse("2026-09-25T00:30:00Z"), reason: "previous session current", previousSession: "s2" }]);
  // A Spawn outside the records read is taken from the Session it created.
  expect(input.actions).toEqual([{ id: "action_s2", spawner: "research/lead", spawnerWorkspace: "research", spawnerSessionId: "s1", target: "research/triage", targetWorkspace: "research", session: "s2", at: Date.parse("2026-09-25T00:00:00Z") }]);
  expect(input.messages[0]).toMatchObject({ from: "Admin", fromWorkspace: null, to: "research/lead", toSessionId: "s1", read: null, anchor: null });
});

test("with nothing read the page holds no Actor, no lane series, and no choice", () => {
  const input = toActorsInput(null);
  expect(input).toMatchObject({ workspaces: [], defaultWorkspace: "default", actors: [], sessions: [] });
  expect(input.metrics.every((lane) => lane.series.length === 0)).toBe(true);
  expect(input.launchOptions).toEqual({ runtimes: [], providers: [], nodes: [] });
});

test("the lanes and New actor's choices follow the daemon's installed Runtimes, Providers, and Nodes", () => {
  const input = toActorsInput(data({
    runtimes: [{ name: "claude-code", label: "Claude Code" }],
    providers: [{ name: "tmux", label: "tmux" }],
    nodes: ["local"],
  }));
  const quota = input.metrics.find((lane) => lane.id === "quota-5h-claude-code")!;
  expect([quota.name, quota.caption]).toEqual(["Claude Code quota · 5h", "Claude Code · 5-hour window · % used"]);
  expect(quota.series.map((series) => [series.provider, series.reading])).toEqual([["Claude Code", true]]);
  const tokens = input.metrics.find((lane) => lane.id === "token-usage")!;
  expect(tokens.series.map((series) => series.provider)).toEqual(["Claude Code"]);
  expect(input.launchOptions).toEqual({ runtimes: [["claude-code", "Claude Code"]], providers: [["tmux", "tmux"]], nodes: [["local", "local"]] });
  expect(input.metrics.find((lane) => lane.id === "active-time")!.caption).toBe("Claude Code only · active seconds/min");
});

test("a lane's series takes its tone from the Runtime's Plugin name and holds only the Runtimes that have the metric", () => {
  const input = toActorsInput(data({ runtimes: [{ name: "codex", label: "Codex" }, { name: "gemini", label: "Gemini" }] }));
  const tokens = input.metrics.find((lane) => lane.id === "token-usage")!;
  expect(tokens.caption).toBe("Codex · Gemini · tokens/min");
  expect(tokens.series.map((series) => series.tone)).toEqual(["codex", "gemini"]);
  const active = input.metrics.find((lane) => lane.id === "active-time")!;
  expect([active.caption, active.series]).toEqual(["active seconds/min", []]);
});

test("the lanes read each Runtime's metrics read, and hold one series per account", () => {
  const at = (minute: number) => `2026-09-25T00:${String(minute).padStart(2, "0")}:00Z`;
  const quota = (account: { id: string; name: string } | null, usedPercent: number) => ({
    ...(account ? { account } : {}),
    window: "five_hour" as const,
    resetsAt: "2026-09-25T04:00:00Z",
    end: "2026-09-25T04:00:00Z",
    points: [{ at: at(1), usedPercent }],
    latest: { at: at(1), usedPercent },
    reportedBy: { session: "s1", workspace: "default", actor: "dev" },
  });
  const input = toActorsInput(data({
    runtimes: [{ name: "claude-code", label: "Claude Code" }, { name: "codex", label: "Codex" }],
    runtimeMetrics: [{
      runtime: "claude-code",
      start: at(0),
      end: at(2),
      bucket: 60,
      metrics: [{ metric: "token_usage", status: "reported", source: "OTLP", sessions: { reported: 3, unavailable: 0, "n/a": 0 }, accounts: [
        { series: [{ start: at(0), end: at(1), value: { total: 60 } }] },
        { account: { id: "a2", name: "personal@example.com" }, series: [{ start: at(0), end: at(1), value: { total: 120 } }] },
        { account: { id: "a1", name: "team@example.com" }, series: [{ start: at(0), end: at(1), value: { total: 180 } }] },
      ] }],
      quota: { status: "reported", source: "Claude Code status line", series: [quota({ id: "a1", name: "team@example.com" }, 30), quota({ id: "a2", name: "personal@example.com" }, 60)] },
    }],
  }));
  const tokens = input.metrics.find((lane) => lane.id === "token-usage")!;
  // Each account keeps the colour it has on the quota lanes; the unknown account's line
  // has one of its own.
  expect(tokens.series.map((series) => [series.provider, series.account, series.accountTone, series.reading ?? false, series.samples?.map((sample) => sample.value)])).toEqual([
    ["Claude Code", undefined, "account-3", false, [60]],
    ["Claude Code", "personal@example.com", "account-2", false, [120]],
    ["Claude Code", "team@example.com", "account-1", false, [180]],
    ["Codex", undefined, undefined, true, []],
  ]);
  const fiveHour = input.metrics.find((lane) => lane.id === "quota-5h-claude-code")!;
  expect(fiveHour.series.map((series) => [series.provider, series.account, series.accountTone])).toEqual([
    ["Claude Code", "team@example.com", "account-1"],
    ["Claude Code", "personal@example.com", "account-2"],
  ]);
  expect(input.metrics.find((lane) => lane.id === "quota-7d-codex")!.series.map((series) => [series.provider, series.reading])).toEqual([["Codex", true]]);
  // A quota lane shows by default once its Runtime reports its window.
  expect(input.metrics.filter((lane) => lane.kind === "quota").map((lane) => [lane.id, lane.defaultVisible])).toEqual([
    ["quota-5h-claude-code", true], ["quota-7d-claude-code", false], ["quota-7d-codex", false],
  ]);
});

test("every window a Runtime reports shows its lane by default, and a line without an account has a colour of its own", () => {
  const point = { at: "2026-09-25T00:01:00Z", usedPercent: 20 };
  const series = (account: { id: string; name: string } | null, window: "five_hour" | "seven_day" = "seven_day") => ({
    ...(account ? { account } : {}),
    window, resetsAt: "2026-09-30T00:00:00Z", end: "2026-09-30T00:00:00Z",
    points: [point], latest: point, reportedBy: { session: "s1", workspace: "default", actor: "dev" },
  });
  const input = toActorsInput(data({
    runtimes: [{ name: "claude-code", label: "Claude Code" }, { name: "codex", label: "Codex" }],
    runtimeMetrics: [
      { runtime: "claude-code", start: null, end: null, bucket: 60, metrics: [], quota: { status: "reported", source: "Claude Code status line", series: [series({ id: "a2", name: "personal@example.com" }, "five_hour"), series({ id: "a2", name: "personal@example.com" })] } },
      { runtime: "codex", start: null, end: null, bucket: 60, metrics: [], quota: { status: "reported", source: "Codex rollout", series: [series({ id: "a1", name: "team@example.com" }), series(null)] } },
    ],
  }));
  expect(input.metrics.filter((lane) => lane.kind === "quota").map((lane) => [lane.id, lane.defaultVisible])).toEqual([
    ["quota-5h-claude-code", true], ["quota-7d-claude-code", true], ["quota-7d-codex", true],
  ]);
  expect(input.metrics.find((lane) => lane.id === "quota-7d-codex")!.series.map((item) => [item.account, item.accountTone])).toEqual([["team@example.com", "account-1"], [undefined, "account-2"]]);
});

test("an Actor's launchSpec keeps its env, and an empty env is none", () => {
  const input = toActorsInput(data({
    actors: [
      apiActor("with-env", { launchSpec: { runtime: "claude-code", workdir: "/work", env: { GH_CONFIG_DIR: "/gh" }, provider: "tmux", node: "local" } }),
      apiActor("empty-env", { launchSpec: { runtime: "claude-code", workdir: "/work", env: {}, provider: "tmux", node: "local" } }),
    ],
  }));
  expect(input.actors[0].launchSpec?.env).toEqual({ GH_CONFIG_DIR: "/gh" });
  expect(input.actors[1].launchSpec?.env).toBeUndefined();
});

test("an Actor keeps its own Sessions, Messages, and Spawns when another Actor holds its name", () => {
  const old = apiActor("dev", { id: "actor_old", deletedAt: "2026-09-20T00:00:00Z" });
  const input = toActorsInput(data({
    actors: [old, apiActor("dev", { id: "actor_new" }), apiActor("gone", { id: "actor_gone", deletedAt: "2026-09-21T00:00:00Z" })],
    sessions: [
      apiSession({ id: "s_old", actor: "dev", actorId: "actor_old", spawnedBy: { spawner: { type: "admin" }, spawnerSessionId: null } } as never),
      apiSession({ id: "s_new", actor: "dev", actorId: "actor_new" }),
      { id: "s_old_subagent_a", workspace: "default", actor: "dev", subAgent: "a", parentSession: "s_old", start: "2026-09-19T00:00:00Z", end: null, startKind: "sub-agent", ownership: null, endKind: null, exitCode: null } as never,
    ],
    messages: [{ id: "m1", fifo: 1, from: { type: "actor", id: "actor_new", workspace: "default", name: "dev" }, fromSessionId: "s_new", to: { id: "actor_old", workspace: "default", name: "dev" }, toSessionId: null, accepted: "2026-09-19T00:00:00Z", label: null, queued: null, read: null, preview: "", anchor: "", waiting: true, deletedAt: null }],
  }));
  expect(input.actors.map((actor) => [actor.id, actor.actorId, actor.deletedAt])).toEqual([
    ["default/dev~actor_old", "actor_old", Date.parse("2026-09-20T00:00:00Z")],
    ["default/dev", "actor_new", null],
    ["default/gone", "actor_gone", Date.parse("2026-09-21T00:00:00Z")],
  ]);
  expect(input.sessions.map((session) => [session.id, session.actor])).toEqual([
    ["s_old", "default/dev~actor_old"],
    ["s_new", "default/dev"],
    ["s_old_subagent_a", "default/dev~actor_old"],
  ]);
  expect(input.messages[0]).toMatchObject({ from: "default/dev", to: "default/dev~actor_old" });
  expect(input.actions[0]).toMatchObject({ session: "s_old", target: "default/dev~actor_old" });
});

test("an Actor's current Session is its listing's, whatever the range", () => {
  const input = toActorsInput(data({ actors: [apiActor("dev", { currentSession: apiSession({ id: "s1", actor: "dev", ownership: "attached" }) })] }));
  expect(input.actors[0].currentSession).toEqual({ id: "s1", ownership: "attached" });
});
