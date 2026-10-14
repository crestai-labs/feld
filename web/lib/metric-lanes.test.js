import assert from "node:assert/strict";
import { test } from "vitest";
import { quotaAccounts, quotaLane, levelReadingAt, latestReading, resetsWithin, reporterAt } from "./metric-lanes.ts";

test("metric-lanes", () => {
  const plain = (value) => JSON.parse(JSON.stringify(value));

  const T = (minutes) => Date.UTC(2026, 8, 19, 12, minutes);
  const iso = (instant) => new Date(instant).toISOString();
  const reporter = (session) => ({ session, workspace: "default", actor: "dev" });
  const series = (window, resetsAt, points, by) => ({
    window,
    resetsAt: iso(resetsAt),
    end: iso(resetsAt),
    points: points.map(([at, usedPercent]) => ({ at: iso(at), usedPercent })),
    latest: points.length ? { at: iso(points.at(-1)[0]), usedPercent: points.at(-1)[1] } : null,
    reportedBy: reporter(by),
  });

  // The quota read answers a window that reset inside the range with one series per
  // resets_at. The daemon orders them by window and reset, so the series that ended comes
  // first; the lane has to keep the one that follows it.
  const runtime = {
    runtime: "claude-code",
    status: "reported",
    source: "Claude Code status line",
    series: [
      series("five_hour", T(60), [[T(0), 10], [T(30), 40]], "s1"),
      series("five_hour", T(360), [[T(90), 5], [T(120), 20]], "s2"),
      series("seven_day", T(10_000), [[T(0), 3], [T(120), 4]], "s2"),
    ],
  };

  const fiveHour = quotaLane(runtime, "five_hour");
  assert.deepEqual(plain(fiveHour.samples).map((sample) => [sample.at, sample.value, sample.until]), [
    [T(0), 10, T(60)],
    [T(30), 40, T(60)],
    [T(90), 5, T(360)],
    [T(120), 20, T(360)],
  ]);
  assert.deepEqual(plain(fiveHour.resets), [T(60), T(360)]);
  assert.deepEqual(plain(fiveHour.segments).map((segment) => [segment.start, segment.end, segment.reporter.session]), [
    [T(0), T(60), "s1"],
    [T(90), T(360), "s2"],
  ]);

  // A reading holds until the next report, and past the reset that ends its series the
  // window has reset to 0 until the next report arrives.
  assert.equal(levelReadingAt(fiveHour.samples, T(-5)), 10);
  assert.equal(levelReadingAt(fiveHour.samples, T(15)), 10);
  assert.equal(levelReadingAt(fiveHour.samples, T(45)), 40);
  assert.equal(levelReadingAt(fiveHour.samples, T(59)), 40);
  assert.equal(levelReadingAt(fiveHour.samples, T(60)), 0);
  assert.equal(levelReadingAt(fiveHour.samples, T(75)), 0);
  assert.equal(levelReadingAt(fiveHour.samples, T(90)), 5);
  assert.equal(levelReadingAt(fiveHour.samples, T(200)), 20);
  assert.equal(levelReadingAt(fiveHour.samples, T(360)), 0);

  // A level reading without a reset (the context window) holds until the next one.
  assert.equal(levelReadingAt([{ at: T(0), value: 7 }, { at: T(10), value: 12 }], T(500)), 12);
  assert.equal(levelReadingAt([], T(0)), 0);

  assert.deepEqual(plain(resetsWithin(fiveHour.resets, { start: T(0), end: T(100) })), [T(60)]);
  assert.deepEqual(plain(resetsWithin(fiveHour.resets, { start: T(0), end: T(400) })), [T(60), T(360)]);
  assert.deepEqual(plain(resetsWithin(fiveHour.resets, { start: T(61), end: T(360) })), []);

  assert.equal(reporterAt(fiveHour.segments, T(-5)).session, "s1");
  assert.equal(reporterAt(fiveHour.segments, T(70)).session, "s1");
  assert.equal(reporterAt(fiveHour.segments, T(90)).session, "s2");
  assert.equal(reporterAt(fiveHour.segments, T(500)).session, "s2");

  // The other window is its own lane, and a window without a series is unavailable.
  assert.deepEqual(plain(quotaLane(runtime, "seven_day").samples).map((sample) => sample.value), [3, 4]);
  assert.equal(quotaLane(runtime, "one_hour"), null);
  assert.equal(quotaLane(undefined, "five_hour"), null);
  assert.equal(quotaLane({ series: [series("five_hour", T(60), [], "s1")] }, "five_hour"), null);

  // A rate lane reads its latest bin in the range, a 0 it measured included; a bin that
  // ended before the range is no sample in it.
  const bins = [{ at: T(0), end: T(1), value: 30 }, { at: T(5), end: T(6), value: 0 }];
  assert.deepEqual(plain(latestReading(bins, { start: T(0), end: T(10) }, false)), { at: T(5), end: T(6), value: 0 });
  assert.deepEqual(plain(latestReading(bins, { start: T(0), end: T(4) }, false)), { at: T(0), end: T(1), value: 30 });
  assert.equal(latestReading(bins, { start: T(6), end: T(10) }, false), null);
  assert.equal(latestReading([], { start: T(0), end: T(10) }, false), null);

  // A level lane reads what it holds at the range's end, from its latest report.
  assert.deepEqual(plain(latestReading(fiveHour.samples, { start: T(100), end: T(150) }, true)), { at: T(120), end: T(120), value: 20 });
  // Past the reset that ends its series, a level lane reads the reset, not a measured 0.
  assert.deepEqual(plain(latestReading(fiveHour.samples, { start: T(40), end: T(80) }, true)), { at: T(60), end: T(60), value: 0, resetAt: T(60) });
  assert.deepEqual(plain(latestReading(fiveHour.samples, { start: T(40), end: T(60) }, true)), { at: T(30), end: T(30), value: 40 });

  // Each account's reports are a line of their own; the reports without one are another.
  const accounts = {
    series: [
      { ...series("five_hour", T(300), [[T(0), 10]], "s1"), account: { id: "a1", name: "team@example.com" } },
      { ...series("five_hour", T(300), [[T(10), 50]], "s2"), account: { id: "a2", name: "" } },
      series("five_hour", T(300), [[T(20), 5]], "s3"),
    ],
  };
  assert.deepEqual(quotaAccounts(accounts, "five_hour"), [{ id: "a1", name: "team@example.com" }, { id: "a2", name: "a2" }, null]);
  assert.deepEqual(plain(quotaLane(accounts, "five_hour", "a2").samples).map((sample) => sample.value), [50]);
  assert.deepEqual(plain(quotaLane(accounts, "five_hour").samples).map((sample) => sample.value), [5]);
});
