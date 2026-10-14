# Feld Web UI views

Defines the read-only screens that the Web UI and CLI draw. The concepts used follow the [glossary](../spec/concepts/glossary.md).

## Console

A read-only view per Session. It displays the Session's `log`: for an agent-runtime Session, the transcript (a sequence of transcript entries that the Runtime Adapter projects from the transcript file into a common shape) as the Conversation view or the All events view, and for a program Session, stdout / stderr as the log view. In both, the Session's [records](../spec/concepts/glossary.md#record) are inserted at the position of the transcript entry that cites the record's ID, or by time when there is none. The Console sends no input.

`spawned` is inserted into the Consoles of both the started side and the starting side, linking to each other's rows. The All events view and the log view insert all kinds. The Conversation view inserts Control Plane requests and Messages (`session.started` / `ended`, `spawned`, `stop.requested` / `kill.requested`, `message.accepted` / `sent` / `read`).

A record row shows what happened in one line; the record kind and ID, the correspondence to transcript entries, and the facts sit in the ledger opened from the row's `N facts`. Links to other Consoles and Jobs appear on the row.

Message records are shown in the shape of an email. The heading shows the delivery state, the line below it shows the counterpart Actor (`To` in the sender's Console, `From` in the receiver's), followed by the full body. A long body shows only its beginning, and a toggle opens the full text.

## Actor card

A per-Actor view opened from the Actor name, `Agent…` in the Agent's `⋮` menu, a Schedule column, a skipped tick mark, or a record link in the Console. It shows the Actor's facts (Kind, launch spec, grants, Mailbox, Sessions) and the list of Sessions, and a Session row leads to that Session's facts and Console. The launch spec in the facts shows all fields. env shows only names, and runtimeSettings and providerOptions mask the values of names that contain key, token, secret, password, or credential. For a Job, the Schedule section (Enabled, Cron, Next run) holds Pause / Resume and Edit. For an Agent, the Agent section holds the diff between the launch spec and the latest Session, and Edit. The diff compares the runtime (the command for an Actor with a command), workdir, Provider, and Node of a Session Feld started from the launch spec (`owned`), and shows that `attached` Sessions cannot be compared. Edit opens the name, the Job's cron and timezone, and the launch spec form in the same place, and gives one to an Agent without a launch spec. The form masks the values of the same names in env, runtimeSettings, and providerOptions, and keeps masked values as the original values while they stay masked. The name cannot change while the current Session is running. The Session list includes ticks the Schedule skipped, and when opened from a skipped tick, that row is highlighted.

## Timeline

A view on the Actors page that draws Sessions on a time axis in a band per Actor. It draws the time during which Feld holds the Session, and a Sub Agent Session draws its part from the parent Session's start onward in the parent's band. Live and past ranges follow the same rules. Transcript entries of a Sub Agent Session before attach remain in the parent's Console.

Spawns and Messages are drawn as lines between Actors. Spawns and Messages with the same source Session and time bin (width depends on zoom) share one dotted trunk, with an end mark per counterpart (an arrow for delivered, a tray for a Message not yet read). Hovering or focusing the trunk lists the Spawns and Messages in it with their times.

## Now

A section in the Web UI's Session Details that lists the "now" of the current Session. `running` only shows that execution has not finished, so facts from different sources are split into the following rows, each with a one-sentence meaning.

- Process: the kind and observation time of the latest [observation](../spec/concepts/glossary.md#observation). Shows whether the underlying process is alive. A Session with no observation yet shows as not observed
- Turn: the [turn](../spec/concepts/glossary.md#turn). Shows whether it is mid-response or waiting for input. Only agent-runtime Sessions have it
- Last entry: the kind and time of the last transcript entry. A tool call without a result is noted as such. For a program Session, the last time it wrote to stdout / stderr
- Mailbox: the [waiting count](../spec/concepts/glossary.md#waiting-count), the accept time of the oldest `waiting`, and the time of the last read of a Message addressed to the Actor
- Stop: a `lifecycle.stop` / `lifecycle.kill` accepted by a Session with no `end`. Without an accepted one, the row is hidden

Only records and derived values are listed; Admin judges whether it is stalled or complete.

## Metrics lane

[Runtime metrics](../spec/concepts/glossary.md#runtime-metrics) displayed as one lane per metric. Additive metrics such as token, cost, count, and active time are summed per account. Each lane holds a series per account, and accounts are distinguished by a color separate from the Runtime colors and the same in every lane (a Runtime with one account keeps the Runtime color). The lanes are the following.

- quota. Claude Code has 5-hour and 7-day lanes, and Codex has a 7-day lane. Values belonging to the account, derived from Session reports. Readings are ordered by utilization, highest first
- Token usage
- Estimated cost
- API errors
- Prompt cache hit rate
- API request latency p95
- Tool calls
- Context window utilization
- Active time

The lanes shown by default are the quota lanes for the windows the Runtime reported, plus Token usage, Estimated cost, and API errors.

The Metrics lane window can be the same width as the Timeline or wider. A wider window contains the Timeline range, is centered on it, and ends no later than now. When the Timeline is Live, Metrics also ends at now. The Timeline range stays bright, the outside is dimmed, and the range is marked on the Metrics time axis. Dragging on Metrics moves the Timeline. A lane's reading is the latest sample in the Metrics window. Only a reading whose sample stopped earlier shows its own time.

A lane collapses more than 3 series. Lanes other than quota list each Runtime's largest series first, keeping every Runtime even when collapsed. The reading per series distinguishes the value, `Measured zero`, `0% since reset`, `No samples in range`, `Telemetry unavailable`, and `N/A` ([metrics.md](../spec/design/metrics.md#metrics-lane)). Aggregation rules are described in [metrics.md](../spec/design/metrics.md#aggregation).
