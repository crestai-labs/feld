// The shapes the page's modules share. The daemon's responses are in feld-api.d.ts.

interface FeldConversationItem {
  kind: FeldApi.TranscriptKind;
  record: FeldApi.TranscriptEntry;
  result: FeldApi.TranscriptEntry | null;
}

interface FeldMetricSample {
  at: number;
  end: number;
  // The reset that ends the sample's series; a level reading holds only until it.
  until?: number | null;
  value: number;
}

interface FeldQuotaSegment {
  start: number;
  end: number | null;
  reporter: FeldApi.QuotaSeries["reportedBy"] | null;
}

interface FeldQuotaLane {
  samples: FeldMetricSample[];
  segments: FeldQuotaSegment[];
  resets: number[];
}

interface FeldMetricRange {
  start: number;
  end: number;
}

interface FeldMetricReading {
  at: number;
  // Where the sample read ends: a bucket covers a stretch of time after its start.
  end: number;
  value: number;
  resetAt?: number;
}

// The last answer of each range read the Actors page holds.
interface FeldRangeHeld {
  // The live Actors, by Workspace.
  actors: Map<string, FeldApi.Actor[]>;
  // The Sessions the range listing named, each replaced by its own read.
  sessions: Map<string, FeldApi.Session>;
  subAgents: Map<string, FeldApi.SubAgentSession[]>;
  // The metrics of the Sessions and Sub Agent Sessions whose metrics read has answered, by id.
  metrics: Map<string, FeldApi.SessionMetrics>;
  records: FeldApi.FeldRecord[];
  messages: FeldApi.MessageSummary[];
  runtimeMetrics: FeldLiveData["runtimeMetrics"];
  runtimes: FeldApi.Plugin[];
  providers: FeldApi.Plugin[];
  nodes: string[];
}

// The range the page shows. A live query has no end and follows the clock.
interface FeldLiveQuery {
  workspace: string | null;
  start: number;
  end: number | null;
  duration: number;
}

type FeldLiveQuota = Partial<FeldApi.RuntimeQuota>;
// One Runtime's metrics read over the Metrics window.
type FeldLiveRuntimeMetrics = { runtime: string } & Partial<FeldApi.RuntimeMetrics>;

// What the range reads have answered so far. A Sub Agent Session follows its parent; a
// Session or Sub Agent Session whose metrics read has not answered has no metrics.
interface FeldLiveData {
  workspaces: string[];
  actors: FeldApi.Actor[];
  sessions: (FeldApi.Session | FeldApi.SubAgentSession)[];
  metrics: FeldApi.SessionMetrics[];
  records: FeldApi.FeldRecord[];
  messages: FeldApi.MessageSummary[];
  // Every installed Runtime's metrics read that has answered, over the Metrics window.
  runtimeMetrics: FeldLiveRuntimeMetrics[] | null;
  // The daemon's installed Runtimes and Providers and its Nodes; empty until read.
  runtimes: FeldApi.Plugin[];
  providers: FeldApi.Plugin[];
  nodes: string[];
}

// One Actor's Sessions whatever the range with their Sub Agent Sessions, and the metrics of
// the one selected.
interface FeldActorSessions {
  sessions: (FeldApi.Session | FeldApi.SubAgentSession)[];
  metrics: FeldApi.SessionMetrics[];
}

interface FeldLiveConnection {
  state: "connecting" | "live" | "error" | "reconnecting";
  message: string;
  // The failed read's message, for an error.
  detail?: string;
}

interface FeldStreamSnapshot {
  connected: boolean;
  event: FeldApi.Event | null;
}

// One of the three cursors the Console reads accept; none reads the newest page.
interface FeldConsoleCursor {
  before?: string | number;
  after?: string | number;
  limit?: number;
}

// A failed daemon request; `status` tells a rejected token from an absent daemon.
interface FeldApiError extends Error {
  status?: number;
}

// A Session as the Console knows it. `console: "log"` is a program Session; `"none"` and
// `"missing"` are a Runtime Session without a readable transcript file.
interface FeldConsoleSession {
  id: string;
  actor: string;
  model?: string;
  // The name of an agent-runtime Session's Runtime, which creates its transcript file.
  runtimeName?: string;
  // The Console header's Placement summary; a registered Session may name it `runtime`.
  placement?: string;
  runtime?: string;
  recording?: string;
  console?: "log" | "none" | "missing";
  sourceUnavailable?: string;
  // A fixture Session whose Console holds only the tail of its records.
  hasOlder?: boolean;
}

// A transcript page, or a fixture recording, in the shape the Console draws.
interface FeldConsoleRecording {
  id: string;
  records: FeldApi.TranscriptEntry[];
  hasOlder: boolean;
  startedAt?: string;
}

// Captured output: each line is its offset from the Session's start, its stream, its text,
// and, for a read line, its sequence.
interface FeldConsoleLog {
  source: string;
  lines: [offset: number, stream: FeldApi.OutputLine["stream"], text: string, sequence?: number][];
}

// Which recording instant coincides with the Session's start.
interface FeldConsoleTimeOrigin {
  recordingInstant: number;
  sessionInstant: number;
}

interface FeldConsoleFact {
  key?: string;
  label: string;
  value: string;
  detail: string;
  provenance: string;
}

interface FeldConsoleLink {
  label: string;
  sessionId?: string;
  recordId?: string;
  agentId?: string;
  kind?: string;
}

// One of Feld's own records as a Console row.
interface FeldConsoleRecord {
  id: string;
  type: string;
  kind: string;
  at: number;
  title: string;
  summary: string;
  body?: string;
  anchor: string | null;
  conversation: boolean;
  facts: FeldConsoleFact[];
  links: FeldConsoleLink[];
  // A Message record's envelope: the other party of this Session, and the Message
  // whose text the row shows. `body` is the whole text when the page holds it, and
  // null when the row reads it from the daemon (`body` of the record is then the preview).
  message?: FeldConsoleEnvelope;
}

interface FeldConsoleEnvelope {
  id: string;
  role: "To" | "From";
  actor: string;
  sessionId: string | null;
  body: string | null;
}

interface FeldConsoleTerminalOptions {
  controls: string;
  actorLabel: string;
  feldRecords: FeldConsoleRecord[];
  timeOrigin: FeldConsoleTimeOrigin | null;
  sessionStart: number;
  status: string;
  statusLabel: string;
  statusTitle: string;
}

// The transcript entry that quotes a record ID.
interface FeldConsoleRuntimeAnchor {
  cursor: string;
  at: number | null;
  label: string;
}

// The last transcript entry that carries a turn, or the last captured output line.
interface FeldConsoleLastRecord {
  kind: FeldApi.TranscriptKind | FeldApi.OutputLine["stream"];
  label: string;
  tool?: string;
  hasResult?: boolean;
  cursor?: string;
  at: number | null;
  recordId: string;
}
