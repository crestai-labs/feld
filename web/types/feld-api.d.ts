// The daemon's responses as the Web UI reads them, named after the schemas that
// `feld-api.generated.d.ts` generates from `docs/guide/daemon-api.openapi.yaml`.

import type { components } from "./feld-api.generated";

type Schemas = components["schemas"];

declare global {
  namespace FeldApi {
    type Timestamp = string;

    type Workspace = Schemas["Workspace"];
    type Plugin = Schemas["Plugin"];
    type DaemonStatus = Schemas["DaemonStatus"];
    type ActorKind = Schemas["ActorKind"];
    type LaunchSpec = Schemas["LaunchSpec"];
    type LaunchSpecPatch = Schemas["LaunchSpecPatch"];
    type Schedule = Schemas["Schedule"];
    type Actor = Schemas["Actor"];
    type Requester = Schemas["Requester"];
    type ActorReference = Schemas["ActorReference"];
    type Outcome = Schemas["Outcome"];
    type ObservationKind = Schemas["ObservationKind"];
    type Observation = Schemas["Observation"];
    type Execution = Schemas["Execution"];
    type Placement = Schemas["Placement"];
    type LifecycleAction = Schemas["LifecycleAction"];
    type StartKind = Schemas["StartKind"];
    type Session = Schemas["Session"];
    type SessionModel = Schemas["SessionModel"];
    type Turn = Schemas["Turn"];
    type ConsoleState = Schemas["ConsoleState"];
    type SubAgentSession = Schemas["SubAgentSession"];
    type SessionResponse = Schemas["SessionResponse"];
    type StartResponse = Schemas["StartResponse"];
    type RecordKind = Schemas["RecordKind"];
    // `Record` in the yaml, renamed away from TypeScript's own `Record`.
    type FeldRecord = Schemas["Record"];
    type TranscriptEntry = Schemas["TranscriptEntry"];
    type TranscriptKind = TranscriptEntry["kind"];
    type TranscriptPage = Schemas["TranscriptPage"];
    type OutputPage = Schemas["OutputPage"];
    type OutputLine = OutputPage["lines"][number];
    type MetricName = Schemas["MetricName"];
    type MetricStatus = Schemas["MetricStatus"];
    type MetricBin = Schemas["MetricBin"];
    type SessionMetrics = Schemas["SessionMetrics"];
    type QuotaPoint = Schemas["QuotaPoint"];
    type RuntimeQuota = Schemas["RuntimeQuota"];
    type QuotaSeries = NonNullable<RuntimeQuota["series"]>[number];
    type RuntimeMetrics = Schemas["RuntimeMetrics"];
    type Account = Schemas["Account"];
    type MessageRead = Schemas["MessageRead"];
    type MessageSummary = Schemas["MessageSummary"];
    type Message = Schemas["Message"];
    type MessageResponse = Schemas["MessageResponse"];

    // One notification of `GET /v1/events`, whose payload the yaml describes in prose.
    interface Event {
      resource: string;
      id: string;
      [key: string]: unknown;
    }
  }
}
