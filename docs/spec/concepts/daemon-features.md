# Feld daemon features

This document organizes Feld's features in abstract form. The API, CLI and Web UI are built on these features.

Features split into those for a Controller, which operates Actors and Sessions from outside Feld, and those for a Runtime, which runs next to a Session. A verb is written as `<noun> <verb>`; the CLI expresses it as `feld <noun> <verb>`, and the Control API as a method on a resource. Verbs for a Runtime take `self` as their subject, which refers to the caller's own current Session. `delete` is a soft delete that records `deletedAt`, and deleted items are reached through a `list` that includes deleted ones. Search is expressed as a filter on `list`. Actor and Session features describe their behavior separately for each kind of target (Agent / Job), and `-` marks a feature that the kind does not cover.

## Contents

- [Features for a Controller](#features-for-a-controller)
  - [Actor](#actor)
  - [Session](#session)
  - [Workspace](#workspace)
  - [Message](#message)
  - [Metrics](#metrics)
  - [Daemon](#daemon)
  - [Browser](#browser)
- [Features for a Runtime](#features-for-a-runtime)
  - [self](#self)

## Features for a Controller

### Actor

| Feature | Verb | Agent | Job |
|---|---|---|---|
| Create | `actor create` | Creates a new Agent in Feld. Whether to have a Session is selectable | Creates a new Job in Feld. Immediate start is selectable |
| Update | `actor update` | Changes the Agent's name, (next) launch location, and model | Changes the Job's name, launch schedule, command, and so on. Includes enabling and disabling the Schedule (`pause` / `resume`) |
| Delete | `actor delete` | Soft deletes an existing Agent | Soft deletes an existing Job |
| Get details | `actor get` | Shows the Agent's details | Shows the Job's details |
| List (search) | `actor list` | Searches Agents | Searches Jobs |

### Session

| Feature | Verb | Agent | Job |
|---|---|---|---|
| Start | `actor start` | Creates a new Agent Session in Feld. Nothing is created implicitly. Passing admission is required, and a spawned record is left | Starts a Job. Nothing is created implicitly. Passing admission is required, and a spawned record is left |
| Attach | `actor attach` | Attaches an Agent Session to a running Runtime process and registers it with Feld. Also creates the Actor if it does not exist. (Re)issues the Session token at this point * | - |
| Restart | `actor restart` | Stops the running Agent Session and starts the next generation in the same slot | Stops the running Job and starts the next generation in the same slot |
| Stop | `session stop` | Ends a running `owned` Agent Session. Sub Agents are also covered, though some may not support it | Ends a running Job |
| Kill | `session kill` | Forcibly ends a running `owned` Agent Session. Sub Agents are also covered, though some may not support it | Forcibly ends a running Job |
| Get details | `session get` | Shows the Session's metadata (launch parameters such as model and execution platform, spawner, stop / kill requester, outcome, latest Observation) | Shows the Job Session's metadata (launch parameters such as model and execution platform, spawner, stop / kill requester, outcome, latest Observation) |
| List (search) | `session list` | Searches Agent Sessions | Searches Job Sessions |
| View log | `session log` | Views the Agent Session's conversation log (transcript) | Views the Job's application log (stdout / stderr) |
| View metrics | `session metrics` | Shows the Agent Session's metrics | - |
| View records | `record list --session` | Shows the records Feld itself left about the Agent Session (who requested the spawn, acceptance and reading of Messages, and so on) | Shows the records Feld itself left about the Job run (whether the run was skipped, the reason for the skip, stop / kill requester, and so on) |
| List records (search) | `record list` | Searches Session records. They are the source of the arrows in the Timeline | Searches Job run records |
| Re-fetch observation | `session observe` | Has the Observation (liveness and so on) re-fetched along User -> Daemon -> Execution Provider | Has the Observation (liveness and so on) re-fetched along User -> Daemon -> Execution Provider |

\* Reissuing a token matters only for an Attached Session. The token is kept in the self index and behaves like a cache on the machine, so it needs reissuing when lost. Reissuing is done by an `attach` to the same Runtime process.

### Workspace

| Feature | Verb | Description |
|---|---|---|
| Create | `workspace create` | Creates a new Workspace |
| Update | `workspace update` | Updates a Workspace |
| Delete | `workspace delete` | Soft deletes a Workspace |
| Get details | `workspace get` | Shows a Workspace's details |
| List (search) | `workspace list` | Searches Workspaces |

### Message

| Feature | Verb | Description |
|---|---|---|
| Send | `message send` | Sends a Message to an Actor |
| Delete | `message delete` | Deletes a sent Message that has not yet been handed to the Runtime |
| Get details | `message get` | Shows a Message's details |
| List (search) | `message list` | Searches Messages (from, to, and so on) |

### Metrics

| Feature | Verb | Description |
|---|---|---|
| View Runtime metrics | `runtime metrics` | Gets the aggregate of Session metrics per Runtime and the account's quota (Claude Code's 5-hour / 7-day, and so on) |

### Daemon

| Feature | Verb | Description |
|---|---|---|
| Start | `daemon start` | Starts the Daemon |
| Stop | `daemon stop` | Stops the Daemon |
| Restart | `daemon restart` | Stops and starts the Daemon again. Applies added and updated Plugins |
| View status | `daemon status` | Shows the Daemon's status |

### Browser

Handles the browser token of the browser that opens the Web UI.

| Feature | Verb | Description |
|---|---|---|
| Issue pairing code | `ui pair` | Issues a short-lived, one-time code that the browser exchanges for a browser token |
| List | `browser list` | Lists the valid browser tokens |
| Revoke | `browser revoke` | Revokes a browser token |

## Features for a Runtime

### self

| Feature | Verb | Description |
|---|---|---|
| Get Session | `self get` | Shows the caller's own Session (Agent / Job) |
| Get token | `self token` | Shows the caller's own Session token (Agent / Job) |
| Receive Telemetry | `self telemetry` | Receives Telemetry. self is determined by the telemetry token in the telemetry header (Agent) |
| Identify Runtime | `self runtime` | Delivers its own session id, model name, and transcript file location to Feld from a Claude Code or Codex hook (Agent) |
| Receive Message | `self message recv` | Takes unread Messages from the caller's mailbox in FIFO order. Can wait for arrival with long-poll (Agent / Job) |
| Mark Message read | `self message read` | Marks a received Message as read (Agent / Job) |
