import { expect, test } from "vitest";
import { badgeStateOf, formatDuration, phaseOf, pluginRowsOf, readIntervalOf, restartingOf, skewSecondsOf, POLL_INTERVAL, RESTART_LIMIT, RETRY_INTERVAL } from "./model.ts";

const status = {
  state: "running",
  instanceId: "dmn_1",
  pid: 1,
  home: "/h",
  api: "http://127.0.0.1:7381",
  startedAt: "2026-09-01T00:00:00Z",
  serverTime: "2026-09-02T06:00:02Z",
  historyStart: null,
  timezone: "Asia/Tokyo",
  runtimes: [],
  providers: [{ name: "tmux", label: "tmux" }, { name: "docker", label: "Docker" }],
  nodes: ["Local Node", "Build Node"],
  webUi: null,
  otlp: "http://127.0.0.1:7382",
  restored: { actors: 0, runningSessions: 0, schedules: 0 },
} as unknown as FeldApi.DaemonStatus;

const outcome = (status: "pending" | "error" | "success", error: unknown, dataUpdatedAt: number, errorUpdatedAt: number) => ({ status, error, dataUpdatedAt, errorUpdatedAt });

test("the phase follows the newest outcome", () => {
  expect(phaseOf(outcome("pending", null, 0, 0), 0)).toBe("reading");
  expect(phaseOf(outcome("success", null, 10, 0), 0)).toBe("reachable");
  expect(phaseOf(outcome("error", new Error("fetch failed"), 10, 20), 0)).toBe("unreachable");
  expect(phaseOf(outcome("error", { status: 500 }, 0, 10), 0)).toBe("unreachable");
});

test("a shutdown makes the daemon unreachable until a newer read answers", () => {
  expect(phaseOf(outcome("success", null, 10, 0), 20)).toBe("unreachable");
  expect(phaseOf(outcome("pending", null, 0, 0), 20)).toBe("unreachable");
  expect(phaseOf(outcome("success", null, 30, 0), 20)).toBe("reachable");
});

test("a restart is under way until another instance answers, within the limit", () => {
  const restart = { at: 1000, instanceId: "dmn_1" };
  expect(restartingOf(restart, status, 2000)).toBe(true);
  expect(restartingOf(restart, { ...status, instanceId: "dmn_2" }, 2000)).toBe(false);
  expect(restartingOf(restart, status, 1000 + RESTART_LIMIT)).toBe(false);
  expect(restartingOf(null, status, 2000)).toBe(false);
});

test("the next read waits by phase", () => {
  expect(readIntervalOf("reachable")).toBe(POLL_INTERVAL);
  expect(readIntervalOf("unreachable")).toBe(RETRY_INTERVAL);
  expect(readIntervalOf("reading")).toBe(false);
});

test("the badge tells a missing Collector", () => {
  expect(badgeStateOf("reachable", status)).toBe("running");
  expect(badgeStateOf("reachable", { ...status, otlp: null })).toBe("degraded");
  expect(badgeStateOf("unreachable", status)).toBe("unreachable");
});

test("the plugin rows list every bundled plugin, installed or not, then the home's own", () => {
  expect(pluginRowsOf("provider", [...status.providers, { name: "herdr", label: "Herdr (local)" }] as FeldApi.Plugin[])).toEqual([
    { name: "tmux", label: "tmux", installed: true },
    { name: "herdr", label: "Herdr (local)", installed: true },
    { name: "docker", label: "Docker", installed: true },
  ]);
  expect(pluginRowsOf("runtime", [])).toEqual([
    { name: "claude-code", label: "Claude Code", installed: false },
    { name: "codex", label: "Codex", installed: false },
  ]);
});

test("durations and the clock skew read as the page shows them", () => {
  expect(formatDuration(59_000)).toBe("0m");
  expect(formatDuration((2 * 60 + 5) * 60_000)).toBe("2h 5m");
  expect(formatDuration(((1 * 24 + 5) * 60 + 47) * 60_000)).toBe("1d 5h 47m");
  expect(skewSecondsOf("2026-09-02T06:00:02Z", Date.parse("2026-09-02T06:00:00Z"))).toBe(2);
  expect(skewSecondsOf("2026-09-02T06:00:00Z", Date.parse("2026-09-02T06:00:03Z"))).toBe(-3);
  expect(skewSecondsOf("2026-09-02T06:00:00Z", Date.parse("2026-09-02T06:00:00.2Z"))).toBe(0);
});
