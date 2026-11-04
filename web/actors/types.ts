// The records the Actors page derives everything from: instants are
// epoch milliseconds and an Actor is referred to by its `workspace/name` key.

export type Instant = number;
export type VisibilityKey = "showSpawnLinks" | "showMessageLinks" | "showSubAgents" | "showScheduleSkips" | "showAgentsOutsideRange" | "showDeletedActors";

export interface ActorLaunchSpec {
  runtime?: string;
  // Passed to the Runtime as declared; values under secret-like keys can be secrets.
  runtimeSettings?: Record<string, unknown>;
  // Lets the Runtime read the settings the workdir carries without asking.
  trustWorkdir?: boolean;
  command?: string[];
  workdir: string;
  env?: Record<string, string>;
  provider: string;
  node: string;
  // The Execution Provider's own choices, as it resolved them.
  providerOptions?: Record<string, unknown>;
}

export interface ActorRecord {
  id: string;
  name: string;
  workspace: string;
  kind: FeldApi.ActorKind;
  launchSpec: ActorLaunchSpec | null;
  // What the Admin lets the Actor's Sessions do to the other Actors of the Workspace.
  grants?: FeldApi.Actor["grants"];
  // The daemon's `id` of the Actor, which its deleted history is read by.
  actorId?: string;
  // When the Actor was deleted; absent while it is live.
  deletedAt?: Instant | null;
  // The Actor's current Session whatever the range, as its listing answers it.
  currentSession?: { id: string; ownership: string | null } | null;
}

export interface SessionPlacement {
  provider: string;
  node: string;
  workdir: string;
  summary: string;
  attachCommand: string | null;
}

export interface LifecycleRequest {
  accepted: Instant;
  requesterSessionId: string | null;
  attempt: string;
  observed: string;
}

export interface EndRequest extends LifecycleRequest {
  kind: "stop" | "kill";
}

export interface SessionLifecycle {
  startSettled?: Instant;
  startFailed?: Instant;
  stop?: LifecycleRequest;
  kill?: LifecycleRequest;
}

export interface SessionObservation {
  kind: FeldApi.ObservationKind;
  at: Instant;
  // The instant the latest kind was recorded; a Session's latest observation carries it.
  since?: Instant | null;
  detail?: string | null;
}

export interface BinSample<V> {
  start: Instant;
  end: Instant;
  value: V;
}

export type MetricProjection = FeldApi.SessionMetrics["metrics"][number];
export type UnreportedStatus = Exclude<FeldApi.MetricStatus, "reported">;
// `reading` is the page's own state while the Session's metrics read has not answered.
export type TokenMetric =
  | { status: "reported"; source: string; samples: BinSample<number[]>[] }
  | { status: UnreportedStatus | "reading"; source: string };
export type ContextMetric =
  | { status: "reported"; source: string; size: number; samples: BinSample<number>[] }
  | { status: UnreportedStatus | "reading"; source: string };

// The Runtime and the model; a program Session has neither.
export type SessionModel = [string, string | null];

export interface AgentSession {
  id: string;
  workspace: string;
  actor: string;
  start: Instant;
  end: Instant | null;
  planned: Instant | null;
  startKind: FeldApi.StartKind | "sub-agent";
  ownership: FeldApi.Session["ownership"] | null;
  endKind: FeldApi.Session["endKind"] | null;
  exitCode: number | null;
  execution: { type: FeldApi.Execution["type"]; runtime?: string | null; label: string; command: string | null; ai: boolean };
  model: SessionModel | null;
  // Whether the Runtime identity has arrived, which `model` alone no longer tells.
  linked: boolean;
  placement: SessionPlacement | null;
  lifecycle: SessionLifecycle;
  observation: SessionObservation | null;
  // A current agent-runtime Session's turn and the instant of the entry it was read from,
  // as the Session read carries them.
  turn?: FeldApi.Turn;
  lastEntryAt?: Instant | null;
  metrics: { tokens: TokenMetric; context: ContextMetric };
  laneMetrics: Partial<Record<FeldApi.MetricName, MetricProjection>>;
  parentSession?: string;
  subAgent?: string;
  consoleState?: FeldApi.ConsoleState;
  consoleDetail?: string | null;
  // Kept by fixtures only.
  recordingOrigin?: string;
  observations?: SessionObservation[];
  attachCommand?: string;
}

export interface ScheduleRecord {
  actor: string;
  cron: string;
  timezone: string;
  enabled: boolean;
  pausedAt?: Instant;
  enabledAt?: Instant;
  nextRun: Instant | null;
}

export interface ScheduleSkip {
  id: string;
  actor: string;
  planned: Instant;
  reason: string;
  previousSession: string;
}

export interface SpawnAction {
  id: string;
  spawner: string;
  spawnerWorkspace: string | null;
  spawnerSessionId: string | null;
  target: string;
  targetWorkspace: string;
  session: string;
  at: Instant;
}

export interface MessageRead {
  readId: string;
  sessionId: string;
  at: Instant;
  fifo: number;
}

export interface MessageRecord {
  id: string;
  from: string;
  fromWorkspace: string | null;
  fromSessionId: string | null;
  to: string;
  toWorkspace: string;
  toSessionId: string | null;
  accepted: Instant;
  read: MessageRead | null;
  anchor: string | null;
  // The first 48 characters of the body, as the Message list carries it.
  preview: string;
  // The whole body when the page holds it (a fixture); null when the daemon's list was read.
  body: string | null;
}

export interface MetricLaneSpec {
  id: string;
  name: string;
  caption: string;
  unit: string;
  maximum: number;
  kind?: "quota";
  level?: boolean;
  window?: FeldApi.QuotaSeries["window"];
  showResets?: boolean;
  metric?: FeldApi.MetricName;
  // The Runtimes whose telemetry has the metric, by Plugin name; every Runtime when absent.
  runtimes?: readonly string[];
  prefix?: string;
  decimals?: number;
  defaultVisible: boolean;
  derivation: string;
}

// One account's series of a Runtime on a lane, or the Runtime's own where the account is
// unknown or the lane has no account to show. One with a `status` has no samples to plot.
export interface MetricSeries {
  provider: string;
  tone: string;
  // The account's display name, on a series whose account is known.
  account?: string;
  // The account's own colour (account-N), where its Runtime has several accounts.
  accountTone?: string;
  source: string;
  status?: "n/a" | "unavailable";
  // The reads the series is drawn from have not all answered.
  reading?: boolean;
  samples?: FeldMetricSample[];
  showResets?: boolean;
  resets?: number[];
  segments?: FeldQuotaSegment[];
}

export interface PlottedSeries extends MetricSeries {
  status?: undefined;
  samples: FeldMetricSample[];
}

export interface MetricLane extends MetricLaneSpec {
  series: MetricSeries[];
}

// A lane as the Metrics section lists it: the series the search keeps, in reading order,
// folded past METRIC_ROW_LIMIT unless expanded, and where the lane sits.
export interface ListedMetricLane {
  metric: MetricLane;
  series: MetricSeries[];
  hiddenCount: number;
  expanded: boolean;
  top: number;
  height: number;
}

export interface CronSpec {
  minute: Set<number> | null;
  hour: Set<number> | null;
  day: Set<number> | null;
  month: Set<number> | null;
  weekday: Set<number> | null;
  /** Whether a day either day field allows matches, as when neither starts with `*` or `?`. */
  eitherDay: boolean;
}

export interface ZonedParts {
  minute: number;
  hour: number;
  day: number;
  month: number;
  weekday: number;
}

export interface ScheduleRun {
  kind: "session" | "skip";
  at: Instant;
  session?: AgentSession;
  skip?: ScheduleSkip;
}

export interface Lane {
  top: number;
  height: number;
  mainY: number;
  sessionY: Map<string, number>;
}

export interface Focus {
  sessions: Set<string>;
  actions: Set<string>;
  messages: Set<string>;
  agents: Set<string>;
}

// A legend entry names its mark by `symbol`; the glyph drawn beside it is the Timeline's own
// mark for that symbol, so the page draws it from the symbol.
export interface LegendEntry {
  symbol: string;
  label: string;
  detail: string;
}

// An Actor's state as the glyph beside its name draws it: whether it has a current Session
// (`key`, inside) and whether its Schedule fires (`schedule`, the outer ring).
export interface ActorState {
  key: "running" | "stopped" | "none";
  schedule: "scheduled" | "paused" | null;
  label: string;
  title: string;
  lines: [schedule: string, now: string];
}

export interface LegendGroup {
  name: string;
  entries: LegendEntry[];
}

export interface RelatedRow {
  at: Instant;
  kind: string;
  id: string;
  glyph: string;
  label: string;
  detail: string;
}

export interface StartOutcome {
  key: string;
  text: string;
}

// The Actor card's edit form. A Job edits its Schedule (cron, timezone) with it, an Agent its
// runtimeSettings and trustWorkdir; both edit the name and the rest of the launchSpec.
export interface EditDraft {
  name: string;
  cron: string;
  timezone: string;
  runtime: string;
  command: string;
  workdir: string;
  provider: string;
  node: string;
  // One NAME=VALUE per line (see parseEnv), the values of secret-like names masked.
  env: string;
  // JSON, with the values under secret-like keys masked (see maskSecrets).
  settings: string;
  // "true" or "".
  trustWorkdir: string;
  // JSON, masked like settings.
  providerOptions: string;
}

// The Actor card's own state: the skipped tick a Job's card was opened at, and the edit
// form while open. `serial` names one opening of the card. A request started from it lands
// only while the same opening is shown, however its fields changed in between.
export interface CardView {
  serial: number;
  agentId: string;
  skipId: string | null;
  editing: boolean;
  draft: EditDraft | null;
  error: string | null;
  result: string | null;
  busy: boolean;
}

export interface ActionRequest {
  agentId: string;
  action: string;
  pending: boolean;
  error: string | null;
}

export interface NewActorResult {
  text: string;
  outcome: StartOutcome | null;
  note: string;
  actorKey: string;
  job: boolean;
  sessionId: string | null;
}

export type ActorDraftField = "workspace" | "kind" | "name" | "runtime" | "command" | "workdir" | "env" | "cron" | "timezone" | "node" | "provider" | "settings";

export interface ActorDraft extends Record<ActorDraftField, string> {
  serial: number;
  // Opened from All workspaces, the draft starts without a Workspace and the form asks for one.
  chooseWorkspace: boolean;
  advancedOpen: boolean;
  error: string | null;
  // What the daemon answered once the declaration was committed. The result the form shows
  // is derived from it and the snapshot read afterwards (see newActorResult), so the
  // Schedule's next run is the daemon's own derivation.
  created: CreatedActor | null;
  submitting: boolean;
  settling: boolean;
}

export interface CreatedActor {
  id: string;
  workspace: string;
  cron: string;
  session: FeldApi.Session | null;
}

export interface WorkspaceDraft {
  serial: number;
  key: string;
  error: string | null;
  submitting: boolean;
}

export interface ConsoleFocus {
  sessionId: string;
  recordId: string;
}

// The Actor card: an Actor, and the Session whose facts it shows, if one is chosen.
export interface ActorCard {
  agentId: string;
  sessionId: string | null;
}

export interface ComposeDraft {
  text: string;
  serial: number;
}

export interface AgentsState {
  workspace: string | null;
  mode: "live" | "historical";
  anchor: Instant;
  duration: number;
  query: string;
  pickerOpen: boolean;
  // Paused holds a Live range's picture until Refresh or until Live again.
  paused: boolean;
  // Each Refresh counts up, so the page reads the range again and moves its clock.
  refreshes: number;
  actionMenuAgent: string | null;
  agentDisplayMenuOpen: boolean;
  timelineGuideOpen: boolean;
  sessionMetric: string;
  showSpawnLinks: boolean;
  showMessageLinks: boolean;
  showSubAgents: boolean;
  showScheduleSkips: boolean;
  showAgentsOutsideRange: boolean;
  // Whether the deleted Actors are read and drawn with the live ones.
  showDeletedActors: boolean;
  // The reader's changes to the default pins, by Actor key: a Job starts pinned and an
  // Agent unpinned.
  actorPins: Record<string, boolean>;
  // Actor keys in the order the reader dragged them to; Actors not in it follow the
  // default order.
  actorOrder: string[];
  cardView: CardView | null;
  actionRequest: ActionRequest | null;
  newActor: ActorDraft | null;
  newWorkspace: WorkspaceDraft | null;
  // The bands a pointer gesture keeps while it runs, so the lanes under the pointer do not
  // change mid-drag; null outside a gesture, when the bands follow visibleAgents.
  frozenBandIds: string[] | null;
  visibleActorDetails: Set<string>;
  actorWidth: number;
  // The Actor table column widths the reader dragged, by column id; the others keep the
  // defaults of ACTOR_DETAIL_OPTIONS.
  actorColumnWidths: Record<string, number>;
  metricsMenuOpen: boolean;
  // The Metrics window the reader chose; the window is never narrower than the Timeline's.
  metricsDuration: number;
  metricsPickerOpen: boolean;
  // The lanes that list every series instead of folding past METRIC_ROW_LIMIT.
  expandedMetricIds: Set<string>;
  // The series whose reading row the pointer is on (`metricId:index`): its line comes
  // forward and the lane's other lines recede.
  emphasizedSeries: string | null;
  // The compact (phone) layout: each Actor band is a name row over a full-width Timeline row.
  compactLayout: boolean;
  actorsCollapsed: boolean;
  metricsCollapsed: boolean;
  visibleMetricIds: Set<string>;
  // The lanes the reader showed or hid; a lane without a choice follows its default.
  metricChoices: Record<string, boolean>;
  agentsPaneHeight: number | null;
  inspectorWidth: number | null;
  inspectedSessionId: string | null;
  focusedActorId: string | null;
  consoleFocus: ConsoleFocus | null;
  consoleMark: ConsoleFocus | null;
  composeAgentId: string | null;
  // Each opening of the composer gets its own serial; a send lands only in the opening it
  // started from.
  composeSerial: number;
  // The unsent text of each Actor's composer, by Actor ID. Each change of a draft gets its
  // own serial, so a send clears only the draft it sent.
  composeDrafts: Record<string, ComposeDraft>;
  // The Actors whose Message is in flight; a send outlasts the composer it started from.
  composeSending: Record<string, true>;
  composeResult: { id: string; body: string } | null;
  composeError: string | null;
  actorCard: ActorCard | null;
}

// What the Actors page derives everything from: the live range reads mapped into the
// shapes above, or a fixture scenario's records.
export interface LaunchOptions {
  runtimes: readonly (readonly [string, string])[];
  providers: readonly (readonly [string, string])[];
  nodes: readonly (readonly [string, string])[];
}

export interface ActorsInput {
  workspaces: string[];
  defaultWorkspace: string;
  actors: ActorRecord[];
  sessions: AgentSession[];
  // The Sessions of the Actor the card lists, read whatever the range. Fixtures leave it
  // out: their `sessions` hold every Session.
  actorSessions?: AgentSession[];
  // The Session among `actorSessions` read with its metrics over its lifetime.
  measuredSession?: string | null;
  schedules: ScheduleRecord[];
  scheduleSkips: ScheduleSkip[];
  actions: SpawnAction[];
  messages: MessageRecord[];
  metrics: MetricLane[];
  // The choices New actor and the Job's launchSpec offer, as [value, label].
  launchOptions: LaunchOptions;
  // Kept by fixtures only: the Console's own Session list, where a scenario states a
  // transcript file that cannot be read. The live page derives it from `sessions`.
  consoleSessions?: FeldConsoleSession[];
}

// The daemon's message for each Actions menu request it refuses, in fixture mode's
// `?state=refusing`; `{actor}` and `{session}` are filled in.
export interface MenuRefusals {
  start: string;
  stop: string;
  kill: string;
  delete: string;
}
