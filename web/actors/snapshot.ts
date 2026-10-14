// The range reads mapped into the records the Actors page derives everything from
// (ActorsInput): instants become epoch milliseconds, an Actor reference becomes its
// `workspace/id` key, and a Spawn becomes an Action record. Each function returns what it
// maps; nothing here holds state between two snapshots.

import { commandLine } from "../lib/command-line.ts";
import { quotaAccounts, quotaLane } from "../lib/metric-lanes.ts";
import {
  METRIC_LANES,
  MINUTE,
  NO_QUOTA_SOURCE,
  NO_SAMPLE_SOURCES,
  NO_SESSION_SOURCE,
  RUNTIME_TONES,
  TOKEN_KINDS,
  actorKeyOf,
  consoleSessionOf,
  instantOf,
  sessionsOf,
  withAccountTones,
} from "./model.ts";
import type {
  ActorLaunchSpec,
  ActorsInput,
  AgentSession,
  BinSample,
  ContextMetric,
  Instant,
  LaunchOptions,
  LifecycleRequest,
  MetricLane,
  MetricLaneSpec,
  MetricProjection,
  MetricSeries,
  ScheduleSkip,
  SessionLifecycle,
  SessionModel,
  SessionObservation,
  SessionPlacement,
  SpawnAction,
  TokenMetric,
  UnreportedStatus,
} from "./types.ts";

// The Workspace the page opens on when the URL names none. A daemon without it opens on
// its first Workspace instead.
export const DEFAULT_WORKSPACE_KEY = "default";

// An Actor reference as the page keys it (see actorKeysOf).
type ActorKeyOf = (reference: { workspace: string; name: string; id?: string }) => string;

function requesterKey(requester: FeldApi.Requester | null | undefined, keyOf: ActorKeyOf) {
  return requester && requester.type === "actor" ? keyOf(requester) : "Admin";
}

function requesterWorkspace(requester: FeldApi.Requester | null | undefined) {
  return requester && requester.type === "actor" ? requester.workspace : null;
}

function mapLaunchSpec(launchSpec: FeldApi.LaunchSpec | null): ActorLaunchSpec | null {
  if (!launchSpec) return null;
  return {
    runtime: launchSpec.runtime || undefined,
    runtimeSettings: launchSpec.runtimeSettings && Object.keys(launchSpec.runtimeSettings).length ? { ...launchSpec.runtimeSettings } : undefined,
    trustWorkdir: launchSpec.trustWorkdir || undefined,
    command: launchSpec.command || undefined,
    workdir: launchSpec.workdir || "",
    env: launchSpec.env && Object.keys(launchSpec.env).length ? { ...launchSpec.env } : undefined,
    provider: launchSpec.provider || "",
    node: launchSpec.node || "",
    providerOptions: launchSpec.providerOptions && Object.keys(launchSpec.providerOptions).length ? { ...launchSpec.providerOptions } : undefined,
  };
}

// The Binding as the Placement rows read it. What the resource is belongs to the
// Execution Provider, which writes its own reading of it (`summary`) and the command a
// person runs to reach it (`attachCommand`) into the Placement it hands back; this page
// shows those two and never reads the Provider's own items.
function mapPlacement(placement: FeldApi.Placement | null): SessionPlacement | null {
  if (!placement) return null;
  const own = placement.placement || {};
  return {
    provider: placement.provider,
    node: placement.node,
    workdir: placement.workdir,
    summary: typeof own.summary === "string" ? own.summary : "",
    attachCommand: typeof own.attachCommand === "string" ? own.attachCommand : null,
  };
}

function mapLifecycleAction(action: FeldApi.LifecycleAction | null | undefined): LifecycleRequest | undefined {
  if (!action) return undefined;
  return {
    accepted: instantOf(action.accepted)!,
    requesterSessionId: action.requesterSessionId || null,
    attempt: action.attempt || "No attempt recorded",
    observed: action.observed?.detail || action.observed?.kind || "no observation recorded",
  };
}

function mapLifecycle(lifecycle: FeldApi.Session["lifecycle"] | undefined): SessionLifecycle {
  if (!lifecycle) return {};
  return {
    startSettled: instantOf(lifecycle.startSettled) || undefined,
    startFailed: instantOf(lifecycle.startFailed) || undefined,
    stop: mapLifecycleAction(lifecycle.stop),
    kill: mapLifecycleAction(lifecycle.kill),
  };
}

function mapObservation(observation: FeldApi.Observation | null): SessionObservation | null {
  if (!observation) return null;
  return { kind: observation.kind, at: instantOf(observation.at)!, since: observation.since ? instantOf(observation.since) : null, detail: observation.detail };
}

const NO_METRIC_SOURCE = "No metric projection was read for this Session";
const READING_METRIC_SOURCE = "The Session's metrics read has not answered yet";
const READING_METRIC: TokenMetric & ContextMetric = Object.freeze({ status: "reading", source: READING_METRIC_SOURCE });

// A field of a value the yaml leaves open: a metric bin's value, which is the metric's own
// shape (a number, `{ value }`, or the token kinds and context fields by name), or a Feld
// record's `data`, whose shape depends on the record's kind.
function feldOf(value: unknown, key: string): unknown {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>)[key] : undefined;
}

function stringOf(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function requesterOf(value: unknown): FeldApi.Requester | undefined {
  const type = feldOf(value, "type");
  return type === "admin" || type === "actor" ? value as FeldApi.Requester : undefined;
}

// A bin's own interval, so the histogram draws it where it was measured. A read without
// an end closes the bin at the bucket the read used.
function binSpan(bin: FeldApi.MetricBin, bucket: number) {
  const start = instantOf(bin.start);
  return { start, end: instantOf(bin.end) ?? (start === null ? null : start + bucket) };
}

function mapTokenMetric(projection: MetricProjection | undefined, bucket: number): TokenMetric {
  if (!projection) return { status: "unavailable", source: NO_METRIC_SOURCE };
  const bins = projection.series || [];
  if (projection.status !== "reported" || !bins.length) return { status: projection.status as UnreportedStatus, source: projection.source };
  return {
    status: "reported",
    source: projection.source,
    samples: bins.map((bin) => ({
      ...binSpan(bin, bucket),
      value: TOKEN_KINDS.map((kind) => Number(feldOf(bin.value, kind) || 0)),
    })).filter((sample): sample is BinSample<number[]> => sample.start !== null),
  };
}

function usedPercentOf(value: unknown) {
  const used = feldOf(value, "used_percent");
  return typeof used === "number" && Number.isFinite(used) ? used : null;
}

// A bin without a used percentage is no reading of the context window: it is left out,
// not read as 0, and a Session with no reading at all has no usage record.
function mapContextMetric(projection: MetricProjection | undefined, bucket: number): ContextMetric {
  if (!projection) return { status: "unavailable", source: NO_METRIC_SOURCE };
  if (projection.status !== "reported") return { status: projection.status as UnreportedStatus, source: projection.source };
  const bins = (projection.series || []).filter((bin) => usedPercentOf(bin.value) !== null);
  if (!bins.length) return { status: "unavailable", source: NO_SAMPLE_SOURCES[0] };
  return {
    status: "reported",
    source: projection.source,
    size: Number(feldOf(bins[bins.length - 1].value, "size") || 0),
    samples: bins.map((bin) => ({
      ...binSpan(bin, bucket),
      value: usedPercentOf(bin.value)!,
    })).filter((sample): sample is BinSample<number> => sample.start !== null),
  };
}

export function mapSession(session: FeldApi.Session | FeldApi.SubAgentSession, projection: FeldApi.SessionMetrics | undefined, actor = actorKeyOf(session.workspace, session.actor)): AgentSession {
  const laneMetrics: AgentSession["laneMetrics"] = {};
  for (const metric of projection?.metrics || []) laneMetrics[metric.metric] = metric;
  const bucket = Number(projection?.bucket || 0) * 1000 || MINUTE;
  const execution = session.execution || ({} as Partial<FeldApi.Execution>);
  const runtime = execution.type === "agent-runtime" ? execution.label || "Runtime" : null;
  return {
    id: session.id,
    workspace: session.workspace,
    actor,
    start: instantOf(session.start)!,
    end: instantOf(session.end),
    planned: instantOf((session as FeldApi.Session).planned),
    startKind: session.startKind,
    ownership: session.ownership || null,
    // A Sub Agent has no lifecycle of Feld's own, so its endKind is null.
    endKind: session.endKind ?? null,
    exitCode: session.exitCode === undefined ? null : session.exitCode,
    execution: {
      type: execution.type || "program",
      runtime: execution.runtime || null,
      label: execution.label || "Execution",
      command: execution.command ? commandLine(execution.command) : null,
      ai: Boolean(execution.ai),
    },
    // An agent-runtime Session whose Runtime Adapter has delivered no record yet has the
    // Runtime name but no model; a program Session has neither.
    model: (session.model as SessionModel | undefined) || (runtime ? [runtime, null] : null),
    linked: Boolean(session.model),
    placement: mapPlacement(session.placement as FeldApi.Placement | null),
    lifecycle: mapLifecycle((session as FeldApi.Session).lifecycle),
    observation: mapObservation(session.observation as FeldApi.Observation | null),
    turn: (session as FeldApi.Session).turn,
    lastEntryAt: instantOf((session as FeldApi.Session).lastEntryAt),
    metrics: projection
      ? {
        tokens: mapTokenMetric(laneMetrics.token_usage, bucket),
        context: mapContextMetric(laneMetrics.context_window_utilization, bucket),
      }
      : { tokens: READING_METRIC, context: READING_METRIC },
    laneMetrics,
    parentSession: (session as FeldApi.SubAgentSession).parentSession || undefined,
    subAgent: (session as FeldApi.SubAgentSession).subAgent || undefined,
    consoleState: (session as FeldApi.SubAgentSession).consoleState,
    consoleDetail: (session as FeldApi.SubAgentSession).consoleDetail,
  };
}

// ---- Metrics lanes from the reads ---------------------------------------------------

type BinValue = FeldApi.MetricBin["value"];

function sumOf(values: BinValue[], read: (value: BinValue) => unknown) {
  return values.reduce<number>((total, value) => total + (Number(read(value)) || 0), 0);
}

function numberOf(value: BinValue) {
  return typeof value === "object" && value !== null ? Number(feldOf(value, "value") || 0) : Number(value || 0);
}

// How the Sessions of one bin meet, in the lane's own unit; null where the bin has no
// reading of the lane's metric.
function laneBinValue(lane: MetricLaneSpec, values: BinValue[], duration: number): number | null {
  const minutes = Math.max(duration, MINUTE) / MINUTE;
  switch (lane.id) {
    case "token-usage":
      return sumOf(values, (value) => feldOf(value, "total")) / minutes;
    case "estimated-cost":
    case "api-errors":
      return sumOf(values, numberOf) / (minutes / 60);
    case "cache-hit-rate": {
      const cacheRead = sumOf(values, (value) => feldOf(value, "cache_read"));
      const denominator = cacheRead + sumOf(values, (value) => feldOf(value, "input")) + sumOf(values, (value) => feldOf(value, "cache_creation"));
      return denominator ? cacheRead / denominator * 100 : null;
    }
    case "request-latency":
      return Math.max(...values.map(numberOf));
    case "context-utilization": {
      const used = values.map(usedPercentOf).filter((value) => value !== null);
      return used.length ? Math.max(...used) : null;
    }
    case "tool-calls":
    case "active-time":
      return sumOf(values, numberOf) / minutes;
    default:
      return 0;
  }
}

const READING_RUNTIME_SOURCE = "The Runtime metrics read has not answered yet";

// One Runtime's series of a lane from its Runtime metrics read, one per account: the read
// aggregated the metric over each account's `reported` Sessions bin by bin, so a bin's
// value has the shape of one Session's and is read as a bin of one Session. The Sessions
// of the unknown account make a series named by the Runtime.
function laneSeriesFor(lane: MetricLaneSpec, installed: FeldApi.Plugin, read: FeldLiveRuntimeMetrics | undefined): MetricSeries[] {
  const provider = installed.label;
  const tone = toneOf(installed);
  if (!read) return [{ provider, tone, source: READING_RUNTIME_SOURCE, reading: true, samples: [] }];
  const projection = read.metrics?.find((metric) => metric.metric === lane.metric);
  if (!projection) return [{ provider, tone, source: NO_SESSION_SOURCE, samples: [] }];
  if (projection.status !== "reported" && !NO_SAMPLE_SOURCES.includes(projection.source)) {
    return [{ provider, tone, status: projection.status === "n/a" ? "n/a" : "unavailable", source: projection.source }];
  }
  const accounts = projection.accounts || [];
  if (!accounts.length) return [{ provider, tone, source: projection.source, samples: [] }];
  return accounts.map((account) => ({
    provider,
    tone,
    ...(account.account ? { account: account.account.name } : {}),
    source: projection.source,
    samples: laneSamples(lane, account.series),
  }));
}

function laneSamples(lane: MetricLaneSpec, series: readonly FeldApi.MetricBin[]) {
  return series
    .map((bin) => ({ at: instantOf(bin.start), end: instantOf(bin.end), value: (bin.value || {}) as BinValue }))
    .filter((bin): bin is { at: Instant; end: Instant | null; value: BinValue } => bin.at !== null)
    .sort((left, right) => left.at - right.at)
    .flatMap((bin) => {
      const end = bin.end || bin.at + MINUTE;
      const value = laneBinValue(lane, [bin.value], end - bin.at);
      return value === null ? [] : [{ at: bin.at, end, value }];
    });
}

function toneOf(runtime: FeldApi.Plugin) {
  return RUNTIME_TONES[runtime.name] || runtime.name;
}

// The quota lanes read the account's rate limit, which belongs to the account and not to
// any Session: one series per account the Runtime's reports name (and one for the reports
// without an account), with the window's reported resets_at as the anchor of its reset
// markers.
function quotaSeriesFor(lane: MetricLaneSpec, read: FeldLiveRuntimeMetrics | undefined, installed: FeldApi.Plugin): MetricSeries[] {
  const provider = installed.label;
  const tone = toneOf(installed);
  if (!read) return [{ provider, tone, source: "The quota read has not answered yet", reading: true, samples: [] }];
  const quota = read.quota;
  const accounts = quotaAccounts(quota, lane.window!);
  if (!accounts.length && (!quota || quota.status === "reported" || NO_SAMPLE_SOURCES.includes(quota.source as string))) {
    return [{ provider, tone, source: quota?.source || NO_QUOTA_SOURCE, samples: [] }];
  }
  if (!accounts.length) {
    return [{ provider, tone, status: quota?.status === "n/a" ? "n/a" : "unavailable", source: quota?.source || NO_QUOTA_SOURCE }];
  }
  return accounts.map((account) => {
    const series = quotaLane(quota, lane.window!, account?.id ?? null)!;
    return {
      provider,
      tone,
      ...(account ? { account: account.name } : {}),
      source: quota!.source!,
      showResets: lane.showResets,
      resets: series.resets,
      segments: series.segments,
      samples: series.samples,
    };
  });
}

// A lane's chart range holds the readings it has to draw; a quota is always a percentage.
function laneMaximum(lane: MetricLaneSpec, series: MetricSeries[]) {
  if (lane.kind === "quota" || lane.unit === "%") return lane.maximum;
  const peak = Math.max(0, ...series.flatMap((item) => (item.samples || []).map((sample) => sample.value)));
  if (peak <= lane.maximum) return lane.maximum;
  const magnitude = 10 ** Math.max(0, Math.floor(Math.log10(peak)) - 1);
  return Math.ceil(peak * 1.15 / magnitude) * magnitude;
}

// Whether a Runtime's read reports a quota window.
function reportsWindow(read: FeldLiveRuntimeMetrics | undefined, window: FeldApi.QuotaSeries["window"]) {
  return quotaAccounts(read?.quota, window).length > 0;
}

// A quota lane of one Runtime and window, named after both. It shows by default once the
// Runtime reports its window.
function quotaLaneOf(lane: MetricLaneSpec, runtime: FeldApi.Plugin, read: FeldLiveRuntimeMetrics | undefined): MetricLane {
  const series = quotaSeriesFor(lane, read, runtime);
  return {
    ...lane,
    id: `${lane.id}-${runtime.name}`,
    name: `${runtime.label} ${lane.name}`,
    caption: `${runtime.label} · ${lane.caption}`,
    defaultVisible: reportsWindow(read, lane.window!),
    series,
    maximum: laneMaximum(lane, series),
  };
}

// One lane per metric with one series per installed Runtime, labelled with its display
// name as the lane's caption is, and one quota lane per Runtime and the window it has.
export function buildMetrics(reads: FeldLiveData["runtimeMetrics"] | null | undefined, installed: readonly FeldApi.Plugin[]): MetricLane[] {
  const readOf = (runtime: FeldApi.Plugin) => reads?.find((item) => item.runtime === runtime.name);
  const runtimesOf = (lane: MetricLaneSpec) => installed.filter((runtime) => !lane.runtimes || lane.runtimes.includes(runtime.name));
  const quotas = METRIC_LANES.filter((lane) => lane.kind === "quota");
  return withAccountTones([
    ...installed.flatMap((runtime) => quotas.filter((lane) => runtimesOf(lane).includes(runtime)).map((lane) => quotaLaneOf(lane, runtime, readOf(runtime)))),
    ...METRIC_LANES.filter((lane) => lane.kind !== "quota").map((lane) => {
      const runtimes = runtimesOf(lane);
      const series = runtimes.flatMap((runtime) => laneSeriesFor(lane, runtime, readOf(runtime)));
      const names = runtimes.map((runtime) => runtime.label).join(" · ");
      const caption = [names && (lane.runtimes ? `${names} only` : names), lane.caption].filter(Boolean).join(" · ");
      return { ...lane, caption, series, maximum: laneMaximum(lane, series) };
    }),
  ]);
}

// What New actor offers: the daemon's installed Runtimes and Providers and its Nodes.
function launchOptionsOf(data: Partial<FeldLiveData>): LaunchOptions {
  const pairs = (plugins: readonly FeldApi.Plugin[] | undefined) => (plugins || []).map(({ name, label }) => [name, label] as const);
  return {
    runtimes: pairs(data.runtimes),
    providers: pairs(data.providers),
    nodes: (data.nodes || []).map((node) => [node, node] as const),
  };
}

// One Spawn as the timeline reads it: who asked, from which Session, and what it created.
function spawnAction(session: string, spawner: FeldApi.Requester | null | undefined, spawnerSessionId: string | null | undefined, target: { workspace: string; name: string; id?: string }, at: Instant, keyOf: ActorKeyOf): SpawnAction {
  return {
    id: `action_${session}`,
    spawner: requesterKey(spawner, keyOf),
    spawnerWorkspace: requesterWorkspace(spawner),
    spawnerSessionId: spawnerSessionId || null,
    target: keyOf(target),
    targetWorkspace: target.workspace,
    session,
    at,
  };
}

// The page keys an Actor by `workspace/name`, the name its pins and order are kept under.
// A name belongs to its live Actor, or, with none, to the Actor listed as deleted last;
// any other Actor that held the name is keyed apart by its `id` (actorKeyOf), so each
// Actor keeps its own Sessions, Messages, and Spawns.
function actorKeysOf(actors: FeldApi.Actor[]): ActorKeyOf {
  const rank = (actor: FeldApi.Actor) => actor.deletedAt ? Date.parse(actor.deletedAt) : Infinity;
  const holders = new Map<string, FeldApi.Actor>();
  for (const actor of actors) {
    const key = actorKeyOf(actor.workspace, actor.name);
    const held = holders.get(key);
    if (!held || rank(actor) > rank(held)) holders.set(key, actor);
  }
  return (reference) => {
    const key = actorKeyOf(reference.workspace, reference.name);
    const holder = holders.get(key);
    return !reference.id || !holder || holder.id === reference.id ? key : actorKeyOf(reference.workspace, reference.name, reference.id);
  };
}

// What the range reads answered, as the records the page derives from. With nothing read
// yet the page holds no Actor, and every lane reads as not answered.
export function toActorsInput(snapshot: FeldLiveData | null | undefined): ActorsInput {
  const data = snapshot || ({} as Partial<FeldLiveData>);
  const workspaces = snapshot?.workspaces || [];
  const defaultWorkspace = workspaces.length && !workspaces.includes(DEFAULT_WORKSPACE_KEY) ? workspaces[0] : DEFAULT_WORKSPACE_KEY;
  const actorList = data.actors || [];
  const keyOf = actorKeysOf(actorList);
  const actors = actorList.map((actor) => ({
    id: keyOf(actor),
    name: actor.name,
    workspace: actor.workspace,
    kind: actor.kind,
    launchSpec: mapLaunchSpec(actor.launchSpec),
    grants: actor.grants || [],
    actorId: actor.id,
    deletedAt: instantOf(actor.deletedAt),
    currentSession: actor.currentSession ? { id: actor.currentSession.id, ownership: actor.currentSession.ownership || null } : null,
  }));
  const projections = new Map((data.metrics || []).map((entry) => [entry.session, entry]));
  // A Sub Agent Session names its Actor only by name, so it follows its parent's Actor.
  const actorIdOf = new Map((data.sessions || []).flatMap((session) => "actorId" in session ? [[session.id, session.actorId] as const] : []));
  const sessionKey = (session: FeldApi.Session | FeldApi.SubAgentSession) =>
    keyOf({ workspace: session.workspace, name: session.actor, id: "actorId" in session ? session.actorId : actorIdOf.get((session as FeldApi.SubAgentSession).parentSession) });
  const sessions = (data.sessions || []).map((session) => mapSession(session, projections.get(session.id), sessionKey(session)));
  const schedules = actorList.filter((actor) => actor.schedule).map((actor) => ({
    actor: keyOf(actor),
    cron: actor.schedule!.cron,
    timezone: actor.schedule!.timezone,
    enabled: actor.schedule!.enabled,
    pausedAt: instantOf(actor.schedule!.pausedAt) || undefined,
    enabledAt: instantOf(actor.schedule!.enabledAt) || undefined,
    nextRun: instantOf(actor.schedule!.nextRun),
  }));
  const actorOfSession = new Map(sessions.map((session) => [session.id, session.actor]));
  const records = data.records || [];
  const scheduleSkips = records.filter((record) => record.kind === "schedule.skipped").map((record) => ({
    id: `skip_${record.sequence}`,
    actor: actorOfSession.get(record.sessionId!) || keyOf(record.actor),
    planned: instantOf(stringOf(record.data.planned)),
    reason: "previous session current",
    previousSession: stringOf(record.data.currentSessionId) || record.sessionId,
  })).filter((skip): skip is ScheduleSkip => skip.planned !== null);
  const actions = records.filter((record) => record.kind === "spawned")
    .map((record) => spawnAction(record.sessionId!, requesterOf(record.data.spawner), stringOf(record.data.spawnerSessionId), record.actor, instantOf(record.at)!, keyOf));
  // A Session kept by Live mode can outlive the range its Spawn record falls in, and the
  // Session read carries that record, so the Spawn is taken from there when the list has
  // none for it.
  const spawned = new Set(actions.map((action) => action.session));
  for (const session of data.sessions || []) {
    const spawnedBy = (session as FeldApi.Session).spawnedBy;
    if (!spawnedBy || spawned.has(session.id)) continue;
    actions.push(spawnAction(session.id, spawnedBy.spawner, spawnedBy.spawnerSessionId, { workspace: session.workspace, name: session.actor, id: (session as FeldApi.Session).actorId }, instantOf(session.start)!, keyOf));
  }
  const messages = (data.messages || []).map((message) => ({
    id: message.id,
    from: requesterKey(message.from, keyOf),
    fromWorkspace: requesterWorkspace(message.from),
    fromSessionId: message.fromSessionId || null,
    to: keyOf(message.to),
    toWorkspace: message.to.workspace,
    toSessionId: message.toSessionId || null,
    accepted: instantOf(message.accepted)!,
    read: message.read
      ? { readId: message.read.readId, sessionId: message.read.sessionId, at: instantOf(message.read.at)!, fifo: message.read.fifo }
      : null,
    anchor: message.anchor || null,
    preview: message.preview || "",
    body: null,
  }));
  return {
    workspaces,
    defaultWorkspace,
    actors,
    sessions,
    schedules,
    scheduleSkips,
    actions,
    messages,
    metrics: buildMetrics(data.runtimeMetrics, data.runtimes || []),
    launchOptions: launchOptionsOf(data),
  };
}

// The Sessions as the Console knows them, which the Console is given: the
// Console reads a Session by id, and these name it while its transcript is not yet read.
export function consoleSessionsOf(input: ActorsInput): FeldConsoleSession[] {
  return input.consoleSessions || sessionsOf(input).map(consoleSessionOf);
}

// The Sessions of one Actor that the card lists, read whatever the range, keyed as the
// page keys that Actor.
export function toActorSessions(read: FeldActorSessions | null, actor: string): AgentSession[] | undefined {
  if (!read) return undefined;
  const projections = new Map(read.metrics.map((entry) => [entry.session, entry]));
  return read.sessions.map((session) => mapSession(session, projections.get(session.id), actor));
}
