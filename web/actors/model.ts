// The Actors page's derivations. The snapshot (ActorsInput), the clock, and the Console's
// reads are closed over by createModel, which the page builds again whenever one of them
// changes; every derivation reads them as the page's module globals were read before, so
// the bodies below keep the rules they had. What does not read them is exported as it is.
// Markup is drawn by the page's components; the computations that were embedded in it are
// named here so the components only lay them out.

import { commandError, commandLine, splitCommand } from "../lib/command-line.ts";
import { latestReading, levelReadingAt, reporterAt, resetsWithin } from "../lib/metric-lanes.ts";
import type { ConsoleReads } from "../console/reads.ts";
import { zoneLabel, type HourCycle } from "../shell/preferences.ts";
import type {
  ActorDraft,
  ActorLaunchSpec,
  ActorRecord,
  ActorsInput,
  AgentSession,
  AgentsState,
  BinSample,
  ContextMetric,
  CreatedActor,
  CronSpec,
  EndRequest,
  Focus,
  Instant,
  Lane,
  LaunchOptions,
  ActorState,
  LegendGroup,
  MessageRead,
  MessageRecord,
  ListedMetricLane,
  MetricLane,
  MetricLaneSpec,
  MetricSeries,
  NewActorResult,
  PlottedSeries,
  RelatedRow,
  EditDraft,
  ScheduleRecord,
  ScheduleRun,
  ScheduleSkip,
  SessionObservation,
  SpawnAction,
  StartOutcome,
  TokenMetric,
  VisibilityKey,
  ZonedParts,
} from "./types.ts";

// ---- Time --------------------------------------------------------------------------

export const MINUTE = 60 * 1000;
export const HOUR = 60 * MINUTE;
export const DAY = 24 * HOUR;

// The clock the page reads. Every render takes one instant so the lanes, the axis, and
// the readings of one frame agree; Live mode follows it as it advances. The timezone is
// the viewer's display zone, and the hour cycle is the viewer's (shell/preferences.ts).
export interface ActorsClock {
  now: Instant;
  timeZone: string;
  hourCycle?: HourCycle;
}

// ---- Geometry ------------------------------------------------------------------------

// Actor lane geometry. `mainOffset` is the main line's y inside the lane; the metric
// histogram sits above it.
export const LANE = Object.freeze({ laneHeight: 44, mainOffset: 30, subAgentRowHeight: 28, barBase: 2, barRange: 13 });
export const METRIC_LANE_HEIGHT = 56;
// A lane's chart runs from the lane's top line to its bottom: 100 % of the range on the
// line that parts it from the lane above, 0 at the bottom.
export const METRIC_CHART_INSET = Object.freeze({ top: 0, bottom: 0 });
// Each reading row past the second makes a lane this much taller.
export const METRIC_ROW_HEIGHT = 16;
// A lane lists this many series and folds the rest behind "+N more".
export const METRIC_ROW_LIMIT = 3;
// The colours of accounts (--series-account-N), apart from the Runtime colours.
export const ACCOUNT_TONE_COUNT = 6;
// Height of the marks line (unread mail, skipped ticks) under an Actor's details.
export const UNREAD_ROW_HEIGHT = 22;
// A Message or Spawn stub whose other end is not drawn runs to this inset from the lane edge.
export const MESSAGE_STUB_INSET = 6;
// The one line of labels of a time axis row.
export const AXIS_ROW_HEIGHT = 22;
export const PLOT_PAD_LEFT = 14;
export const PLOT_PAD_RIGHT = 24;
// Fallback for --agents-timeline-min-width in styles/agents.css; the effective floor is
// read from that property so the compact layout can lower it without a second constant.
export const TIMELINE_MIN_WIDTH = 280;
// Mirrors the compact breakpoint in styles/shell.css and styles/agents.css.
export const COMPACT_LAYOUT_QUERY = "(max-width: 760px)";
export const TOKEN_BIN_PIXEL_TARGET = 6;
export const TOKEN_BIN_OPTIONS = [MINUTE, 2 * MINUTE, 5 * MINUTE, 10 * MINUTE, 15 * MINUTE, 30 * MINUTE, HOUR, 2 * HOUR, 6 * HOUR];
export const WINDOW_OPTIONS: readonly (readonly [string, number])[] = [
  ["15m", 15 * MINUTE],
  ["30m", 30 * MINUTE],
  ["1h", HOUR],
  ["3h", 3 * HOUR],
  ["12h", 12 * HOUR],
  ["1d", DAY],
  ["3d", 3 * DAY],
  ["7d", 7 * DAY],
];

export const DEFAULT_WINDOW = HOUR;
// The Metrics windows a reader can choose, as Quick ranges. The window is never narrower
// than the Timeline's, so a choice narrower than the Timeline reads as the Timeline's
// until the Timeline narrows again.
export const METRICS_WINDOWS: readonly number[] = Object.freeze([HOUR, DAY, 7 * DAY]);
export const METRICS_WINDOW_STORAGE_KEY = "feld-mock-metrics-window";

// The Metrics window is a context around the Timeline's range: never narrower, so the
// Timeline's range is always inside it. Its own width is the reader's choice; where it
// sits follows the Timeline, centred on it and ending at now at the latest, so a Live
// Timeline has a Live Metrics window too.
export function metricsDurationFor(state: Pick<AgentsState, "duration" | "metricsDuration">) {
  return Math.max(state.duration, state.metricsDuration);
}

export function metricsRangeOf(range: FeldMetricRange, duration: number, now: Instant): FeldMetricRange {
  if (duration <= range.end - range.start) return range;
  const end = Math.min(now, (range.start + range.end) / 2 + duration / 2);
  return { start: end - duration, end };
}

// A range that ends at now follows the clock: a pan that reaches now is Live again, and
// a Metrics window that does is read as Live.
export function reachesNow(range: FeldMetricRange, now: Instant) {
  return range.end >= now;
}

// The Metrics window the Runtime metrics are read over (see metricsRangeOf). A window
// that ends at now, as a Live range's does and a past range's wider window can, follows
// the clock like a Live range, so its read keeps one key.
export function metricsQueryOf(state: Pick<AgentsState, "mode" | "anchor" | "duration" | "metricsDuration">, now: Instant): FeldLiveQuery {
  const duration = metricsDurationFor(state);
  const live: FeldLiveQuery = { workspace: null, start: now - duration, end: null, duration };
  if (state.mode === "live") return live;
  const range = metricsRangeOf({ start: state.anchor - state.duration / 2, end: state.anchor + state.duration / 2 }, duration, now);
  if (reachesNow(range, now)) return live;
  return { workspace: null, start: range.start, end: range.end, duration };
}

// How the range reads on its button: a live range by its length, as a Quick range names
// it, a historical one by its dates.
export const WINDOW_NAMES: Readonly<Record<number, string>> = Object.freeze({
  [15 * MINUTE]: "15 minutes", [30 * MINUTE]: "30 minutes", [HOUR]: "1 hour", [3 * HOUR]: "3 hours",
  [12 * HOUR]: "12 hours", [DAY]: "24 hours", [3 * DAY]: "3 days", [7 * DAY]: "7 days",
});

// Whether the range follows the daemon: a Live range that is not paused re-reads on each
// change the daemon announces over SSE, follows the current Sessions' tails, and moves now
// with the clock. Otherwise a notification only marks the range's reads stale, and the
// range and the tails are read on Refresh.
export function followsLive(state: Pick<AgentsState, "mode" | "paused">) {
  return state.mode === "live" && !state.paused;
}

// ---- Time axis ---------------------------------------------------------------------------
// Ticks at round wall-clock times in the display zone: the smallest step that leaves each
// label its room, from 5 minutes up to a day. A tick too near an edge for its label to
// fit is dropped.
const AXIS_STEPS = [5, 10, 15, 30, 60, 120, 180, 360, 720, 1440].map((minutes) => minutes * MINUTE);
// The minor step between two labelled ticks, for the Timeline's grid.
const MINOR_STEPS = new Map([[5, 1], [10, 5], [15, 5], [30, 10], [60, 15], [120, 30], [180, 60], [360, 120], [720, 180], [1440, 360]]
  .map(([major, minor]) => [major * MINUTE, minor * MINUTE]));

// The zone's offset from UTC at `instant`, as wall clock minus instant.
export function zoneOffset(instant: Instant, timeZone: string) {
  const parts = Object.fromEntries(zoneClockFor(timeZone).formatToParts(instant).map((part) => [part.type, Number(part.value)]));
  const wall = Date.UTC(parts.year, parts.month - 1, parts.day, parts.hour, parts.minute, parts.second);
  return wall - Math.floor(instant / 1000) * 1000;
}

function axisRoom(range: FeldMetricRange) {
  return range.end - range.start > DAY ? 72 : 64;
}

export function axisStep(range: FeldMetricRange, usable: number) {
  const duration = range.end - range.start;
  return AXIS_STEPS.find((candidate) => duration / candidate <= usable / axisRoom(range)) || AXIS_STEPS[AXIS_STEPS.length - 1];
}

export function minorStepOf(step: number) {
  return MINOR_STEPS.get(step) ?? null;
}

// Every instant of `step` in the range, at round wall-clock times in the zone.
export function stepTimes(range: FeldMetricRange, step: number, timeZone: string) {
  const offset = zoneOffset(range.start, timeZone);
  const times: Instant[] = [];
  for (let tick = Math.ceil((range.start + offset) / step) * step - offset; tick <= range.end; tick += step) times.push(tick);
  return times;
}

export function axisTicks(range: FeldMetricRange, usable: number, timeZone: string) {
  const duration = range.end - range.start;
  const edge = duration / usable * (axisRoom(range) / 2);
  return stepTimes(range, axisStep(range, usable), timeZone)
    .filter((tick) => tick - range.start >= edge * .6 && range.end - tick >= edge * .6);
}

// The labels of a time axis, on one line. Ticks a day apart name their date. Otherwise a
// tick names its time, and the first tick and each tick on a new day name the date before
// it. Each label is centred on its tick and kept inside the axis. The labels of a new day
// are placed first, then the first tick's, then the rest; a label that would overlap one
// already placed is dropped, except that the first tick's falls back to its time alone.
export interface AxisLabel {
  instant: Instant;
  left: number;
  date: string | null;
  time: string | null;
}

const AXIS_LABEL_GAP = 8;

export function axisLabels(ticks: Instant[], x: (instant: Instant) => number, width: number, format: { date: (instant: Instant) => string; time: (instant: Instant) => string }, measure: (text: string, bold: boolean) => number): AxisLabel[] {
  const daily = ticks.length > 1 && ticks[1] - ticks[0] >= DAY;
  const fit = (instant: Instant, date: string | null, time: string | null) => {
    const size = (date ? measure(date, true) : 0) + (date && time ? measure(" ", false) : 0) + (time ? measure(time, false) : 0);
    const left = Math.max(0, Math.min(x(instant) - size / 2, width - size));
    return { instant, left, right: left + size, date, time };
  };
  const candidates = ticks.map((instant, index) => {
    const date = format.date(instant);
    const time = daily ? null : format.time(instant);
    const newDay = daily || (index > 0 && date !== format.date(ticks[index - 1]));
    const choices = newDay || index === 0 ? [fit(instant, date, time)] : [];
    if (!newDay && time) choices.push(fit(instant, null, time));
    return { rank: newDay ? 0 : index === 0 ? 1 : 2, choices };
  });
  const placed: ReturnType<typeof fit>[] = [];
  for (const { choices } of [...candidates].sort((a, b) => a.rank - b.rank)) {
    const label = choices.find((choice) => placed.every((other) => choice.right + AXIS_LABEL_GAP <= other.left || other.right + AXIS_LABEL_GAP <= choice.left));
    if (label) placed.push(label);
  }
  return placed.sort((a, b) => a.instant - b.instant).map(({ instant, left, date, time }) => ({ instant, left, date, time }));
}

export function isMidnight(instant: Instant, timeZone: string) {
  return (instant + zoneOffset(instant, timeZone)) % DAY === 0;
}

// The vertical grid of a plot: a major line at every labelled tick and, with `minor`, a
// lighter one at each minor step between them.
export function gridLines(range: FeldMetricRange, usable: number, timeZone: string, { minor: withMinor = true } = {}) {
  const step = axisStep(range, usable);
  const minor = (withMinor && minorStepOf(step)) || step;
  return stepTimes(range, minor, timeZone).map((instant) => ({ instant, major: (instant + zoneOffset(instant, timeZone)) % step === 0 }));
}

// ---- Actor kind ------------------------------------------------------------------------

// Actor kind is meaning declared at creation, not an inference from the executable,
// duration, or whether the execution uses a model. Execution shape belongs to each
// Session below because one durable Actor can change it between executions.
// In the shared lifecycle the central rule the kind changes is how the unrequested
// exit of an `owned` Session is concluded; the page keeps that branch behind
// KIND_RULES so it stays visible (see outcomeFor). The other surface rules that
// differ by kind are that only a Job carries a Schedule and that `feld actor create`
// starts an Agent by default and a Job on request. Every Actor has a mailbox and one
// current Session slot, and admission, fence, token, and Provider do not depend
// on the kind.
export const KIND_RULES: Readonly<Record<string, { label: string; unrequestedExit: string; summary: string; rule: string }>> = Object.freeze({
  agent: Object.freeze({
    label: "Agent",
    // An Agent loop runs until Feld records a stop or kill; an exit without one is not
    // a normal end, whatever the exit code.
    unrequestedExit: "attention",
    summary: "An Agent Loop that runs until a stop or kill is requested.",
    rule: "An owned Session that exits without a recorded stop or kill request needs attention, whatever its exit code.",
  }),
  job: Object.freeze({
    label: "Job",
    // A bounded program ends on its own; the exit code is the outcome.
    unrequestedExit: "exit-code",
    summary: "A bounded program that ends on its own; its exit code is the outcome.",
    rule: "An owned Session that exits without a stop or kill request completed normally: exit 0 is succeeded, anything else is failed; with no exit code reported the outcome is unknown.",
  }),
});

export function kindRules(kind: string) {
  return KIND_RULES[kind] || KIND_RULES.agent;
}

export function kindLabel(kind: string) {
  return kindRules(kind).label;
}

// ---- Metrics lanes ---------------------------------------------------------------
// The Sessions' metrics aggregated per Runtime by each metric's own rule (the Runtime
// metrics read over the Metrics window) and shown as one lane per metric: additive metrics (tokens, cost, counts, active time) are summed
// over the Sessions of a bin; the cache hit rate, the latency p95, and the context
// window follow their own rule, and each `derivation` says which. Sub Agent Sessions are
// not counted: their telemetry is already in the parent Session. The quota lanes are the
// exception: an account's rate limit belongs to the account, not to any Session, so a
// quota lane holds one series per account from the same read's quota. A series whose signal cannot be collected carries
// its `status` (`n/a` or `unavailable`) and the `source` that explains it; one that can
// but has no sample in the range has no samples. Neither is plotted as zero, and each
// lane reading says which case it is.
export const QUOTA_DERIVATION = "Feld groups the reports of each Runtime and window by the account of the reporting Session, one series per account, each with its latest reporting Session. Reset markers are the reported resets_at";
export const COUNT_DERIVATION = "summed over every Session of the account in the bin; a Session whose token usage is reported counts 0 when none arrived";
// A series' tone by the Runtime's Plugin name; another Runtime's tone is its name.
export const RUNTIME_TONES: Readonly<Record<string, string>> = Object.freeze({ "claude-code": "claude", codex: "codex" });
export const NO_SESSION_SOURCE = "No Session of this Runtime ran in the range";
export const NO_QUOTA_SOURCE = "No Session reported this Runtime's rate limit";
// The `unavailable` sources that mean the Runtime's telemetry is collected but no data
// point fell in the range: the lane reads them as No samples in range.
export const NO_SAMPLE_SOURCES = Object.freeze(["No correlated Runtime usage record", "No rate limit report"]);

// One lane per metric, with one series per installed Runtime that has the metric; the
// caption leads with those Runtimes' display names. `metric` names the metric of the Runtime metrics read that
// feeds it, `level` marks a lane whose reading holds until the next one (a quota or the
// context window) rather than falling back to zero between bins, and `window` names the
// quota window a quota lane reads.
export const METRIC_LANES: readonly MetricLaneSpec[] = Object.freeze([
  // A quota lane is one per Runtime and window (see quotaLaneOf); only Claude Code has a 5-hour lane.
  {
    id: "quota-5h", name: "quota · 5h", caption: "5-hour window · % used", unit: "%", maximum: 100,
    kind: "quota", level: true, window: "five_hour", runtimes: ["claude-code"], showResets: true, defaultVisible: true, derivation: QUOTA_DERIVATION,
  },
  {
    id: "quota-7d", name: "quota · 7d", caption: "7-day window · % used", unit: "%", maximum: 100,
    kind: "quota", level: true, window: "seven_day", showResets: true, defaultVisible: true, derivation: QUOTA_DERIVATION,
  },
  {
    id: "token-usage", name: "Token usage", caption: "tokens/min", unit: " tok/min", maximum: 2600,
    metric: "token_usage", defaultVisible: true,
    derivation: "Feld maps each Runtime's tokens to input, output, cache read, and cache creation, sums them over every Session of the account in the bin, and divides by the bin duration",
  },
  {
    id: "estimated-cost", name: "Estimated cost", caption: "USD/hour", prefix: "$", unit: "/h", decimals: 2, maximum: 6,
    metric: "estimated_cost", defaultVisible: true,
    derivation: "The Runtime's own estimate, summed over every Session of the account in the bin; not a billing record",
  },
  {
    id: "api-errors", name: "API errors", caption: "errors/hour", unit: "/h", maximum: 12,
    metric: "api_errors", defaultVisible: true,
    derivation: `Failed model API requests ${COUNT_DERIVATION}`,
  },
  {
    id: "cache-hit-rate", name: "Prompt cache hit rate", caption: "derived", unit: "%", maximum: 100,
    metric: "token_usage", defaultVisible: false,
    derivation: "Feld sums each token kind over every Session of the account in the bin, then takes cache_read / (input + cache_read + cache_creation); a bin whose denominator is 0 is empty",
  },
  {
    id: "request-latency", name: "API request latency p95", caption: "ms", unit: " ms", maximum: 3200,
    metric: "api_request_latency_p95", defaultVisible: false,
    derivation: "Feld takes the p95 of every request duration of the account's Sessions in the bin, interpolated within the bucket where the Runtime reports a histogram",
  },
  {
    id: "tool-calls", name: "Tool calls", caption: "calls/min", unit: "/min", maximum: 12,
    metric: "tool_calls", defaultVisible: false,
    derivation: `Tool calls ${COUNT_DERIVATION}`,
  },
  {
    id: "context-utilization", name: "Context window utilization", caption: "% of window", unit: "%", maximum: 100,
    metric: "context_window_utilization", level: true, defaultVisible: false,
    derivation: "Feld takes each Session's latest value in the bin and shows the largest of the account's Sessions",
  },
  {
    id: "active-time", name: "Active time", caption: "active seconds/min", unit: " s/min", maximum: 180,
    metric: "active_time", runtimes: ["claude-code"], defaultVisible: false,
    derivation: "Feld sums the active seconds of every Session of the account in the bin and divides by the bin duration; with several Sessions it exceeds 60 s/min",
  },
]);

// ---- Session metrics ------------------------------------------------------------------
// Runtime telemetry attributed to a Session. Feld stores it as the Session's Telemetry
// under the Session's token, apart from the Session record, and projects it into one
// series per metric when read. Each metric carries its own `status` and the `source` that
// explains it, decided in this order:
//   n/a          a program Session (no Runtime Adapter attributes telemetry to it, even
//                when the program calls a model itself); a Sub Agent Session, whose
//                telemetry is counted in its parent Session; or a Runtime that does not
//                provide the metric
//   unavailable  no usable Runtime telemetry correlated with the Session (an attached
//                Codex, whose export header was fixed at launch without a token,
//                keeps only what its hook reads out of the rollout file)
//   reported     telemetry exists; `samples` are spaced evenly over the Session
// Only `reported` Sessions enter an aggregate, so the band histogram and the Metrics lanes never count a Sub Agent Session on top of its parent.
// Token usage samples are [input, output, cache_read, cache_creation] per interval; the
// total is derived at read time, never stored. Context window samples are the used
// percentage of the reported window `size`: it rises as the conversation grows and drops
// where the Runtime compacted it. Account rate limits (5-hour / 7-day quota) belong to
// the account, not to a Session, and appear only in the Metrics lanes below the timeline.
export const TOKEN_KINDS = Object.freeze(["input", "output", "cache_read", "cache_creation"]);
export const TOKEN_KIND_LABELS: Readonly<Record<string, string>> = Object.freeze({ input: "input", output: "output", cache_read: "cache read", cache_creation: "cache creation" });

// ---- Schedule derivations -------------------------------------------------------------
// 5-field cron expressions are evaluated in the Schedule's IANA timezone, with the DST
// rules the browser's Intl data carries for it. Supported per field: `*`, numbers, lists,
// ranges, `/step`, `?` in the day fields, and the daemon's month and day-of-week names.
// As in standard cron, when neither day field starts with `*` or `?`, a day either allows matches.
// 400 years, the horizon within which the daemon accepts an expression.
export const CRON_SEARCH_HORIZON = 146097 * DAY;
const zoneFormatters = new Map<string, Intl.DateTimeFormat>();

function zoneFormatter(timezone: string) {
  if (!zoneFormatters.has(timezone)) {
    zoneFormatters.set(timezone, new Intl.DateTimeFormat("en-US", { timeZone: timezone, hourCycle: "h23", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", weekday: "short" }));
  }
  return zoneFormatters.get(timezone)!;
}

const WEEKDAYS: Readonly<Record<string, number>> = Object.freeze({ Sun: 0, Mon: 1, Tue: 2, Wed: 3, Thu: 4, Fri: 5, Sat: 6 });

export function zonedParts(instant: Instant, timezone: string): ZonedParts {
  const parts = Object.fromEntries(zoneFormatter(timezone).formatToParts(instant).map((part) => [part.type, part.value]));
  return { minute: Number(parts.minute), hour: Number(parts.hour), day: Number(parts.day), month: Number(parts.month), weekday: WEEKDAYS[parts.weekday] };
}

interface CronFieldRule { label: string; range: string; min: number; max: number; stepMax?: number; any?: "feld" | "item"; names?: readonly (readonly string[])[]; strictNames?: boolean }

// Names read case-insensitively; the name at index i stands for `min + i`. A step runs
// to `max` unless `stepMax` says otherwise. `?` reads as `*` where `any` allows it: in
// day of month as any list item, in day of week as the whole field.
const CRON_MINUTE: CronFieldRule = { label: "minute", range: "0-59", min: 0, max: 59 };
const CRON_HOUR: CronFieldRule = { label: "hour", range: "0-23", min: 0, max: 23 };
const CRON_DAY: CronFieldRule = { label: "day of month", range: "1-31", min: 1, max: 31, any: "item" };
// A month name stands alone or bounds a range of two names.
const CRON_MONTH: CronFieldRule = { label: "month", range: "1-12 or JAN-DEC", min: 1, max: 12, strictNames: true, names: [["jan", "january"], ["feb", "february"], ["mar", "march"], ["apr", "april"], ["may"], ["jun", "june"], ["jul", "july"], ["aug", "august"], ["sep", "september"], ["oct", "october"], ["nov", "november"], ["dec", "december"]] };
// 0 and 7 are both Sunday; `N/step` runs to 7 and a step has no upper bound.
const CRON_WEEKDAY: CronFieldRule = { label: "day of week", range: "0-7 or SUN-SAT", min: 0, max: 7, stepMax: Infinity, any: "feld", names: [["sun", "sunday"], ["mon", "monday"], ["tue", "tues", "tuesday"], ["wed", "wednesday"], ["thu", "thurs", "thursday"], ["fri", "friday"], ["sat", "saturday"]] };
const CRON_FIELDS = [CRON_MINUTE, CRON_HOUR, CRON_DAY, CRON_MONTH, CRON_WEEKDAY];
export const CRON_FIELD_COUNT_ERROR = "Cron expression must have five fields (minute, hour, day of month, month, day of week).";

function cronValue(token: string, rule: CronFieldRule) {
  if (/^\d+$/.test(token)) return Number(token);
  const index = rule.names?.findIndex((names) => names.includes(token.toLowerCase())) ?? -1;
  return index < 0 ? undefined : rule.min + index;
}

function cronField(spec: string, rule: CronFieldRule) {
  if (spec === "*" || (rule.any && spec === "?")) return null;
  const values = new Set<number>();
  for (const part of spec.split(",")) {
    const match = part.match(/^(\*|\?|[a-z\d]+)(?:-([a-z\d]+))?(?:\/(\d+))?$/i);
    if (!match) return undefined;
    const [, start, end, stepText] = match;
    const every = start === "*" || start === "?";
    if ((start === "?" && rule.any !== "item") || (every && end)) return undefined;
    const named = [start, end].filter(Boolean).map((token) => /^[a-z]+$/i.test(token));
    if (rule.strictNames && named.includes(true) && (named.includes(false) || (stepText && !end))) return undefined;
    const step = stepText ? Number(stepText) : 1;
    const low = every ? rule.min : cronValue(start, rule);
    const high = end ? cronValue(end, rule) : every || stepText ? rule.max : low;
    if (low === undefined || high === undefined || low < rule.min || high > rule.max || low > high || step < 1 || step > (rule.stepMax ?? rule.max)) return undefined;
    for (let value = low; value <= high; value += step) values.add(value);
  }
  return values;
}

function cronFields(cron: unknown) {
  return String(cron || "").trim().split(/\s+/);
}

// Why the daemon refuses the expression, naming the field it breaks, or null.
export function cronError(cron: unknown) {
  const fields = cronFields(cron);
  if (fields.length !== CRON_FIELDS.length) return CRON_FIELD_COUNT_ERROR;
  const index = CRON_FIELDS.findIndex((rule, i) => cronField(fields[i], rule) === undefined);
  return index < 0 ? null : `Cron ${CRON_FIELDS[index].label} field '${fields[index]}' must be ${CRON_FIELDS[index].range}.`;
}

export function parseCron(cron: unknown) {
  const fields = cronFields(cron);
  if (fields.length !== CRON_FIELDS.length) return null;
  const [minute, hour, day, month, weekday] = fields.map((field, index) => cronField(field, CRON_FIELDS[index]));
  const parsed = { minute, hour, day, month, weekday: weekday && new Set([...weekday].map((value) => value % 7)) };
  if (Object.values(parsed).some((field) => field === undefined)) return null;
  const eitherDay = !fields[2].startsWith("*") && !fields[2].startsWith("?") && !fields[4].startsWith("*") && !fields[4].startsWith("?");
  return { ...parsed, eitherDay } as CronSpec;
}

function cronAllows(set: Set<number> | null, value: number) {
  return set === null || set.has(value);
}

function cronDateMatches(parsed: CronSpec, parts: ZonedParts) {
  const day = cronAllows(parsed.day, parts.day);
  const weekday = cronAllows(parsed.weekday, parts.weekday);
  return cronAllows(parsed.month, parts.month) && (parsed.eitherDay ? day || weekday : day && weekday);
}

function cronMatches(parsed: CronSpec, parts: ZonedParts) {
  return cronAllows(parsed.minute, parts.minute) && cronAllows(parsed.hour, parts.hour) && cronDateMatches(parsed, parts);
}

// Whether some month holds one of the days, so that the search can end at all.
const MONTH_DAYS = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

function cronDateExists(parsed: CronSpec) {
  return MONTH_DAYS.some((days, index) => cronAllows(parsed.month, index + 1) && (parsed.day === null || parsed.eitherDay || [...parsed.day].some((day) => day <= days)));
}

// The first instant of the local day after the one `parts` reads at `instant`. The jump
// is taken only when the minute before its landing still reads that day; across an
// offset change before midnight it is shortened by up to three hours, and failing that
// the search goes on by the minute.
function nextLocalDay(instant: Instant, parts: ZonedParts, timezone: string) {
  const left = 24 * 60 - parts.hour * 60 - parts.minute;
  for (const minutes of [left, left - 180]) {
    if (minutes <= 1) break;
    const before = zonedParts(instant + (minutes - 1) * MINUTE, timezone);
    if (before.day === parts.day && before.month === parts.month) return instant + minutes * MINUTE;
  }
  return instant + MINUTE;
}

// The first tick strictly after `from` within the search horizon, or null when the
// expression never fires there. Minutes whose local minute cannot match are skipped
// without formatting, by the offset the zone reads as the search enters each UTC hour.
// When a later read shows that offset moved by other than whole hours within the hour,
// as a half-hour DST shift does, that hour is searched again by the minute. A local day
// whose date cannot match is skipped whole.
export function nextCronInstant(cron: string, timezone: string, from: Instant) {
  const parsed = parseCron(cron);
  if (!parsed || !cronDateExists(parsed)) return null;
  const end = from + CRON_SEARCH_HORIZON;
  const first = Math.floor(from / MINUTE) * MINUTE + MINUTE;
  let instant = first;
  let hourStart: number | null = null;
  let minuteOffset = 0;
  let hourByMinute: number | null = null;
  while (instant <= end) {
    const currentHour = Math.floor(instant / HOUR);
    if (currentHour !== hourStart) {
      const offset = zonedParts(instant, timezone).minute - Math.floor((instant % HOUR) / MINUTE);
      if (hourStart === currentHour - 1 && hourByMinute !== hourStart && (offset - minuteOffset) % 60 !== 0) {
        hourByMinute = hourStart;
        instant = Math.max(hourStart * HOUR, first);
        continue;
      }
      hourStart = currentHour;
      minuteOffset = offset;
    }
    const byMinute = hourByMinute === currentHour;
    const minute = (Math.floor((instant % HOUR) / MINUTE) + minuteOffset + 120) % 60;
    if (!byMinute && parsed.minute !== null && !parsed.minute.has(minute)) {
      instant += MINUTE;
      continue;
    }
    const parts = zonedParts(instant, timezone);
    if (!byMinute && parts.minute !== minute) {
      hourByMinute = currentHour;
      instant = Math.max(currentHour * HOUR, first);
      continue;
    }
    if (cronMatches(parsed, parts)) return instant;
    instant = cronDateMatches(parsed, parts) ? instant + MINUTE : nextLocalDay(instant, parts, timezone);
  }
  return null;
}

// ---- Launch specs ------------------------------------------------------------------------
// The argv display rule `command-line.ts` holds, shared with the CLI: what the page
// shows can be copied into a terminal, and what a reader writes is read back the same
// way.

// A Runtime's display name among the installed ones; another is named by itself.
export function runtimeName(runtime: string, options: LaunchOptions["runtimes"]) {
  return (options.find(([value]) => value === runtime) || [runtime, runtime])[1];
}

// A launchSpec as one line: the Runtime or the argv.
export function launchSpecLine(launchSpec: ActorLaunchSpec) {
  return launchSpec.runtime ? `--runtime ${launchSpec.runtime}` : commandLine(launchSpec.command);
}

// ---- Actor column ------------------------------------------------------------------------

// Between the Actor name and the Timeline the page keeps a table of the latest
// top-level Session's context, one column per selected detail. Detailed evidence
// remains in Session details.
// `width` is a column's default; the reader drags it between `min` and `max`.
export const ACTOR_DETAIL_OPTIONS = Object.freeze([
  { id: "status", label: "Session status", width: 176, min: 120, max: 320, detail: "Lifecycle state and time of the latest Session" },
  { id: "coding-agent", label: "Coding Agent", width: 116, min: 80, max: 240, detail: "Coding Agent used by the latest Session" },
  { id: "execution", label: "Execution", width: 116, min: 80, max: 240, detail: "Execution Provider used by the latest Session: tmux, Docker, Docker Sandbox, or Herdr" },
  { id: "schedule", label: "Schedule", width: 196, min: 120, max: 360, detail: "Next run or paused state of a Job's Schedule; only Jobs with a Schedule show it" },
]);
export const DEFAULT_ACTOR_DETAILS = ["status", "coding-agent", "execution", "schedule"];
export const ACTOR_WIDTH = 340;
export const ACTOR_WIDTH_LIMITS = Object.freeze({ min: 180, max: 480 });
export const ACTOR_WIDTH_STORAGE_KEY = "feld-mock-actor-width";
// The Actor table column widths the reader dragged, by column id.
export const ACTOR_COLUMN_WIDTHS_STORAGE_KEY = "feld-mock-actor-column-widths";
// The reader's pins and row order: preferences of this browser, like the column widths.
export const ACTOR_PINS_STORAGE_KEY = "feld-mock-actor-pins";
export const ACTOR_ORDER_STORAGE_KEY = "feld-mock-actor-order";
// The Metrics lanes the reader showed or hid, by lane id; the other lanes follow their default.
export const METRIC_LANES_STORAGE_KEY = "feld-mock-metric-lanes";

// What choosing and ordering the bands reads from the page state.
type BandsState = Pick<AgentsState, "workspace" | "query" | "mode" | "anchor" | "duration" | "actorOrder" | "actorPins" | "showAgentsOutsideRange" | "frozenBandIds">;

// A pinned Actor keeps its row whatever the range and sits above the unpinned ones. A
// Job is pinned until the reader unpins it, wherever it was created, since its Sessions
// are sparse and a range often holds none; an Agent is pinned only by the reader.
export function isPinned(state: Pick<AgentsState, "actorPins">, agent: ActorRecord) {
  return Object.hasOwn(state.actorPins, agent.id) ? state.actorPins[agent.id] : agent.kind === "job";
}

// The pins with one Actor's pin set; the pins hold only the changes to the default.
export function pinsWith(pins: Record<string, boolean>, agent: ActorRecord, pinned: boolean) {
  const next = { ...pins };
  if (pinned === (agent.kind === "job")) delete next[agent.id];
  else next[agent.id] = pinned;
  return next;
}

// The row order after dropping one row above or below another of the rows shown. Actors
// outside this view keep their place after the ones shown.
export function orderWithMove(order: string[], shown: string[], dragged: string, onto: string, after: boolean) {
  const keys = shown.filter((key) => key !== dragged);
  keys.splice(keys.indexOf(onto) + (after ? 1 : 0), 0, dragged);
  return keys.concat(order.filter((key) => !keys.includes(key)));
}

export function clampActorWidth(width: number) {
  return Math.max(ACTOR_WIDTH_LIMITS.min, Math.min(ACTOR_WIDTH_LIMITS.max, width));
}

// A stored width is kept only while it lies within its column's limits.
export function isWithin(width: unknown, limits: { min: number; max: number }): width is number {
  return typeof width === "number" && Number.isFinite(width) && width >= limits.min && width <= limits.max;
}

export function actorColumnOption(id: string) {
  return ACTOR_DETAIL_OPTIONS.find((option) => option.id === id) || null;
}

export function clampActorColumnWidth(id: string, width: number) {
  const option = actorColumnOption(id);
  return option ? Math.max(option.min, Math.min(option.max, width)) : width;
}

// The widths within their limits, of known columns only.
export function actorColumnWidthsOf(stored: unknown): Record<string, number> {
  if (!stored || typeof stored !== "object" || Array.isArray(stored)) return {};
  return Object.fromEntries(Object.entries(stored).filter(([id, width]) => {
    const option = actorColumnOption(id);
    return option && isWithin(width, option);
  })) as Record<string, number>;
}

// The Actor table columns the reader selected, in the order of ACTOR_DETAIL_OPTIONS, each
// at the width the reader gave it. A hidden column keeps its width for when it returns.
export function actorColumns(state: Pick<AgentsState, "visibleActorDetails" | "actorColumnWidths">) {
  return ACTOR_DETAIL_OPTIONS.filter((option) => state.visibleActorDetails.has(option.id)).map((option) => ({ ...option, width: actorColumnWidth(state, option.id) }));
}

export function actorColumnWidth(state: Pick<AgentsState, "actorColumnWidths">, id: string) {
  return state.actorColumnWidths[id] ?? actorColumnOption(id)?.width ?? 0;
}

export function actorColumnsWidth(state: Pick<AgentsState, "visibleActorDetails" | "actorColumnWidths">) {
  return actorColumns(state).reduce((total, option) => total + option.width, 0);
}

export const OBSERVATION_MEANING: Readonly<Record<string, string>> = Object.freeze({
  running: "Owned resource, process, and planned parent still match",
  gone: "Process exit proven decisively",
  lost: "Resource no longer exposed; process exit not proven",
  relocated: "The recorded resource is alive under a different parent",
  mismatched: "The resource no longer matches the recorded process",
  unknown: "The Provider could not say what the recorded resource is doing",
});

// ---- Records -------------------------------------------------------------------------------

export function instantOf(value: string | null | undefined) {
  if (!value) return null;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
}

// The bare id inside an Actor key (`workspace/id`).
export function actorNameOf(actorId: string) {
  const separator = actorId.indexOf("/");
  const name = separator < 0 ? actorId : actorId.slice(separator + 1);
  const apart = name.indexOf(ACTOR_ID_SEPARATOR);
  return apart < 0 ? name : name.slice(0, apart);
}

// An Actor's key on this page: `workspace/name`, and `workspace/name~<id>` for an Actor
// whose name another Actor holds (see snapshot.ts). Names never hold `~`.
const ACTOR_ID_SEPARATOR = "~";
export function actorKeyOf(workspace: string, name: string, actorId?: string) {
  return `${workspace}/${name}${actorId ? `${ACTOR_ID_SEPARATOR}${actorId}` : ""}`;
}

// Ownership: Feld started an `owned` Session and holds the authority to stop it; an
// `attached` Session was found already running, and Feld neither started it nor stops
// it. It is the Session's own fact, fixed at start; a Sub Agent has none.
export function ownershipOf(session: AgentSession) {
  return session.ownership;
}

// The Details Execution row's note: the recorded command, or why none is recorded. The
// ownership says which, not the missing command: an owned agent-runtime Session was
// started through its Runtime Adapter, which builds the command.
export function executionDetailFor(session: AgentSession) {
  if (session.execution.command !== null) return session.execution.command;
  if (session.parentSession) return "- · Runtime-internal Sub Agent: its parent Session's Runtime started it";
  if (ownershipOf(session) === "owned") return `- · owned: Feld started the Runtime via the ${session.execution.label} Adapter; command is Adapter-managed`;
  return "- · attached: Feld did not start it and records no command";
}

// The stop and kill requests recorded for a Session, in order of acceptance.
export function endRequestsFor(session: AgentSession) {
  const lifecycle = session.lifecycle || {};
  return [lifecycle.stop && { kind: "stop", ...lifecycle.stop }, lifecycle.kill && { kind: "kill", ...lifecycle.kill }].filter(Boolean) as EndRequest[];
}

// A start that never began. `lifecycle.startFailed` is the Control Plane fact that the
// Provider made no execution, and the Provider's observation detail is the only account
// of why; the outcome, the Console, and the Details all put that account in front of the
// reader instead of the generic kind.
export function startFailure(session: { lifecycle?: { startFailed?: unknown } | null; observation?: { detail?: string | null } | null }) {
  if (!session.lifecycle?.startFailed) return null;
  return session.observation?.detail || "The Provider reported no reason";
}

// A program Session has no model record (N/A); an agent-runtime Session whose Runtime
// Adapter has delivered no Runtime identity yet has the Runtime name but no model
// (Unavailable).
export function modelLabel(session: AgentSession) {
  if (!session.model) return "N/A";
  return session.model[1] || "Unavailable";
}

export function runtimeLabel(session: AgentSession) {
  return session.model ? session.model[0] : "Program";
}

// Token totals are derived from the reported series, never stored on the Session.
export function tokenTotalsFor(session: AgentSession) {
  const metric = session.metrics.tokens;
  if (metric.status !== "reported") return null;
  const totals = Object.fromEntries(TOKEN_KINDS.map((kind, index) => [kind, metric.samples.reduce((sum, sample) => sum + sample.value[index], 0)]));
  totals.total = TOKEN_KINDS.reduce((sum, kind) => sum + totals[kind], 0);
  return totals;
}

export function tokenTotalFor(session: AgentSession) {
  const totals = tokenTotalsFor(session);
  return totals ? totals.total : null;
}

// What a metric shows when it has no series: N/A where the metric does not apply,
// Unavailable where its signal has not arrived, and Reading… until the Session's metrics
// read answers. Missing telemetry is never shown as zero.
export function metricPlaceholder(metric: TokenMetric | ContextMetric) {
  if (metric.status === "reading") return "Reading…";
  return metric.status === "n/a" ? "N/A" : "Unavailable";
}

export const METRIC_STATUS_MEANING: Readonly<Record<string, string>> = Object.freeze({
  "n/a": "not applicable: a program execution, a Sub Agent Session counted in its parent, or a Runtime that does not provide the metric",
  unavailable: "unavailable: Feld does not collect the metric for this Runtime, or no usable Runtime telemetry correlated with this Session; not read as zero",
  reported: "reported: Runtime telemetry attributed to this Session's token and projected into a series",
  reading: "reading: the Session's metrics have not been read yet",
});

export function metricTitleFor(metric: TokenMetric | ContextMetric, name: string) {
  return `${name} — ${METRIC_STATUS_MEANING[metric.status]} · ${metric.source}`;
}

export function usageValueFor(session: AgentSession) {
  const total = tokenTotalFor(session);
  return total === null ? metricPlaceholder(session.metrics.tokens) : total.toLocaleString("en-US");
}

export function usageDetailFor(session: AgentSession) {
  const totals = tokenTotalsFor(session);
  if (!totals) return session.metrics.tokens.source;
  return `${session.metrics.tokens.source} · ${TOKEN_KINDS.map((kind) => `${TOKEN_KIND_LABELS[kind]} ${totals[kind].toLocaleString("en-US")}`).join(" · ")}`;
}

// The latest context window sample: the used percentage the Runtime reported last.
export function contextValueFor(session: AgentSession) {
  const metric = session.metrics.context;
  if (metric.status !== "reported") return metricPlaceholder(metric);
  return `${Math.round(metric.samples[metric.samples.length - 1].value)}%`;
}

export function contextDetailFor(session: AgentSession) {
  const metric = session.metrics.context;
  if (metric.status !== "reported") return metric.source;
  const peak = Math.max(...metric.samples.map((sample) => sample.value));
  const compactions = metric.samples.filter((sample, index) => index > 0 && sample.value < metric.samples[index - 1].value).length;
  const compaction = compactions === 0 ? "no compaction" : `${compactions} compaction${compactions === 1 ? "" : "s"}`;
  return `of ${metric.size.toLocaleString("en-US")} tokens · peak ${Math.round(peak)}% · ${compaction} · ${metric.source}`;
}

export function runsOn(session: AgentSession) {
  if (session.parentSession) return `${modelLabel(session)} (Runtime-internal Sub Agent)`;
  return `${session.execution.label} on ${session.placement!.provider}`;
}

export function placementSummary(session: AgentSession) {
  if (session.parentSession) return "Runtime-internal Sub Agent";
  return `${session.placement!.provider} · ${session.placement!.node}`;
}

export const BINDING_PROVENANCE = "Control Plane fact committed on the Execution Binding before the resource was created; never re-derived from labels or the caller's terminal";
export const OWNED_PROVENANCE = "Control Plane fact committed on the Execution Binding once the resource was created; lifecycle operations target exactly this resource";

// The four Execution Providers and their display names. The record value is the id.
export const PROVIDER_LABELS: Readonly<Record<string, string>> = Object.freeze({ tmux: "tmux", docker: "Docker", "docker-sandbox": "Docker Sandbox", herdr: "Herdr" });

export function providerLabel(provider: string) {
  return PROVIDER_LABELS[provider] || provider;
}

// Each row is one fact the Execution Binding records: the Node, the Provider, the
// working directory, and the two the Provider answered about its own resource. Values
// come from the Placement record and are never inferred. Attach hands off to a live
// resource, so a Session that ended or was last observed gone has no Attach row.
export function placementRows(session: AgentSession) {
  const placement = session.placement!;
  const rows = [];
  rows.push({ label: "Node", value: placement.node, detail: "Execution Binding", title: `Node ${placement.node} — ${BINDING_PROVENANCE}` });
  rows.push({ label: "Provider", value: providerLabel(placement.provider), detail: `Execution Provider ${placement.provider}`, title: `Execution Provider ${placement.provider} — ${BINDING_PROVENANCE}` });
  rows.push(placement.summary
    ? { label: "Resource", value: placement.summary, detail: `The ${providerLabel(placement.provider)} resource of this Session`, title: `${placement.summary} — ${OWNED_PROVENANCE}` }
    : { label: "Resource", value: "Unavailable", detail: `The ${providerLabel(placement.provider)} Provider reported no resource yet`, title: `No resource is recorded on this Binding — ${BINDING_PROVENANCE}` });
  rows.push({ label: "Working directory", value: placement.workdir, detail: "Initial directory of the Session", title: `Working directory ${placement.workdir} — ${BINDING_PROVENANCE}` });
  if (session.end !== null || session.observation?.kind === "gone") return rows;
  rows.push(placement.attachCommand
    ? { label: "Attach", value: placement.attachCommand, detail: "Run in a terminal on the Node; this page never sends input to a Session", title: "Handoff to the real terminal: the command the Provider derived from this Binding" }
    : { label: "Attach", value: "Unavailable", detail: `The ${providerLabel(placement.provider)} Provider records no terminal to attach to`, title: "No attach command can be derived from this Binding" });
  return rows;
}

// The Session the page is reading: the one in the Console, or the one whose facts the
// Actor card shows.
export function selectedSessionId(state: Pick<AgentsState, "inspectedSessionId" | "actorCard">) {
  return state.inspectedSessionId || state.actorCard?.sessionId || null;
}

// The range's Sessions, then those of the card's Actor that the range lacks.
// The first record of each key, as a search from the front of the list finds it.
function firstBy<T>(records: T[], keyOf: (record: T) => string): Map<string, T> {
  const first = new Map<string, T>();
  records.forEach((record) => {
    const key = keyOf(record);
    if (!first.has(key)) first.set(key, record);
  });
  return first;
}

function groupBy<T>(records: T[], keyOf: (record: T) => string): Map<string, T[]> {
  const groups = new Map<string, T[]>();
  records.forEach((record) => {
    const key = keyOf(record);
    const group = groups.get(key);
    if (group) group.push(record);
    else groups.set(key, [record]);
  });
  return groups;
}

// The Session measured over its lifetime is found by id as the card read it, and every
// other as the range read it.
export function sessionsOf(input: ActorsInput): AgentSession[] {
  if (!input.actorSessions?.length) return input.sessions;
  const measured = input.actorSessions.find((session) => session.id === input.measuredSession);
  const sessions = measured ? input.sessions.map((session) => (session.id === measured.id ? measured : session)) : input.sessions;
  const held = new Set(sessions.map((session) => session.id));
  return [...sessions, ...input.actorSessions.filter((session) => !held.has(session.id))];
}

// A Session as the Console knows it: the header's names and whether its source can be read.
export function consoleSessionOf(session: AgentSession): FeldConsoleSession {
  return {
    id: session.id,
    actor: session.subAgent ? `${actorNameOf(session.actor)} / ${session.subAgent}` : actorNameOf(session.actor),
    model: modelLabel(session),
    runtimeName: session.execution.type === "agent-runtime" ? session.execution.label : undefined,
    placement: session.parentSession ? `Not tracked · inside ${session.parentSession}` : placementSummary(session),
    // A program Session has captured output; a Runtime Session has whatever its record
    // file's state is. Only a Sub Agent Session carries that state on its parent's read,
    // so for every other Session the Console's own read settles it.
    console: session.execution.type === "program" ? "log"
      : session.consoleState === "readable" ? undefined
        : session.consoleState === "missing" ? "missing" : "none",
    sourceUnavailable: session.consoleState === "lost"
      ? session.consoleDetail || "the transcript file's location can no longer be reached"
      : undefined,
  };
}

// "Needs attention" is a UI inference used for ordering and a band tag only; it is
// not a Feld state. It is derived from the Session outcome concluded from ownership and
// the kind rule (outcome unknown, an owned Job run that failed, an owned Agent that
// exited without a stop or kill request) and from the current Session's Provider
// observation that left the process unproven (lost, mismatched). Unread mail and skipped
// ticks are marks of the Actor and its Schedule, shown on the marks line, and do not feed
// attention.
export const ATTENTION_OBSERVATIONS: string[] = ["lost", "mismatched"];

// The y of a record in a band: the row of the Session it belongs to, or the band's main
// line when the Session is not drawn (hidden Sub Agents, or no Session named).
export function laneYFor(lane: Lane, sessionId: string | null | undefined) {
  return (sessionId && lane.sessionY.get(sessionId)) || lane.mainY;
}

// A lane series is plotted only when it has values; one with a `status` reads N/A or
// Telemetry unavailable instead.
export function seriesPlotted(series: MetricSeries): series is PlottedSeries {
  return !series.status;
}

export function seriesStatusLabel(series: MetricSeries) {
  return series.status === "n/a" ? "N/A" : "Telemetry unavailable";
}

// The bin of a rate series that holds an instant, if any.
function rateBinAt(series: PlottedSeries, instant: Instant) {
  return series.samples.find((item) => instant >= item.at && instant < item.end) || null;
}

// The reading of a series at one instant. A level lane (a quota, the context window)
// holds its latest reading until the next one; a rate lane reads the bin that holds the
// instant, and has none outside its bins.
export function metricValueAt(metric: MetricLaneSpec, series: MetricSeries, instant: Instant) {
  if (!seriesPlotted(series)) return null;
  const samples = series.samples;
  if (!samples.length) return metric.level ? 0 : null;
  if (metric.level) return levelReadingAt(samples, instant);
  return rateBinAt(series, instant)?.value ?? null;
}

const compactNumber = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 });

// Six-digit readings and above are compact (339.2K), so a reading leaves room for its
// Runtime name.
export function formatMetricValue(metric: MetricLaneSpec, value: number) {
  if (Math.abs(value) >= 100000) return `${metric.prefix || ""}${compactNumber.format(value)}${metric.unit}`;
  const decimals = metric.decimals ?? (metric.maximum <= 12 ? 1 : 0);
  const rounded = Number(value.toFixed(decimals));
  return `${metric.prefix || ""}${rounded.toLocaleString("en-US", { minimumFractionDigits: decimals, maximumFractionDigits: decimals })}${metric.unit}`;
}

// The Runtime-reported resets_at of every series of the window that falls in range.
export function resetTimesFor(series: MetricSeries, range: FeldMetricRange) {
  if (!seriesPlotted(series)) return [];
  return resetsWithin(series.resets, range);
}

export function metricTitle(metric: MetricLane) {
  const provenance = [...new Set(metric.series.map(seriesSourceLine))].concat(`Feld: ${metric.derivation}`);
  const framing = metric.kind === "quota" ? "Account rate limit reported by Sessions, not a Session metric" : "Session metrics aggregated per Runtime and account by this metric's rule; Sub Agent Sessions are counted in their parent";
  return `${metric.name} — ${framing}. ${provenance.join(". ")}.`;
}

// A series' source line in the Metrics display: its Runtime, its status when it has no
// samples to plot, and the source.
export function seriesSourceLine(series: MetricSeries) {
  return `${series.provider}: ${seriesPlotted(series) ? "" : `${seriesStatusLabel(series)} · `}${series.source}`;
}

// The Session the quota read names as the latest reporter of the series of this
// account's window that the instant falls in.
export function quotaReporterAt(series: MetricSeries, instant: Instant) {
  const reporter = reporterAt(series.segments, instant);
  return reporter ? { id: reporter.session } : null;
}

export function metricReadingTitle(metric: MetricLaneSpec, series: MetricSeries, instant: Instant, text: string) {
  const reporter = metric.kind === "quota" ? quotaReporterAt(series, instant) : null;
  return `${seriesName(series, true)} ${metric.name}: ${text}${reporter ? ` · latest report from ${reporter.id}` : ""}`;
}

// A series is named by its account where it has one, else by its Runtime; `full` adds
// the Runtime to an account for text that stands alone.
export function seriesName(series: MetricSeries, full = false) {
  if (!series.account) return series.provider;
  return full ? `${series.provider} · ${series.account}` : series.account;
}

// A series as its lane names it: a quota lane holds one Runtime, so its account names it;
// another lane holds several Runtimes, so the Runtime goes with the account.
export function laneSeriesName(metric: MetricLaneSpec, series: MetricSeries) {
  return seriesName(series, metric.kind !== "quota");
}

// The order a lane lists its series in, given each one's reading: a quota lane's accounts
// highest first, so the account nearest its limit leads; another lane leads with each
// Runtime's highest series in Runtime order, so folding keeps every Runtime in sight, and
// lists the rest highest first.
export function listedSeriesOrder(metric: MetricLaneSpec, series: readonly MetricSeries[], valueOf: (series: MetricSeries) => number) {
  const highest = [...series].sort((left, right) => valueOf(right) - valueOf(left));
  if (metric.kind === "quota") return highest;
  const leads = [...new Set(series.map((item) => item.provider))].map((provider) => highest.find((item) => item.provider === provider)!);
  return [...leads, ...highest.filter((item) => !leads.includes(item))];
}

// Each account has one colour on every lane it appears in, in the order the lanes list
// accounts. A Runtime whose lines are one account's keeps the Runtime's colour: there is
// no line to tell apart. The line of the unknown account counts as one more, so it has a
// colour of its own beside an account's.
export function withAccountTones(lanes: MetricLane[]) {
  const plotted = lanes.flatMap((lane) => lane.series).filter((series) => seriesPlotted(series) && series.samples.length);
  const keyOf = (series: MetricSeries) => `${series.provider}/${series.account ?? ""}`;
  const linesByRuntime = new Map<string, Set<string>>();
  for (const series of plotted) {
    if (!linesByRuntime.has(series.provider)) linesByRuntime.set(series.provider, new Set());
    linesByRuntime.get(series.provider)!.add(keyOf(series));
  }
  const tones = new Map<string, string>();
  for (const series of plotted) {
    if (linesByRuntime.get(series.provider)!.size < 2) continue;
    const key = keyOf(series);
    if (!tones.has(key)) tones.set(key, `account-${tones.size % ACCOUNT_TONE_COUNT + 1}`);
    series.accountTone = tones.get(key);
  }
  return lanes;
}

// The colour a series is drawn in: its account's, else its Runtime's.
export function seriesTone(series: MetricSeries) {
  return series.accountTone || series.tone;
}

// The reset that ends the quota window a reading falls in: the resets_at of its report.
export function resetAfter(series: MetricSeries, instant: Instant) {
  if (!seriesPlotted(series)) return null;
  return series.samples.filter((sample) => sample.at <= instant).at(-1)?.until ?? null;
}

// What a lane series reads over the range: N/A where the metric does not apply,
// Telemetry unavailable where its signal cannot be collected, No samples in range where
// it can but no sample fell in the range, and otherwise its latest sample, which reads
// Measured zero when it is 0. Every reading names the Runtime, and a sample its time.
export function laneReading(metric: MetricLaneSpec, series: MetricSeries, range: FeldMetricRange) {
  if (!seriesPlotted(series)) return { state: series.status === "n/a" ? "na" : "unavailable", text: seriesStatusLabel(series), at: null, end: null };
  const latest = latestReading(series.samples, range, metric.level);
  if (!latest) return { state: "empty", text: series.reading ? "Reading…" : "No samples in range", at: null, end: null };
  if (latest.resetAt != null) return { state: "reset", text: `${formatMetricValue(metric, 0)} since reset`, at: latest.at, end: latest.end };
  return latest.value === 0
    ? { state: "zero", text: "Measured zero", at: latest.at, end: latest.end }
    : { state: "value", text: formatMetricValue(metric, latest.value), at: latest.at, end: latest.end };
}

// Whether a series has a value to draw at an instant: a level series from its first
// report on, since nothing was reported before it; a rate series inside its bins.
export function seriesSampledAt(metric: MetricLaneSpec, series: MetricSeries, instant: Instant) {
  if (!seriesPlotted(series)) return false;
  if (!metric.level) return rateBinAt(series, instant) !== null;
  return series.samples.length > 0 && instant >= series.samples[0].at;
}

// Where a reading's marker sits on its line: a level reading at its sample, one held from
// before the range at the range's start; a rate reading at the last instant of its bin in
// the range.
export function readingMarkerAt(metric: MetricLaneSpec, reading: { at: Instant; end: Instant }, range: FeldMetricRange) {
  return Math.max(range.start, metric.level ? reading.at : Math.min(reading.end, range.end) - 1);
}

// The instants a lane series is sampled at to draw its line, as runs of instants with a
// value: evenly over the range, the last at the range's final instant, plus the reading's
// marker so the line passes through it, either side of every reset so a quota drops where
// its window reset, a level series' first report, where its line starts, and the edges of
// every rate bin, so each bin draws and the line breaks where no bin follows it.
export function metricSampleTimes(metric: MetricLaneSpec, series: MetricSeries, range: FeldMetricRange, width: number, markerAt: Instant) {
  const sampleCount = Math.max(48, Math.round(width / 10));
  const resetTimes = metric.kind === "quota" ? resetTimesFor(series, range) : [];
  const sampleTimes = new Set<number>([markerAt]);
  for (let sample = 0; sample <= sampleCount; sample += 1) {
    sampleTimes.add(Math.min(range.end - 1, range.start + (range.end - range.start) * sample / sampleCount));
  }
  resetTimes.forEach((reset) => {
    sampleTimes.add(Math.max(range.start, reset - 1));
    sampleTimes.add(reset);
  });
  const first = seriesPlotted(series) ? series.samples[0]?.at : undefined;
  if (metric.level && first !== undefined && first > range.start && first < range.end) sampleTimes.add(first);
  if (!metric.level && seriesPlotted(series)) {
    series.samples.forEach((sample) => {
      if (sample.end <= range.start || sample.at >= range.end) return;
      sampleTimes.add(Math.max(range.start, sample.at));
      sampleTimes.add(Math.min(range.end, sample.end) - 1);
      if (sample.end < range.end) sampleTimes.add(sample.end);
    });
  }
  const segments: Instant[][] = [[]];
  [...sampleTimes].sort((left, right) => left - right).forEach((instant) => {
    if (seriesSampledAt(metric, series, instant)) segments.at(-1)!.push(instant);
    else if (segments.at(-1)!.length) segments.push([]);
  });
  return { resetTimes, segments: segments.filter((instants) => instants.length) };
}

export const TIMELINE_ELEMENT_OPTIONS: readonly (readonly [VisibilityKey, string, string])[] = Object.freeze([
  ["showSpawnLinks", "Spawns", "Actor-triggered Agent spawns, on the trunk of their time bin"],
  ["showMessageLinks", "Messages", "Actor-to-Actor Messages, one trunk per sender Session and zoom-dependent time bin"],
  ["showSubAgents", "Sub-agent branches", "Nested Session branch lines"],
  ["showScheduleSkips", "Schedule skips", "Planned ticks skipped because the previous Session was still current"],
]);

// The Session metric the Timeline display offers for the band histogram.
export const SESSION_METRIC_OPTIONS: readonly (readonly [string, string, string])[] = Object.freeze([
  ["none", "Hidden", "Session lines only"],
  ["total-tokens", "Token usage", "Input, output, cache read, and cache creation tokens"],
  ["context", "Context utilization", "Percentage of context window"],
  ["input-tokens", "Input tokens", "Prompt and cached input"],
  ["output-tokens", "Output tokens", "Generated output"],
]);

// ---- Timeline guide ----------------------------------------------------------------
// Every mark drawn in the Actors chart, with the shape as it is drawn and the rule it
// follows. The Timeline column's header opens it; the page draws no standing legend.
// Each entry's glyph is the Timeline mark of its `symbol`.
export const TIMELINE_LEGEND: readonly LegendGroup[] = Object.freeze([
  {
    name: "Line",
    entries: [
      { symbol: "session", label: "Session", detail: "A Session, drawn along its Actor's line from start to end." },
      { symbol: "sub-agent", label: "Sub Agent", detail: "A Runtime-internal Sub Agent, branching off the Session that opened it." },
      { symbol: "usage", label: "Usage", detail: "The Session metric chosen in Timeline display, binned over the Session." },
      { symbol: "continues", label: "Continues", detail: "The Session reaches past this edge of the visible range." },
    ],
  },
  {
    name: "Start",
    entries: [
      { symbol: "started", label: "Started", detail: "Feld started this Session; a scheduled run keeps the same mark." },
      { symbol: "attached", label: "Attached", detail: "Feld observed a Session it did not start." },
      { symbol: "replacement", label: "Replacement", detail: "A replacement Session took the Actor's place." },
      { symbol: "skipped", label: "Skipped tick", detail: "A planned tick passed while the previous Session was still current, so no Session began." },
    ],
  },
  {
    name: "End",
    entries: [
      { symbol: "running", label: "Running", detail: "No end is recorded yet; the line is drawn through now." },
      { symbol: "succeeded", label: "Succeeded", detail: "An owned Job run completed on its own." },
      { symbol: "failed", label: "Failed", detail: "An owned Job run ended on its own with a non-zero exit code." },
      { symbol: "exited", label: "Exited", detail: "An owned Agent loop ended with no stop or kill recorded before it." },
      { symbol: "stopped", label: "Stopped", detail: "A stop or kill was accepted before the end, or an attached or Sub Agent Session ended." },
      { symbol: "unknown", label: "Unknown", detail: "The end could not be proven. The Session line is dashed from the instant Feld lost sight of the execution (observed lost or mismatched) to its end, or through now while it is open." },
    ],
  },
  {
    name: "Actor state",
    entries: [
      { symbol: "actor-running", label: "Running", detail: "Beside the Actor name: the Actor has a current Session." },
      { symbol: "actor-stopped", label: "Stopped", detail: "The Actor's latest Session has ended." },
      { symbol: "actor-none", label: "No Session", detail: "The Actor has never had a Session." },
      { symbol: "actor-scheduled", label: "Scheduled", detail: "A green outer ring: the Job's Schedule fires its ticks." },
      { symbol: "actor-paused", label: "Schedule paused", detail: "A grey dotted outer ring: the Schedule is paused, so no tick fires. No outer ring: the Actor has no Schedule." },
    ],
  },
  {
    name: "Between lanes",
    entries: [
      { symbol: "spawn", label: "Spawn", detail: "A Spawn shares the dotted trunk of the Messages its Session sent in the same time bin; each spawned Actor keeps its own arrow. Hover or focus the trunk for exact times." },
      { symbol: "message", label: "Message", detail: "Messages from one sender Session in the same zoom-dependent time bin share a dotted trunk; each recipient keeps its own arrow. Hover or focus the trunk for exact times." },
      { symbol: "waiting", label: "Waiting", detail: "An open tray on a recipient means that its bundle contains at least one Message still waiting in that mailbox." },
    ],
  },
]);

// ---- Actor edit form ------------------------------------------------------------------

// A launchSpec's env as the forms edit it: one NAME=VALUE per line. Its values can be
// secrets, so only the forms show them; the launchSpec rows show the names. A value keeps
// to its line with `\n`, `\r`, and `\\` for a line break, a carriage return, and a
// backslash; any other backslash is itself.
const ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;
const ENV_ESCAPES: Record<string, string> = { "\\": "\\\\", "\n": "\\n", "\r": "\\r" };
const ENV_UNESCAPES: Record<string, string> = { "\\": "\\", n: "\n", r: "\r" };

const ACTOR_NAME = /^[a-z0-9][a-z0-9-]*$/;
const ACTOR_NAME_ERROR = "Name must be lowercase letters, digits, and hyphens, starting with a letter or digit.";

export function envText(env: Record<string, string> | undefined) {
  return Object.entries(env || {}).map(([name, value]) => `${name}=${value.replace(/[\\\n\r]/g, (character) => ENV_ESCAPES[character])}`).join("\n");
}

// The env the text declares (null when it declares none), or the first line the daemon
// would refuse. Names follow the daemon's rule; a value is everything after the first `=`.
export function parseEnv(text: string): { env: Record<string, string> | null; error: string | null } {
  // Without a prototype, every name the daemon takes (`__proto__` among them) is a key.
  const env: Record<string, string> = Object.create(null);
  for (const line of text.split(/\r?\n/)) {
    if (!line.trim()) continue;
    const separator = line.indexOf("=");
    const name = (separator < 0 ? line : line.slice(0, separator)).trim();
    if (separator < 0) return { env: null, error: `Environment line ${name} must be NAME=VALUE.` };
    if (!ENV_NAME.test(name)) return { env: null, error: `Environment name ${name || "(empty)"} must be letters, digits, and _, not starting with a digit.` };
    if (Object.hasOwn(env, name)) return { env: null, error: `Environment name ${name} appears twice.` };
    env[name] = line.slice(separator + 1).replace(/\\([\\nr])/g, (_, escaped: string) => ENV_UNESCAPES[escaped]);
  }
  return { env: Object.keys(env).length ? env : null, error: null };
}

function sameEnv(left: Record<string, string> | null | undefined, right: Record<string, string> | null | undefined) {
  const sorted = (env: Record<string, string> | null | undefined) => JSON.stringify(Object.entries(env || {}).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
  return sorted(left) === sorted(right);
}

// runtimeSettings as the Actor card shows them: a value under a key that names a secret
// (key, token, secret, password, credential) reads as the mask, and so does everything
// beneath such a key.
export const SECRET_MASK = "••••••••";
const SECRET_KEY = /key|token|secret|password|credential/i;

export function maskSecrets(value: unknown, secret = false): unknown {
  if (Array.isArray(value)) return value.map((item) => maskSecrets(item, secret));
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, maskSecrets(item, secret || SECRET_KEY.test(key))]));
  }
  return secret && value !== null && value !== "" ? SECRET_MASK : value;
}

// The edited settings with every mask left in place given back its declared value, or
// null when a mask stands where nothing was declared.
function unmaskSecrets(edited: unknown, declared: unknown): { value: unknown } | null {
  if (edited === SECRET_MASK) return declared === undefined || typeof declared === "object" ? null : { value: declared };
  if (Array.isArray(edited)) {
    const items = edited.map((item, index) => unmaskSecrets(item, Array.isArray(declared) ? declared[index] : undefined));
    return items.every(Boolean) ? { value: items.map((item) => item!.value) } : null;
  }
  if (edited && typeof edited === "object") {
    const own = declared && typeof declared === "object" && !Array.isArray(declared) ? declared as Record<string, unknown> : {};
    const entries = Object.entries(edited).map(([key, item]) => [key, unmaskSecrets(item, own[key])] as const);
    return entries.every(([, item]) => item) ? { value: Object.fromEntries(entries.map(([key, item]) => [key, item!.value])) } : null;
  }
  return { value: edited };
}

export function settingsText(settings: Record<string, unknown> | undefined) {
  return settings ? JSON.stringify(maskSecrets(settings), null, 2) : "";
}

// The JSON object the text declares, masks given back their declared values (null when it
// declares none), or why the daemon could not take it. `label` names the field.
export function parseSettings(text: string, declared: Record<string, unknown> | undefined, label = "Runtime settings"): { settings: Record<string, unknown> | null; error: string | null } {
  if (!text.trim()) return { settings: null, error: null };
  let edited: unknown;
  try {
    edited = JSON.parse(text);
  } catch {
    return { settings: null, error: `${label} must be a JSON object.` };
  }
  if (!edited || typeof edited !== "object" || Array.isArray(edited)) return { settings: null, error: `${label} must be a JSON object.` };
  const unmasked = unmaskSecrets(edited, declared);
  if (!unmasked) return { settings: null, error: `${label} hold ${SECRET_MASK} where no value was declared; write the value itself.` };
  const settings = unmasked.value as Record<string, unknown>;
  return { settings: Object.keys(settings).length ? settings : null, error: null };
}

// The env as the edit form shows it: the value of a secret-like name is masked.
export function maskedEnvText(env: Record<string, string> | undefined) {
  return envText(env && Object.fromEntries(Object.entries(env).map(([name, value]) => [name, SECRET_KEY.test(name) && value ? SECRET_MASK : value])));
}

// The env the edit form declares, a mask left in place given back its declared value.
export function parseEditedEnv(text: string, declared: Record<string, string> | undefined): { env: Record<string, string> | null; error: string | null } {
  const parsed = parseEnv(text);
  if (!parsed.env) return parsed;
  for (const [name, value] of Object.entries(parsed.env)) {
    if (value !== SECRET_MASK) continue;
    if (!declared || !Object.hasOwn(declared, name)) return { env: null, error: `Environment ${name} holds ${SECRET_MASK} but has no declared value; write the value itself.` };
    parsed.env[name] = declared[name];
  }
  return parsed;
}

function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical((value as Record<string, unknown>)[key])]));
  return value;
}

function sameJson(left: unknown, right: unknown) {
  return JSON.stringify(canonical(left ?? null)) === JSON.stringify(canonical(right ?? null));
}

// Whether the Actor starts a Runtime: its launchSpec says so, and an Agent without one
// (created by attach) is given a Runtime.
function runsRuntime(actor: Pick<ActorRecord, "kind" | "launchSpec">) {
  return actor.launchSpec ? Boolean(actor.launchSpec.runtime) : actor.kind === "agent";
}

// The Actor card's edit form: the name, a Job's Schedule (cron, timezone), the launchSpec
// (runtime, its settings and trustWorkdir, or the command; the working directory; env;
// Provider, Node, and the Provider's options). Each group is a `feld actor update` sending
// only the fields it was given; the rest keep their values. An Actor without a launchSpec
// starts from the first Runtime, Provider, and Node offered.
export function editDraftOf(actor: Pick<ActorRecord, "name" | "kind" | "launchSpec">, schedule: ScheduleRecord | null, options?: LaunchOptions): EditDraft {
  const launchSpec = actor.launchSpec;
  const first = (offered: LaunchOptions["runtimes"] | undefined) => offered?.[0]?.[0] ?? "";
  return {
    name: actor.name,
    cron: schedule?.cron || "",
    timezone: schedule?.timezone || "",
    runtime: launchSpec ? launchSpec.runtime || "" : runsRuntime(actor) ? first(options?.runtimes) : "",
    command: launchSpec?.command ? commandLine(launchSpec.command) : "",
    workdir: launchSpec?.workdir || "",
    provider: launchSpec ? launchSpec.provider : first(options?.providers),
    node: launchSpec ? launchSpec.node : first(options?.nodes),
    env: maskedEnvText(launchSpec?.env),
    settings: settingsText(launchSpec?.runtimeSettings),
    trustWorkdir: launchSpec?.trustWorkdir ? "true" : "",
    providerOptions: settingsText(launchSpec?.providerOptions),
  };
}

// Whether the form gives an Actor without a launchSpec one: only once a field of it is
// filled, so a rename alone sends no launchSpec.
function givesLaunchSpec(draft: EditDraft, actor: Pick<ActorRecord, "kind" | "launchSpec">) {
  return Boolean(actor.launchSpec) || Boolean(draft.workdir.trim() || draft.command.trim() || draft.env.trim() || draft.settings.trim() || draft.providerOptions.trim());
}

// The draft as the submitted form reads it: trimmed, and with only the fields the form
// shows for this Actor.
export function submittedEditDraft(draft: EditDraft, actor: Pick<ActorRecord, "kind" | "launchSpec">, schedule: ScheduleRecord | null): EditDraft {
  const runtime = runsRuntime(actor);
  return {
    name: draft.name.trim(),
    cron: schedule ? draft.cron.trim() : "",
    timezone: schedule ? draft.timezone : "",
    runtime: runtime ? draft.runtime : "",
    command: runtime ? "" : draft.command.trim(),
    workdir: draft.workdir.trim(),
    provider: draft.provider,
    node: draft.node,
    env: draft.env,
    settings: runtime ? draft.settings : "",
    trustWorkdir: runtime ? draft.trustWorkdir : "",
    providerOptions: draft.providerOptions,
  };
}

export function validateEditDraft(draft: EditDraft, actor: Pick<ActorRecord, "kind" | "launchSpec">, schedule: ScheduleRecord | null) {
  if (!ACTOR_NAME.test(draft.name)) return ACTOR_NAME_ERROR;
  const cronProblem = schedule ? cronError(draft.cron) : null;
  if (cronProblem) return cronProblem;
  if (!givesLaunchSpec(draft, actor)) return null;
  const launchSpec = actor.launchSpec;
  const commandProblem = runsRuntime(actor) ? null : commandError(draft.command);
  if (commandProblem) return commandProblem;
  if (!draft.workdir.startsWith("/")) return "Working directory must be an absolute path on the Node.";
  return parseEditedEnv(draft.env, launchSpec?.env).error
    || (runsRuntime(actor) ? parseSettings(draft.settings, launchSpec?.runtimeSettings).error : null)
    || parseSettings(draft.providerOptions, launchSpec?.providerOptions, "Provider options").error;
}

// The `launchSpec` patch for what the edit form changed, or null when it changed
// nothing. Only the fields the form owns are sent; the rest keep their values. `env`,
// `runtimeSettings`, and `providerOptions` move as a whole: the edited value replaces it,
// and an emptied field removes it (null). A value whose mask was left in place is unchanged.
export function launchSpecPatchOf(draft: EditDraft, actor: Pick<ActorRecord, "kind" | "launchSpec">) {
  if (!givesLaunchSpec(draft, actor)) return null;
  const launchSpec: Partial<ActorLaunchSpec> = actor.launchSpec || {};
  const patch: FeldApi.LaunchSpecPatch = {};
  if (draft.workdir !== launchSpec.workdir) patch.workdir = draft.workdir;
  if (runsRuntime(actor)) {
    if (draft.runtime !== launchSpec.runtime) patch.runtime = draft.runtime;
    const { settings } = parseSettings(draft.settings, launchSpec.runtimeSettings);
    if (!sameJson(settings, launchSpec.runtimeSettings)) patch.runtimeSettings = settings;
    if (Boolean(draft.trustWorkdir) !== Boolean(launchSpec.trustWorkdir)) patch.trustWorkdir = Boolean(draft.trustWorkdir);
  } else {
    const argv = splitCommand(draft.command);
    if (commandLine(argv) !== commandLine(launchSpec.command)) patch.command = argv;
  }
  const { env } = parseEditedEnv(draft.env, launchSpec.env);
  if (!sameEnv(env, launchSpec.env)) patch.env = env;
  if (draft.provider && draft.provider !== launchSpec.provider) patch.provider = draft.provider;
  if (draft.node && draft.node !== launchSpec.node) patch.node = draft.node;
  const { settings: providerOptions } = parseSettings(draft.providerOptions, launchSpec.providerOptions, "Provider options");
  // The options belong to the Provider that resolved them: with another Provider, the
  // ones left as they were are dropped (null), and the new Provider fills its own.
  if (patch.provider && actor.launchSpec && sameJson(providerOptions, launchSpec.providerOptions)) patch.providerOptions = null;
  else if (!sameJson(providerOptions, launchSpec.providerOptions)) patch.providerOptions = providerOptions;
  return Object.keys(patch).length ? patch : null;
}

// ---- New actor ------------------------------------------------------------------
// The form declares an Actor the way `feld actor create --kind agent` / `--kind job` would:
// Kind, then the launchSpec every created Actor has (a Runtime for an Agent, a command
// for a Job), and for a Job an optional Schedule. A Session the person runs themselves
// is registered from inside it with `/feld attach`, so attach is not offered here.
// The form is create-only: an existing name is refused here although the
// CLI and API treat a repeated `create` as a replacement of the declaration (a UI policy).
// The Nodes, Providers, and Runtimes the page offers are the daemon's (ActorsInput's
// `launchOptions`).

// A launchSpec choice the page offers: the draft's own when offered, else the first offered.
export function launchChoice(value: string, options: LaunchOptions["runtimes"]) {
  return !options.length || options.some(([offered]) => offered === value) ? value : options[0][0];
}

// The draft starts in the page's timezone, the one the Timezone select lists first, and on
// the first Runtime, Provider, and Node offered.
export function emptyActorDraft(serial: number, workspace: string, timeZone: string, options: LaunchOptions): ActorDraft {
  const first = (offered: LaunchOptions["runtimes"]) => offered[0]?.[0] ?? "";
  // `advancedOpen` belongs to the draft like every other field, so the disclosure stays
  // as the reader left it while the snapshot changes under the form.
  return { serial, workspace, chooseWorkspace: !workspace, kind: "job", name: "", runtime: first(options.runtimes), command: "", workdir: "", cron: "", timezone: timeZone, node: first(options.nodes), provider: first(options.providers), settings: "", env: "", advancedOpen: false, error: null, created: null, submitting: false, settling: false };
}

// The launchSpec the declaration carries: a Runtime for an Agent, a command for a Job, the
// combinations the daemon starts. The Provider's own choices are left out, so its
// `resolve` fills them the way it does for `feld actor create` without any --tmux-… option.
export function draftLaunchSpec(draft: ActorDraft) {
  const settings = draft.settings.trim();
  const execution = draft.kind === "agent"
    ? { runtime: draft.runtime, ...(settings ? { runtimeSettings: JSON.parse(settings) } : {}) }
    : { command: splitCommand(draft.command) };
  const { env } = parseEnv(draft.env);
  return { ...execution, workdir: draft.workdir.trim(), ...(env ? { env } : {}), provider: draft.provider, node: draft.node };
}

// What the form can decide by itself, the name's uniqueness among the Actors it has read
// included. The rest of the launchSpec rules are answered by the daemon.
export function validateActorDraft(draft: ActorDraft, takenNames: readonly string[] = []) {
  const id = draft.name.trim();
  if (!draft.workspace) return "Choose the Workspace to declare the Actor in.";
  if (!ACTOR_NAME.test(id)) return ACTOR_NAME_ERROR;
  if (takenNames.includes(id)) return `An Actor named ${id} already exists in this Workspace; names are unique.`;
  if (!draft.workdir.trim().startsWith("/")) return "Working directory must be an absolute path on the Node.";
  const commandProblem = draft.kind === "job" ? commandError(draft.command) : null;
  if (commandProblem) return commandProblem;
  const cronProblem = draft.kind === "job" && draft.cron.trim() ? cronError(draft.cron) : null;
  if (cronProblem === CRON_FIELD_COUNT_ERROR) return "Cron expression must have five fields (minute, hour, day of month, month, day of week), or be left empty for a Job without a Schedule.";
  if (cronProblem) return cronProblem;
  const envProblem = parseEnv(draft.env).error;
  if (envProblem) return envProblem;
  if (draft.kind === "agent" && draft.settings.trim()) {
    try {
      JSON.parse(draft.settings);
    } catch {
      return "Runtime settings must be a JSON object.";
    }
  }
  return null;
}

// How long the New actor screen waits for a start it just accepted to settle, and how
// often it reads the Session while it waits. The daemon observes every quarter of a
// second, so a Session that fails at once is settled well inside the budget; a slower
// one is reported as pending rather than guessed at.
export const START_SETTLE_BUDGET = 2500;
export const START_SETTLE_POLL = 250;

// A start is settled once the Session ended, once `lifecycle.startSettled` proves it
// ran, or once `lifecycle.startFailed` proves it never did.
export function startSettled(session: FeldApi.Session) {
  return Boolean(session.end || session.lifecycle?.startSettled || session.lifecycle?.startFailed);
}

// What a start the Control Plane has just accepted came to, in the words the CLI's
// `result:` line uses. Until the Provider's first observation lands, a Session that
// never began looks like one that is still starting, so an unsettled start is reported
// as pending rather than read as a success.
export function startOutcome(session: FeldApi.Session, kind: string): StartOutcome {
  const code = session.exitCode === undefined ? null : session.exitCode;
  const codeLabel = code === null ? "no exit code reported" : `exit ${code}`;
  const failure = startFailure(session);
  if (failure) return { key: "failed", text: `Start failed: ${failure}` };
  if (!session.end) return session.lifecycle?.startSettled ? { key: "ok", text: "Started and running" } : { key: "pending", text: "Start accepted; outcome pending" };
  if (session.endKind === "unknown") return { key: "attention", text: "Started, then ended without the Provider proving it; the outcome is unknown" };
  if (session.endKind === "stopped") return { key: "attention", text: `Started, then ended on a stop or kill request (${codeLabel})` };
  if (kind !== "job") return { key: "attention", text: `Started, then exited without a stop or kill request (${codeLabel})` };
  if (code === null) return { key: "attention", text: "Started, then ended with no exit code reported; the outcome is unknown" };
  return code === 0 ? { key: "ok", text: "Started, then succeeded (exit 0)" } : { key: "failed", text: `Started, then failed (${codeLabel})` };
}

export function formatDuration(milliseconds: number) {
  if (milliseconds < MINUTE) return `${Math.floor(milliseconds / 1000)}s`;
  const minutes = Math.round(milliseconds / MINUTE);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const remainder = minutes % 60;
  return remainder ? `${hours}h ${remainder}m` : `${hours}h`;
}

export function formatAge(milliseconds: number) {
  if (milliseconds < MINUTE) return "<1m ago";
  if (milliseconds < DAY) return `${formatDuration(milliseconds)} ago`;
  const hours = Math.floor(milliseconds / HOUR);
  const days = Math.floor(hours / 24);
  const remainder = hours % 24;
  return `${days}d${remainder ? ` ${remainder}h` : ""} ago`;
}

export function millisecondsLabel(later: Instant, earlier: Instant) {
  const gap = later - earlier;
  if (gap < 1000) return `${gap} ms later`;
  return `${(gap / 1000).toFixed(gap < 10000 ? 3 : 1).replace(/\.?0+$/, "")} s later`;
}

// What `feld session stop` and `feld session kill` mean. The Provider decides what it tries and records
// it as the `attempt` of the request, which the Session's records show; this page names the request.
export const END_ATTEMPTS = Object.freeze({
  stop: "asks the Runtime to end itself",
  kill: "ends the execution",
});

// Agent Actions expose only the lifecycle primitives the product contract names:
// `feld session stop`, `feld session kill`, `feld actor start`, and `feld message send`.
// Which of them apply follows from two facts: does the Actor have a current Session,
// and (for Start) does it have a launchSpec.
// While a request of this menu is in flight every item is disabled, and a refusal stays
// under them until the reader picks something else.
export const MENU_ACTION_LABELS: Readonly<Record<string, string>> = Object.freeze({ start: "Starting the Session", stop: "Asking the execution to end", kill: "Ending the execution", pause: "Pausing the Schedule", resume: "Resuming the Schedule", delete: "Deleting the Actor" });
// What the toast says once the daemon accepted the request, and what it did not do when
// the daemon refused it.
export const MENU_ACTION_DONE: Readonly<Record<string, string>> = Object.freeze({ start: "Session start accepted", stop: "Stop accepted", kill: "Kill accepted", pause: "Schedule paused", resume: "Schedule resumed", delete: "Actor deleted" });
export const MENU_ACTION_REFUSED: Readonly<Record<string, string>> = Object.freeze({ start: "Session not started", stop: "Stop refused", kill: "Kill refused", pause: "Schedule not paused", resume: "Schedule not resumed", delete: "Actor not deleted" });
export const MENU_ACTION_NOTES: Readonly<Record<string, string>> = Object.freeze({
  start: "The Session appears on the Timeline once the daemon reads it back.",
  stop: "The execution is asked to end; the Session ends when it does.",
  kill: "The execution is ended without asking.",
  pause: "No tick fires until resumed; a running Session is not touched.",
  resume: "The next run is computed from now.",
  delete: "Its Sessions and Messages stay; the band goes once the Actors are read again.",
});

// The range button says which kind of picture this is: a live range named by its length
// ("Last 1 hour"), which ends at now and follows it, or a historical one named by its
// dates, which does not.
export const MODE_TITLES = Object.freeze({
  historical: "History: a fixed range that does not refresh. The Actor table and the running-state marks still describe the latest records, not this range.",
  live: "Live: the range ends at now and moves with it, re-read on each change unless paused.",
});

// Delivery summary shown in a Console's record row is derived per Message (see
// deliverySummary in createModel). The transcript entry that quotes a Message is found
// in the reader's Session for an inbound read, otherwise in the sender's Session by the
// Message's `anchor`.
export function messageRuntimeRef(message: MessageRecord) {
  if (message.read) return { sessionId: message.read.sessionId, anchor: message.id };
  return { sessionId: message.fromSessionId, anchor: message.anchor };
}

export const START_KIND_MEANING: Readonly<Record<string, string>> = Object.freeze({
  start: "the Actor's first generation",
  replacement: "a later generation; an earlier Session of the Actor exists",
  schedule: "started by the daemon at a planned tick of the Job's Schedule",
  attach: "a running execution registered with Feld",
  "sub-agent": "a Runtime-internal Sub Agent inside its parent Session",
});

// ---- Timeline geometry ---------------------------------------------------------------

export function arrowheadPath(x: number, fromY: number, toY: number) {
  const direction = toY >= fromY ? 1 : -1;
  const baseY = toY - direction * 7;
  return `M ${x.toFixed(1)} ${toY.toFixed(1)} L ${(x - 4).toFixed(1)} ${baseY.toFixed(1)} L ${(x + 4).toFixed(1)} ${baseY.toFixed(1)} Z`;
}

// Terminal marker for a waiting Message: an open tray at the recipient lane, opening
// toward the incoming dotted line. Distinct from the filled arrowhead (read), the
// Session circles, the failed ×, and the continuation chevron.
export function mailboxMarkerPath(x: number, fromY: number, toY: number) {
  const direction = toY >= fromY ? 1 : -1;
  const lipY = toY - direction * 6;
  return `M ${(x - 5).toFixed(1)} ${lipY.toFixed(1)} L ${(x - 5).toFixed(1)} ${toY.toFixed(1)} L ${(x + 5).toFixed(1)} ${toY.toFixed(1)} L ${(x + 5).toFixed(1)} ${lipY.toFixed(1)}`;
}

export function tokenBinDuration(windowDuration: number, plotWidth: number) {
  const desiredDuration = windowDuration * TOKEN_BIN_PIXEL_TARGET / Math.max(plotWidth, 1);
  return TOKEN_BIN_OPTIONS.find((duration) => duration >= desiredDuration)
    || TOKEN_BIN_OPTIONS[TOKEN_BIN_OPTIONS.length - 1];
}

// The Session's own metric bins, each at the instants the read gave it. They are passed
// through as they are: a bin belongs to the interval it was measured in, not to an even
// share of the Session.
export function tokenBinSamples(session: AgentSession, kinds: readonly string[] = TOKEN_KINDS): BinSample<number>[] {
  const metric = session.metrics.tokens;
  if (metric.status !== "reported") return [];
  const indexes = kinds.map((kind) => TOKEN_KINDS.indexOf(kind));
  return metric.samples.map((sample) => ({
    start: sample.start,
    end: sample.end,
    value: indexes.reduce((sum, index) => sum + sample.value[index], 0),
  }));
}

export function contextBinSamples(session: AgentSession) {
  const metric = session.metrics.context;
  return metric.status === "reported" ? metric.samples : [];
}

// The Session metric options read the Session's own metrics: three token views (every
// kind, the prompt side, the generated side) and the context window's used percentage.
export const SESSION_TOKEN_METRICS: Readonly<Record<string, { kinds: readonly string[]; label: string }>> = Object.freeze({
  "total-tokens": Object.freeze({ kinds: TOKEN_KINDS, label: "tokens" }),
  "input-tokens": Object.freeze({ kinds: Object.freeze(["input", "cache_read", "cache_creation"]), label: "input tokens" }),
  "output-tokens": Object.freeze({ kinds: Object.freeze(["output"]), label: "output tokens" }),
});

// The x of an instant on a plot `width` wide.
export function plotX(range: FeldMetricRange, width: number, instant: Instant) {
  return PLOT_PAD_LEFT + (instant - range.start) / (range.end - range.start) * (width - PLOT_PAD_LEFT - PLOT_PAD_RIGHT);
}

// A Session Console needs more room than a Message or composer inspector, so the card's
// limits depend on which one the workspace holds.
export function inspectorBounds(sessionConsole: boolean, workspaceWidth: number) {
  const minimum = sessionConsole ? 360 : 240;
  return { minimum, maximum: Math.max(minimum, workspaceWidth - (sessionConsole ? 320 : 420)) };
}

// The pane handle moves the Actors pane down to what leaves the Metrics pane its room;
// `available` is the bands grid's height.
export function clampPaneHeight(height: number, available: number | null) {
  if (available === null) return Math.max(96, height);
  return Math.max(96, Math.min(Math.max(96, available - 7 - 110), height));
}

export const RESIZE_CLASSES = Object.freeze({ pane: "is-resizing-rows", inspector: "is-resizing-inspector", actor: "is-resizing-actor" });

export type ActorsModel = ReturnType<typeof createModel>;

// ---- The derivations over one snapshot ----------------------------------------------

export function createModel(input: ActorsInput, clock: ActorsClock, consoleReads: ConsoleReads) {
  // These hold what the range reads answered, in the shape the fixture keeps
  // them. An Actor is keyed by `workspace/id`: an id is unique only inside its
  // Workspace, and the All workspaces view mixes them. `name` is the bare id the Workspace
  // knows it by.
  const WORKSPACES = input.workspaces;
  const DEFAULT_WORKSPACE = input.defaultWorkspace;
  const ACTORS = input.actors;
  // Placement records mirror what the Execution Binding commits for one Session: the
  // Execution Provider, the Node, the working directory, and the Provider's own items.
  // `placement: null` marks a Runtime-internal Sub Agent, for which Feld records no
  // Binding at all.
  //
  // Session evidence is the Provider `observation`: the latest kind and the instant it
  // was recorded. Feld writes an `observation` record only when the kind changes (the
  // first `running` at `lifecycle.startSettled` included). Feld keeps no
  // Session-originated report and derives no interpretation of its own beyond the
  // outcome below.
  //
  // `ownership` is a Session fact fixed at start: `owned` for a spawn, replacement, or
  // schedule start, `attached` for an attach. It decides whether Feld can stop or kill
  // the Session and how an unrequested end is read.
  //
  // Lifecycle end is two Control Plane facts, not one outcome. `endKind` says how the
  // Session ended: `running` (no end), `stopped` (ended after a recorded `feld session stop` or
  // `feld session kill`, kept in `lifecycle.stop` / `lifecycle.kill`), `exited` (ended with no
  // such request), or `unknown` (the observation could not prove the end; `end` is then
  // the instant Feld stopped tracking it, at the next start request or a stop / kill).
  // `exitCode` is the recorded exit status where the Provider reported one; null means it
  // reported none. What that pair means is derived at render time from the Session's
  // ownership and the Actor's kind (see outcomeFor).
  const SESSIONS = input.sessions;
  // A Schedule is part of a Job's record when the Job has one. Everything else shown about
  // it (next run, recent runs) is derived at render time from this record and the Session
  // history. Only `job` Actors carry one, and a Job need not. `pausedAt` and `enabledAt`
  // are the accepted instants of the latest pause and resume requests.
  const SCHEDULES = input.schedules;
  // `schedule.skipped` is the one Schedule-specific event Feld records: at a planned
  // tick the Actor's current Session was still running (or unobserved), so no spawn was
  // submitted. The record names the Session that was current at the tick.
  const SCHEDULE_SKIPS = input.scheduleSkips;
  // A Spawn record names the spawner and the target as Workspace-qualified Actor
  // references, the Session it created, the instant, and `spawnerSessionId`.
  const ACTIONS = input.actions;
  // Each Message keeps the product's delivery facts apart and never promotes one into
  // another: `accepted`, `fromSessionId`, `toSessionId` (the recipient's Session current
  // at acceptance), and `read` (the one authenticated mailbox read). "Waiting" is not a
  // stored state: it is derived at query time as accepted with no read recorded.
  const MESSAGES = input.messages;
  const METRICS = input.metrics;
  // With the card's Actor's Sessions the range lacks: found by id and listed by the card,
  // while the Timeline draws the range's alone.
  const KNOWN_SESSIONS = sessionsOf(input);
  const CONSOLE_SESSIONS = input.consoleSessions || KNOWN_SESSIONS.map(consoleSessionOf);
  const NOW = clock.now;
  // The lookups by id, built once per snapshot. The grouped lists are shared, so a reader
  // copies one before sorting it.
  const ACTOR_BY_ID = firstBy(ACTORS, (actor) => actor.id);
  const SESSION_BY_ID = firstBy(KNOWN_SESSIONS, (session) => session.id);
  const CONSOLE_SESSION_BY_ID = firstBy(CONSOLE_SESSIONS, (session) => session.id);
  const MESSAGE_BY_ID = firstBy(MESSAGES, (message) => message.id);
  const ACTION_BY_ID = firstBy(ACTIONS, (action) => action.id);
  const SPAWN_BY_SESSION = firstBy(ACTIONS, (action) => action.session);
  const SCHEDULE_BY_ACTOR = firstBy(SCHEDULES, (schedule) => schedule.actor);
  const SESSIONS_BY_ACTOR = groupBy(SESSIONS, (session) => session.actor);
  const KNOWN_SESSIONS_BY_ACTOR = groupBy(KNOWN_SESSIONS, (session) => session.actor);
  const WAITING_BY_ACTOR = groupBy(
    MESSAGES.filter((message) => message.read === null).sort((left, right) => left.accepted - right.accepted),
    (message) => message.to,
  );

  function now() {
    return NOW;
  }

  // Instants are shown in the page's timezone on the viewer's hour cycle, and ZONE names
  // the zone beside them.
  const TIME_ZONE = clock.timeZone;
  const { dateTime, timeOnly, timeInput, shortDate, timeWithSeconds, dateInput, wallClock } = formattersFor(TIME_ZONE, clock.hourCycle || "h23");
  const ZONE = zoneLabel(TIME_ZONE, NOW);

  // ---- Actors ------------------------------------------------------------------------

  function kindOf(agentId: string) {
    return (ACTOR_BY_ID.get(agentId) || { kind: "agent" }).kind;
  }

  function scheduleFor(agentId: string) {
    return SCHEDULE_BY_ACTOR.get(agentId) || null;
  }

  // Next run is a query-time derivation from the Job's record and the current time: never
  // stored, and null while the Job is paused. The daemon derives it too, so its answer is
  // taken where the record carries one and the cron is evaluated here otherwise.
  function nextRunFor(schedule: ScheduleRecord | null) {
    if (!schedule || !schedule.enabled) return null;
    if (schedule.nextRun) return schedule.nextRun;
    return nextCronInstant(schedule.cron, schedule.timezone, now());
  }

  // Runs of a Job's schedule are its Sessions with startKind schedule, and its skips are
  // the recorded schedule.skipped events; both are listed newest first.
  function scheduleRunsFor(schedule: ScheduleRecord) {
    const runs = sessionsFor(schedule.actor).filter((session) => session.startKind === "schedule" && !session.parentSession)
      .map((session): ScheduleRun => ({ kind: "session", at: session.planned || session.start, session }));
    const skips = SCHEDULE_SKIPS.filter((skip) => skip.actor === schedule.actor)
      .map((skip): ScheduleRun => ({ kind: "skip", at: skip.planned, skip }));
    return [...runs, ...skips].sort((left, right) => right.at - left.at);
  }

  // Ticks skipped while the Actor's current Session runs. Present only while both
  // hold: the Actor has a current Session, and a schedule.skipped record names a
  // planned instant after that Session started. It clears when the Session ends, because
  // the next tick will then fire; it feeds the band mark and the attention ordering.
  function skippedWhileCurrentFor(agentId: string) {
    const schedule = scheduleFor(agentId);
    const session = currentSessionsFor(agentId)[0] || null;
    if (!schedule || !session) return { session: null, skips: [] as ScheduleSkip[] };
    const skips = SCHEDULE_SKIPS.filter((skip) => skip.actor === schedule.actor && skip.planned >= session.start).sort((left, right) => left.planned - right.planned);
    return { session, skips };
  }

  function rangeFor(state: Pick<AgentsState, "mode" | "anchor" | "duration">): FeldMetricRange {
    if (state.mode === "live") {
      return { start: now() - state.duration, end: now() };
    }
    return {
      start: state.anchor - state.duration / 2,
      end: state.anchor + state.duration / 2,
    };
  }

  // The range ends at now at the latest: every anchor change goes through this.
  function anchorUpToNow(anchor: Instant, duration: number) {
    return Math.min(anchor, now() - duration / 2);
  }

  function sessionEnd(session: AgentSession) {
    return session.end === null ? now() : session.end;
  }

  // A Sub Agent Session is drawn from its parent's start at the earliest: an attached
  // Session's transcript can name Sub Agents from before the attach, and the Timeline
  // shows only the time Feld holds the parent.
  function timelineStart(session: AgentSession) {
    const parent = session.parentSession ? sessionById(session.parentSession) : null;
    return parent ? Math.max(session.start, parent.start) : session.start;
  }

  // The instant Feld lost sight of the Session's execution, from which the Timeline dashes
  // its line to the end: the latest observation is `lost` or `mismatched` since that
  // instant. A Session closed as unknown with no such observation is dashed over its whole
  // run. A Sub Agent Session has no observation of its own, so its line stays solid.
  function lostSince(session: AgentSession): Instant | null {
    if (session.parentSession) return null;
    const observation = session.observation;
    if (observation && ATTENTION_OBSERVATIONS.includes(observation.kind)) return observation.since ?? observation.at;
    return session.endKind === "unknown" ? session.start : null;
  }

  function sessionById(sessionId: string | null | undefined) {
    return (sessionId && SESSION_BY_ID.get(sessionId)) || null;
  }

  function messageById(messageId: string | undefined) {
    return (messageId && MESSAGE_BY_ID.get(messageId)) || null;
  }

  function actorFor(id: string): ActorRecord {
    return ACTOR_BY_ID.get(id)
      || { id, name: actorNameOf(id), workspace: id.includes("/") ? id.slice(0, id.indexOf("/")) : DEFAULT_WORKSPACE, kind: "agent", launchSpec: null };
  }

  // How an Actor is named on this page: its bare id inside the selected Workspace, and
  // `workspace/actor` for an Actor of any other Workspace or when all Workspaces are
  // shown (state.workspace null). The Admin is not an Actor and keeps its name.
  function actorLabel(actorId: string, state: Pick<AgentsState, "workspace">, workspace: string | null = actorFor(actorId).workspace) {
    if (actorId === "Admin") return actorId;
    const name = actorNameOf(actorId);
    return state.workspace && workspace === state.workspace ? name : `${workspace}/${name}`;
  }

  // The current Sessions of an Actor: top-level Sessions with no end recorded. Every Actor
  // has one current Session slot, so there is at most one. Derived at query time, never
  // stored.
  function currentSessionsFor(agentId: string) {
    return sessionsFor(agentId)
      .filter((session) => !session.parentSession && session.end === null)
      .sort((left, right) => left.start - right.start);
  }

  // How a Session's end is concluded, derived from two Control Plane facts (`endKind` and
  // `exitCode`), the Session's ownership, and the Actor's kind. The kind rule is the one
  // rule the kind changes, and this is the only place the page reads it. `attention` feeds
  // band ordering, and `reason` is the short why the end marker's tooltip puts beside the label.
  //   key        meaning
  //   running    no end recorded
  //   stopped    ended after a recorded `feld session stop` / `kill`; the exit code is kept
  //              but is not read as success or failure, whatever the kind
  //   succeeded  owned Job Session ended on its own with exit 0
  //   failed     owned Job Session ended on its own with a non-zero exit (attention)
  //   exited     owned Agent Session ended with no stop or kill recorded (attention)
  //   ended      attached Session, or a Runtime-internal Sub Agent, ended: Feld holds no
  //              stop authority over it, so "unrequested" does not apply, whatever the kind
  //   unknown    the observation could not prove the end, an owned Job ended on its own
  //              and the Provider reported no exit code, or the start never began
  //              (attention); the label, `endKind`, and `exitCode` beside it tell them apart
  function outcomeFor(session: AgentSession) {
    const kind = kindOf(session.actor);
    const code = session.exitCode;
    const codeLabel = code === null ? "no exit code reported" : `exit ${code}`;
    if (session.end === null) return { key: "running", label: "Running", detail: `since ${timeOnly.format(session.start)}`, reason: "no end recorded", attention: false };
    if (session.parentSession) return { key: "ended", label: "Ended", reason: "Runtime-internal Sub Agent", detail: "Runtime-internal Sub Agent; its end comes from the Runtime's transcript, and Feld records no lifecycle for it", attention: false };
    const failure = startFailure(session);
    if (failure) return { key: "unknown", label: "Start failed", detail: failure, reason: failure, attention: true };
    if (session.endKind === "unknown") return { key: "unknown", label: "Outcome unknown", reason: "the Provider could not prove the end", detail: "the Provider observation could not prove the end; Feld stopped tracking the Session at the next start request, stop or kill", attention: true };
    if (session.endKind === "stopped") {
      const requests = endRequestsFor(session).map((request) => `feld session ${request.kind}`).join(", then ");
      return { key: "stopped", label: "Stopped", reason: `${requests} · ${codeLabel}`, detail: `${requests} accepted before the process ended · ${codeLabel}, not read as an outcome`, attention: false };
    }
    // `exited`: no stop or kill was recorded before the end. Whether Feld could have
    // recorded one is the ownership; only then does the kind say what the exit means.
    if (ownershipOf(session) === "attached") {
      return { key: "ended", label: "Ended", reason: `${codeLabel} · attached Session`, detail: `${codeLabel} · attached Session ended; Feld holds no stop authority over it, so no request could have been recorded`, attention: false };
    }
    if (kindRules(kind).unrequestedExit === "exit-code") {
      if (code === null) return { key: "unknown", label: "Outcome unknown", reason: codeLabel, detail: "an owned Job run ended on its own and the Provider reported no exit code, so neither 0 nor non-zero is a fact", attention: true };
      return code === 0
        ? { key: "succeeded", label: "Succeeded", reason: codeLabel, detail: `${codeLabel} · an owned Job run ending on its own is a normal completion`, attention: false }
        : { key: "failed", label: "Failed", reason: codeLabel, detail: `${codeLabel} · an owned Job run ending on its own with a non-zero code`, attention: true };
    }
    return { key: "exited", label: "Exited without request", reason: codeLabel, detail: `${codeLabel} · no stop or kill was recorded before an owned Agent loop ended`, attention: true };
  }

  // How an ended Session's outcome was reached, for the Details Outcome row, the Console
  // end row, and the evidence titles. The kind rule is applied only to an owned Session
  // that ended without a request; every other case names why it was not.
  function outcomeBasis(session: AgentSession) {
    const kind = kindOf(session.actor);
    const ownership = ownershipOf(session);
    if (session.endKind === "unknown") return { short: "kind rule not applied (end unproven)", full: "the Provider observation could not prove the end, so neither ownership nor the kind rule is applied" };
    if (startFailure(session)) return { short: "kind rule not applied (never started)", full: "the Session was never observed running, so there is no exit for the kind rule to read; the Provider's detail says why the launch stopped" };
    if (session.endKind === "stopped") return { short: `${ownership} · kind rule not applied (requested end)`, full: `${ownership} Session · a stop or kill request was recorded before the end, so the exit is not read against the kind rule, whatever the kind` };
    if (ownership === "attached") return { short: "attached · kind rule not applied", full: "attached Session · Feld holds no stop authority over it, so its end is not read against the kind rule, whatever the kind" };
    if (session.exitCode === null && kindRules(kind).unrequestedExit === "exit-code") return { short: `owned · ${kindLabel(kind)} kind rule (no exit code)`, full: `owned Session · ${kindLabel(kind)} kind rule reads the exit code, and the Provider reported none, so the outcome is unknown rather than failed` };
    return { short: `owned · ${kindLabel(kind)} kind rule`, full: `owned Session · ${kindLabel(kind)} kind rule: ${kindRules(kind).rule}` };
  }

  // The Session a Spawn or Message was issued from: the exact Session ID the record
  // saved when the request was accepted, or null for the Admin.
  function spawnerSessionFor(action: SpawnAction) {
    return sessionById(action.spawnerSessionId);
  }

  // The Spawns a Session issued: those the range read holds, and those its own records
  // read adds from outside the range.
  function spawnsBy(session: AgentSession): SpawnAction[] {
    const spawns = ACTIONS.filter((entry) => entry.spawnerSessionId === session.id);
    const known = new Set(spawns.map((entry) => entry.session));
    (consoleReads.feldRecordsFor(session.id) || []).forEach((record) => {
      const at = instantOf(record.at);
      if (record.kind !== "spawned" || record.data?.spawnerSessionId !== session.id || !record.sessionId || known.has(record.sessionId) || at === null) return;
      known.add(record.sessionId);
      spawns.push({ id: `action_${record.sessionId}`, spawner: session.actor, spawnerWorkspace: session.workspace, spawnerSessionId: session.id, target: actorKeyOf(record.actor.workspace, record.actor.name), targetWorkspace: record.actor.workspace, session: record.sessionId, at });
    });
    return spawns;
  }

  function senderSessionFor(message: MessageRecord) {
    return sessionById(message.fromSessionId);
  }

  // The Session whose records hold a Message's accepted record: the recipient's Session
  // current at acceptance, saved on the record; null when none was current.
  function recipientSessionFor(message: MessageRecord) {
    return sessionById(message.toSessionId);
  }

  function overlaps(session: AgentSession, range: FeldMetricRange) {
    const start = timelineStart(session);
    return start < range.end && sessionEnd(session) > Math.max(start, range.start);
  }

  function currentSession(agentId: string) {
    return SESSIONS.some((session) => session.actor === agentId && session.end === null);
  }

  function formatRange(range: FeldMetricRange) {
    const sameDay = shortDate.format(range.start) === shortDate.format(range.end);
    if (sameDay) {
      return `${shortDate.format(range.start)}, ${timeOnly.format(range.start)}–${timeOnly.format(range.end)} ${ZONE}`;
    }
    return `${dateTime.format(range.start)} – ${dateTime.format(range.end)} ${ZONE}`;
  }

  function rangeButtonLabel(state: Pick<AgentsState, "mode" | "anchor" | "duration">) {
    return state.mode === "live" ? `Last ${WINDOW_NAMES[state.duration] || formatDuration(state.duration)}` : formatRange(rangeFor(state));
  }

  function toDateInput(epoch: Instant) {
    const parts = dateInput.formatToParts(epoch);
    const map = Object.fromEntries(parts.map((part) => [part.type, part.value]));
    return `${map.year}-${map.month}-${map.day}`;
  }

  // The instant a date and a time typed into the range picker name in the page's
  // timezone, or NaN when they name none.
  function instantOfLocal(date: string, time: string) {
    const [year, month, day] = date.split("-").map(Number);
    const [hour, minute] = time.split(":").map(Number);
    if (![year, month, day, hour, minute].every(Number.isFinite)) return Number.NaN;
    const wall = Date.UTC(year, month - 1, day, hour, minute);
    const offsetAt = (instant: Instant) => {
      const parts = Object.fromEntries(wallClock.formatToParts(instant).map((part) => [part.type, part.value]));
      return Date.UTC(Number(parts.year), Number(parts.month) - 1, Number(parts.day), Number(parts.hour), Number(parts.minute)) - Math.floor(instant / MINUTE) * MINUTE;
    };
    const guess = wall - offsetAt(wall);
    return wall - offsetAt(guess);
  }

  function latestSessionFor(agentId: string) {
    return sessionsFor(agentId)
      .filter((session) => !session.parentSession)
      .sort((left, right) => right.start - left.start)[0] || null;
  }

  function lifecycleEvidence(session: AgentSession) {
    if (session.parentSession) {
      return { value: session.end === null ? "running" : "ended", detail: `${session.end === null ? `since ${timeOnly.format(session.start)}` : dateTime.format(session.end)} · Runtime-internal Sub Agent`, title: "Lifecycle: the Runtime's transcript of its Sub Agent; Feld records no lifecycle for it" };
    }
    if (session.end === null) {
      return { value: "running", detail: `since ${timeOnly.format(session.start)}`, title: `Lifecycle: running since ${dateTime.format(session.start)} ${ZONE} (Control Plane fact)` };
    }
    const outcome = outcomeFor(session);
    const value = outcome.label.toLowerCase();
    return { value, detail: `${dateTime.format(session.end)} · ${outcome.detail}`, title: `Lifecycle: ended ${session.endKind} at ${dateTime.format(session.end)} ${ZONE} (Control Plane fact), concluded as "${value}" · ${outcomeBasis(session).full}` };
  }

  function observationEvidence(session: AgentSession) {
    if (!session.observation && session.parentSession) {
      return { value: "not observed", detail: "Runtime-internal Sub Agent", title: "Feld records no Provider observation for a Sub Agent: it runs inside the parent Session's Runtime process" };
    }
    if (!session.observation) {
      return { value: "not observed", detail: "no Provider observation yet", title: "Feld has recorded no Provider observation for this Session yet" };
    }
    const age = formatAge(now() - session.observation.at);
    const reported = session.observation.detail ? ` · ${session.observation.detail}` : "";
    return {
      value: session.observation.kind,
      detail: `${age} · ${timeOnly.format(session.observation.at)}${reported}`,
      title: `Provider observation: ${session.observation.kind} — ${OBSERVATION_MEANING[session.observation.kind]}. Recorded ${dateTime.format(session.observation.at)} ${ZONE} (${age}); not a claim about current external reality`,
    };
  }

  // An instant within a day of now reads as a time; further away it carries its date.
  function nearTime(instant: Instant) {
    return Math.abs(instant - now()) < DAY ? timeOnly.format(instant) : dateTime.format(instant);
  }

  function scheduleSummary(schedule: ScheduleRecord) {
    if (!schedule.enabled) return `Paused since ${nearTime(schedule.pausedAt!)}`;
    const next = nextRunFor(schedule);
    return next === null ? "Never fires" : `Next run ${nearTime(next)}`;
  }

  // The Schedule cell of the Actor table. Shown for a Job with a Schedule. "Next run" is
  // derived at render time from the cron expression and now; "Paused since" is the
  // accepted pause request. Clicking opens the Actor card at its Job section. Skipped ticks are a separate
  // mark on the marks line under the Actor name (skipMarkFor).
  function scheduleCellFor(agent: ActorRecord, state: Pick<AgentsState, "workspace">) {
    const schedule = scheduleFor(agent.id);
    if (!schedule) return null;
    const label = actorLabel(agent.id, state);
    const summary = scheduleSummary(schedule);
    const basis = schedule.enabled
      ? `Derived from the cron expression (${schedule.cron}, ${schedule.timezone}) and the current time; not a stored fact`
      : `${label} is paused (Control Plane fact: the pause request was accepted ${dateTime.format(schedule.pausedAt)} ${ZONE}); no tick fires until it is resumed`;
    return { paused: !schedule.enabled, summary, title: `${summary} · ${schedule.cron} ${schedule.timezone} · ${basis}`, ariaLabel: `Schedule of ${label}: ${summary}` };
  }

  // The skip mark: present only while the Actor's current Session has outlived one or
  // more planned ticks (a schedule.skipped record at or after that Session's start). It
  // clears when the Session ends, because the next tick will then fire. Amber like the
  // unread-mail mark: both are marks that something the current Session should have let
  // happen has not happened; neither is an outcome, so neither orders the bands.
  function skipMarkFor(agent: ActorRecord) {
    const skipped = skippedWhileCurrentFor(agent.id);
    if (!skipped.skips.length) return null;
    const latest = skipped.skips[skipped.skips.length - 1];
    const count = skipped.skips.length;
    const title = `${count} Schedule tick${count === 1 ? "" : "s"} skipped (${skipped.skips.map((skip) => timeOnly.format(skip.planned)).join(", ")}) because ${skipped.session!.id} was still current · schedule.skipped records · clears when the Session ends`;
    return { count, skipId: latest.id, title };
  }

  // The waiting count and the unread-mail mark (see unreadWhileCurrentFor for when the
  // mark applies). With a current Session the Actor shows the mark: that Session has not
  // read these mailbox positions, and clicking it opens the current Session's Console at
  // the oldest waiting Message's accepted row. With no current Session the Actor shows a
  // neutral waiting count, so a report delivered to a stopped Job is still visible in its
  // row. Both reuse the Timeline's open-tray glyph so they read as the same mail.
  // The mark is derived at render time, stores nothing, and applies no age threshold: it
  // states that the current Session has not read these mailbox positions, not that the
  // Session is broken. It is a mark, not an outcome, so it does not order the bands.
  function unreadMarkFor(agent: ActorRecord, state: Pick<AgentsState, "workspace">) {
    const unread = unreadWhileCurrentFor(agent.id);
    if (!unread.session) {
      const waiting = waitingMessagesFor(agent.id);
      if (!waiting.length) return null;
      const oldest = waiting[0];
      const title = `Waiting count: ${waiting.length} Message${waiting.length === 1 ? "" : "s"} in the mailbox with no read recorded · no current Session, so the next Session reads them · oldest from ${actorLabel(oldest.from, state, oldest.fromWorkspace)}, accepted ${timeOnly.format(oldest.accepted)} (${formatAge(now() - oldest.accepted)})`;
      return { idle: true as const, count: waiting.length, title, sessionId: null, recordId: null };
    }
    const oldest = unread.messages[0];
    if (!oldest) return null;
    const count = unread.messages.length;
    const title = `${count} Message${count === 1 ? "" : "s"} accepted and not read while ${unread.session.id} is current · oldest from ${actorLabel(oldest.from, state, oldest.fromWorkspace)}, accepted ${timeOnly.format(oldest.accepted)} (${formatAge(now() - oldest.accepted)}) · derived from Control Plane facts, no age threshold`;
    return { idle: false as const, count, title, sessionId: unread.session.id, recordId: `${oldest.id}:accepted` };
  }

  // The unlinked mark: the current Session is an owned agent-runtime its Runtime has not
  // linked to yet, so no Runtime identity has arrived. It shows for a moment after every
  // start; one that stays means the Runtime waits for an answer in its pane, such as
  // whether to trust the workdir, and nothing reaches it until a person answers there.
  // It is a mark, not an outcome, so it does not order the bands.
  function unlinkedMarkFor(agent: ActorRecord) {
    const session = currentSessionsFor(agent.id)[0];
    if (!session || session.ownership !== "owned" || session.execution.type !== "agent-runtime" || session.linked) return null;
    const attach = session.placement?.attachCommand;
    const title = `${session.id} has no Runtime identity yet · a Runtime that stays unlinked is waiting for an answer in its pane, such as whether to trust the workdir${attach ? ` · answer it there: ${attach}` : ""}`;
    return { sessionId: session.id, title };
  }

  // One text cell of the Actor table (the Schedule cell is scheduleCellFor). Every column
  // reads the latest top-level Session, so a cell is null when that Session carries
  // nothing for it (a program has no Coding Agent).
  function actorDetailCellFor(agent: ActorRecord, detailId: string) {
    const session = latestSessionFor(agent.id);
    if (!session) return detailId === "status" ? { kind: "none" as const, title: "No Session recorded · Control Plane fact" } : null;
    if (detailId === "status") {
      const outcome = outcomeFor(session);
      const when = session.end === null ? `since ${timeOnly.format(session.start)}` : nearTime(session.end);
      const title = session.end === null ? currentStatusSummary(session) : `${outcome.label}: ${outcome.detail}`;
      return { kind: "status" as const, key: outcome.key, label: outcome.label, when, title: `${title} · ${session.id}` };
    }
    if (detailId === "coding-agent") {
      if (session.execution.type !== "agent-runtime") return null;
      return { kind: "text" as const, value: runtimeLabel(session), title: `${runtimeLabel(session)} · ${modelLabel(session)} · ${session.id}` };
    }
    const provider = providerLabel(session.placement!.provider);
    return { kind: "text" as const, value: provider, title: `${provider} · ${placementSummary(session)} · ${session.id}` };
  }

  // Whether the Actor has a current Session, and whether its Schedule fires. The Actor-name
  // glyph draws the key inside and the schedule as its outer ring; its tooltip reads `lines`.
  function actorStateFor(agent: ActorRecord): ActorState {
    const schedule = scheduleFor(agent.id);
    const scheduleKey = !schedule ? null : schedule.enabled ? "scheduled" as const : "paused" as const;
    const scheduleLine = !schedule ? "No Schedule" : `${schedule.enabled ? "Scheduled" : "Schedule paused"} (${schedule.cron} ${schedule.timezone}) · ${scheduleSummary(schedule)}`;
    const session = latestSessionFor(agent.id);
    if (!session) return { key: "none", schedule: scheduleKey, label: "No Session", title: "No Session recorded (Control Plane fact)", lines: [scheduleLine, "Now: No Session"] };
    if (session.end === null) {
      const observed = session.observation
        ? `Last Provider observation: ${session.observation.kind}, ${formatAge(now() - session.observation.at)} at ${dateTime.format(session.observation.at)} ${ZONE}`
        : "No Provider observation yet";
      return { key: "running", schedule: scheduleKey, label: "Running", title: `Running: current Session ${session.id} (Control Plane fact). ${observed}`, lines: [scheduleLine, `Now: Running (since ${nearTime(session.start)})`] };
    }
    const outcome = outcomeFor(session);
    return { key: "stopped", schedule: scheduleKey, label: "Stopped", title: `Stopped: no current Session (Control Plane fact). Latest Session ${session.id} ${outcome.label.toLowerCase()} at ${dateTime.format(session.end)} ${ZONE}`, lines: [scheduleLine, `Now: ${outcome.label} (since ${nearTime(session.end)})`] };
  }

  // The Actors of the selected Workspace, or of every Workspace when none is selected.
  function workspaceAgents(state: Pick<AgentsState, "workspace">) {
    return ACTORS.filter((agent) => !state.workspace || agent.workspace === state.workspace);
  }

  function matchingAgents(state: Pick<AgentsState, "workspace" | "query">) {
    const query = state.query.trim().toLowerCase();
    return workspaceAgents(state).filter((agent) => {
      const placementTerms = (session: AgentSession) => session.placement ? [session.placement.provider, session.placement.node] : [];
      const sessionTerms = sessionsFor(agent.id)
        .flatMap((session) => [session.id, session.execution.label, session.execution.command || "", ...(session.model || []).filter(Boolean), ...placementTerms(session)]);
      // `paused` and `enabled` match a Job's state the way a kind name matches the
      // kind, so the paused Jobs can be listed without a filter chip.
      const schedule = scheduleFor(agent.id);
      const scheduleTerms = schedule ? [schedule.enabled ? "enabled" : "paused", schedule.cron] : [];
      const launchTerms = agent.launchSpec ? [launchSpecLine(agent.launchSpec)] : [];
      return [agent.id, agent.workspace, agent.kind, kindLabel(agent.kind), ...scheduleTerms, ...launchTerms, ...sessionTerms].join(" ").toLowerCase().includes(query);
    });
  }

  function hasSessionInRange(agentId: string, range: FeldMetricRange) {
    return sessionsFor(agentId).some((session) => overlaps(session, range));
  }

  // The Session whose evidence can put an Actor in the attention group: its latest
  // top-level Session, which is the current Session when one exists.
  function attentionSessionsFor(agentId: string) {
    const latest = latestSessionFor(agentId);
    return latest ? [latest] : [];
  }

  function attentionReasons(agentId: string) {
    const reasons: string[] = [];
    attentionSessionsFor(agentId).forEach((session) => {
      const outcome = outcomeFor(session);
      if (outcome.attention) reasons.push(`${session.id} ${outcome.label.toLowerCase()} (Control Plane fact · ${outcomeBasis(session).short})`);
      if (session.end === null && session.observation && ATTENTION_OBSERVATIONS.includes(session.observation.kind)) reasons.push(`${session.id} observed ${session.observation.kind} (Provider observation)`);
    });
    return reasons;
  }

  // Waiting mail is a query-time view over Message facts, not a stored state: every
  // Message accepted for this Agent with no authenticated read recorded. It is derived
  // the same way whether or not the Actor has a current Session.
  function waitingMessagesFor(agentId: string) {
    return WAITING_BY_ACTOR.get(agentId) || [];
  }

  // Unread mail while a Session is current: the AND of two Control Plane facts, the
  // Actor has a current Session (lifecycle) and a Message accepted for it has no
  // authenticated read recorded (messaging). Waiting mail with no current Session is shown
  // as the neutral waiting count instead. No age threshold is applied, because a
  // threshold would be a guess. It is a mark of the Actor, not an outcome.
  function unreadWhileCurrentFor(agentId: string) {
    const session = currentSessionsFor(agentId)[0] || null;
    if (!session) return { session: null, messages: [] as MessageRecord[] };
    return { session, messages: waitingMessagesFor(agentId) };
  }

  function lastActivityFor(agentId: string) {
    return sessionsFor(agentId).reduce((latest, session) => Math.max(latest, sessionEnd(session)), Number.NEGATIVE_INFINITY);
  }

  function bandRank(agentId: string) {
    if (currentSession(agentId)) return 0;
    if (attentionReasons(agentId).length) return 1;
    return 2;
  }

  // In Live mode a Job with an enabled Schedule stays in the table whatever the range:
  // Live answers "what is going on now", and a standing request to run is part of that
  // even when the last run fell outside the window. A paused Schedule is not going on, so
  // pausing is how one retires from Live; a Job without a Schedule and History keep the
  // range rule alone.
  function keptBySchedule(agentId: string, state: Pick<AgentsState, "mode">) {
    return state.mode === "live" && !actorFor(agentId).deletedAt && Boolean(scheduleFor(agentId)?.enabled);
  }

  // Band order: pinned Actors above the rest; within each group the order the reader
  // dragged rows into, then the default: Agents with a current Session, then Agents
  // needing attention, then everything else by latest activity, then by name.
  // Actors whose Sessions do not overlap the visible range are hidden unless pinned,
  // kept by an enabled Job in Live, or shown by the display settings.
  function visibleAgents(state: BandsState) {
    const range = rangeFor(state);
    const orderOf = (agent: ActorRecord) => {
      const index = state.actorOrder.indexOf(agent.id);
      return index < 0 ? Infinity : index;
    };
    return matchingAgents(state)
      .filter((agent) => state.showAgentsOutsideRange || isPinned(state, agent) || hasSessionInRange(agent.id, range) || keptBySchedule(agent.id, state))
      .sort((left, right) => Number(isPinned(state, right)) - Number(isPinned(state, left))
        || (orderOf(left) === orderOf(right) ? 0 : orderOf(left) - orderOf(right))
        || bandRank(left.id) - bandRank(right.id)
        || lastActivityFor(right.id) - lastActivityFor(left.id)
        || left.name.localeCompare(right.name)
        || left.id.localeCompare(right.id));
  }

  // The bands drawn. A pointer gesture keeps the set it started with, so the lanes and the
  // label column under the pointer do not change mid-drag; otherwise the visible Actors.
  function bandAgents(state: BandsState) {
    if (!state.frozenBandIds) return visibleAgents(state);
    return state.frozenBandIds.map((id) => ACTOR_BY_ID.get(id)).filter(Boolean) as ActorRecord[];
  }

  // The Actors the range holds back: those with no Session in it and no pin. The button
  // says what it does, shows them, and once shown offers to hide them again, so an Actor
  // whose Sessions all fall outside the range stays visible as an absence. Null when there
  // are none.
  function hiddenBandsFor(state: BandsState, agents: ActorRecord[]) {
    const range = rangeFor(state);
    const plural = (count: number) => `${count} Actor${count === 1 ? "" : "s"}`;
    if (state.showAgentsOutsideRange) {
      const outside = agents.filter((agent) => !isPinned(state, agent) && !hasSessionInRange(agent.id, range) && !keptBySchedule(agent.id, state)).length;
      if (!outside) return null;
      return { shown: true, label: `Hide ${plural(outside)} with no Sessions in range`, title: `Hide the ${plural(outside)} with no Session in ${formatRange(range)}` };
    }
    const hidden = matchingAgents(state).length - agents.length;
    if (hidden <= 0) return null;
    return { shown: false, label: `Show ${plural(hidden)} with no Sessions in range`, title: `Show the ${plural(hidden)} whose Sessions all fall outside ${formatRange(range)}; pin one to keep it shown` };
  }

  // Why the Timeline has no band drawn. A Workspace that holds no Actor yet offers the
  // declaration; the other two name the filter that emptied it.
  function emptyBandsReason(state: BandsState): "no-actors" | "no-match" | "no-range" {
    if (!workspaceAgents(state).length) return "no-actors";
    if (!matchingAgents(state).length) return "no-match";
    return "no-range";
  }

  function agentCountTitle(state: BandsState, agents: ActorRecord[]) {
    const matching = matchingAgents(state);
    const total = workspaceAgents(state).length;
    const hiddenBySearch = total - matching.length;
    const hiddenByRange = matching.length - agents.length;
    const range = rangeFor(state);
    const kept = agents.filter((agent) => !hasSessionInRange(agent.id, range) && keptBySchedule(agent.id, state)).length;
    const parts = [`${agents.length} of ${total} Actors shown${state.workspace ? ` in Workspace ${state.workspace}` : " across all Workspaces"}.`];
    if (hiddenByRange) parts.push(`${hiddenByRange} hidden because no Session overlaps the visible range (display settings can keep them).`);
    if (kept) parts.push(`${kept} kept by an enabled Schedule although no Session overlaps the range (Live mode).`);
    if (hiddenBySearch) parts.push(`${hiddenBySearch} hidden by the search query.`);
    parts.push("Order: current Session first, then needs attention (UI inference), then latest activity.");
    return parts.join(" ");
  }

  function sessionsFor(agentId: string) {
    return SESSIONS_BY_ACTOR.get(agentId) || [];
  }

  // Every Session of the Actor the page knows, in the range or not: the Actor card's list.
  function allSessionsFor(agentId: string) {
    return KNOWN_SESSIONS_BY_ACTOR.get(agentId) || [];
  }

  function subAgentSessionsFor(agentId: string, state: Pick<AgentsState, "showSubAgents">) {
    if (!state.showSubAgents) return [];
    return sessionsFor(agentId)
      .filter((session) => session.parentSession && sessionEnd(session) > timelineStart(session))
      .sort((left, right) => left.start - right.start);
  }

  // A band with the marks line (unread mail, skipped ticks) takes one more line; the SVG
  // lane grows with it so the label column and the Timeline keep the same geometry.
  function unreadRowHeightFor(agentId: string) {
    return unreadWhileCurrentFor(agentId).messages.length || skippedWhileCurrentFor(agentId).skips.length ? UNREAD_ROW_HEIGHT : 0;
  }

  // Rows inside one band, as sessionId -> row index. Row 0 is the band's main line, where
  // every top-level Session sits (one current Session slot, so generations never overlap);
  // each Sub Agent Session takes its own row below, drawn as a branch.
  function bandRowsFor(agentId: string, state: Pick<AgentsState, "showSubAgents">) {
    const rows = new Map<string, number>();
    sessionsFor(agentId).filter((session) => !session.parentSession).forEach((session) => rows.set(session.id, 0));
    const subAgents = subAgentSessionsFor(agentId, state);
    subAgents.forEach((session, index) => rows.set(session.id, 1 + index));
    return { rows, count: 1 + subAgents.length };
  }

  function laneHeightFor(agentId: string, state: Pick<AgentsState, "showSubAgents">) {
    return LANE.laneHeight + unreadRowHeightFor(agentId) + (bandRowsFor(agentId, state).count - 1) * LANE.subAgentRowHeight;
  }

  function laneLayoutFor(agents: ActorRecord[], state: Pick<AgentsState, "showSubAgents">) {
    let top = 0;
    const layout = new Map<string, Lane>();
    agents.forEach((agent) => {
      const bandRows = bandRowsFor(agent.id, state);
      const height = laneHeightFor(agent.id, state);
      const mainY = top + LANE.mainOffset;
      layout.set(agent.id, {
        top,
        height,
        mainY,
        sessionY: new Map([...bandRows.rows].map(([sessionId, row]) => [sessionId, mainY + row * LANE.subAgentRowHeight])),
      });
      top += height;
    });
    return { agents: layout, height: top };
  }

  // The Metrics window of the Timeline's range (see metricsRangeOf).
  function metricsRangeFor(state: Pick<AgentsState, "mode" | "anchor" | "duration" | "metricsDuration">) {
    return metricsRangeOf(rangeFor(state), metricsDurationFor(state), now());
  }

  function hasMetricsContext(state: Pick<AgentsState, "duration" | "metricsDuration">) {
    return metricsDurationFor(state) !== state.duration;
  }

  // The Metrics window on its button: Live by its length, else by its dates.
  function metricsButtonLabel(state: Pick<AgentsState, "mode" | "anchor" | "duration" | "metricsDuration">) {
    const range = metricsRangeFor(state);
    return state.mode === "live" ? `Last ${WINDOW_NAMES[range.end - range.start] || formatDuration(range.end - range.start)}` : formatRange(range);
  }

  // The series of a lane the search keeps: every series when the lane's name matches,
  // else those whose Runtime or account does, so "claude" keeps only the Claude Code rows
  // of a lane that holds both Runtimes. The Actor table is searched by matchingAgents
  // with the same query.
  function searchedSeries(metric: MetricLane, query: string) {
    if (!query || metric.name.toLowerCase().includes(query)) return metric.series;
    return metric.series.filter((series) => `${series.provider} ${series.account || ""}`.toLowerCase().includes(query));
  }

  // The visible lanes with the series the search keeps. A lane whose name does not match
  // and none of whose series the search keeps is left out; a lane it keeps stays whole, so
  // its label still names every Runtime.
  function visibleMetrics(state: Pick<AgentsState, "visibleMetricIds" | "query">) {
    const query = state.query.trim().toLowerCase();
    return METRICS.filter((metric) => state.visibleMetricIds.has(metric.id)).flatMap((metric) => {
      const series = searchedSeries(metric, query);
      return series === metric.series || series.length ? [{ metric, series }] : [];
    });
  }

  // The lanes as the Metrics section lists them, each in listedSeriesOrder by the latest
  // reading. A lane with more than METRIC_ROW_LIMIT series folds the rest unless it is
  // expanded, and grows by one reading row per row it lists past two.
  function metricLanesFor(state: Pick<AgentsState, "mode" | "anchor" | "duration" | "metricsDuration" | "visibleMetricIds" | "query" | "expandedMetricIds">): ListedMetricLane[] {
    const range = metricsRangeFor(state);
    let top = 0;
    return visibleMetrics(state).map(({ metric, series: searched }) => {
      const valueOf = (series: MetricSeries) => {
        const reading = laneReading(metric, series, range);
        return reading.at === null ? -1 : reading.state === "reset" ? 0 : metricValueAt(metric, series, reading.at)!;
      };
      const ordered = listedSeriesOrder(metric, searched, valueOf);
      const foldable = ordered.length > METRIC_ROW_LIMIT;
      const expanded = foldable && state.expandedMetricIds.has(metric.id);
      const series = foldable && !expanded ? ordered.slice(0, METRIC_ROW_LIMIT) : ordered;
      const rows = series.length + (foldable ? 1 : 0);
      const height = Math.max(METRIC_LANE_HEIGHT, METRIC_LANE_HEIGHT + (rows - 2) * METRIC_ROW_HEIGHT);
      const lane = { metric, series, hiddenCount: ordered.length - series.length, expanded, top, height };
      top += height;
      return lane;
    });
  }

  // The time of a sample, with its date only when it is not the day the range ends on.
  function sampleTimeText(at: Instant, range: FeldMetricRange) {
    return shortDate.format(at) === shortDate.format(range.end) ? timeOnly.format(at) : dateTime.format(at);
  }

  // One series' reading of a lane as its row shows it: the reading, the row's title, the
  // reset that ends a quota reading, and the sample time beside it when the samples
  // stopped before the end of the range.
  function metricReadingFor(metric: MetricLane, series: MetricSeries, range: FeldMetricRange) {
    const reading = laneReading(metric, series, range);
    const reset = metric.kind === "quota" && reading.at !== null && reading.state !== "reset" ? resetAfter(series, reading.at) : null;
    const title = reading.at === null
      ? `${seriesName(series, true)} ${metric.name}: ${reading.text} · ${series.source}`
      : `${metricReadingTitle(metric, series, reading.at, reading.text)} · ${reading.state === "reset" ? "window reset" : "latest sample"} at ${dateTime.format(reading.at)} ${ZONE}${reset ? ` · resets at ${dateTime.format(reset)} ${ZONE}` : ""}`;
    const stopped = reading.end !== null && reading.end < range.end - 1 - MINUTE;
    return { ...reading, title, resetTime: reset ? `resets ${sampleTimeText(reset, range)}` : null, sampleTime: stopped ? `at ${sampleTimeText(reading.at!, range)}` : null };
  }

  // A quota reset marker's title on the Metrics lane.
  function resetTitle(metric: MetricLaneSpec, series: MetricSeries, reset: Instant) {
    return `${seriesName(series, true)} ${metric.name} resets at ${dateTime.format(reset)} ${ZONE} (Runtime-reported resets_at)`;
  }

  // ---- The Actor card's Job section ------------------------------------------------
  // The page offers a short list; the product accepts any IANA name and, when the
  // declaration omits it, resolves the daemon's local timezone to an IANA name at creation
  // and stores that.
  const TIMEZONE_OPTIONS = Object.freeze([...new Set([TIME_ZONE, "UTC", "America/Los_Angeles", "Europe/Berlin"])]);

  // Only ever called with a next-run instant, which is a whole minute after now.
  function relativeLabel(instant: Instant) {
    const gap = instant - now();
    if (gap < DAY) return `in ${formatDuration(gap)}`;
    return `in ${Math.floor(gap / DAY)}d ${Math.floor((gap % DAY) / HOUR)}h`;
  }

  // One row of a Job's runs: a skipped tick, or a Session its tick started.
  function scheduleRunRowFor(entry: ScheduleRun, highlightSkipId: string | null) {
    if (entry.kind === "skip") {
      const skip = entry.skip!;
      return { kind: "skip" as const, id: skip.id, highlighted: skip.id === highlightSkipId, time: dateTime.format(skip.planned), detail: `${skip.previousSession} was still current at the planned tick · schedule.skipped record` };
    }
    const session = entry.session!;
    const outcome = outcomeFor(session);
    const planned = session.planned || session.start;
    const detail = session.end === null
      ? `running since ${timeOnly.format(session.start)}`
      : `${outcome.detail.split(" · ")[0]} · ended ${timeOnly.format(session.end)}`;
    return { kind: "session" as const, id: session.id, time: dateTime.format(planned), timeTitle: `Planned ${dateTime.format(planned)} ${ZONE} · spawn accepted ${preciseTime(session.start)}`, outcomeKey: outcome.key, outcomeLabel: outcome.label, detail };
  }

  // The Actor card's facts of a scheduled Job, in the words its rows show.
  function scheduleFactsFor(schedule: ScheduleRecord) {
    const next = nextRunFor(schedule);
    return {
      enabled: schedule.enabled ? `Control Plane fact${schedule.enabledAt ? ` · resumed ${dateTime.format(schedule.enabledAt)} ${ZONE}` : ""}` : `Paused since ${dateTime.format(schedule.pausedAt)} ${ZONE} · Control Plane fact (the pause request)`,
      nextRun: schedule.enabled ? next === null ? "Never" : `${dateTime.format(next)} (${relativeLabel(next)})` : "—",
    };
  }

  // The latest Session of a Job without a Schedule.
  function unscheduledJobLatest(actor: ActorRecord) {
    const latest = latestSessionFor(actor.id);
    const line = latest
      ? latest.end === null ? `running since ${timeOnly.format(latest.start)} · startKind ${latest.startKind}` : `${outcomeFor(latest).label} ${dateTime.format(latest.end)} · startKind ${latest.startKind}`
      : "No Session recorded";
    return { session: latest, line };
  }

  function validateWorkspaceKey(key: string) {
    if (!/^[a-z0-9][a-z0-9-]*$/.test(key)) return "Key must be lowercase letters, digits, and hyphens, starting with a letter or digit.";
    if (WORKSPACES.includes(key)) return `A Workspace named ${key} already exists; keys are unique.`;
    return null;
  }

  // The startKind of an Actor's next Session: `start` for its first generation,
  // `replacement` when an earlier generation exists.
  function nextStartKindFor(agentId: string) {
    return sessionsFor(agentId).some((session) => !session.parentSession) ? "replacement" : "start";
  }

  // The declaration as the form reports it, read back from the snapshot the daemon
  // answered with so the Schedule's next run is the daemon's own derivation. The start is
  // reported separately, because a start the daemon accepted is not yet a start that ran.
  function newActorResult(draft: ActorDraft, created: CreatedActor, state: Pick<AgentsState, "mode">): NewActorResult {
    const actorKey = actorKeyOf(created.workspace, created.id);
    const schedule = created.cron ? scheduleFor(actorKey) : null;
    const next = nextRunFor(schedule);
    const parts = [`${created.id} was declared as ${kindLabel(draft.kind)}`];
    if (created.cron) parts.push(`(${created.cron} ${schedule ? schedule.timezone : draft.timezone}; next run ${next === null ? "never" : `${dateTime.format(next)} ${ZONE}`})`);
    else if (created.session) parts[parts.length - 1] += ",";
    if (created.session) parts.push(`and the start of ${created.session.id} was accepted (startKind ${created.session.startKind}).`);
    else parts.push(created.cron ? "and no Session was started; the first run is the next tick." : "and no Session was started; one starts on feld actor start.");
    const note = state.mode === "live" && (created.session || created.cron) ? "The band is in the table." : created.session || created.cron ? "Choose a Last range in the range picker to see the band; History keeps the range rule." : "The band appears once a Session overlaps the range.";
    return { text: parts.join(" "), outcome: created.session ? startOutcome(created.session, draft.kind) : null, note, actorKey, job: actorFor(actorKey).kind === "job", sessionId: created.session?.id || null };
  }

  // How the launchSpec differs from what the latest Session was started with: the
  // execution its start recorded and the Placement Feld decided. Only a Session Feld
  // started from the launchSpec (`owned`) compares; an `attached` one ran however the
  // person started it. A field the Session did not record is named as such, and env,
  // runtimeSettings, trustWorkdir, and providerOptions are not recorded with a Session.
  // Null without a Session to compare with.
  function launchSpecDriftFor(agent: ActorRecord) {
    const session = latestSessionFor(agent.id);
    const launchSpec = agent.launchSpec;
    if (!session || !launchSpec) return null;
    if (ownershipOf(session) !== "owned") return { session, comparable: false, changes: [], unrecorded: [] };
    const placement = session.placement;
    const pairs: [field: string, declared: string, recorded: string | null][] = [
      launchSpec.runtime ? ["runtime", launchSpec.runtime, session.execution.runtime ?? null] : ["command", commandLine(launchSpec.command), session.execution.command],
      ["workdir", launchSpec.workdir, placement?.workdir || null],
      ["provider", launchSpec.provider, placement?.provider || null],
      ["node", launchSpec.node, placement?.node || null],
    ];
    return {
      session,
      comparable: true,
      changes: pairs.filter(([, declared, recorded]) => recorded !== null && declared !== recorded).map(([field, declared, recorded]) => ({ field, declared, recorded: recorded! })),
      unrecorded: pairs.filter(([, , recorded]) => recorded === null).map(([field]) => field),
    };
  }

  // The Actor's current Session while it runs, which the daemon's listing answers whatever
  // the range; the range's latest Session stands in without one. A Session ending notifies
  // the Session alone, so the listing may still name one the page holds as ended.
  function runningSessionOf(agent: ActorRecord) {
    if (agent.currentSession === undefined) {
      const latest = latestSessionFor(agent.id);
      return latest && latest.end === null ? { id: latest.id, ownership: ownershipOf(latest) } : null;
    }
    const held = agent.currentSession && SESSION_BY_ID.get(agent.currentSession.id);
    return held && held.end !== null ? null : agent.currentSession;
  }

  // The items of an Actor's Actions menu, in order, with the separators between groups,
  // and the note of a request of this menu in flight or refused.
  function agentMenuFor(agent: ActorRecord, state: Pick<AgentsState, "actionRequest" | "actorPins">) {
    const session = latestSessionFor(agent.id);
    const current = Boolean(session && session.end === null);
    // Feld holds no stop authority over an attached Session: Stop and Kill are not
    // offered while one is current; the person who started it ends it.
    const attached = current && ownershipOf(session!) === "attached";
    const schedule = scheduleFor(agent.id);
    const request = state.actionRequest?.agentId === agent.id ? state.actionRequest : null;
    const busy = Boolean(request?.pending);
    type MenuEntry = { kind: "item"; action: string; label: string; note: string; disabled: boolean } | { kind: "rule" };
    const item = (action: string, label: string, note: string, disabled = false): MenuEntry => ({ kind: "item", action, label, note, disabled: disabled || busy });
    const pinned = isPinned(state, agent);
    const items: MenuEntry[] = [
      item("toggle-pin", pinned ? "Unpin" : "Pin to top", pinned
        ? `${agent.kind === "job" ? "Jobs start pinned · " : ""}the row leaves the top and shows only while a Session overlaps the range`
        : "keeps the row above the unpinned ones, whatever the range"),
      { kind: "rule" },
    ];
    // A deleted Actor keeps its history only: nothing starts it or addresses it.
    if (agent.deletedAt) {
      items.push(item("deleted", "Deleted", `${dateTime.format(agent.deletedAt)} ${ZONE} · its Sessions and Messages stay; feld actor list -a --include-deleted lists it`, true));
      return { sessionId: session?.id || null, items, note: null };
    }
    if (agent.kind === "agent") items.push(item("open-agent", "Agent…", "the launch spec, grants, and name in the Actor card, with Edit"));
    if (current) {
      items.push(item("open-console", "Open Console", `Console of ${session!.id} over the Timeline · read-only`));
      if (!attached) {
        items.push(item("stop-session", "Stop session", `feld session stop · ${END_ATTEMPTS.stop} (graceful; the ${providerLabel(session!.placement!.provider)} Provider decides what it tries)`));
        items.push(item("terminate-session", "Kill session", `feld session kill · ${END_ATTEMPTS.kill} without asking (force; the ${providerLabel(session!.placement!.provider)} Provider decides what it tries)`));
      }
    } else {
      // `feld actor start` starts a Session from the Actor's launchSpec now; its startKind is
      // replacement when an earlier generation exists. Without a launchSpec there is
      // nothing to start from.
      items.push(agent.launchSpec
        ? item("start-replacement", "Start session", `feld actor start · runs the launchSpec now (${launchSpecLine(agent.launchSpec)}) · startKind ${nextStartKindFor(agent.id)}`)
        : item("start-replacement", "Start session", `No launchSpec: this Actor was created by attach · give one with feld actor update ${agent.id} … to start it from Feld`, true));
    }
    // Pause and Resume exist only with a Schedule, toggle its `enabled`, and are recorded
    // like any other request; the current Session is never touched. The Job's Schedule,
    // launch spec and runs are in its Actor card, behind its name.
    if (agent.kind === "job" && schedule) {
      items.push({ kind: "rule" });
      items.push(schedule.enabled
        ? item("pause-job", "Pause schedule", "feld actor pause · no tick fires until resumed; the current Session is not touched")
        : item("resume-job", "Resume schedule", "feld actor resume · the next run is computed from now, not caught up"));
      items.push({ kind: "rule" });
    }
    items.push(item("send-message", "Send message to mailbox", "feld message send · stored in the durable inbox, not typed into a console"));
    // The daemon refuses the delete while the Actor's current Session runs.
    const running = runningSessionOf(agent);
    items.push({ kind: "rule" });
    items.push(running
      ? item("delete-actor", "Delete actor", `${running.id} is running · ${running.ownership === "attached" ? "the person who started it ends it" : "stop or kill it"} before the delete`, true)
      : item("delete-actor", "Delete actor", "feld actor delete · leaves the bands, start, and Message addressing; its Sessions and Messages stay"));
    return { sessionId: session?.id || null, items };
  }

  // The Message composer's facts: whether the Actor has a current Session to read the
  // Message, and the Messages already waiting ahead of it.
  function composerFor(agentId: string) {
    const session = latestSessionFor(agentId);
    const current = Boolean(session && session.end === null);
    return { session, current, waiting: waitingMessagesFor(agentId) };
  }

  // What the Console of a Session is drawn with, apart from the controls the card adds.
  function consoleOptionsFor(session: AgentSession, state: Pick<AgentsState, "workspace">) {
    const outcome = outcomeFor(session);
    return {
      actorLabel: session.subAgent ? `${actorLabel(session.actor, state)} / ${session.subAgent}` : actorLabel(session.actor, state),
      feldRecords: feldRecordsFor(session, state),
      timeOrigin: recordingOriginFor(session),
      sessionStart: session.start,
      status: outcome.key,
      statusLabel: outcome.label,
      statusTitle: outcome.detail,
      timeZone: TIME_ZONE,
    };
  }

  function consoleSessionFor(sessionId: string) {
    return CONSOLE_SESSION_BY_ID.get(sessionId) || null;
  }

  // Delivery summary shown in a Console's record row. It is a query-time view
  // derived from the delivery facts below and stores no state of its own:
  //   no read recorded  -> "Waiting in mailbox since <accepted>"
  //   read recorded     -> "Read by <session> at <read>"
  // The one read record covers both routes to the model (the Plugin Monitor helper's
  // long-poll and the Session's own `feld self message recv`). Whether the Runtime retained the
  // read is a transcript entry, shown separately, and semantic processing is always
  // not_claimed, so neither changes the summary.
  function deliverySummary(message: MessageRecord) {
    if (!message.read) return `Waiting in mailbox since ${timeOnly.format(message.accepted)}`;
    return `Read by ${message.read.sessionId} at ${timeOnly.format(message.read.at)}`;
  }

  // The transcript entry that quotes a record ID, found in the transcript file of the
  // Session that issued or read the record. Feld never stores this link: the Runtime
  // copied the ID from what Feld printed (the Monitor event line or the `feld message send`
  // output), and the Console finds it again by text at render time.
  function runtimeRecordFor(sessionId: string | null, anchor: string | null) {
    if (!sessionId || !anchor) return null;
    const session = sessionById(sessionId);
    if (!session) return null;
    return consoleReads.runtimeRecordFor(sessionId, anchor, recordingOriginFor(session)) || null;
  }

  function runtimeRecordFact(message: MessageRecord) {
    const ref = messageRuntimeRef(message);
    const session = sessionById(ref.sessionId);
    if (!session) {
      return { key: "runtime", label: "Transcript entry", value: "Not applicable", detail: "Unread, and not sent from a Session: a transcript entry can quote it only once a Session reads it", provenance: "No transcript entry" };
    }
    if (!ref.anchor) {
      return { key: "runtime", label: "Transcript entry", value: "No ID to match", detail: `${session.id} · this fixture record carries no Feld ID to look for in the transcript file`, provenance: "Transcript entry · not searched" };
    }
    const found = runtimeRecordFor(session.id, ref.anchor);
    if (!found) {
      return { key: "runtime", label: "Transcript entry", value: "Not quoted", detail: `${session.id} · no readable transcript file of this Session quotes ${ref.anchor}`, provenance: "Transcript entry · searched by ID" };
    }
    return { key: "runtime", label: "Transcript entry", value: found.label, detail: `${session.id} · entry ${found.cursor} at ${preciseDateTime(found.at!)} ${ZONE} quotes ${ref.anchor}`, provenance: "Transcript entry · matched by ID", sessionId: session.id, recordId: message.id };
  }

  // The delivery facts of one Message, each attributed on its own. None of them is
  // promoted into another; a missing record is stated as missing rather than inferred.
  function messageFactRows(message: MessageRecord, state: Pick<AgentsState, "workspace">) {
    const rows: (FeldConsoleFact & { sessionId?: string; recordId?: string })[] = [
      { key: "from", label: "From", value: message.from === "Admin" ? "Admin" : `${actorLabel(message.from, state, message.fromWorkspace)} · ${message.fromSessionId}`, detail: message.fromSessionId ? "Workspace-qualified Actor and the exact Session whose token sent it, saved at acceptance" : "Sent by the Admin with a local or browser token; no Session", provenance: "Control Plane fact" },
      { key: "to", label: "To", value: `${actorLabel(message.to, state, message.toWorkspace)} · ${message.toSessionId || "no current Session"}`, detail: "Workspace-qualified Actor mailbox and the Session current at acceptance, saved then (toSessionId)", provenance: "Control Plane fact" },
      { key: "accepted", label: "Accepted", value: `${preciseDateTime(message.accepted)} ${ZONE}`, detail: "Persisted in the durable inbox before the sender was acknowledged", provenance: "Control Plane fact" },
    ];
    if (message.read) {
      rows.push({ key: "read", label: "Authenticated mailbox read", value: message.read.sessionId, detail: `${preciseDateTime(message.read.at)} ${ZONE} · read with its Session-scoped token · ${message.read.readId} · fifo ${message.read.fifo}`, provenance: "Control Plane fact" });
    } else {
      rows.push({ key: "read", label: "Authenticated mailbox read", value: "Not recorded", detail: "Accepted and waiting; no Session has read this mailbox position", provenance: "Control Plane fact · no read record" });
    }
    rows.push(runtimeRecordFact(message));
    rows.push({ key: "semantic", label: "Semantic processing", value: "not_claimed", detail: "Always; Feld does not judge whether the model noticed or acted", provenance: "Not claimed by Feld" });
    return rows;
  }

  // Session details' facts that are sentences rather than records: the Ended row, the
  // schedule timing of the Start kind row, and the Outcome row's title.
  function sessionDetailsFor(session: AgentSession) {
    const end = sessionEnd(session);
    const outcome = outcomeFor(session);
    return {
      started: dateTime.format(session.start),
      ended: session.end === null ? "Not recorded" : dateTime.format(end),
      endedDetail: `${session.end === null ? `Drawn through ${timeOnly.format(end)} · ` : ""}${formatDuration(end - session.start)}`,
      startKindDetail: session.parentSession ? "Spawned as a Sub Agent inside it · transcript entry" : `${session.startKind === "schedule" ? `planned ${preciseTime(session.planned!)} · spawn accepted ${preciseTime(session.start)} (${millisecondsLabel(session.start, session.planned!)}) · ` : ""}Control Plane fact`,
      outcomeTitle: `Derived from two Control Plane facts (ended ${session.endKind} with ${session.exitCode === null ? "no exit code" : `exit ${session.exitCode}`}), the Session's ownership, and the kind rule · ${outcomeBasis(session).full}`,
      outcome,
      basis: outcomeBasis(session),
    };
  }

  // The Console of the Session whose records hold a record, at that record's row: a
  // Spawn opens the spawner's Session named on the record (or the created Session's); a
  // Message opens the sender's Session (or the reader's when the sender is the Admin,
  // or the recipient's Session named as current at acceptance). A Message with none of
  // the three has no Console row and is listed only in the mailbox, so it opens nothing
  // (null).
  function recordConsoleTarget(kind: "action" | "message", recordId: string | undefined) {
    if (kind === "action") {
      const action = recordId ? ACTION_BY_ID.get(recordId) : undefined;
      return action ? { sessionId: spawnerSessionFor(action)?.id || action.session, recordId: action.id } : null;
    }
    const message = messageById(recordId);
    if (!message) return null;
    const sender = senderSessionFor(message);
    const recipient = recipientSessionFor(message);
    if (sender) return { sessionId: sender.id, recordId: message.id };
    if (message.read) return { sessionId: message.read.sessionId, recordId: message.id };
    if (recipient) return { sessionId: recipient.id, recordId: `${message.id}:accepted` };
    return null;
  }

  // What the selected Session or Actor touches: the Sessions, Spawn actions, Messages, and
  // Actors that the timeline should keep at full strength while everything else recedes.
  // Derived at draw time; nothing is stored. A Session is related to the Spawn that created
  // it, the Spawns and Messages its Actor issued while it ran, the Messages it read or that
  // arrived while it ran, and its parent or Sub Agent Sessions. A selected Actor is related
  // to every Spawn and Message it sent or received. The Sessions and Actors at the other end
  // of those links are related as well. A selected Session takes precedence over a selected
  // Actor.
  function focusFor(state: Pick<AgentsState, "inspectedSessionId" | "actorCard" | "focusedActorId">): Focus | null {
    const session = sessionById(selectedSessionId(state));
    const actorId = session ? session.actor : state.focusedActorId;
    if (!actorId) return null;
    const focus = { sessions: new Set<string>(), actions: new Set<string>(), messages: new Set<string>(), agents: new Set<string>() };
    const addSession = (candidate: string | AgentSession | null | undefined) => { if (candidate) focus.sessions.add(typeof candidate === "string" ? candidate : candidate.id); };
    const addAction = (entry: SpawnAction) => {
      focus.actions.add(entry.id);
      focus.agents.add(entry.spawner);
      focus.agents.add(entry.target);
      addSession(entry.session);
      addSession(spawnerSessionFor(entry));
    };
    const addMessage = (entry: MessageRecord) => {
      focus.messages.add(entry.id);
      if (entry.from !== "Admin") focus.agents.add(entry.from);
      focus.agents.add(entry.to);
      addSession(senderSessionFor(entry));
      addSession(entry.read ? entry.read.sessionId : recipientSessionFor(entry));
    };
    focus.agents.add(actorId);
    if (session) {
      addSession(session);
      addSession(session.parentSession);
      SESSIONS.filter((candidate) => candidate.parentSession === session.id).forEach(addSession);
      // A record belongs to this Session when the record names it: as the issuing
      // Session, the reader, or the recipient's Session current at acceptance.
      ACTIONS.filter((entry) => entry.session === session.id || entry.spawnerSessionId === session.id).forEach(addAction);
      MESSAGES.filter((entry) => (entry.read && entry.read.sessionId === session.id)
        || entry.fromSessionId === session.id
        || entry.toSessionId === session.id).forEach(addMessage);
    } else {
      sessionsFor(actorId).forEach(addSession);
      ACTIONS.filter((entry) => entry.spawner === actorId || entry.target === actorId).forEach(addAction);
      MESSAGES.filter((entry) => entry.from === actorId || entry.to === actorId).forEach(addMessage);
    }
    return focus;
  }

  // Rows for Session details' Related activity list, in time order. Each row opens
  // the record it names.
  function relatedActivityFor(session: AgentSession, state: Pick<AgentsState, "workspace">) {
    const rows: RelatedRow[] = [];
    const focus = focusFor({ inspectedSessionId: session.id, actorCard: null, focusedActorId: null })!;
    const schedule = session.startKind === "schedule" ? scheduleFor(session.actor) : null;
    if (schedule) {
      rows.push({ at: session.planned || session.start, kind: "schedule", id: schedule.actor, glyph: "◷", label: `Scheduled run of ${schedule.actor}`, detail: `planned ${timeOnly.format(session.planned || session.start)} · ${schedule.cron} ${schedule.timezone}` });
    }
    if (session.parentSession) {
      rows.push({ at: session.start, kind: "session", id: session.parentSession, glyph: "⌥", label: `Sub Agent of ${session.parentSession}`, detail: `${timeOnly.format(session.start)} · parent Session` });
    }
    SESSIONS.filter((candidate) => candidate.parentSession === session.id).forEach((child) => {
      rows.push({ at: child.start, kind: "session", id: child.id, glyph: "⌥", label: `Sub Agent ${child.subAgent}`, detail: `${timeOnly.format(child.start)}–${timeOnly.format(sessionEnd(child))} · ${modelLabel(child)}` });
    });
    ACTIONS.filter((entry) => focus.actions.has(entry.id)).forEach((entry) => {
      rows.push(entry.session === session.id
        ? { at: entry.at, kind: "action", id: entry.id, glyph: "↯", label: `Spawned by ${actorLabel(entry.spawner, state, entry.spawnerWorkspace)}`, detail: `${timeOnly.format(entry.at)} · ${entry.id}` }
        : { at: entry.at, kind: "action", id: entry.id, glyph: "↯", label: `Spawned ${actorLabel(entry.target, state, entry.targetWorkspace)}`, detail: `${timeOnly.format(entry.at)} · ${entry.session}` });
    });
    MESSAGES.filter((entry) => focus.messages.has(entry.id)).forEach((entry) => {
      const delivery = entry.read ? `read by ${entry.read.sessionId} ${timeOnly.format(entry.read.at)}` : "waiting in mailbox";
      rows.push(entry.from === session.actor
        ? { at: entry.accepted, kind: "message", id: entry.id, glyph: "✉", label: `Message to ${actorLabel(entry.to, state, entry.toWorkspace)}`, detail: `${timeOnly.format(entry.accepted)} · ${delivery}` }
        : { at: entry.accepted, kind: "message", id: entry.id, glyph: "✉", label: `Message from ${actorLabel(entry.from, state, entry.fromWorkspace)}`, detail: `${timeOnly.format(entry.accepted)} · ${delivery}` });
    });
    return rows.sort((left, right) => left.at - right.at);
  }

  // The instant of the recording that coincides with the Session's start on the fixture
  // clock. A Session that mirrors a real run names the real instant; every other Session
  // aligns the first record of its exercise recording with its own start. The Console
  // shifts displayed Runtime instants by the difference and keeps their order and gaps.
  function recordingOriginFor(session: AgentSession): FeldConsoleTimeOrigin | null {
    if (session.recordingOrigin) return { recordingInstant: Date.parse(session.recordingOrigin), sessionInstant: session.start };
    const recording = consoleReads.recordingFor(session.id);
    if (!recording?.startedAt) return null;
    return { recordingInstant: Date.parse(recording.startedAt), sessionInstant: session.start };
  }

  // The last entry of a current Session's execution: for an agent-runtime Session the
  // last transcript entry whose kind carries a turn (its kind, instant, and for a tool call
  // the tool and whether its result is held), for a program Session the last stdout or
  // stderr line. Null when there is no readable record.
  function lastRecordFor(session: AgentSession) {
    const live = session.end === null;
    if (session.execution.type === "program") return consoleReads.lastLogLineFor(session.id, session.start, live) || null;
    return consoleReads.lastRuntimeRecordFor(session.id, recordingOriginFor(session), live) || null;
  }

  // turn: whether the Runtime of a current agent-runtime Session is in the middle of a
  // response or waiting for input, as the Session read carries it. Null for a program
  // Session, a Sub Agent, or an ended Session. Never feeds attention.
  function turnFor(session: AgentSession) {
    if (session.end !== null || session.parentSession || session.execution.type !== "agent-runtime") return null;
    return session.turn || "unavailable";
  }

  // The Console's last record when it is the entry the Session read's turn was read from,
  // for what the turn alone does not carry: the entry's kind and a tool call's tool and
  // result. Null until the Console reads that entry.
  function turnRecordFor(session: AgentSession) {
    const last = lastRecordFor(session);
    return last && session.lastEntryAt != null && last.at === session.lastEntryAt ? last : null;
  }

  // The Actor line's Session status for a current Session: the lifecycle, the Provider
  // observation when it is not `running`, and the turn with the last entry's instant.
  function currentStatusSummary(session: AgentSession) {
    const turn = turnFor(session);
    if (!turn || turn === "unavailable" || session.lastEntryAt == null) return `Running since ${timeOnly.format(session.start)}`;
    const running = !session.observation ? "Running (not observed yet)" : session.observation.kind === "running" ? "Running" : `Running (observed ${session.observation.kind})`;
    const since = nearTime(session.lastEntryAt);
    if (turn === "waiting-input") return `${running} · waiting for input since ${since}`;
    const last = turnRecordFor(session);
    if (last?.kind === "tool-call" && !last.hasResult) return `${running} · tool call ${last.tool}, no result since ${since}`;
    return `${running} · responding, last entry ${since}`;
  }

  function processMeaning(session: AgentSession) {
    switch (session.observation?.kind) {
      case undefined: return "The Provider has not observed the recorded resource yet.";
      case "running": return `The ${providerLabel(session.placement!.provider)} Provider shows the recorded resource alive; says nothing about progress.`;
      case "lost": return "The Provider no longer exposes the recorded resource; whether the process ended is not proven.";
      case "gone": return "The Provider proved the process exited; the Session end is not recorded yet.";
      default: return "The resource no longer matches the recorded process.";
    }
  }

  // Why a turn reads as it does. An unavailable turn names what the Console knows of the
  // transcript file: a location that can no longer be read, a file not created yet, or no
  // location delivered. The Console's own read of the file is newer than the range reads'.
  // A responding turn names the kind of its entry once the Console holds that entry.
  function turnMeaning(session: AgentSession, turn: string, last: ReturnType<typeof turnRecordFor>) {
    if (turn === "waiting-input") return "The last entry ends a response; the Runtime waits for a prompt or a notification.";
    if (turn === "unavailable") {
      const listed = consoleSessionFor(session.id);
      const known = listed ? consoleReads.consoleSessionFor(listed) : null;
      if (known?.sourceUnavailable) return `The transcript file cannot be read: ${known.sourceUnavailable}.`;
      if (known?.console === "missing") return "No transcript file exists yet; the Runtime creates it at the first input.";
      return "The Runtime Adapter has not delivered the transcript file's location.";
    }
    if (!last) return "The Runtime is in the middle of a response: the model is working, or the Runtime is stuck.";
    if (last.kind === "tool-call") return "The tool is still running, or the Runtime is stuck.";
    if (last.kind === "tool-result") return "The Runtime has the tool result and has not answered it yet: the model is working, or the Runtime is stuck.";
    if (last.kind === "thinking") return "The Runtime is reasoning: the model is working, or the Runtime is stuck.";
    return "The Runtime has an input with no response after it yet: the model is working, or the Runtime is stuck.";
  }

  function lastRecordRow(session: AgentSession, last: ReturnType<typeof lastRecordFor>) {
    if (!last) {
      return { value: "None readable", meaning: session.execution.type === "program" ? "The Provider has captured no output from this program." : "No transcript entry can be read for this Session." };
    }
    const age = formatAge(now() - last.at!);
    const noResult = last.kind === "tool-call" && !last.hasResult ? " · no result" : "";
    const meaning = session.execution.type === "program"
      ? "The last line the program wrote; a program can work without writing."
      : last.kind === "tool-call" && !last.hasResult
        ? "No result is recorded for this tool call; the time is when the call was recorded."
        : "The last entry in the transcript file; how long ago it was is for you to judge.";
    return { value: `${last.label}${noResult} · ${nowInstant(last.at!)} (${age})`, meaning };
  }

  // A Now instant: to the second within a day of now, with its date further away.
  function nowInstant(instant: Instant) {
    return Math.abs(instant - now()) < DAY ? timeWithSeconds.format(instant) : dateTime.format(instant);
  }

  function lastReadFor(agentId: string) {
    return MESSAGES
      .filter((message) => message.to === agentId && message.read)
      .reduce<MessageRead | null>((latest, message) => latest && latest.at >= message.read!.at ? latest : message.read, null);
  }

  // The Now section of Session Details: facts from different sources about a current
  // Session, one row each with what the row means. It orders nothing, applies no age
  // threshold, and concludes nothing about progress. Null for an ended Session and a Sub
  // Agent. The Last entry row carries the record the Console opens at, when there is one.
  function nowSectionFor(session: AgentSession) {
    if (session.end !== null || session.parentSession) return null;
    const turn = turnFor(session);
    const last = lastRecordFor(session);
    const observed = session.observation;
    const waiting = waitingMessagesFor(session.actor);
    const lastRead = lastReadFor(session.actor);
    const lastRow = lastRecordRow(session, last);
    const rows: { label: string; value: string; meaning: string; openRecordId?: string }[] = [];
    rows.push({ label: "Process", value: observed ? `${observed.kind} · observed ${nowInstant(observed.at)} (${formatAge(now() - observed.at)})` : "not observed yet", meaning: processMeaning(session) });
    if (turn) rows.push({ label: "Turn", value: turn, meaning: turnMeaning(session, turn, turnRecordFor(session)) });
    rows.push({ label: "Last entry", value: lastRow.value, meaning: lastRow.meaning, ...(last ? { openRecordId: last.recordId } : {}) });
    rows.push({ label: "Mailbox", value: `${waiting.length} waiting${waiting.length ? ` · oldest accepted ${nearTime(waiting[0].accepted)}` : ""} · ${lastRead ? `last read ${nearTime(lastRead.at)}` : "no read recorded"}`, meaning: "Messages accepted with no read recorded; a read means the Message was handed to the Session, not that it was acted on." });
    endRequestsFor(session).forEach((request) => {
      rows.push({ label: "Stop", value: `feld session ${request.kind} accepted ${nowInstant(request.accepted)}`, meaning: "Accepted and the Session has not ended; the attempt has not finished, or it did not end the process." });
    });
    return rows;
  }

  // The milliseconds follow the seconds, before a 12-hour clock's day period.
  function preciseTime(instant: Instant) {
    const milliseconds = String(new Date(instant).getMilliseconds()).padStart(3, "0");
    return timeWithSeconds.format(instant).replace(/(\d{2}:\d{2}:\d{2})/, `$1.${milliseconds}`);
  }

  // The latest observation as the dashed line and the Console's unknown end name it: the
  // instant Feld lost sight of the execution first, then the last time it looked.
  function lostLabel(session: AgentSession) {
    const observation = session.observation;
    if (!observation) return "never observed";
    const last = `last observed ${observation.kind} ${preciseTime(observation.at)}`;
    return ATTENTION_OBSERVATIONS.includes(observation.kind) ? `observed ${observation.kind} since ${preciseTime(observation.since ?? observation.at)} · ${last}` : last;
  }

  // "Aug 30, 15:16:21.528": Feld instants keep their millisecond precision where two
  // records of the same second have to be told apart.
  function preciseDateTime(instant: Instant) {
    return dateTime.format(instant).replace(/\d{2}:\d{2}(\s?[AP]M)?$/, preciseTime(instant));
  }

  // The observation records of a Session: one per change of the observed kind, the first
  // `running` included. Feld records each of them, so they are read from the Session's
  // own records; until that read arrives they are the settled start and, if it differs, the
  // latest kind, which is all the Session read carries.
  function observationRecordsFor(session: AgentSession): SessionObservation[] {
    if (!session.observation) return [];
    if (session.observations) return session.observations;
    const feldRecords = consoleReads.feldRecordsFor(session.id);
    const recorded = (feldRecords || [])
      .filter((record) => record.kind === "observation" && record.data?.kind)
      .map((record) => ({ kind: record.data.kind, at: instantOf(record.at), detail: record.data.detail || null }))
      .filter((observation): observation is Required<SessionObservation> => observation.at !== null);
    if (recorded.length) return recorded;
    // A start that never began has no `running` behind it, so the latest kind is the only
    // record the Session holds until the records read arrives.
    if (session.lifecycle?.startFailed) return [session.observation];
    const settled = session.lifecycle?.startSettled || session.start;
    const first: SessionObservation = { kind: "running", at: settled };
    return session.observation.kind === "running" ? [first] : [first, session.observation];
  }

  // Feld's own records of one Session, in the shape the Console interleaves with the
  // transcript file: the same ten record types `feld record list` returns for the
  // Session (session.started, spawned, message.accepted, message.sent, message.read,
  // stop.requested, kill.requested, session.ended, observation, schedule.skipped), each
  // with the instant Feld recorded, the facts behind it with their provenance, the Feld
  // ID that the transcript file may quote, and the surfaces at the other end.
  // `conversation` says whether the Conversation view shows the row; observation and
  // schedule.skipped rows appear only in All events and the log view. Derived at render
  // time from the lifecycle, Spawn, Message, observation, and skip records; nothing is
  // stored, and no transcript entry is reinterpreted as a Feld fact.
  function feldRecordsFor(session: AgentSession, state: Pick<AgentsState, "workspace">) {
    const rows: FeldConsoleRecord[] = [];
    const end = sessionEnd(session);
    const lifecycle = session.lifecycle || {};
    const spawn = SPAWN_BY_SESSION.get(session.id);
    const label = (actorId: string, workspace?: string | null) => actorLabel(actorId, state, workspace);
    const row = (type: string, kind: string, fields: Partial<FeldConsoleRecord> & Pick<FeldConsoleRecord, "id" | "at" | "title" | "summary" | "facts">): FeldConsoleRecord => ({ type, kind, conversation: type !== "observation" && type !== "schedule.skipped", anchor: null, links: [], ...fields });
    const requesterFact = (requesterSessionId: string | null, actorId: string) => requesterSessionId
      ? { label: "Requester", value: `${label(actorId)} · ${requesterSessionId}`, detail: "The exact Session whose token made the request, saved at acceptance", provenance: "Control Plane fact" }
      : { label: "Requester", value: "Admin", detail: "Requested by the Admin with a local or browser token; no Session", provenance: "Control Plane fact" };

    if (session.parentSession) {
      // A Sub Agent's rows are the Runtime's transcript projected into the same shape.
      rows.push(row("session.started", "session-start", { id: `${session.id}:start`, at: session.start, title: "Sub Agent started", summary: `inside ${session.parentSession} · transcript entry`, facts: [
        { label: "Parent Session", value: session.parentSession, detail: "Runs inside the parent Session's Runtime process; its start, end, and parent come from the Runtime's transcript, and Feld records no lifecycle, observation, or Binding for it", provenance: "Transcript entry" },
      ], links: [{ label: `Open ${session.parentSession}`, sessionId: session.parentSession, recordId: `${session.parentSession}:start` }] }));
      if (session.end !== null) {
        rows.push(row("session.ended", "end-ended", { id: `${session.id}:end`, at: end, title: "Sub Agent ended", summary: "Transcript entry · no Feld lifecycle", facts: [
          { label: "Ended", value: `${preciseDateTime(end)} ${ZONE}`, detail: "The Runtime's transcript of the Sub Agent; the Provider records no exit status for it", provenance: "Transcript entry" },
        ] }));
      }
      return rows;
    }

    const settled = lifecycle.startSettled || null;
    const schedule = session.startKind === "schedule" ? scheduleFor(session.actor) : null;
    const ownership = ownershipOf(session);
    const facts = [
      { label: "Start kind", value: session.startKind, detail: `${preciseDateTime(session.start)} ${ZONE} · ${START_KIND_MEANING[session.startKind]}`, provenance: "Control Plane fact" },
      { label: "Ownership", value: ownership!, detail: ownership === "attached" ? "Feld attached to an execution that was already running; it neither started it nor holds the authority to stop it" : "Feld started this execution and holds the authority to stop it", provenance: "Control Plane fact" },
      // A scheduled start names the planned instant it fired for; the request's
      // idempotency key is derived from the Actor and that instant, so a daemon
      // restart in the middle of the tick cannot create a second Session for it.
      ...(session.startKind === "schedule" ? [{ label: "Schedule", value: `planned ${preciseDateTime(session.planned!)} ${ZONE}`, detail: `${schedule ? `${schedule.cron} ${schedule.timezone} · ` : ""}idempotency key derived from (actor, planned instant)`, provenance: "Control Plane fact" }] : []),
      spawn
        ? spawn.spawnerSessionId
          ? { label: "Spawner", value: `${label(spawn.spawner, spawn.spawnerWorkspace)} · ${spawn.spawnerSessionId}`, detail: "Workspace-qualified Actor and the exact Session whose token made the request, saved at acceptance", provenance: "Control Plane fact" }
          : { label: "Spawner", value: "Admin", detail: "Requested by the Admin with a local or browser token; no Session", provenance: "Control Plane fact" }
        : session.startKind === "attach"
          ? { label: "Requester", value: session.attachCommand ? "Admin" : "The Session itself", detail: session.attachCommand || "feld actor attach --self from inside the running Session; the pane it runs in was recorded as the existing resource", provenance: "Control Plane fact" }
          : session.startKind === "schedule"
            ? { label: "Requester", value: "The daemon, for the Job's Schedule", detail: "A standing request the Admin declared in advance; not a Session token and not a reaction to the mailbox or to a failure", provenance: "Control Plane fact" }
            : requesterFact(null, session.actor),
      ...(session.execution.command === null ? [{ label: "Execution", value: `agent-runtime · ${session.execution.label}`, detail: ownership === "attached" ? "From the attach request's --runtime; Feld did not start the execution and records no command" : `From the Actor's runtime; Feld started the execution via the ${session.execution.label} Adapter, which manages the command`, provenance: "Control Plane fact" }] : []),
    ];
    const parentSession = spawn ? spawnerSessionFor(spawn) : null;
    const summary = session.startKind === "schedule"
      ? `schedule fired · planned ${preciseTime(session.planned!)} · spawn accepted ${millisecondsLabel(session.start, session.planned!)}${settled ? ` · running observed ${millisecondsLabel(settled, session.start)}` : ""}`
      : `${session.startKind} accepted${spawn ? ` · spawner ${label(spawn.spawner, spawn.spawnerWorkspace)}` : ""}${settled ? ` · running observed ${millisecondsLabel(settled, session.start)}` : ""}`;
    const links: FeldConsoleLink[] = parentSession ? [{ label: `Open ${label(spawn!.spawner, spawn!.spawnerWorkspace)}'s Console`, sessionId: parentSession.id, recordId: spawn!.id }] : [];
    if (schedule) links.push({ label: "Open job", agentId: schedule.actor, kind: "schedule" });

    // A Spawn names two Sessions, and both Consoles show it: the created Session's as
    // where it came from, ahead of the start it admitted, and the spawner's as what it
    // started. Each row opens the other.
    const spawnFacts = (entry: SpawnAction) => [
      { label: "Spawn", value: "accepted", detail: `${preciseDateTime(entry.at)} ${ZONE} · created ${entry.session} for ${label(entry.target, entry.targetWorkspace)}`, provenance: "Control Plane fact" },
      requesterFact(entry.spawnerSessionId, entry.spawner),
    ];
    if (spawn) {
      rows.push(row("spawned", "spawn", { id: spawn.id, at: spawn.at, title: `Spawned by ${label(spawn.spawner, spawn.spawnerWorkspace)}`, summary: "", facts: spawnFacts(spawn), links: parentSession ? [{ label: `Open ${parentSession.id}`, sessionId: parentSession.id, recordId: spawn.id }] : [] }));
    }
    rows.push(row("session.started", "session-start", { id: `${session.id}:start`, at: session.start, title: session.startKind === "attach" ? "Session attached" : "Session started", summary, facts, links }));

    spawnsBy(session).forEach((entry) => {
      rows.push(row("spawned", "spawn", { id: entry.id, at: entry.at, title: `Spawned ${label(entry.target, entry.targetWorkspace)}`, summary: "", facts: spawnFacts(entry), links: [{ label: `Open ${entry.session}`, sessionId: entry.session, recordId: entry.id }] }));
    });

    MESSAGES.forEach((message) => {
      const from = label(message.from, message.fromWorkspace);
      // The envelope line names the other party: who it is for in the sender's Console,
      // who sent it in the recipient's. The body is the whole text when the page holds it.
      const envelope = (role: FeldConsoleEnvelope["role"], actorId: string, workspace: string | null, sessionId: string | null): FeldConsoleEnvelope =>
        ({ id: message.id, role, actor: actorId === "Admin" ? "Admin" : label(actorId, workspace), sessionId, body: message.body });
      if (message.toSessionId === session.id) {
        // Accepted for this Actor while this Session was current: the record names the
        // Session. Shown as waiting while no read is recorded.
        const waiting = message.read === null;
        const delivery = waiting
          ? `no read recorded${session.end === null ? " (current)" : " before this Session ended"}`
          : `read by ${message.read!.sessionId} ${millisecondsLabel(message.read!.at, message.accepted)}`;
        rows.push(row("message.accepted", waiting ? "message-waiting" : "message-accepted", { id: `${message.id}:accepted`, at: message.accepted, title: waiting ? "Message accepted · waiting" : "Message accepted", summary: delivery, body: message.preview, message: envelope("From", message.from, message.fromWorkspace, message.fromSessionId), facts: messageFactRows(message, state), links: message.read && message.read.sessionId !== session.id ? [{ label: `Open ${message.read.sessionId}`, sessionId: message.read.sessionId, recordId: message.id }] : [] }));
      }
      if (message.read?.sessionId === session.id) {
        const sender = senderSessionFor(message);
        rows.push(row("message.read", "message-read", { id: message.id, at: message.read.at, anchor: message.id, title: "Message read", summary: `accepted ${preciseTime(message.accepted)} · read by this Session ${millisecondsLabel(message.read.at, message.accepted)} · ${message.read.readId}`, body: message.preview, message: envelope("From", message.from, message.fromWorkspace, message.fromSessionId), facts: messageFactRows(message, state), links: sender ? [{ label: `Open ${from}'s Console`, sessionId: sender.id, recordId: message.id }] : [] }));
      }
      if (message.fromSessionId === session.id) {
        const reader = message.read ? sessionById(message.read.sessionId) : null;
        const recipientSession = message.read ? null : recipientSessionFor(message);
        rows.push(row("message.sent", "message-sent", { id: message.id, at: message.accepted, anchor: message.anchor, title: "Message sent", summary: deliverySummary(message).replace(/^Read/, "read").replace(/^Waiting/, "waiting"), body: message.preview, message: envelope("To", message.to, message.toWorkspace, message.toSessionId), facts: messageFactRows(message, state), links: reader
          ? [{ label: `Open ${reader.id}`, sessionId: reader.id, recordId: message.id }]
          : recipientSession ? [{ label: `Open ${recipientSession.id}`, sessionId: recipientSession.id, recordId: `${message.id}:accepted` }] : [] }));
      }
    });

    endRequestsFor(session).forEach((request) => {
      rows.push(row(`${request.kind}.requested`, request.kind, { id: `${session.id}:${request.kind}`, at: request.accepted, title: request.kind === "stop" ? "Stop requested" : "Kill requested", summary: `accepted ${preciseTime(request.accepted)} · ${request.attempt}`, facts: [
        { label: "Request", value: `feld session ${request.kind} · accepted`, detail: `${preciseDateTime(request.accepted)} ${ZONE} · ${request.kind === "stop" ? "graceful: the execution is asked to end" : "force: the execution is ended without asking"}`, provenance: "Control Plane fact" },
        requesterFact(request.requesterSessionId, session.actor),
        { label: "Provider attempt", value: request.attempt, detail: `Observed afterwards: ${request.observed}`, provenance: "Provider observation" },
      ] }));
    });

    const failure = startFailure(session);
    observationRecordsFor(session).forEach((observation, index) => {
      // The first observation of a start that never settled is the launch failure itself,
      // so the row names it rather than the generic kind the Provider wrote.
      const launchFailure = failure !== null && index === 0;
      rows.push(row("observation", "observation", { id: `${session.id}:observation:${index}`, at: observation.at, title: launchFailure ? "Start failed" : `Observed ${observation.kind}`, summary: launchFailure ? `${failure} · no running observation was ever recorded` : OBSERVATION_MEANING[observation.kind], facts: [
        ...(observation.detail ? [{ label: "Provider detail", value: observation.detail, detail: "What the Provider reported alongside the kind, in its own words", provenance: "Provider observation" }] : []),
        { label: "Provider observation", value: observation.kind, detail: `${preciseDateTime(observation.at)} ${ZONE} · recorded because the observed kind changed; the latest observation instant is kept on the Session, not here`, provenance: "Provider observation" },
      ] }));
    });

    SCHEDULE_SKIPS.filter((skip) => skip.previousSession === session.id).forEach((skip) => {
      const skipSchedule = scheduleFor(skip.actor);
      rows.push(row("schedule.skipped", "schedule-skipped", { id: skip.id, at: skip.planned, title: "Schedule tick skipped", summary: `planned ${preciseTime(skip.planned)} · this Session was still current`, facts: [
        { label: "Planned tick", value: `${preciseDateTime(skip.planned)} ${ZONE}`, detail: `${skipSchedule ? `${skipSchedule.cron} ${skipSchedule.timezone} · ` : ""}no spawn was submitted because ${session.id} was current`, provenance: "Control Plane fact · schedule.skipped record" },
      ], links: [{ label: "Open job", agentId: skip.actor, kind: "schedule" }] }));
    });

    if (session.end !== null) {
      // The end row states the two facts (how it ended, exit code) and, separately, what the
      // ownership and the Actor's kind conclude from them. The conclusion is derived, not stored. Its
      // `kind` (end-<outcome>) colours the Console row the same way the Timeline colours the
      // Session's end marker, so the two read as one fact.
      const outcome = outcomeFor(session);
      const requests = endRequestsFor(session);
      const lastRequest = requests[requests.length - 1] || null;
      const codeValue = session.exitCode === null ? "Not reported" : String(session.exitCode);
      const summary = failure
        ? `${failure} · the Session never began`
        : session.endKind === "unknown"
          ? `closed as unknown ${lastRequest ? `by feld session ${lastRequest.kind}` : "when the next start request was admitted"} · ${lostLabel(session)}`
          : lastRequest
            ? `observed gone ${millisecondsLabel(end, lastRequest.accepted)} after the ${lastRequest.kind} was accepted`
            : `${session.endKind} with ${codeValue === "Not reported" ? "no exit code" : `exit ${codeValue}`} · ${outcome.detail.split(" · ").slice(1).join(" · ") || outcome.detail}`;
      rows.push(row("session.ended", `end-${outcome.key}`, { id: `${session.id}:end`, at: end, title: outcome.key === "unknown" ? outcome.label : `Session ${outcome.label.toLowerCase()}`, summary, facts: [
        ...(failure ? [{ label: "Why", value: failure, detail: "The Provider's account of the launch it could not complete; no execution was ever observed running", provenance: "Provider observation" }] : []),
        { label: "Lifecycle state", value: session.endKind!, detail: `${preciseDateTime(end)} ${ZONE}${session.endKind === "unknown" ? " · the instant Feld stopped tracking the Session; the end itself was never proven" : lastRequest ? ` · ${lastRequest.observed}` : session.endKind === "exited" ? " · no stop or kill accepted before the end" : ""}`, provenance: "Control Plane fact" },
        { label: "Exit code", value: codeValue, detail: codeValue === "Not reported" ? "The Provider reported no exit status; null is not read as non-zero" : "As reported by the Execution Provider for the exact incarnation", provenance: codeValue === "Not reported" ? "No record" : "Control Plane fact" },
        { label: "Concluded as", value: outcome.label, detail: outcomeBasis(session).full, provenance: "Derived from Control Plane facts, ownership, and the kind declared at creation; not stored" },
      ] }));
    }

    return rows.sort((left, right) => left.at - right.at);
  }

  // Samples folded into bins of `binDuration` inside the range; `combine` says how two
  // samples of one bin meet (a sum for tokens, the latest value for a level). A sum counts
  // in the bin its sample starts in; a level holds over every bin its sample spans.
  function mergedBins(session: AgentSession, range: FeldMetricRange, binDuration: number, samples: BinSample<number>[], combine: (previous: number, value: number) => number, level = false) {
    const bins = new Map<number, number>();
    samples.forEach((sample) => {
      if (sample.end <= range.start || sample.start >= range.end) return;
      const first = Math.floor(Math.max(sample.start, level ? range.start : -Infinity) / binDuration) * binDuration;
      const last = level ? Math.ceil(Math.min(sample.end, range.end) / binDuration) * binDuration - binDuration : first;
      for (let binStart = first; binStart <= last; binStart += binDuration) {
        bins.set(binStart, bins.has(binStart) ? combine(bins.get(binStart)!, sample.value) : sample.value);
      }
    });
    return [...bins.entries()].map(([start, value]) => ({
      start: Math.max(start, session.start, range.start),
      end: Math.min(start + binDuration, sessionEnd(session), range.end),
      value,
    })).filter((bin) => bin.end > bin.start);
  }

  function sessionMetricBins(session: AgentSession, range: FeldMetricRange, binDuration: number, metric: string) {
    if (metric === "context") {
      const context = session.metrics.context;
      if (context.status !== "reported") return [];
      // A reading holds until the next one, the last until the Session's end.
      const samples = contextBinSamples(session);
      const held = samples.map((sample, index) => ({ ...sample, end: samples[index + 1]?.start ?? sessionEnd(session) }));
      return mergedBins(session, range, binDuration, held, (_previous, latest) => latest, true)
        .map((bin) => ({ ...bin, title: `${Math.round(bin.value)}% of the ${context.size.toLocaleString("en-US")}-token context window` }));
    }
    const option = SESSION_TOKEN_METRICS[metric];
    if (!option) return [];
    return mergedBins(session, range, binDuration, tokenBinSamples(session, option.kinds), (sum, value) => sum + value)
      .map((bin) => ({ ...bin, title: `${Math.round(bin.value).toLocaleString("en-US")} ${option.label}` }));
  }

  // ---- Drawing inputs -----------------------------------------------------------------

  // The records the Timeline draws over the bands of `agentIds`: the Sessions that overlap
  // the range (Sub Agents only while they are shown), the skipped ticks of a Job that still
  // has its Schedule, and the Spawns and Messages accepted in the range. A skipped tick is
  // a recorded schedule.skipped event, never a derivation, so ticks that passed while the
  // daemon was down are not drawn. A Spawn or Message is drawn when either end has a band.
  function timelineRecordsFor(state: AgentsState, agentIds: Set<string>, range: FeldMetricRange) {
    return {
      sessions: SESSIONS.filter((session) => agentIds.has(session.actor)
        && overlaps(session, range)
        && (state.showSubAgents || !session.parentSession)),
      skips: state.showScheduleSkips ? SCHEDULE_SKIPS.filter((skip) => skip.planned >= range.start && skip.planned < range.end && agentIds.has(skip.actor) && scheduleFor(skip.actor)) : [],
      spawns: state.showSpawnLinks ? ACTIONS.filter((action) => action.at >= range.start && action.at < range.end && (agentIds.has(action.spawner) || agentIds.has(action.target))) : [],
      messages: state.showMessageLinks ? MESSAGES.filter((message) => message.accepted >= range.start && message.accepted < range.end && (agentIds.has(message.from) || agentIds.has(message.to))) : [],
    };
  }

  // A mark recedes when the selected Actor or inspected Session does not touch it.
  function muteClassFor(focus: Focus | null) {
    return (related: boolean | undefined) => (focus && !related ? " is-muted" : "");
  }

  return {
    input,
    now,
    TIME_ZONE,
    ZONE,
    TIMEZONE_OPTIONS,
    dateTime,
    timeOnly,
    timeInput,
    shortDate,
    kindOf,
    scheduleFor,
    nextRunFor,
    scheduleRunsFor,
    skippedWhileCurrentFor,
    rangeFor,
    anchorUpToNow,
    sessionEnd,
    timelineStart,
    lostSince,
    lostLabel,
    sessionById,
    messageById,
    actorFor,
    actorLabel,
    currentSessionsFor,
    outcomeFor,
    outcomeBasis,
    spawnerSessionFor,
    senderSessionFor,
    recipientSessionFor,
    overlaps,
    currentSession,
    formatRange,
    rangeButtonLabel,
    toDateInput,
    instantOfLocal,
    latestSessionFor,
    runningSessionOf,
    lifecycleEvidence,
    observationEvidence,
    nearTime,
    scheduleSummary,
    scheduleCellFor,
    skipMarkFor,
    unreadMarkFor,
    unlinkedMarkFor,
    actorDetailCellFor,
    actorStateFor,
    workspaceAgents,
    matchingAgents,
    hasSessionInRange,
    attentionReasons,
    waitingMessagesFor,
    unreadWhileCurrentFor,
    lastActivityFor,
    bandRank,
    keptBySchedule,
    visibleAgents,
    bandAgents,
    hiddenBandsFor,
    emptyBandsReason,
    agentCountTitle,
    sessionsFor,
    allSessionsFor,
    subAgentSessionsFor,
    bandRowsFor,
    laneHeightFor,
    laneLayoutFor,
    metricsRangeFor,
    hasMetricsContext,
    metricsButtonLabel,
    visibleMetrics,
    metricLanesFor,
    sampleTimeText,
    metricReadingFor,
    resetTitle,
    relativeLabel,
    scheduleRunRowFor,
    scheduleFactsFor,
    unscheduledJobLatest,
    validateWorkspaceKey,
    nextStartKindFor,
    newActorResult,
    agentMenuFor,
    launchSpecDriftFor,
    composerFor,
    consoleOptionsFor,
    consoleSessionFor,
    consoleSessions: CONSOLE_SESSIONS,
    deliverySummary,
    runtimeRecordFor,
    runtimeRecordFact,
    messageFactRows,
    sessionDetailsFor,
    recordConsoleTarget,
    focusFor,
    relatedActivityFor,
    recordingOriginFor,
    lastRecordFor,
    turnFor,
    turnRecordFor,
    currentStatusSummary,
    processMeaning,
    turnMeaning,
    lastRecordRow,
    nowInstant,
    lastReadFor,
    nowSectionFor,
    preciseTime,
    preciseDateTime,
    observationRecordsFor,
    feldRecordsFor,
    mergedBins,
    sessionMetricBins,
    timelineRecordsFor,
    muteClassFor,
  };
}

// The formatters of one timezone and hour cycle, made once: a model is built again every
// few seconds.
const formatterCache = new Map<string, ReturnType<typeof makeFormatters>>();

const zoneClocks = new Map<string, Intl.DateTimeFormat>();

function zoneClockFor(timeZone: string) {
  if (!zoneClocks.has(timeZone)) {
    zoneClocks.set(timeZone, new Intl.DateTimeFormat("en-US", { timeZone, hourCycle: "h23", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", second: "numeric" }));
  }
  return zoneClocks.get(timeZone)!;
}

function formattersFor(timeZone: string, hourCycle: HourCycle) {
  const key = `${timeZone} ${hourCycle}`;
  if (!formatterCache.has(key)) formatterCache.set(key, makeFormatters(timeZone, hourCycle));
  return formatterCache.get(key)!;
}

// The displayed instants follow the hour cycle; the values of inputs and the zone math
// stay 24-hour.
function makeFormatters(timeZone: string, hourCycle: HourCycle) {
  return {
    dateTime: new Intl.DateTimeFormat("en-US", { timeZone, month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hourCycle }),
    timeOnly: new Intl.DateTimeFormat("en-US", { timeZone, hour: "2-digit", minute: "2-digit", hourCycle }),
    timeInput: new Intl.DateTimeFormat("en-US", { timeZone, hour: "2-digit", minute: "2-digit", hourCycle: "h23" }),
    shortDate: new Intl.DateTimeFormat("en-US", { timeZone, month: "short", day: "numeric" }),
    timeWithSeconds: new Intl.DateTimeFormat("en-US", { timeZone, hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle }),
    dateInput: new Intl.DateTimeFormat("en-CA", { timeZone, year: "numeric", month: "2-digit", day: "2-digit" }),
    wallClock: new Intl.DateTimeFormat("en-US", { timeZone, hourCycle: "h23", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric" }),
  };
}
