// The shapes the fixture keeps its data in (data/*.js, under `window.FeldWebUI.data`).
// Instants are epoch milliseconds, and an Actor is referred to by its bare id: the fixture
// keeps every id distinct across Workspaces.

import type { ActorLaunchSpec, SessionLifecycle, SessionObservation, SessionPlacement } from "../actors/types.ts";

export interface MockClock {
  now: number;
  timeZone: string;
}

export interface MockScenario {
  anchor: number;
  duration: number;
  pinJobs?: boolean;
}

export interface MockActor {
  id: string;
  workspace: string;
  kind: FeldApi.ActorKind;
  launchSpec: ActorLaunchSpec | null;
  grants?: FeldApi.Actor["grants"];
}

type Unreported = { status: "n/a" | "unavailable"; source: string };

// Token samples are [input, output, cache_read, cache_creation] per interval, context
// samples the used percentage; both are spaced evenly over the Session.
export type MockTokenMetric = { status: "reported"; source: string; samples: number[][] } | Unreported;
export type MockContextMetric = { status: "reported"; source: string; size: number; samples: number[] } | Unreported;

export interface MockSession {
  id: string;
  actor: string;
  start: number;
  end: number | null;
  planned?: number;
  startKind: FeldApi.StartKind | "sub-agent";
  ownership: FeldApi.Session["ownership"] | null;
  endKind: FeldApi.Session["endKind"] | null;
  exitCode: number | null;
  execution: { type: FeldApi.Execution["type"]; label: string; command: string | null; ai: boolean };
  model: [string, string | null] | null;
  // The account a Runtime Session runs under, which its quota reports belong to.
  account?: string;
  placement: SessionPlacement | null;
  metrics: { tokens: MockTokenMetric; context: MockContextMetric };
  observation: SessionObservation | null;
  observations?: SessionObservation[];
  lifecycle?: SessionLifecycle;
  parentSession?: string;
  subAgent?: string;
  recordingOrigin?: string;
  attachCommand?: string;
}

export interface MockSchedule {
  actor: string;
  cron: string;
  timezone: string;
  enabled: boolean;
  pausedAt?: number;
  enabledAt?: number;
}

export interface MockScheduleSkip {
  id: string;
  actor: string;
  planned: number;
  reason: string;
  previousSession: string;
}

export interface MockAction {
  id: string;
  spawner: string;
  spawnerWorkspace: string | null;
  spawnerSessionId: string | null;
  target: string;
  targetWorkspace: string;
  session: string;
  at: number;
}

export interface MockMessage {
  id: string;
  // An Actor id, or "Admin" (with no Workspace) for a Message the Admin sent.
  from: string;
  fromWorkspace: string | null;
  fromSessionId: string | null;
  to: string;
  toWorkspace: string;
  toSessionId: string | null;
  accepted: number;
  read: { readId: string; sessionId: string; at: number; fifo: number } | null;
  anchor: string | null;
  // The whole body; the fixture's list preview is derived from it as the daemon's is.
  body: string;
}

// A lane series is either a status with its source, or the parameters of the periodic
// generator the mock draws it from (metricValueAt in components/agents.js).
export interface MockMetricSeries {
  provider: string;
  account?: string;
  tone: string;
  source: string;
  status?: "n/a" | "unavailable";
  until?: number;
  gaps?: readonly (readonly [number, number])[];
  period?: number;
  resetAnchor?: number;
  minimum?: number;
  span?: number;
  variation?: number;
  phase?: number;
  showResets?: boolean;
}

export interface MockMetricLane {
  id: string;
  name: string;
  caption: string;
  unit: string;
  maximum: number;
  kind?: "quota";
  prefix?: string;
  decimals?: number;
  defaultVisible: boolean;
  derivation: string;
  series: MockMetricSeries[];
}

export interface MockActorsData {
  scenarios: Record<string, MockScenario>;
  workspaces: string[];
  defaultWorkspace: string;
  actors: MockActor[];
  sessions: MockSession[];
  schedules: MockSchedule[];
  scheduleSkips: MockScheduleSkip[];
  actions: MockAction[];
  messages: MockMessage[];
  metricLanes: MockMetricLane[];
  nodePrograms: string[];
}

// A recording of data/session-*.js: the common-form transcript entries of one transcript
// file.
export interface MockRecording {
  id: string;
  sourceName: string;
  startedAt: string;
  endedAt: string;
  records: FeldApi.TranscriptEntry[];
}

export interface MockDaemonData {
  status: FeldApi.DaemonStatus;
  degradedStatus: FeldApi.DaemonStatus;
  lastRead: string;
  readFailure: string;
  refusals: MockRefusals;
  pairing: MockPairing;
}

// The pairing codes `feld ui pair` issued, each good once.
export interface MockPairing {
  codes: string[];
}

// The daemon's message for each request it refuses; `{actor}` and `{session}` are filled in.
export interface MockRefusals {
  restart: string;
  shutdown: string;
  start: string;
  stop: string;
  kill: string;
  delete: string;
}

export interface MockData {
  clock: MockClock;
  actors: MockActorsData;
  consoleSessions: FeldConsoleSession[];
  sessionRecordings: Record<string, MockRecording>;
  sessionLogs: Record<string, FeldConsoleLog>;
  daemon: MockDaemonData;
}
