# Feld concept ER diagram

This diagram shows the core objects Feld persists and the relationships between them. The meaning of each field follows [`glossary.md`](glossary.md); only the fields that determine identifiers and relationships are listed here.

```mermaid
erDiagram
  Workspace ||--o{ Actor : "namespace"
  Actor ||--o| LaunchSpec : "launch recipe"
  Actor ||--o| Schedule : "Job run schedule"
  Actor ||--o{ Session : "run (generation)"
  Session ||--o| Placement : "placement"
  Session |o..o{ Session : "Sub Agent (parentSession)"
  Session ||--|{ SessionToken : "issues"
  Session ||--o{ Telemetry : "delivered telemetry"
  Actor ||--o{ Message : "destination mailbox (to)"
  Actor |o..o{ Message : "sender (from)"
  Actor ||--o{ Record : "subject"
  Session |o..o{ Record : "involved Session"

  Workspace {
    string id PK
    string key "unique among live Workspaces"
    datetime deletedAt
  }
  Actor {
    string id PK
    string workspaceId FK
    string name "unique among live Actors in the Workspace"
    string kind "agent | job"
    datetime deletedAt
  }
  LaunchSpec {
    string runtime "name of an installed Runtime"
    string command "argv"
    string workdir
    string provider "name of an installed Execution Provider"
    string node
  }
  Schedule {
    string cron
    string timezone
    boolean enabled
  }
  Session {
    string id PK
    string actorId FK
    int generation "sequential within the Actor"
    string parentSession FK "only a Sub Agent Session has it"
    string startKind "start | replacement | attach | schedule | sub-agent"
    string ownership "owned | attached"
    string endKind "running | stopped | exited | unknown"
    int exitCode
    datetime start
    datetime end "absent means current Session"
    json execution "copy of the LaunchSpec at launch"
    json lifecycle "startSettled / startFailed / stop / kill"
    json observation "latest observation"
  }
  Placement {
    string provider
    string node
    string workdir
  }
  SessionToken {
    string token UK
  }
  Telemetry {
    string sessionId FK
    string signal "metrics | logs"
    datetime receivedAt
  }
  Message {
    string id PK
    string from FK "Actor id or Admin"
    string fromSessionId FK
    string to FK
    string toSessionId FK
    int fifo "position in the mailbox"
    datetime accepted
    json read "sessionId / at / readId"
    datetime deletedAt
  }
  Record {
    int sequence PK "global order"
    string kind
    datetime at
    string actorId FK
    string sessionId FK
    json data "fields per kind"
  }
  TokenSecret {
    string kind "local | internal"
    string secret UK
    datetime createdAt
  }
```

## Notes on relationships

- Each Actor has at most one Session without `end`, and this is the current Session slot. The Session of the next generation enters the same slot
- Only an Actor of kind `job` has a Schedule. An Actor without a launchSpec is always an Agent created by `attach` alone
- A Sub Agent Session points to its parent with `parentSession` and has no Placement, token, or Telemetry; its lifecycle and observation belong to the parent Session
- A Session token is reissued on every `attach`, so one Session can have several
- Session metrics, Runtime metrics, and the account's quota are all values derived from Telemetry at read time and do not appear as entities
- TokenSecret is the secret of the daemon's tokens (local token, internal token) and is held apart from the Session token, independent of the core objects. The browser token is also held as a hash independent of the core objects
- When a Message's `from` is `Admin`, it has no reference to a sender Actor or Session. `toSessionId` and the `sessionId` in `read` point to a Session of the destination Actor
- The `spawned` record has `spawner` (an Actor's `id` or `Admin`) and `spawnerSessionId` in `data`
- Admin, Runtime, and Execution Provider are not persistent objects. Runtime and Execution Provider are referenced by name from launchSpec and Placement, and Admin appears as a value of `from` and `spawner`
