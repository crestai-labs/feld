# Plugin

This document collects the contract under which Feld core installs, discovers, and invokes Plugins. A Plugin is a component that connects a supported product to Feld core, and there are two kinds: [Runtime Adapter](../concepts/glossary.md#runtime-adapter) (`runtime`) and [Execution Provider](../concepts/glossary.md#execution-provider) (`provider`). The bundled implementations are described in [adapter/claude-code.md](adapter/claude-code.md), [adapter/codex.md](adapter/codex.md), [provider/tmux.md](provider/tmux.md), and [provider/herdr.md](provider/herdr.md).

## Contents

- [Overview](#overview)
- [Shape of a plugin](#shape-of-a-plugin)
  - [Directory and manifest](#directory-and-manifest)
  - [Invoking an operation](#invoking-an-operation)
- [Installation and selection](#installation-and-selection)
  - [Installation and discovery](#installation-and-discovery)
  - [Supported versions](#supported-versions)
  - [Selection from launchSpec](#selection-from-launchspec)
- [What core provides](#what-core-provides)
- [Runtime Adapter](#runtime-adapter)
  - [manifest](#manifest)
  - [operation](#operation)
  - [Helpers running beside the Runtime](#helpers-running-beside-the-runtime)
  - [Common shape of a transcript entry](#common-shape-of-a-transcript-entry)
  - [Transcript states](#transcript-states)
- [Execution Provider](#execution-provider)
  - [manifest](#manifest-1)
  - [operation](#operation-1)
  - [Passing the launch environment](#passing-the-launch-environment)
  - [Provider-specific fields of Placement](#provider-specific-fields-of-placement)
- [Division of work on attach](#division-of-work-on-attach)

## Overview

A plugin is a directory holding a manifest and one executable. core runs the executable with a fixed operation name, passes the request as JSON on stdin, and reads the JSON on stdout as the response. core owns the rules for records, admission, tokens, the Console, and metrics; the plugin owns product-specific knowledge (building launch arguments, the shape of transcript entries, identifying and observing resources). A plugin does not touch the store and uses only the input core passes and the daemon's API.

## Shape of a plugin

### Directory and manifest

```
$FELD_HOME/plugins/<kind>/<name>/
  plugin.json    manifest
  plugin         executable that core invokes
  …              files the plugin uses (helpers, skills, templates, etc.)
```

The manifest has `name`, `kind` (`runtime` or `provider`), `label` (display name), and kind-specific fields. `name` matches the directory name, `kind` matches the parent directory name, and `name` follows the same grammar as an Actor's `id`.

```json
{ "name": "claude-code", "kind": "runtime", "label": "Claude Code", "stopInput": ["/exit", ""], "ownedSettings": ["otelHeadersHelper", "statusLine"] }
```

### Invoking an operation

core runs `<dir>/plugin <operation>` and passes the request on stdin and the response on stdout, each as a single JSON object.

- Exit 0 means the plugin answered. Negative answers such as `gone` or `matched: false` also exit 0
- A non-zero exit means the plugin could not answer, and it writes the reason to stderr. core treats this as `unknown` for an observation, and for a launch as a failure in which the existence of the entity cannot be determined ([launch in provider/tmux.md](provider/tmux.md#launch))
- The time limit for one operation is 30 seconds. core kills a plugin that has not finished by the limit and treats it as not having answered. The answer is complete when the plugin exits; core does not wait even if a process the plugin started keeps stdout open
- A plugin started by core inherits only the stdin, stdout, and stderr fds
- The environment contains `FELD_HOME` and `FELD_PLUGIN_DIR` (the plugin's own directory), and inherits the caller's environment as is. `FELD_API` is set when the caller knows the daemon's API, and removed from the environment when it does not (when invoked without the daemon, as in Session token resolution or `feld init`)
- Some operations are called by the daemon, and some are called by the CLI within the caller's environment (`self`, `identity`, `attached`, `match`, `attach`, `candidates`, `prepare`, `restore`)

A bundled plugin's executable answers `--version` with one line `feld-plugin-<name> <version>` on stdout and exits 0. The version equals that of `feld` from the same build, and [`feld doctor`](../../guide/cli.md#feld-doctor) checks that they match.

The request and response fields for each operation are listed in the tables below. A plugin may freely add fields not in the tables, and core does not read them.

## Installation and selection

### Installation and discovery

- `feld init` extracts the plugins chosen from the bundled ones into `$FELD_HOME/plugins/`. A non-bundled plugin places a directory of the same shape there
- The bundled Runtime Adapters are `claude-code` and `codex`, and the bundled Execution Providers are `tmux` and `herdr`. What `feld init` selects without asking (`--yes`) is the bundled plugins whose executables are found on that machine's PATH (`claude-code` if no Runtime Adapter is found), and it presents the rest as options ([cli.md](../../guide/cli.md#feld-init))
- At startup, the daemon replaces installed bundled plugins (those whose directory and manifest `name` and `kind` match a bundled one) with the contents of its own version. Files are written to a temporary file and swapped in by rename, so the hooks and monitors of running Sessions launch either the pre- or post-replacement executable as is
- At startup, the daemon reads `$FELD_HOME/plugins/` and lists installed plugins in `GET /v1/runtimes` and `GET /v1/providers`. Adding and updating plugins takes effect with `feld daemon restart`
- Directories whose manifest cannot be read and directories without an executable are excluded from installed and recorded in the daemon's log

### Supported versions

The products the bundled plugins connect work from the following versions onward. The same applies on macOS and Linux.

| Product | Minimum | Command that reads the version |
|---|---|---|
| tmux | 3.2 | `tmux -V` |
| herdr | 0.9.0 | `herdr --version` |
| Claude Code | 2.1.181 | `claude --version` |
| Codex | 0.156.0 | `codex --version` |

The version is read as the first word in the command's answer that starts with a digit, split by `.` into a sequence of numbers, and characters after each number (the `a` in `3.2a`) are ignored. An answer without a word starting with a digit (`next-3.7`, `openbsd-7.5`, etc.) is treated as meeting the minimum. [`feld init`](../../guide/cli.md#feld-init) and [`feld doctor`](../../guide/cli.md#feld-doctor) compare the product versions of installed plugins against the minimum. The pre-release CI (manual run of `.github/workflows/ci.yml`) verifies everything at these minimum versions.

### Selection from launchSpec

- `runtime` is the `name` of an installed Runtime Adapter, and `provider` is the `name` of an installed Execution Provider. Names not installed at the time an Actor is created or updated are rejected
- `runtimeSettings` is passed to the Runtime Adapter and `providerOptions` to the Execution Provider, as JSON. core does not read their contents; the plugin defines their meaning. `providerOptions` is an object the CLI builds from `--<provider>-<key> <value>`, so `--tmux-socket feld` becomes `{"socket": "feld"}`
- `env` is added to the Session's launch environment. For a Job, core passes it to the Provider; for an Agent, the Runtime Adapter's `launch` includes it in the response's `env` and passes it to the Provider. For the same name, variables Feld sets ([What core provides](#what-core-provides) and those the Runtime Adapter sets) take precedence, and fields through which the Runtime defines the environment of its child processes in its own settings (Claude Code's `runtimeSettings.env`, Codex's `runtimeSettings.shell_environment_policy`) are layered on top of `env`
- When `--provider` is omitted, the provider named by the prefix of the option is selected. If there is no such option, the provider whose `self` answers a selector from the caller's environment is selected; failing that, if exactly one is installed, that one is selected. If none of these determine it, the request is rejected
- A Session's `execution.label` is the `label` in the Runtime Adapter's manifest

## What core provides

- The daemon's API and a per-Session token
- Environment variables passed to the Runtime. core prepares the `FELD_*` variables other than `FELD_SESSION_ID`, and the Runtime Adapter sets `FELD_SESSION_ID` and `OTEL_*` (`OTEL_*` is built from the Collector's endpoint). For an `owned` Session, the Runtime Adapter's `launch` includes these in `env`; for a Runtime a person launches, `feld env` puts `FELD_HOME`, `FELD_API`, and `OTEL_*` into the shell
  - `FELD_API`: the API URL
  - `FELD_SESSION_TOKEN`: the Session token
  - `FELD_SESSION_ID`: the Session's id. Components running on the environment's token use it for the per-Session mailbox lock and the [spool](spool.md) directory. The Runtime Adapter sets it in `launch`'s `env` and in components launched with the environment's token (Claude Code's `bin/hook` and `bin/monitor`, Codex's queue helper)
  - `FELD_ACTOR`: the Actor's `name`
  - `FELD_WORKSPACE`: the Workspace
  - `FELD_HOME`: Feld's state directory
  - `OTEL_*`: telemetry destination and headers
- The path of the per-Session directory `$FELD_HOME/sessions/<sessionId>/runtime/` (`launch`'s `sessionDir`). The Runtime Adapter creates it owner-only and places Session-specific files there
- Token resolution. The `feld` CLI and the bundled plugins' helpers call the implementation of [the resolution order in auth.md](auth.md#runtime-adapter-resolution-order) (`feld_core::session_token`) in-process. `feld self token` prints its answer
- The self index. It stores the tokens of `attached` Sessions, and core builds its key from a provider's `name` and the selector the provider's `attach` answered (same shape as `self`), or from a Runtime Adapter's `name` and the selector the Runtime Adapter's `self` answered
- Capture of `program` stdout / stderr. For the Console's log view, core wraps the command in a capture wrapper before passing it to the provider. The wrapper removes from the environment variables starting with `CLAUDE` (other than `CLAUDE_CONFIG_DIR`) that the daemon or the provider's server inherited from Claude Code, so that the program's parent is Feld

## Runtime Adapter

### manifest

- `stopInput`: the sequence of exit inputs delivered to the Runtime on `feld session stop`. The Provider delivers them one at a time as lines only while the entity remains, so a Runtime that asks for exit confirmation also declares the answer to the confirmation (for Claude Code, `/exit` and an empty input that answers the confirmation). If the entity remains after the whole sequence is delivered, the Provider delivers the sequence again from the start (the sequence is delivered at most 3 times in total). If absent, the Provider requests exit the same way as for `program`
- `ownedSettings`: the list of top-level `runtimeSettings` keys the Adapter builds itself. core rejects `runtimeSettings` containing these keys when an Actor is created or updated (`POST …/actors`, `PATCH`)
- `metrics`: the mapping from instrument names the Runtime exports to Feld metrics. The key is the instrument name and the value is a metric name from [metrics.md](metrics.md); for a metric made of multiple values, `<metric>.<part>` points down to the part (`size`, `used_percent`, `resets_at`). Multiple instruments may satisfy the same metric. core owns the meaning of attributes and the aggregation rules

```json
{
  "claude_code.token.usage": "token_usage",
  "claude_code.api_request": "api_request_latency_p95",
  "feld.claude_code.context_window.used_percent": "context_window_utilization.used_percent",
  "feld.claude_code.rate_limit.resets_at": "quota.resets_at"
}
```

### operation

| operation | Caller | request | response |
|---|---|---|---|
| `launch` | daemon | `session`, `actor`, `workspace`, `workdir`, `runtimeSettings`, `trustWorkdir` (the launchSpec value; false if absent), `env` (launchSpec's `env`), `sessionDir`, `token`, `collector` (the Collector's OTLP endpoint; null if absent) | `command` (argv), `env` (environment variables passed to the Runtime), optional `identity` (Runtime identity determined before launch) |
| `match` | CLI and components running beside the Runtime | `process` (`pid`, `startedAt`, `executable`, `argv`), `collector` (the Collector's OTLP endpoint; null if absent) | `matched` (whether the process is this Runtime), `telemetryPrepared` (whether the process started in a form that lets its telemetry be attributed to the Session; true if absent), `telemetryReason` (why `telemetryPrepared` is false and the conditions under which telemetry arrives; the CLI shows it on the `telemetry` line) |
| `identity` | CLI | `process` (the Runtime process of an entity pointed to from outside; same shape as `match`; absent when called from inside the Runtime) | `identity` (Runtime identity obtained from the environment or from `process`), or null if none is obtained |
| `attached` | CLI | `session`, `token`, `identity` (the Runtime identity attach delivered; null if absent) | `notes` (a list of strings the CLI shows as `note:` lines) |
| `self` | CLI and components running beside the Runtime (within the caller's environment) | — | `selector` (the session on this Runtime's side to which the caller belongs), or null if it belongs to none. `notes` (what a person can do so that the location is determined without asking them; the CLI shows these when it asks a person for a pane) |
| `prepare` | CLI | `apply` (writes if true), `collector` | `entries` (`path`, `key`, `current` (current value), `next` (value to write), `action`), `prepared` (whether every `action` is `keep`), `notes` (a list of strings the CLI shows as `note:` lines), `daemonNotes` (`notes` about the running daemon's environment; not shown when `feld init` started or restarted the daemon in the CLI's environment), `telemetryPrepared` (whether an already running Runtime can attribute telemetry to the Session; true if absent), `telemetryReason` (same as `match`) |
| `restore` | CLI | `apply` (restores if true), `record` (the prepare record at `$FELD_HOME/prepared/<runtime>.json`; null if absent) | `entries` (`path`, `key`, `current`, `next` (value to restore), `action`), `notes` |
| `transcript` | daemon | `identity`, `after` (cursor; null for the beginning), `limit` | `state` (`missing` / `lost` / `readable`, or `invalid-cursor` if `after` points to no transcript entry), `reason`, `records`, `next` (cursor for the continuation) |

`self` is answered by a Runtime that may run its Runtime-side session in a process other than the foreground process of the Provider's resource, and it returns a selector from the caller's environment alone. core checks with `match` whether the Runtime process running that session is among the caller's ancestors ([auth.md](auth.md#runtime-adapter-resolution-order)). Since it does not read the process table, it can answer even inside a sandbox. Other Runtimes answer null, and their child processes look up the self index with the Provider's `self`.

`launch` places Session-specific files in `sessionDir` and builds `runtimeSettings` into the form passed as the Runtime's settings. `identity` is the set of the Runtime-side session id, the transcript location, and `model`, and is recorded under the same rules from both the `launch` response and `POST /v1/self/runtime`. If the Runtime determines its identity before launch, `launch` returns it; if after launch, a helper delivers it.

`attached` starts, with the attach token, the helpers that begin running after attach without waiting for the Runtime's mechanisms to fire. A Runtime for which helpers launched by the Runtime's mechanisms suffice answers without launching anything.

`prepare` sets up a Runtime that a person launches themselves so that it can be attached. The attach preparation for [Claude Code](adapter/claude-code.md#preparing-for-attach) and [Codex](adapter/codex.md#preparing-for-attach) corresponds to this. The CLI writes it in [`feld init`](../../guide/cli.md#feld-init) and checks it in `feld actor attach`. What is written depends only on the feld version and the Feld home path (Codex's [spool](spool.md#codex-preparation)); daemon-specific values (API, Collector endpoint, Plugin path) arrive from the environment of the shell that launched the Runtime ([`feld env`](../../guide/cli.md#feld-env)).

`action` is the handling for each entry. With `apply: false` the Adapter writes nothing and answers only `entries`; with `apply: true` it writes as the entries say. `chain` and `replace` change a person's values and `add` widens the Runtime's permissions, so the CLI obtains consent before calling with `apply: true`. `action` is a protocol word; for a person, the CLI renders `current` as a `-` line and `next` (or `path` if absent) as a `+` line ([`feld init`](../../guide/cli.md#feld-init)).

| action | Meaning |
|---|---|
| `write` | There is no current value; write Feld's value |
| `keep` | The current value is Feld's value; keep it as is |
| `chain` | Replace the current value by passing it to Feld's helper's `--chain` |
| `replace` | Keep the current value in the prepare record and replace it with Feld's value. For a directory, keep the person's files with other names and write Feld's files over files with the same name (overwritten files are not kept in the record) |
| `add` | Add Feld's elements to the current value (a list) and keep the other elements |

The Adapter places the prepare record at `$FELD_HOME/prepared/<runtime>.json`; it holds the values `prepare` wrote and the values before replacement. `restore` is called by [`feld uninstall`](../../guide/cli.md#feld-uninstall); it compares the record the CLI passes with the current settings and reverts `prepare`'s entries. With `apply: false` it writes nothing and answers only `entries`.

| action | Meaning | `prepare` action |
|---|---|---|
| `remove` | Delete an entry Feld wrote, or an element Feld added | `write`, `add` |
| `restore` | Revert to the pre-replacement value in the record | `replace` |
| `unchain` | Revert to the value of the `--chain` argument and keep other fields | `chain` |
| `keep` | Keep entries that a person has changed from the record's value and directories holding the person's files, and show them in `notes` | (a person's change) |

### Helpers running beside the Runtime

A helper is an executable the plugin holds, launched by the Runtime's mechanisms (hooks, monitors, settings commands), as a wrapper of the `command` `launch` returns, or from `attached`. A helper calls the daemon's API directly with the Session token. It obtains the token through [core's token resolution](#what-core-provides). When the launching side passes a token through the environment to a helper launched as a separate process, the helper uses only that token (Codex's [queue helper](adapter/codex.md#queue-helper)). While no token is available (before attach), it waits without calling the Session APIs.

A helper notifies the daemon that lines were added to the transcript file by the arrival of `POST /v1/self/runtime`, and core reports that arrival as a change in the Console's cursor. The plugin configures the Runtime's mechanisms so that a helper launches on the events that add lines to the transcript file (a person's input, completion of a response, tool execution, etc.). core calls `transcript` for Console reads and for deriving turns and Sub Agent Sessions, and does not call it periodically.

### Common shape of a transcript entry

`transcript` projects the lines of the transcript file into transcript entries of the following shape. The Console, [turn](../concepts/glossary.md#turn), and [Sub Agent Session](../concepts/glossary.md#sub-agent-session) are drawn from this shape alone.

- `cursor`: a string pointing to the entry. The plugin defines it, and it is passed as is to `after`
- `at`: the entry's time
- `kind`: `input` (a person's input), `notification` (a Runtime notification, such as a Message notification), `response` (a model response not stopped at a tool call), `thinking`, `tool-call`, `tool-result`, `system` (a Runtime line added to the Conversation view), `other` (an entry that appears only in the All events view)
- `text`: the body for display. For a tool call, the name and input; for a tool result, the result
- `error`: whether the tool result reports a failure. Only `tool-result` has it
- `subAgent`: the id of the Sub Agent the entry belongs to. Only Sub Agent entries have it
- `subAgentEnded`: the id of the Sub Agent whose completion this entry indicates
- `state`: whether what this entry represents can be read. `lost` means the transcript file cannot be reached, and `text` gives the reason
- `raw`: the line exactly as the Runtime wrote it

The turn is derived from the last entry after skipping Sub Agent entries, `system`, and `other`: `waiting-input` if it is `response`, otherwise `responding`. A Sub Agent Session's `start` is the `at` of the first entry with `subAgent`, and its `end` is the `at` the parent Session's entry indicated with `subAgentEnded`, or the parent Session's `end` if none indicates it. A Sub Agent whose transcript file cannot be reached appears as one entry with `subAgent` and `state` `lost`, and becomes a Sub Agent Session whose Console cannot be read.

The Console's two views are drawn from the same list of entries.

- The Conversation view draws entries whose kind is not `other`. It draws `input` and `notification` distinctly, and draws `tool-call` paired with the following `tool-result`
- The All events view lists all entries as is in `cursor` order

### Transcript states

A transcript read returns the transcript file's state distinctly, and the Console draws each differently. core determines "location pending" from whether an identity exists, and the plugin's `transcript` returns the other three as `state`. In the three states where the transcript cannot be read, only records are listed in the form of the All events view.

- Location pending: waiting for the Runtime identity to arrive. The Console shows "transcript file location pending"
- No file (`missing`): the identity arrived, but the file does not exist. The same applies when the location's directory does not exist yet. For an open Session, the Runtime has not yet started writing the transcript file; for a closed Session, the Runtime either ended without writing the transcript file or deleted it. The Console shows "no transcript file", and for a closed Session it adds these two reasons. The event on which the Runtime starts writing the transcript file in an open Session is described per plugin
- Location lost (`lost`): the location itself cannot be reached (the container is gone, no permission), or reading the file fails. The Console shows it with the reason in `reason`
- Readable (`readable`): drawn in the Conversation view and the All events view

## Execution Provider

### manifest

There are no kind-specific fields.

### operation

| operation | Caller | request | response |
|---|---|---|---|
| `self` | CLI and components running beside the Runtime (within the caller's environment) | — | `selector` (the caller's own location as indicated by its environment; for tmux, the endpoint and pane), or null if the environment is not inside this provider |
| `resolve` | daemon (when an Actor is created or updated) | `options` (`providerOptions`), `caller` (the answer of the requesting CLI's `self`) | `options` (`providerOptions` with defaults filled in; saved in launchSpec) |
| `place` | daemon (when a launch is accepted) | `options`, `node`, `workdir` | `placement` (without incarnation), `summary` |
| `launch` | daemon | `placement`, `name` (the resource name core builds from the Session), `command`, `env`, `workdir` | `placement` (including incarnation), `observation`, `summary`, `attachCommand` |
| `observe` | daemon | `placements` (a list of placements) | `results` (`observation`, `summary`, `attachCommand` per placement, in the same order as the request) |
| `stop` | daemon | `placement`, `input` (the Runtime Adapter's `stopInput` list; null for `program` and for a Runtime without `stopInput`) | `attempt`, `observation` |
| `kill` | daemon | `placement` | `attempt`, `observation` |
| `cleanup` | daemon | `placement` | — |
| `attach` | CLI | `selector` (a specification of an existing entity built from `--<provider>-<key>`, or the answer of `self`) | `placement` (the `existing` resource and incarnation), `workdir`, `process` (the candidate Runtime in the foreground; `pid`, `startedAt`, `executable`, `argv`), `selector` (a selector of the same shape `self` answers from inside the resource), `summary`, `attachCommand` |
| `candidates` | CLI | `selector` (the answer of the caller's `self`; null if absent) | `candidates` (a list of existing resources attach can take. Each element has `options` (`providerOptions` pointing to the resource, passed again as `--<provider>-<key> <value>`), `summary`, `workdir`, `process` (the foreground process; `pid`, `startedAt`, `executable`, `argv`)) |

`observation` has `kind` (one of the six kinds of [observation](../concepts/glossary.md#observation)), `exitCode` (null if it cannot be reported), and `detail` (a human-readable note; for `relocated`, the current location); core adds the recording time. `attempt` is a string describing what stop / kill attempted, recorded as is in `lifecycle.stop` / `lifecycle.kill`. `summary` is the Placement's display (`window @7 (main:1, created)`, etc.), and `attachCommand` is the command a person uses to reach the entity (null for a provider that cannot derive it). The plugin also returns these two inside `placement`; core reads the display from the saved Placement and rewrites it each time `observe` answers anew.

`node` is only passed to the provider, and the plugin owns the path to a remote Node.

At daemon startup and in periodic observation every 250 ms thereafter, core groups the placements of Sessions without `end` by Provider and passes them to a single `observe` call. For a Session with a pending stop / kill, core retries that action instead of observing. Periodic observation is a safeguard for catching unrequested exits; when the current state is needed, observe with `POST /v1/sessions/{sessionId}/observe`.

### Passing the launch environment

`launch` passes `env` only to the pane's launch script, over a Unix domain socket ([token exposure prevention](auth.md#preventing-token-exposure)).

1. For each launch, the Provider creates an owner-only directory under a temporary directory (`/tmp` if the socket path does not fit in 103 bytes) and listens on a socket inside it
2. The launch script runs the Provider's executable as `<plugin> launch-env <socket> <Provider pid>` and `eval`s the `export` statements on its standard output. If it cannot receive them, the shell exits there and the program does not launch
3. The Provider writes the environment only to a connection whose peer is the same OS user and a descendant of that pane's process, and closes other connections. If the environment is not received by the deadline, the launch fails
4. `launch-env` outputs the environment, received through to the end, only when the socket's peer is the pid in argv and the same OS user
5. When the launch finishes, the Provider removes the directory

The peer is read with `LOCAL_PEERPID` and `getpeereid` on macOS, and `SO_PEERCRED` on Linux.

### Provider-specific fields of Placement

core holds the Placement's `provider`, `node`, and `workdir`, and saves the other fields as is from the JSON the plugin returned as `placement`. The answer of `place` is written in the same transaction as the Session record and replaced by the answer of `launch`. The incarnation that identifies the entity is also included here, and core passes it as is to `observe`, `stop`, `kill`, and `cleanup`. The Provider puts the Runtime process an `attached` Session attached to in `process`, and the process of the `<command>` an `owned` Session launched in `program`; core reads these two when matching on attach. For display, the `summary` and `attachCommand` held by the Placement are used. Per-provider fields are described in [Execution Provider in the glossary](../concepts/glossary.md#execution-provider).

## Division of work on attach

`feld actor attach` calls two plugins in order within the CLI and passes only the identified result to the daemon.

1. Determine the selector: from `--<provider>-<key>` if present, otherwise (`--self`) from the provider's `self`
2. Obtain `placement`, `workdir`, and `process` with the provider's `attach`. With `--self`, check that `process` is among the CLI's ancestors. If this cannot be confirmed, and the Runtime Adapter's `self` answers a selector and a process that passes `match` is among the CLI's ancestors, the Runtime is running its Runtime-side session outside `process`, so a person chooses the resource
   - If `--<provider>-<key>` is present, take that resource without the ancestor check and proceed to 3
   - Otherwise, reject, showing the resources among the provider's `candidates` whose `workdir` matches the CLI's cwd and whose `process` passes `match`, together with a retry command pointing to each and the Runtime Adapter's `notes`. Even a single candidate is not chosen automatically. If `candidates` cannot answer, show that fact and the form of the retry
3. Confirm with the Runtime Adapter's `match` that `process` is the `--runtime` Runtime
4. Call the Runtime Adapter's `prepare` with `apply: false` to check the person's Runtime preparation, and show `notes` in the output. attach continues even without preparation
5. Call `POST …/attach` with `runtime`, `placement` (including `process`), and `runtimeTelemetryPrepared` (the `telemetryPrepared` of 3's `match` and 4's `prepare`), and write the token in the response to the self index. The key is built from the Runtime Adapter name and its `self` selector if a person chose the resource, otherwise from the provider name and the `selector` answered by 2's `attach`
6. Deliver the Runtime identity answered by the Runtime Adapter's `identity` with `POST /v1/self/runtime`. With `--self`, the caller's environment is inside the Runtime, so `process` is not passed; for an entity pointed to from outside, 2's `process` is passed. If the answer is null, that Runtime's helper delivers the identity
7. Call the Runtime Adapter's `attached` with 5's token and the identity delivered in 6, and show `notes` in the output
