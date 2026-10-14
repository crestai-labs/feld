# Session lifecycle and admission

This document explains, in time order, when Feld writes which record from the start of a Session until it closes, how it judges a launch request, and how it interprets the end. The terms and rules are listed in [`concepts/glossary.md`](../concepts/glossary.md) and [`daemon-api.md`](../../guide/daemon-api.md); this document covers only the flow and the order of decisions.

## Contents

- [Records and derived values](#records-and-derived-values)
- [Four ways a Session starts](#four-ways-a-session-starts)
  - [start](#start)
  - [replacement](#replacement)
  - [attach](#attach)
  - [schedule](#schedule)
- [admission](#admission)
  - [Transaction that closes the previous Session](#transaction-that-closes-the-previous-session)
  - [When admission rejects a Schedule tick](#when-admission-rejects-a-schedule-tick)
- [From launch to startSettled](#from-launch-to-startsettled)
- [Stopping](#stopping)
  - [feld session stop](#feld-session-stop)
  - [feld session kill](#feld-session-kill)
  - [already requested](#already-requested)
  - [stop / kill on a lost / mismatched Session](#stop--kill-on-a-lost--mismatched-session)
- [Interpreting the end](#interpreting-the-end)
- [Scheduler supervision](#scheduler-supervision)
- [Recovery after a daemon restart](#recovery-after-a-daemon-restart)
- [Sub Agent Session](#sub-agent-session)

## Records and derived values

Feld writes only the fact that it accepted a request and the facts that a Provider observed. A Session's `start` / `end`, `startKind`, `ownership`, `endKind`, `exitCode`, `lifecycle` (`startSettled`, `startFailed`, `stop`, `kill`), observation, `spawned`, and `schedule.skipped` are all records and never change after they are written.

outcome, attention, waiting, waiting count, unread-mail mark, skip mark, unlinked mark, and next run are not records; they are derived from records on every read. A derived value is returned alongside the records it comes from (`endKind`, `exitCode`, observation). Records grow only when a request is accepted and when the kind of a Provider observation changes; while observations of the same kind continue, only `observation.at` advances.

## Four ways a Session starts

A Session starts by one of the four paths of `startKind`. On every path, the Session record is written when the request is accepted, and `start` is the time of acceptance. ownership follows from `startKind`: as described in [ownership](../concepts/glossary.md#ownership), only `attach` is `attached` and the others are `owned`.

### start

An Admin or another Session requests a launch with `POST …/start`, and the Actor has no previous generation.

1. Judge [admission](#admission). Reject an Actor that has no `launchSpec`
2. Ask the Provider to `place`, and in one transaction write the Session record (`startKind: start`, `owned`, a copy of the launchSpec in `execution`, the items resolved from the launchSpec and the `place` answer in `placement`) and the spawned record (`spawner`, `spawnerSessionId`, `target`, `session`)
3. Issue a Session token and ask the Provider to launch. The token is passed to the Session as an environment variable ([auth.md](auth.md))

The default launch of `feld actor create` requests `start` after committing the creation of the Actor. The creation remains regardless of the admission result.

### replacement

A `start` request when the Actor has a previous generation, and a `restart` request. The flow is the same as start; the Session record's `startKind` becomes `replacement` and the generation advances by one. If the previous Session is closed, admission passes; if it is `lost` / `mismatched`, the launch goes through the [transaction that closes the previous Session](#transaction-that-closes-the-previous-session). `restart` responds once it has requested a stop of the `owned`, running current Session, and launches the next generation when that Session closes (including an end by kill). The spawned record remains as in start.

### attach

`POST …/attach` attaches to an agent-runtime execution that is already running. Feld's involvement is limited to attaching and observing; the only request to the Provider is "identify and observe the specified pane and the Runtime process running in it".

1. Match the Runtime process the Provider identified against the Runtime process recorded in each Actor's current Session (the program's process for an `owned` Session) (attach in [provider/tmux.md](provider/tmux.md#attach) and [provider/herdr.md](provider/herdr.md#attach)). If it matches the `attached` current Session of the Actor with the same name, respond by reissuing that Session's token without creating a Session ([token recovery in auth.md](auth.md#token-recovery)). If it matches the current Session of a different Actor, reject. Otherwise, continue
2. If the Actor does not exist, create it with kind `agent`. An existing Actor keeps its `launchSpec`; reject if its kind is `job`
3. If the Actor with the same name has a current Session, judge the same admission as start
4. When the Provider can observe the specified entity as `running` or `relocated`, write in one transaction the Session record (`startKind: attach`, `attached`, `execution` is `agent-runtime` determined from the declared `runtime`, `placement` is the declared value) and the first observation (`running`). Therefore `startSettled` equals the time attach was accepted
5. Include the Session token in the response. How the child processes of the attached execution obtain the token is described in [auth.md](auth.md)

### schedule

The cron of a Job with a Schedule fires.

1. next run is the scheduled time derived from `cron`, `timezone`, and `enabled`, and the daemon wakes a tick at that time
2. If the wake time is 2 minutes past the scheduled time (host sleep, etc.), write the ticks from that tick until now to `schedule.missed` without firing them, and wait for the next tick after the current time
3. Judge [admission](#admission). On rejection, write [`schedule.skipped`](#when-admission-rejects-a-schedule-tick)
4. If allowed, write the Session record (`startKind: schedule`, `owned`, the tick's scheduled time in `planned`, a copy of the launchSpec in `execution`, the items resolved from the launchSpec and the `place` answer in `placement`) and ask the Provider to launch. A spawned record remains only for `start` / `restart` requests

## admission

`start`, the next generation of `restart`, attach, and Schedule ticks are judged by the same rules. The only input is the latest observation of the current Session, and the rules are those of admission in [observation](../concepts/glossary.md#observation). admission guarantees that each Actor has one current Session and that the token of a closed Session cannot be used with the daemon API. When launching from `lost` / `mismatched`, the previous entity may still be running; the Admin is responsible for stopping that process and handling its external side effects.

- No current Session, or the latest observation is `gone`: allow
- `running` / `relocated`: reject. The entity is running
- `lost` / `mismatched`: allow. Close the previous Session, then launch
- `unknown`, or no observation before `startSettled`: reject, and prompt a re-observation with `--refresh`

A rejection responds with 409.

### Transaction that closes the previous Session

A current Session whose observation is `lost` / `mismatched` is closed when the next launch request passes admission.

1. Write `end` (the time the launch request was accepted), `endKind: unknown`, and `exitCode: null` to the previous Session
2. Write the next Session's record (and the spawned record for `start` / `restart`)
3. Commit 1 and 2 in one transaction. From this point the closed Session's token is invalid ([auth.md](auth.md))

A closed Session is never observed again. Even if the same incarnation keeps running, the Session stays closed and the next Session keeps the slot.

### When admission rejects a Schedule tick

When admission rejects a tick, write the tick's scheduled time and the Session that was current at that time to `schedule.skipped`. The skipped tick ends there, and admission is judged again at the next tick. The skip mark is derived from this record.

## From launch to startSettled

An `owned` Session has no entity when its record is written; it starts running only when the Provider launches it.

1. The Provider creates the entity according to the launchSpec and launches it with the Session token, `FELD_*`, and the launchSpec's `env` as the environment. For an execution with a Runtime Adapter, the launching side of the Adapter builds the launch arguments and environment
2. The Provider builds an incarnation from the resources it created, observes it, and in one transaction writes the Placement's incarnation and the first observation record. If the observation is `running`, its time is written to `lifecycle.startSettled`. If the first observation is `gone`, there is no `startSettled`, and the Session closes according to [Interpreting the end](#interpreting-the-end)
3. Until the first observation is written, launch requests to the same Actor are rejected by admission
4. When the launch fails partway and the absence of the entity is certain, `gone` is recorded. This is the case when the Provider answers `gone` without returning a Placement, and when the launch stops before the Provider is asked (a failure to prepare the Runtime Adapter, etc.). core writes that time to `lifecycle.startFailed`, and the Session closes as `stopped` / `exited` (`exitCode: null`) depending on whether a stop was requested. `startFailed` is the fact that "the execution never started", and it distinguishes such a Session from one that started and then ended. The observation's `detail` holds the reason for the failure. The next launch request passes
5. When the Provider cannot answer the launch request, whether the entity exists is unknown, so the observation is written as `unknown` and the Session remains without `end`
6. When the daemon stops after the resources are created but before the incarnation is written, the Session becomes `lost` in [recovery after a restart](#recovery-after-a-daemon-restart)

From then on, the Provider writes a record only when the kind of the observation changes, and the latest observation time is kept as `observation.at`. Each Provider's observation rules are described in [provider/tmux.md](provider/tmux.md#observation) and [provider/herdr.md](provider/herdr.md#observation).

## Stopping

Only an `owned` Session accepts stop / kill. The acceptance of a request and the Provider's attempt and observation are recorded as separate facts in `lifecycle.stop` / `lifecycle.kill`. A Session is closed by a Provider observation, or by a request to a Session already known by observation to have lost its entity; accepting a request alone does not close it.

The handling for each state of the current Session is as follows.

| Session state | Accepting stop / kill | Operation on the entity | Condition for closing the Session |
|---|---|---|---|
| `attached` (regardless of observation) | Reject | None | When the Provider observes `gone` (`exited`), or when the next launch request passes admission while still `lost` / `mismatched` (`unknown`) |
| `owned`, no observation (before `startSettled`) | Accept | After the first observation is written, follow the row for its kind | Follow the row for that kind |
| `owned`, `running` / `relocated` | Accept | Ask the recorded incarnation to end. kill ends it without asking | When the Provider observes `gone` (`stopped`), or observes `lost` / `mismatched` (`unknown`) |
| `owned`, `unknown` | Accept | Re-observe, and once the kind is determined follow the row for it | Follow the row for that kind |
| `owned`, `lost` / `mismatched` | Accept | None | In the same transaction as the acceptance (`unknown`) |
| Has `end` | Reject | None | — |

### feld session stop

1. Accept `POST /v1/sessions/{sessionId}/stop`, and write `accepted` (the acceptance time) and `requesterSessionId` (that Session for a Session token, null for an Admin token) to `lifecycle.stop`. `stop.requested` appears in the records
2. The Provider asks the execution to end and writes what it attempted to `attempt`. What it attempts depends on the Provider
3. The Provider observes the result and writes it to `observed`. When it observes `gone`, write the Session's `end`, `endKind: stopped`, and `exitCode` (the value the Provider reported, or null if it cannot report one) together with the observation record. When it observes `lost` / `mismatched`, write `end`, `endKind: unknown`, and `exitCode: null` to the Session in the same transaction. When the Provider [cannot answer](plugin.md#invoking-an-operation), write `attempt` as null and `observed` and the observation as `unknown` (with the reason in `detail`), and the Session remains without `end`
4. If the Runtime keeps running and there is no `end`, the Session stays `running` and has only `lifecycle.stop`

### feld session kill

Even after a stop is accepted, `POST /v1/sessions/{sessionId}/kill` is accepted as long as there is no `end`. The flow is the same as stop; it is recorded in the same shape in `lifecycle.kill`, and `kill.requested` appears in the records. If either `lifecycle.stop` or `lifecycle.kill` was accepted before `end`, `endKind` becomes `stopped`.

### already requested

A second request of the same kind (stop after stop, kill after kill) and stop after kill return the existing record as already requested. When a request of the same kind arrives for a request whose `observed` is `unknown`, the Provider retries it as already requested, and `attempt` and `observed` are rewritten. The accepted order is one way: stop → kill.

### stop / kill on a lost / mismatched Session

stop / kill on a Session whose observation is `lost` / `mismatched` is also accepted.

1. Write `accepted` and `requesterSessionId` to `lifecycle.stop` or `lifecycle.kill`
2. Without having the Provider attempt anything, write `attempt: nothing to kill` and the latest observation (`lost` / `mismatched`) to `observed`
3. In the same transaction, write `end` (the acceptance time), `endKind: unknown`, and `exitCode: null` to the Session, and free the slot

Issuing a kill means a person has decided to "stop tracking this Session". The Provider is not called, and a different entity at the place recorded as `mismatched` stays as it is.

## Interpreting the end

The facts settled about the end accumulate in the following order, and outcome is derived from them on every read. The derivation rules are in [outcome](../concepts/glossary.md#outcome).

1. At launch: `startKind`, `ownership`, and the Actor's `kind` are settled. This determines how to read an end without a request
2. At a stop request: `accepted` of `lifecycle.stop` / `lifecycle.kill` is settled. If it was accepted before `end`, the end is read as requested
3. At the end: when the Provider observes `gone`, `end`, `endKind` (`stopped` if requested, otherwise `exited`), and `exitCode` are settled. A Session closed from `lost` / `mismatched` is settled with `endKind: unknown` and `exitCode: null`
4. At read: outcome is derived from `endKind`, `exitCode`, `ownership`, and `kind`, and [attention](../concepts/glossary.md#attention) is derived from outcome and the observation of the current Session

When an `owned` Job ends without a request and `exitCode` is null, the outcome is `unknown`; it is distinguished from the `unknown` of `endKind: unknown` by the `endKind` and `exitCode` shown alongside. The end of an `attached` Session is `ended` regardless of kind, and the outcome of a Session closed with `endKind: unknown` is `unknown` regardless of ownership.

## Scheduler supervision

The daemon runs the Scheduler in a separate task and checks it every 15 seconds.

- When the Scheduler does not wake 2 minutes after the scheduled time of the tick it is waiting for, or when it ends for a reason other than shutdown, the daemon logs the reason and restarts it. The restart count and the last reason appear in `daemon status`
- The wait before a restart starts at 1 second and doubles each time the Scheduler stops within 60 seconds of starting, up to 60 seconds. After a Scheduler that ran for 60 seconds or more, it returns to 1 second
- On restart, for each enabled Schedule, all ticks from the tick it was waiting for until now are written to `schedule.missed`. A Schedule the stopped Scheduler had not planned (such as one enabled / resumed after the last wake) is treated as having waited for the first tick after the last wake. For every Schedule, the ticks after the time it was last enabled / resumed or changed are counted with the current cron and written. A tick that already has a Session or `schedule.skipped` is not written. Overdue ticks and `schedule.missed` count ticks while the daemon is running
- All times are measured in wall clock. The Scheduler re-reads the wall clock at least every 30 seconds while waiting. If at a check the wall clock has advanced much more than the elapsed time (OS sleep, etc.), it waits another 2 minutes from that time before judging
- An overdue tick is a tick for which no Session, `schedule.skipped`, or `schedule.missed` exists 60 seconds after its scheduled time

## Recovery after a daemon restart

While the daemon is stopped, Sessions keep running on the Provider and records remain in the store.

1. Restore Actors (including Schedules), Sessions, records, and Messages from the store. A Session with `end` stays closed, and a Session without `end` becomes the current Session of its Actor
2. Have the Provider re-observe the entities of Sessions without `end`. The result is `running` if the same entity as the recorded Placement's incarnation is running, `relocated` if it moved from the recorded place, `gone` if it ended, `lost` if the entity is lost, and `mismatched` if a different entity is at the recorded place; an observation record is written only when the kind differs from the previous one. For a Session with an accepted stop / kill that the Provider has not attempted, the Provider attempts that request (kill takes precedence) instead of re-observing
3. A Session observed as `gone` closes as `stopped` / `exited` depending on whether a stop was requested. A `lost` / `mismatched` Session stays current and appears in attention; it closes at the next launch request or, if `owned`, when stop / kill is accepted
4. An `owned` Session without an incarnation cannot be matched, so the Provider records `lost` and it is handled as in 3. Resources that were created remain on the Provider. A Session that has an incarnation and was before `startSettled` is re-observed as in 2, and if `running` is observed, that time becomes `startSettled`
5. For each Schedule, next run is derived from `cron`, `timezone`, and `enabled` relative to the current time

A Session token is valid on the conditions that "the Session has no `end`" and "it is the last token issued for the Session", and it stays valid across daemon restarts. Child processes of a Session launched before the restart can keep calling the daemon API with the same token. Issuing and verifying tokens is described in [auth.md](auth.md).

## Sub Agent Session

A Sub Agent inside a Runtime is a Session projected from the transcript's `start` / `end` / `parentSession`, and its lifecycle, observation, and ownership belong to the parent Session. admission, stopping, and interpreting the end are all done against the parent Session, and the parent Session receives launch requests and stop requests.
