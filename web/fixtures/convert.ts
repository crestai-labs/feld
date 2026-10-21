// The fixture in the shapes the page draws from. The page keys an Actor by
// `workspace/name`, keeps a Session's metrics as bins at their own instants, and draws a
// Metrics lane from its samples, so each of those is derived here from the fixture's records.

import type {
  ActorRecord, ActorsInput, AgentSession, BinSample, ContextMetric, LaunchOptions, MessageRecord, MetricLane, MetricSeries,
  ScheduleRecord, ScheduleSkip, SpawnAction, TokenMetric,
} from "../actors/types.ts";
import { withAccountTones } from "../actors/model.ts";
import { displayedInstant, timeOffset } from "../console/model.ts";
import { lastTurnRecord } from "../lib/runtime-records.ts";
import type { DaemonRead } from "../daemon/source.ts";
import { normalizeCode } from "../api/auth.ts";
import type {
  MockActorsData, MockClock, MockContextMetric, MockDaemonData, MockMetricLane, MockMetricSeries, MockPairing, MockRecording,
  MockScenario, MockSession, MockTokenMetric,
} from "./mock.ts";

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const TOKEN_KIND_COUNT = 4;
const ADMIN = "Admin";

// The Runtimes, Providers, and Nodes the fixture's New actor offers.
const LAUNCH_OPTIONS: LaunchOptions = {
  runtimes: [["claude-code", "Claude Code"], ["codex", "Codex"]],
  providers: [["tmux", "tmux · named socket feld"], ["docker", "Docker"], ["docker-sandbox", "Docker Sandbox"], ["herdr", "Herdr"]],
  nodes: [["local", "Local Node"], ["build", "Build Node"], ["batch", "Batch Node"], ["sandbox", "Sandbox Node"]],
};

export const DEFAULT_SCENARIO = "three-days-ago-1h";
export type FixtureDaemonState = "running" | "degraded" | "unreachable" | "reading" | "refusing";
// The Actors page's `?state=`: how its reads fare, or a daemon refusing the Actions menu.
export type FixtureActorsState = "connecting" | "reconnecting" | "error" | "refusing";
export const ACTORS_STATES: readonly FixtureActorsState[] = Object.freeze(["connecting", "reconnecting", "error", "refusing"]);
export const DAEMON_STATES: readonly FixtureDaemonState[] = Object.freeze(["running", "degraded", "unreachable", "reading", "refusing"]);
// The `?state=` of every page that puts the pairing screen in its place: at once without a
// browser token, or after a while when a later read answers 401.
export type FixtureAuthState = "unauthorized" | "expired";
export const AUTH_STATES: readonly FixtureAuthState[] = Object.freeze(["unauthorized", "expired"]);

export interface FixtureRange {
  mode: "historical";
  anchor: number;
  duration: number;
}

// The scenario's pins when it opens: false unpins every Job, so the rows hold only the Actors
// the scenario is about; true keeps the reader's pins.
export function pinsJobsOf(scenarios: Record<string, MockScenario>, scenario: string | null) {
  return ((scenario && scenarios[scenario]) || scenarios[DEFAULT_SCENARIO]).pinJobs !== false;
}

export interface ConsoleFixture {
  sessions: FeldConsoleSession[];
  // What each Session's Console shows, by Session id: its own recording, the one its
  // `recording` names, or the shared exercise recording; and a program Session's output.
  recordings: Map<string, FeldConsoleRecording>;
  logs: Map<string, FeldConsoleLog>;
}

export function actorKey(workspace: string, name: string) {
  return `${workspace}/${name}`;
}

// An unknown scenario opens the default one.
export function rangeOf(scenarios: Record<string, MockScenario>, scenario: string | null): FixtureRange {
  const chosen = (scenario && scenarios[scenario]) || scenarios[DEFAULT_SCENARIO];
  return { mode: "historical", anchor: chosen.anchor, duration: chosen.duration };
}

// ---- Session metrics ---------------------------------------------------------------------

// The fixture's samples spread evenly over the Session, interpolated to one sample per
// minute; a summed metric is rescaled so the minutes add up to the fixture's total.
export function minuteBins(start: number, end: number, values: readonly number[], summed: boolean): BinSample<number>[] {
  if (!values.length) return [];
  const duration = Math.max(MINUTE, end - start);
  const sampleCount = Math.max(1, Math.ceil(duration / MINUTE));
  const raw = Array.from({ length: sampleCount }, (_unused, index) => {
    const position = sampleCount === 1 ? 0 : index * (values.length - 1) / (sampleCount - 1);
    const left = Math.floor(position);
    const right = Math.min(values.length - 1, left + 1);
    const fraction = position - left;
    return values[left] * (1 - fraction) + values[right] * fraction;
  });
  const scale = summed ? values.reduce((total, value) => total + value, 0) / (raw.reduce((total, value) => total + value, 0) || 1) : 1;
  return raw.map((value, index) => ({
    start: start + index * duration / sampleCount,
    end: start + (index + 1) * duration / sampleCount,
    value: value * scale,
  }));
}

// Each token kind is interpolated and rescaled on its own, so every kind's total, and the
// Session facts read from them, equal the fixture's.
export function tokenMetricOf(metric: MockTokenMetric, start: number, end: number): TokenMetric {
  if (metric.status !== "reported") return { status: metric.status, source: metric.source };
  const kinds = Array.from({ length: TOKEN_KIND_COUNT }, (_unused, kind) => minuteBins(start, end, metric.samples.map((sample) => sample[kind]), true));
  return {
    status: "reported",
    source: metric.source,
    samples: kinds[0].map((bin, index) => ({ start: bin.start, end: bin.end, value: kinds.map((bins) => bins[index].value) })),
  };
}

// Context samples stay as the fixture spaced them: the Session facts read the latest value,
// the peak, and the drops between samples, which interpolation would change.
export function contextMetricOf(metric: MockContextMetric, start: number, end: number): ContextMetric {
  if (metric.status !== "reported") return { status: metric.status, source: metric.source };
  const count = metric.samples.length;
  const duration = Math.max(MINUTE, end - start);
  return {
    status: "reported",
    source: metric.source,
    size: metric.size,
    samples: metric.samples.map((value, index) => ({
      start: start + index * duration / count,
      end: start + (index + 1) * duration / count,
      value,
    })),
  };
}

// ---- Metrics lanes -------------------------------------------------------------------------

// Lanes whose reading holds until the next one, as the page's own lanes of the same metric.
const LEVEL_LANES = new Set(["context-utilization"]);

export interface LaneDomain {
  start: number;
  end: number;
  step: number;
}

function positiveModulo(value: number, divisor: number) {
  return ((value % divisor) + divisor) % divisor;
}

// The fixture's periodic generator of a lane series:
// none past `until` or inside a `gaps` span, where no Session reported.
export function generatedValueAt(lane: MockMetricLane, series: MockMetricSeries, instant: number): number | null {
  if (series.status || instant > (series.until ?? Infinity)) return null;
  if ((series.gaps || []).some(([start, end]) => instant >= start && instant < end)) return null;
  const period = series.period!;
  const phase = series.phase!;
  const variation = series.variation!;
  const progress = positiveModulo(instant - series.resetAnchor!, period) / period;
  if (lane.kind === "quota") {
    const wave = Math.sin(progress * Math.PI * 4 + phase) * variation;
    return Math.max(0, Math.min(lane.maximum, series.minimum! + progress * series.span! + wave));
  }
  const wave = Math.sin(progress * Math.PI * 2 + phase) * variation;
  const detail = Math.sin(progress * Math.PI * 6 + phase * .7) * variation * .35;
  return Math.max(0, Math.min(lane.maximum, series.minimum! + series.span! * .5 + wave + detail));
}

// The first reset of a quota series strictly after the instant.
function nextReset(series: MockMetricSeries, instant: number) {
  return series.resetAnchor! + (Math.floor((instant - series.resetAnchor!) / series.period!) + 1) * series.period!;
}

function resetsWithin(series: MockMetricSeries, domain: LaneDomain) {
  const resets: number[] = [];
  for (let reset = nextReset(series, domain.start - 1); reset < domain.end; reset += series.period!) resets.push(reset);
  return resets;
}

// The Session taken as the latest quota report of an account at an instant: of
// the Sessions of its Runtime and account whose status line reports arrive, the one running
// latest, the most recently started on a tie.
function quotaReporterAt(candidates: readonly AgentSession[], instant: number, now: number) {
  let best: AgentSession | null = null;
  let bestEnd = -Infinity;
  for (const session of candidates) {
    if (session.start > instant) continue;
    const end = Math.min(session.end ?? now, instant);
    if (end > bestEnd || (end === bestEnd && best && session.start > best.start)) {
      best = session;
      bestEnd = end;
    }
  }
  return best;
}

function quotaSegments(sessions: readonly AgentSession[], series: MockMetricSeries, instants: readonly number[], context: LaneContext): FeldQuotaSegment[] {
  const candidates = sessions.filter((session) => !session.parentSession && session.model?.[0] === series.provider && (!series.account || context.accounts.get(session.id) === series.account) && session.metrics.context.status === "reported");
  const segments: FeldQuotaSegment[] = [];
  for (const instant of instants) {
    const reporter = quotaReporterAt(candidates, instant, context.now);
    const last = segments.at(-1);
    if (last && (last.reporter?.session ?? null) === (reporter?.id ?? null)) continue;
    if (last) last.end = instant;
    segments.push({
      start: instant,
      end: null,
      reporter: reporter ? { session: reporter.id, workspace: reporter.workspace, actor: reporter.actor.slice(reporter.workspace.length + 1) } : null,
    });
  }
  return segments;
}

// What the lanes read of the Sessions besides their records: the fixture's clock and the
// account each Session runs under.
export interface LaneContext {
  now: number;
  accounts: ReadonlyMap<string, string>;
}

function seriesOf(lane: MockMetricLane, series: MockMetricSeries, domain: LaneDomain, sessions: readonly AgentSession[], context: LaneContext): MetricSeries {
  const sampled = sampledSeriesOf(lane, series, domain, sessions, context);
  return series.account ? { ...sampled, account: series.account } : sampled;
}

// Each step reads the generator at its last instant, or where the series stops, as a
// range is read just before its end: a quota report at that instant, a rate bin over
// the step.
function sampledSeriesOf(lane: MockMetricLane, series: MockMetricSeries, domain: LaneDomain, sessions: readonly AgentSession[], context: LaneContext): MetricSeries {
  if (series.status) return { provider: series.provider, tone: series.tone, source: series.source, status: series.status };
  const quota = lane.kind === "quota";
  const samples: FeldMetricSample[] = [];
  const first = Math.ceil(domain.start / domain.step) * domain.step;
  for (let start = first; start < domain.end; start += domain.step) {
    const at = Math.min(start + domain.step - 1, series.until ?? Infinity);
    const value = at < start ? null : generatedValueAt(lane, series, at);
    if (value === null) continue;
    samples.push(quota
      ? { at, end: at, until: nextReset(series, at), value }
      : { at: start, end: start + domain.step, value });
  }
  if (!quota) return { provider: series.provider, tone: series.tone, source: series.source, samples };
  return {
    provider: series.provider,
    tone: series.tone,
    source: series.source,
    samples,
    showResets: series.showResets,
    resets: resetsWithin(series, domain),
    segments: quotaSegments(sessions, series, samples.map((sample) => sample.at), context),
  };
}

function quotaWindow(lane: MockMetricLane): MetricLane["window"] {
  const period = lane.series.find((series) => series.period)?.period;
  return period === 5 * HOUR ? "five_hour" : period === 7 * DAY ? "seven_day" : undefined;
}

// The fixture's lanes as they are (one quota lane per Runtime and window), each series sampled
// from its generator once per `domain.step`.
export function metricLanesOf(lanes: readonly MockMetricLane[], domain: LaneDomain, sessions: readonly AgentSession[], context: LaneContext): MetricLane[] {
  return withAccountTones(lanes.map((lane) => {
    const quota = lane.kind === "quota";
    const spec: MetricLane = {
      id: lane.id,
      name: lane.name,
      caption: lane.caption,
      unit: lane.unit,
      maximum: lane.maximum,
      defaultVisible: lane.defaultVisible,
      derivation: lane.derivation,
      series: lane.series.map((series) => seriesOf(lane, series, domain, sessions, context)),
    };
    if (quota) Object.assign(spec, { kind: "quota", level: true, window: quotaWindow(lane), showResets: lane.series.some((series) => series.showResets) });
    else if (LEVEL_LANES.has(lane.id)) spec.level = true;
    if (lane.prefix !== undefined) spec.prefix = lane.prefix;
    if (lane.decimals !== undefined) spec.decimals = lane.decimals;
    return spec;
  }));
}

// ---- Actors --------------------------------------------------------------------------------

// With the Console's fixture, each current agent-runtime Session carries the turn the
// daemon's Session read adds, read from the recording its Console shows.
export function actorsInputOf(data: MockActorsData, clock: MockClock, domain: LaneDomain, console?: ConsoleFixture): ActorsInput {
  const workspaceOf = new Map(data.actors.map((actor) => [actor.id, actor.workspace]));
  // An id the fixture does not declare belongs to the default Workspace.
  const keyOf = (id: string, workspace?: string | null) => actorKey(workspace || workspaceOf.get(id) || data.defaultWorkspace, id);

  const actors: ActorRecord[] = data.actors.map((actor) => ({
    id: keyOf(actor.id, actor.workspace),
    name: actor.id,
    workspace: actor.workspace,
    kind: actor.kind,
    launchSpec: actor.launchSpec ? { ...actor.launchSpec } : null,
    grants: actor.grants || [],
  }));

  const sessions = data.sessions.map((session): AgentSession => {
    const converted = sessionOf(session, keyOf(session.actor), clock.now);
    return console ? { ...converted, ...turnReadOf(converted, console.recordings.get(session.id)) } : converted;
  });

  const schedules: ScheduleRecord[] = data.schedules.map((schedule) => ({
    actor: keyOf(schedule.actor),
    cron: schedule.cron,
    timezone: schedule.timezone,
    enabled: schedule.enabled,
    pausedAt: schedule.pausedAt,
    enabledAt: schedule.enabledAt,
    // The page derives the next run from the cron expression and its clock.
    nextRun: null,
  }));

  const scheduleSkips: ScheduleSkip[] = data.scheduleSkips.map((skip) => ({ ...skip, actor: keyOf(skip.actor) }));

  const actions: SpawnAction[] = data.actions.map((action) => ({
    ...action,
    spawner: keyOf(action.spawner, action.spawnerWorkspace),
    target: keyOf(action.target, action.targetWorkspace),
  }));

  const messages: MessageRecord[] = data.messages.map((message) => ({
    ...message,
    preview: messagePreview(message.body),
    from: message.from === ADMIN && !message.fromWorkspace ? ADMIN : keyOf(message.from, message.fromWorkspace),
    to: keyOf(message.to, message.toWorkspace),
    read: message.read ? { ...message.read } : null,
  }));

  return {
    workspaces: [...data.workspaces],
    defaultWorkspace: data.defaultWorkspace,
    actors,
    sessions,
    schedules,
    scheduleSkips,
    actions,
    messages,
    metrics: metricLanesOf(data.metricLanes, domain, sessions, { now: clock.now, accounts: new Map(data.sessions.filter((session) => session.account).map((session) => [session.id, session.account!] as const)) }),
    launchOptions: LAUNCH_OPTIONS,
    ...(console ? { consoleSessions: console.sessions } : {}),
  };
}

// The preview `GET /v1/messages` carries: the first 48 characters of the body, with `…`
// when there is more.
export function messagePreview(body: string) {
  const characters = Array.from(body);
  return characters.length > 48 ? `${characters.slice(0, 48).join("")}…` : body;
}

export function sessionOf(session: MockSession, actor: string, now: number): AgentSession {
  const end = session.end ?? now;
  const converted: AgentSession = {
    id: session.id,
    workspace: actor.slice(0, actor.indexOf("/")),
    actor,
    start: session.start,
    end: session.end,
    planned: session.planned ?? null,
    startKind: session.startKind,
    ownership: session.ownership,
    endKind: session.endKind,
    exitCode: session.exitCode,
    execution: { ...session.execution },
    model: session.model ? [session.model[0], session.model[1]] : null,
    linked: Boolean(session.model),
    placement: session.placement ? { ...session.placement } : null,
    lifecycle: { ...session.lifecycle },
    observation: session.observation ? { ...session.observation } : null,
    metrics: {
      tokens: tokenMetricOf(session.metrics.tokens, session.start, end),
      context: contextMetricOf(session.metrics.context, session.start, end),
    },
    laneMetrics: {},
  };
  if (session.parentSession) converted.parentSession = session.parentSession;
  if (session.subAgent) converted.subAgent = session.subAgent;
  if (session.recordingOrigin) converted.recordingOrigin = session.recordingOrigin;
  if (session.observations) converted.observations = session.observations.map((observation) => ({ ...observation }));
  if (session.attachCommand) converted.attachCommand = session.attachCommand;
  return converted;
}

// The turn and its entry's instant as the daemon's Session read carries them, for a current
// agent-runtime Session that is not a Sub Agent: read from the Session's own last entry
// that carries a turn, and shifted onto the fixture clock as the Console shows it (the
// instant `recordingOrigin` names, or the recording's start, coincides with the Session's
// start). Without a readable recording or such an entry the turn is unavailable.
export function turnReadOf(session: AgentSession, recording: FeldConsoleRecording | null | undefined): Pick<AgentSession, "turn" | "lastEntryAt"> {
  if (session.end !== null || session.parentSession || session.execution.type !== "agent-runtime") return {};
  const last = recording ? lastTurnRecord(recording.records) : null;
  if (!last) return { turn: "unavailable", lastEntryAt: null };
  const origin = session.recordingOrigin || recording!.startedAt;
  const offset = timeOffset(origin ? { recordingInstant: Date.parse(origin), sessionInstant: session.start } : null);
  return { turn: last.kind === "response" ? "waiting-input" : "responding", lastEntryAt: displayedInstant(last.at, offset) };
}

// ---- Console -------------------------------------------------------------------------------

export function recordingOf(recording: MockRecording, hasOlder = false): FeldConsoleRecording {
  return {
    id: recording.id,
    records: [...recording.records],
    hasOlder,
    startedAt: recording.startedAt,
  };
}

// Each Session's recording and log, resolved once per Session.
export function consoleFixtureOf(sessions: readonly FeldConsoleSession[], recordings: Record<string, MockRecording>, logs: Record<string, FeldConsoleLog>): ConsoleFixture {
  const fixture: ConsoleFixture = { sessions: sessions.map((session) => ({ ...session })), recordings: new Map(), logs: new Map() };
  for (const session of fixture.sessions) {
    if (session.console === "log") {
      const log = logs[session.id];
      if (log) fixture.logs.set(session.id, { source: log.source, lines: log.lines.map((line) => [...line]) as FeldConsoleLog["lines"] });
      continue;
    }
    if (session.sourceUnavailable || session.console) continue;
    const recording = recordings[session.recording || session.id] || recordings.shared_short;
    if (recording) fixture.recordings.set(session.id, recordingOf(recording, Boolean(session.hasOlder)));
  }
  return fixture;
}

// ---- Daemon --------------------------------------------------------------------------------

// What the Settings page draws of the daemon in each of the fixture's states. An unreachable
// daemon keeps the last status the page read. The Workspaces are the Actors fixture's.
export function daemonReadOf(data: MockDaemonData, actors: MockActorsData, clock: MockClock, state: FixtureDaemonState): DaemonRead {
  const base = { workspaces: [...actors.workspaces], defaultWorkspace: actors.defaultWorkspace, actionError: null, restartedAt: null, now: clock.now };
  if (state === "unreachable") return { ...base, phase: "unreachable", status: data.status, lastRead: data.lastRead };
  // Before the first read answers, nothing is read.
  if (state === "reading") return { ...base, phase: state, status: null, lastRead: null, workspaces: null };
  const status = state === "degraded" ? data.degradedStatus : data.status;
  return { ...base, phase: "reachable", status, lastRead: status.serverTime };
}

// POST /v1/ui/pair against the fixture's daemon: a code works once.
export function pairingOf(pairing: MockPairing): (code: string) => boolean {
  const codes = new Set(pairing.codes.map(normalizeCode));
  return (code) => codes.delete(normalizeCode(code));
}

export function daemonStateOf(value: string | null): FixtureDaemonState {
  return DAEMON_STATES.includes(value as FixtureDaemonState) ? value as FixtureDaemonState : "running";
}
