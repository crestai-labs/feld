# Codex Adapter

This document describes how the [Runtime Adapter](../../concepts/glossary.md#runtime-adapter) `codex` launches `codex`, what it runs beside the Runtime, and how it projects Codex's rollout file onto the Session, the Console, and Sub Agent Sessions. The contract with core and the Runtime-independent projection rules follow [plugin.md](../plugin.md), the API follows the Runtime API (`Self`) in [OpenAPI](../../../guide/daemon-api.openapi.yaml), and the commands follow [`cli.md`](../../../guide/cli.md). Token distribution and resolution are in [auth.md](../auth.md), the meaning of a Message read in [message-delivery.md](../message-delivery.md), telemetry attribution in [otel.md](../otel.md), and aggregation into metrics in [metrics.md](../metrics.md).

## Contents

- [Overview](#overview)
- [The launching side](#the-launching-side)
  - [Launch arguments and environment](#launch-arguments-and-environment)
  - [Initial prompt](#initial-prompt)
  - [Per-Session configuration](#per-session-configuration)
  - [Workdir trust](#workdir-trust)
  - [Associating the Runtime identity](#associating-the-runtime-identity)
  - [Exit input](#exit-input)
- [The side beside the Runtime](#the-side-beside-the-runtime)
  - [hook](#hook)
  - [queue helper](#queue-helper)
  - [Telemetry header](#telemetry-header)
  - [Rollout gauges](#rollout-gauges)
- [`feld` inside the sandbox](#feld-inside-the-sandbox)
- [Projection onto the Console](#projection-onto-the-console)
  - [Projecting transcript entries](#projecting-transcript-entries)
  - [Rollout file state](#rollout-file-state)
- [Projecting Sub Agent Sessions](#projecting-sub-agent-sessions)
- [metrics](#metrics)
- [When attached](#when-attached)
  - [Preparing for attach](#preparing-for-attach)
  - [The attach flow](#the-attach-flow)
- [Known limitations](#known-limitations)
  - [The sandbox reverts when the app-server restarts](#the-sandbox-reverts-when-the-app-server-restarts)

## Overview

The Adapter consists of two sides. The launching side is `launch`, which the daemon calls when it starts an `owned` Session; it assembles the argv and environment that start Codex's interactive TUI inside the Placement. The side beside the Runtime consists of two parts, the hook and the queue helper. The hook runs as a child process that Codex starts through its own hook mechanism, and the queue helper runs as a long-lived process started by a hook or by `attached` after attach. Both call the daemon directly over the Feld home socket with the Session token, and the Collector with the telemetry token ([preventing token exposure in auth.md](../auth.md#preventing-token-exposure)).

Codex calls a Session a thread, uses the thread id as the Runtime-side session id, and writes the conversation history to a rollout file (transcript file). The rollout file is `sessions/<YYYY>/<MM>/<DD>/rollout-<start time>-<thread id>.jsonl` under Codex's configuration root (`CODEX_HOME`, or `$HOME/.codex` if unset). The Adapter handles only the connection to the Runtime. The side that creates the Actor decides the model, approval policy, and sandbox through `runtimeSettings`, and the Adapter passes them to Codex as they are.

## The launching side

### Launch arguments and environment

When starting an Actor whose launchSpec `runtime` is `codex`, the daemon calls the Adapter's `launch`, and the Adapter assembles the argv and environment to pass to the Provider as follows.

1. The argv is `codex --no-daemon`, followed by the [Per-Session configuration](#per-session-configuration) as a series of `-c <key>=<value>` (plus `--dangerously-bypass-hook-trust` when [Workdir trust](#workdir-trust) is declared), with the [Initial prompt](#initial-prompt) at the end. `--no-daemon` runs the thread inside the TUI process ([The sandbox reverts when the app-server restarts](#the-sandbox-reverts-when-the-app-server-restarts))
2. The environment holds the launchSpec `env`, with `FELD_API`, `FELD_SESSION_TOKEN`, `FELD_SESSION_ID`, `FELD_ACTOR`, `FELD_WORKSPACE`, and `FELD_HOME` placed on top of it. Codex passes the environment on to the shell the model calls according to `shell_environment_policy`, and `runtimeSettings.shell_environment_policy.set` layers on top of that. The hook and helpers inherit Codex's environment as is and read these. The `FELD_*` variables, including `FELD_SESSION_TOKEN`, also reach the `feld` the model calls from the shell, and `feld` reaches the daemon as the `owned` Session with the token from the environment ([`feld` inside the sandbox](#feld-inside-the-sandbox))
3. The environment's `OTEL_EXPORTER_OTLP_HEADERS` holds the telemetry token header, and `OTEL_METRIC_EXPORT_INTERVAL` holds the metrics export interval (milliseconds; [time and temporality in metrics.md](../metrics.md#time-and-temporality)). Codex reads the header at startup and attaches the same header to every export for the life of the process ([Telemetry header](#telemetry-header))
4. `workdir` is passed to the Provider as in the launchSpec, and Codex makes it the workspace root. If the daemon's environment has Codex's configuration root (`CODEX_HOME`), the Adapter passes it in the launch environment. Otherwise Codex determines the configuration root from the environment of the Provider's server (the server-side `CODEX_HOME`, or `$HOME/.codex` if unset). The login and the person's `config.toml` stay as they are

### Initial prompt

Codex creates the rollout file on the first input and fires the SessionStart hook there. `launch` places the initial prompt at the end of the argv so the first turn starts right after launch, aligning the arrival of the Runtime identity and the start of the [queue helper](#queue-helper) with the start of the Session. The text is the Adapter's fixed template: it states that this is a Feld Session and that Messages arrive as the input of the next turn after a turn ends, and asks the model to answer `ready`, end the turn, and wait. `initialPrompt` in `runtimeSettings` replaces the text.

### Per-Session configuration

`runtimeSettings` represents entries of Codex's `config.toml` as JSON, and the Adapter maps each top-level entry's value to TOML and turns it into `-c <key>=<value>`. Codex reads `-c` as a layer that takes precedence over `config.toml`, so the per-Session configuration reaches Codex without placing a file in Codex's configuration root.

```json
{ "model": "<available-model>", "approval_policy": "never", "sandbox_mode": "workspace-write" }
```

The Adapter's own entries are `hooks` and `otel`. The Adapter owns these keys as [`ownedSettings`](../plugin.md#manifest) in the manifest, and rejects them at Actor creation and update if they appear in `runtimeSettings`. `initialPrompt` is a key the Adapter reads; it is not mapped to `-c` and becomes the [Initial prompt](#initial-prompt).

- `hooks`: declares, for the events the [hook](#hook) uses (`SessionStart`, `UserPromptSubmit`, `PostToolUse`, `Stop`, `SubagentStart`, `SubagentStop`, `PostCompact`, `SessionEnd`), the hook executable in the Adapter's directory as the command. The shape is the same as Codex's `hooks.json`. `hooks.state` holds the trust for the declared hooks and for those groups of the [`feld init` hook](#preparing-for-attach) in the configuration root's `hooks.json` that remain as `feld init` wrote them
- `otel`: points the logs and metrics exporters at the Collector's loopback endpoint over OTLP/HTTP (protocol `binary`). Codex uses the endpoint as the POST target as is, so it includes the per-signal path. If the daemon has no Collector, the exporter is `none`

```console
-c 'hooks.state={"/<session-flags>/config.toml:session_start:0:0"={trusted_hash="sha256:<hash>"},…}'
-c 'hooks.SessionStart=[{matcher="",hooks=[{type="command",command="<plugin-dir>/bin/hook"}]}]'
-c 'otel.exporter={otlp-http={endpoint="http://127.0.0.1:<collector-port>/v1/logs",protocol="binary"}}'
-c 'otel.metrics_exporter={otlp-http={endpoint="http://127.0.0.1:<collector-port>/v1/metrics",protocol="binary"}}'
-c 'check_for_update_on_startup=false'
-c 'sandbox_workspace_write.writable_roots=["/work","$FELD_HOME/spool/<sessionId>"]'
```

Codex asks about an update check at startup, but no person in the Session answers it, so the Adapter sets `check_for_update_on_startup` to false.

`sandbox_workspace_write.writable_roots` goes in the last `-c`, limiting the spool the sandbox can write to the Session's directory ([the spool directory in spool.md](../spool.md#spool-directories)). The value starts from the array `runtimeSettings` declares for `sandbox_workspace_write.writable_roots`, or otherwise the array in the configuration root's `config.toml`; the Adapter removes the spool and paths inside it, and appends the Session's directory (`spool/<sessionId>` in the resolved Feld home). This key can be declared, and the other elements of the declared array arrive as they are.

When the sandbox mode (`sandbox_mode` in `runtimeSettings`, or otherwise the value in `config.toml`) is `workspace-write`, the Adapter declares the permission profile `feld-session` and selects it with `default_permissions`. Codex gives `default_permissions` precedence over `sandbox_mode`. The profile extends Codex's `:workspace`; network follows the value of `sandbox_workspace_write.network_access` (false if unset); filesystem sets the writable roots to `write`, `$FELD_HOME/admin`, `$FELD_HOME/spool`, and `$FELD_HOME/tokens` to `deny`, and the Session's directory to `write`. The narrower path wins, so the sandbox reads and writes only its own directory and cannot read other Sessions' requests and responses, Admin files, or the tokens of `attached` Sessions. If `runtimeSettings` declares `default_permissions` or `permissions`, the declared profile takes effect as is, and the Adapter declares no profile.

```console
-c 'permissions.feld-session={extends=":workspace",network={enabled=false},filesystem={"/work"="write","$FELD_HOME/admin"="deny","$FELD_HOME/spool"="deny","$FELD_HOME/spool/<sessionId>"="write","$FELD_HOME/tokens"="deny"}}'
-c 'default_permissions="feld-session"'
```

Before running a hook for the first time, Codex asks the person for trust. Trust binds the hook's hash, as `trusted_hash`, to a key made of the hook's source (`/<session-flags>/config.toml` for a `-c` hook, the file's path for a `hooks.json` hook), the event, and the positions of the group and the hook. The hash is the SHA-256 of the JSON of the hook, with the event name (in the `session_start` form), `matcher` (omitted for `UserPromptSubmit` and `Stop`), `timeout` (1 for `SessionEnd`, 600 otherwise), and `async` filled in, and its keys in order. The Adapter places the trust for its own hooks in `-c` in this form, and Codex runs them without asking. Trust goes only in `-c`, never in the person's `config.toml`.

### Workdir trust

Codex reads the project `.codex/` (`config.toml` and `hooks.json`) in the workdir and above it only when the workdir is trusted. For any other workdir, it asks whether to trust it before the first input. Admin decides whether to trust the workdir with `trustWorkdir` in the launchSpec.

- `trustWorkdir` is true: the Adapter marks the workdir trusted with `-c 'projects={"<workdir>"={trust_level="trusted"}}'` (Codex resolves the directory before looking up the entry, so both the resolved path and the given path are set) and adds `--dangerously-bypass-hook-trust` to the argv. The `.codex/` configuration and hooks take effect without confirmation
- `trustWorkdir` is false: the Adapter leaves the workdir's trust to the `config.toml` in Codex's configuration root. For a workdir trusted there, Codex waits for input immediately, and the `.codex/` hooks do not run until the person trusts them. For any other workdir, Codex shows the trust prompt and waits

A Session stopped at the prompt has no Runtime identity yet, so the [unlinked mark](../../concepts/glossary.md#unlinked-mark) remains. Admin enters the pane with the `attach_command` shown by `feld session get` and answers, marks the workdir trusted in `config.toml`, or declares `trustWorkdir` and starts the Session again.

### Associating the Runtime identity

The Session and the Codex run are linked by the Runtime identity that arrives through `POST /v1/self/runtime`. It carries three things, the thread id, the location of the rollout file, and `model`, and the hook takes all of them from the JSON Codex passes on the hook's standard input (`session_id`, `transcript_path`, `model`). Codex fixes the thread id at startup and the rollout file appears when the first turn starts, so `launch` returns no identity, and the SessionStart hook that fires there delivers it. For an `attached` Session, `feld actor attach` also delivers it, with `model` set to null. `--self` takes it from the thread id Codex places in the child process's environment (`CODEX_SESSION_ID`, or `CODEX_THREAD_ID` if unset; in a Sub Agent's tool, the parent's thread) and the rollout file under the configuration root's `sessions/` whose name has that id; an attach from outside takes it from the rollout file the foreground Codex has open ([The attach flow](#the-attach-flow)). The daemon side follows [plugin.md](../plugin.md#operation).

### Exit input

For `feld session stop`, the Provider delivers the manifest's [`stopInput`](../plugin.md#manifest) to the pane. Codex reads `/quit` as a request to exit without confirmation, and the TUI shows command suggestions for input that starts with `/` and selects the suggestion on the first newline, so `stopInput` consists of two inputs: `/quit` and an empty input that sends it. Codex fires the SessionEnd hook on exit, and the hook stops the queue helper.

## The side beside the Runtime

The two parts inherit Codex's environment and run outside Codex's sandbox, obtaining the token by the [resolution order in auth.md](../auth.md#runtime-adapter-resolution-order). Even when the sandbox closes the network for the model's tools, the two parts reach the daemon and the Collector. The target Session is given by the environment's `FELD_SESSION_ID` for `owned`, and by the self index for `attached`.

### hook

The hook is a command declared in the [Per-Session configuration](#per-session-configuration) or in `$CODEX_HOME/hooks.json`. Codex starts it in an independent process group on every hook event, and it reads the needed fields from the JSON on standard input and delivers them to the daemon and the Collector. The hook inherits Codex's environment as is. Codex places no variable identifying the thread in the hook's environment, so before resolving the token the hook sets the input's `session_id` as `CODEX_SESSION_ID` and looks up the self index with the same thread key as the tools ([The attach flow](#the-attach-flow)). For a Sub Agent's event, `session_id` is the parent's thread.

The JSON on standard input always has `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `model`, and `permission_mode`, plus per-event fields. UserPromptSubmit, PostToolUse, and Stop have `turn_id`; SubagentStart and SubagentStop have the Sub Agent's thread id (`agent_id`) and `agent_type`; SessionEnd has `reason`; PostCompact has `trigger`.

- SessionStart: delivers the Runtime identity with `POST /v1/self/runtime`. It also tells the model, through `hookSpecificOutput.additionalContext` on stdout, that the JSON lines the queue helper inserts are Messages addressed to it, and how to treat them by sender kind ([message-delivery.md](../message-delivery.md#reading-by-the-helper)). It returns this regardless of whether a token exists, so it also works in a Session that later attaches a Codex prepared by `feld init`
- UserPromptSubmit, PostToolUse, Stop, SubagentStart, SubagentStop: resend the same Runtime identity. Its arrival signals new transcript entries, and UserPromptSubmit and Stop correspond to [turn](../../concepts/glossary.md#turn) boundaries. A Session whose SessionStart ended before Feld got involved, as with attach, gets its identity from this resend
- Stop, PostCompact: send the [Rollout gauges](#rollout-gauges) to the Collector
- SessionStart, UserPromptSubmit, Stop: start the [queue helper](#queue-helper) if it is not running
- SessionEnd: asks the queue helper to exit

The hook exits 0 when it finishes sending, and exits 1 when it cannot resolve Feld home. It also exits 0 when it cannot reach the daemon and when it cannot obtain the Session token (before attach), keeping Codex moving. In that case the identity stays undelivered on the Feld side, and arrives when a hook fires after the token becomes available.

### queue helper

The helper is a long-lived process that the [hook](#hook) or `attached` after attach ([The attach flow](#the-attach-flow)) starts as a process in a separate session (`setsid`); it runs for the whole Session, free of the timeout Codex imposes on hooks. The starter passes the Session token, the Session id, the target thread, and its rollout file. A hook takes these from its input's `session_id` and `transcript_path`, and `attached` from the Runtime identity it delivered. The starter also passes the configuration root holding the rollout file as `CODEX_HOME`, so `codex queue` reaches the thread regardless of the configuration root in the starter's environment. The helper long-polls the mailbox with `GET /v1/self/messages` and, one Message at a time, records `queued` with `POST /v1/self/messages/queued`, turns it into one JSON line, inserts it into Codex's queue with `codex queue --thread <thread id> --message <line>`, and records the read with `POST /v1/self/messages/read` once a turn takes it in and it appears in the rollout file. The read flow and the line format are in [message-delivery.md](../message-delivery.md#reading-by-the-helper).

Codex passes a queued Message to the model as the input of the next turn, and if a turn is in progress, after that turn ends. A Message a turn takes in appears in the rollout file as a user message, and the Console renders it distinctly from human input by the shape of its body ([Projecting transcript entries](#projecting-transcript-entries)). The helper reads the records after the end of the rollout file as of the time it queued the Message, and records the read when it finds a user message whose body is that Message's line. Until then the Message stays `waiting`, and the helper takes out no next Message. A queued Message cannot be withdrawn, so deleting a Message that has `queued` is rejected. A Message whose `queued` record is rejected (a Message deleted in the meantime) is not queued. A Message that `codex queue` rejects has its `queued` withdrawn with `DELETE /v1/self/messages/{message}/queued` and is handed out again on the next take. The `queued` of a Message whose helper stopped before withdrawing it is cleared when the next helper requeues it, or when the Session closes as `stopped` or `exited`. A Message that arrives while a turn is blocked by a long tool call shows up in the Actor's waiting count and unread-mail mark. `codex queue` reaches only a thread that has a rollout file, and a Message that could not be inserted records no read and is handed out again on the next take.

One helper runs per Session. The helper takes a per-Session lock in `$FELD_HOME` and exits if it cannot take it. A helper started by `attached` waits until the lock is free when it cannot take it. Even if the helper restarts before recording a read, a Message without a read is returned again on the next take. Notification is at least once, and each Message has one read record. The helper exits when the SessionEnd hook asks it to, when the Session closes and the token is revoked, and when `$FELD_HOME` disappears. While waiting for the take-in, it also checks for token revocation with `GET /v1/self`.

### Telemetry header

Codex builds the OTLP header only once, from the startup environment `OTEL_EXPORTER_OTLP_HEADERS`, so the telemetry token is the same for the life of the process. For an `owned` Session, `launch` sets this environment, and the Collector attributes telemetry to the Session by this header. The header of an `attached` Session carries no token, so the metrics taken from Runtime telemetry are `unavailable` ("Codex telemetry is attributed only in owned Sessions"), and only context window utilization and quota, taken from the [Rollout gauges](#rollout-gauges), arrive. The attribution rules are in [otel.md](../otel.md), and how the metrics `status` is decided is in [metrics.md](../metrics.md#status-and-source).

### Rollout gauges

Codex's OTel lacks the context window and the account's rate limits, so the hook extracts them from the rollout file and sends them as telemetry.

- The Stop and PostCompact hooks build the following [gauges](../metrics.md#adapter-gauges) from the last `token_count` event in the rollout file at `transcript_path`, and send them to the Collector over OTLP/HTTP. The destination is the Collector endpoint kept in `$FELD_HOME`, and the header carries the telemetry token
- The hook also exits 0 when it cannot reach the Collector and when it cannot obtain the token (before attach)

| gauge | value | `token_count` event field |
|---|---|---|
| `feld.codex.context_window.size` | tokens | `info.model_context_window` |
| `feld.codex.context_window.used_percent` | 0–100 | The percentage ratio of `info.last_token_usage.total_tokens` to `info.model_context_window`, each minus a baseline of 12,000 tokens (clamped to 0–100). It matches the value Codex itself displays |
| `feld.codex.rate_limit.used_percent` | 0–100, attributes `window` and account | `rate_limits.<window>.used_percent` |
| `feld.codex.rate_limit.resets_at` | epoch seconds, attributes `window` and account | `rate_limits.<window>.resets_at` |

The account attributes of the rate limit gauges are set when `CODEX_HOME/auth.json` (`$HOME/.codex/auth.json` if unset) has the `tokens` of a ChatGPT login: `account.id` is `tokens.account_id` (or `chatgpt_account_id` in `tokens.id_token` if absent), and `account.name` is `email` in `tokens.id_token`.

`rate_limits` has two windows, `primary` and `secondary`, and `secondary` is null depending on the plan. The `window` value is decided by the window length (`window_minutes`): 300 is `five_hour`, 10080 is `seven_day`. A Codex logged in with a ChatGPT subscription has `rate_limits` after its first API response.

## `feld` inside the sandbox

The `feld` the model calls from the shell runs inside Codex's sandbox. The `workspace-write` sandbox denies loopback TCP and reading the process table, so the CLI reaches the daemon through the [spool](../spool.md). For an `owned` Session's sandbox, `launch` passes the Session's spool directory ([Per-Session configuration](#per-session-configuration)); for a Codex the person starts, [Preparing for attach](#preparing-for-attach) adds `spool/shared`, which `attached` Sessions share, to `writable_roots` in `config.toml`. The sandbox can keep outbound communication closed, and in a `network_access: true` sandbox the CLI reaches the daemon over TCP ([choosing the route in spool.md](../spool.md#choosing-the-path)). The spool carries only Session tokens, so attach, being an Admin operation, runs outside the sandbox ([The attach flow](#the-attach-flow)).

A Codex sandbox that reaches Feld is not closed with respect to Feld operations. The operations callable with the Session token (including starting a Job that grants open) are callable from inside the sandbox, and the daemon performs them outside the sandbox ([holes in the sandbox in spool.md](../spool.md#the-sandbox-hole)). A `network_access: true` sandbox also reaches the tmux and herdr pane sockets and can run commands outside the sandbox, so Feld reports this combination before launch (same section).

The `workspace-write` sandbox reads all of Feld home. A sandbox configuration that can restrict reads by path denies `$FELD_HOME/admin`, separating Sessions from the files only Admin reads ([assumptions and trust boundaries in auth.md](../auth.md#assumptions-and-trust-boundary)). The CLI reads only the part outside `admin/`, so it works under this configuration. For an `owned` Session, the permission profile `launch` declares denies it, and also makes only the Session's own spool directory readable ([Per-Session configuration](#per-session-configuration)). The sandbox of a Codex the person starts keeps the person's configuration, and reads all of Feld home and the other Sessions' requests and responses in `spool/shared/`.

## Projection onto the Console

### Projecting transcript entries

The Adapter's `transcript` reads the rollout file (JSONL) the Runtime identity points to, one line per entry, and returns the entries projected into the [common shape](../plugin.md#common-shape-of-a-transcript-entry). `cursor` is built from the entry's time, the rollout file, and the line number, and arranges the parent's and Sub Agents' entries into one sequence in time order. `raw` is the line Codex wrote (`timestamp`, `type`, `payload`) itself. The kind is decided from `type` and `payload.type` as follows.

- `message` in `response_item`: role `user` is `input`, or `notification` if the body is a queue helper JSON line (the same Message appears with a `client_id` in a `UserMessage` item of `event_msg` `item_completed`); role `assistant` is `response`; role `developer` is `other`. Context Codex itself inserts (such as `environment_context`) is also a role `user` message; input a person gave is one whose `content_item_kinds` has an item starting with `user.` (including one without `content_item_kinds`), and anything else is `other`
- `reasoning` in `response_item`: `thinking`
- Tool calls in `response_item` (`function_call`, `custom_tool_call`, `web_search_call`, etc.): `tool-call`. Their outputs (`function_call_output`, `custom_tool_call_output`): `tool-result`. `error` is decided from the output's own report (`Script failed` on the first line for an exec script, or items such as `success` or `error` for structured output)
- `agent_message` in `response_item` (messages between a Sub Agent and its parent): `system`
- `task_started` and `task_complete` in `event_msg`: `system`
- `session_meta`, `turn_context`, `compacted`, `world_state`, `inter_agent_communication_metadata`, and other `event_msg` (`token_count`, `item_completed`, `thread_settings_applied`, etc.): `other`
- An entry in a Sub Agent's rollout file has that thread id in `subAgent`. An entry in the parent's rollout that marks the Sub Agent's completion has `subAgentEnded` ([Projecting Sub Agent Sessions](#projecting-sub-agent-sessions))

### Rollout file state

The [transcript state](../plugin.md#transcript-states) is common. A `missing` open Session is one where Codex is waiting for the first input, and the Console shows that the first input creates the transcript file. For a closed Session, either it ended without input or Codex deleted an old rollout file.

## Projecting Sub Agent Sessions

A Sub Agent that Codex starts inside a Session has its own thread. Its rollout file is `rollout-<start time>-<Sub Agent's thread id>.jsonl` in the same dir as the parent's, and its `session_meta` has `parent_thread_id` and `thread_source: "subagent"`. `transcript` gets `agent_thread_id` from the `SubAgentActivity` item (`kind` is `started`) of `item_completed` in the parent's rollout, and mixes that rollout file's records into the parent's sequence with `subAgent`. A `SubAgentActivity` record whose `kind` is one of `completed`, `interrupted`, `failed`, or `closed` has `subAgentEnded`. Starting and operating a Sub Agent remain in the parent's rollout as `function_call`s whose `namespace` is `collaboration` (`spawn_agent`, `wait_agent`, `interrupt_agent`, `close_agent`). The rules by which core projects Sub Agent Sessions from this follow [plugin.md](../plugin.md#common-shape-of-a-transcript-entry) and [session-lifecycle.md](../session-lifecycle.md#sub-agent-session).

## metrics

The metrics in [metrics.md](../metrics.md) are taken from the following sources. The metrics and logs exporters are enabled in the [Per-Session configuration](#per-session-configuration), and the context window and quota come from the hook's [Rollout gauges](#rollout-gauges). The `source` of `reported` is "Codex Runtime telemetry" for metrics taken from OTel, and "Codex rollout" for metrics taken from gauges. Codex's Sums and histograms are always delta.

| metric | source | how the value is taken |
|---|---|---|
| token usage | metric `codex.turn.token_usage` (histogram) | Map the `sum` per `token_type` to the [four token kinds](../metrics.md#the-four-token-kinds) by the mapping below |
| estimated cost | metric `codex.turn.cost_microusd` | Divide the value by 1,000,000 |
| API errors | event `codex.api_request`, metric `codex.websocket.request` | The count of `success=false` events plus the sum of values |
| prompt cache hit rate | token usage | The formula in [four token kinds](../metrics.md#the-four-token-kinds) |
| API request latency p95 | metric `codex.turn.ttft.duration_ms` (histogram) | Time to first token per turn |
| tool calls | metric `codex.tool.call` | Sum of values whose `tool` is other than `exec` |
| context window utilization | gauge `feld.codex.context_window.used_percent` / `.size` | value |
| active time | — | `n/a` ("Not provided by Codex") |
| quota | gauge `feld.codex.rate_limit.used_percent` / `.resets_at` | Value per attribute `window` (`five_hour` / `seven_day`) |

| kind | `token_type` |
|---|---|
| `input` | `input` − `cached_input` |
| `output` | `output` |
| `cache_read` | `cached_input` |
| `cache_creation` | `cache_write_input` |

`input` includes `cached_input`, and `output` includes `reasoning_output`.

Codex emits request records differently depending on the transport it uses to talk to the model. Over HTTP it emits the log event `codex.api_request` (`success`, `http.response.status_code`, `duration_ms`, `endpoint`); over websocket it emits the metric `codex.websocket.request` (sum, `success`) and `codex.websocket.request.duration_ms`. `codex.api_request` also includes requests with no model response, so it counts only API errors. Codex's log events carry the instrument name in the `event.name` attribute. `codex.tool.call` emits two points for one shell execution, with `tool` as `exec_command` and `exec`, so `exec` is excluded from the count.

`codex.turn.cost_microusd` is not exported by a Codex logged in with a ChatGPT subscription, and estimated cost is then `unavailable` ("No correlated Runtime usage record"). Cost is exported after a delay from the end of the turn, and what arrives after the Session closes is excluded from the Session's metrics by the rule for [closed Sessions in otel.md](../otel.md#closed-sessions).

## When attached

### Preparing for attach

To make a Codex the person starts themselves attachable, set up the following. For an `owned` Session the Adapter assembles these at launch, so no preparation is needed.

1. Keep the daemon running with `feld daemon start`
2. Place the [hook](#hook) declarations in `$CODEX_HOME/hooks.json`, place the `feld` skill in `$CODEX_HOME/skills/feld/`, and add `$FELD_HOME/spool/shared` to `writable_roots` in `[sandbox_workspace_write]` of `$CODEX_HOME/config.toml` ([preparing Codex in spool.md](../spool.md#codex-preparation)). The hook commands in `hooks.json` point at the parts in `skills/feld/bin/`, and at run time the parts execute `plugins/runtime/codex/plugin` in `FELD_HOME` (`~/.feld` if unset). If that Plugin is absent, they exit doing nothing. Other entries in `hooks.json` are kept as they are. Codex reads these from the next Session on, and asks the person for trust the first time it runs a hook. The trust the person grants remains in `[hooks.state]` of `config.toml` as a per-hook hash, and after that the hooks run without asking
3. Start `codex` inside a Provider pane (tmux or herdr). `feld actor attach --self` assembles the Placement from the environment the Provider's `self` reads (`TMUX` and `TMUX_PANE` for tmux, `HERDR_SOCKET_PATH` and `HERDR_PANE_ID` for herdr). When Codex runs the thread on the managed app-server (`codex app-server --listen unix:// --managed-daemon`), the person chooses the Provider pane ([The attach flow](#the-attach-flow)). A Codex started with `codex --no-daemon` runs the thread inside the pane

Step 2 is the Adapter's `prepare`; [`feld init`](../../../guide/cli.md#feld-init) writes it, and `feld actor attach --runtime codex` only checks it. The write target is the CLI's configuration root (`CODEX_HOME`, or `$HOME/.codex` if unset). It adds while keeping existing entries, so an entry's `action` is `write`, `add` (`writable_roots`), `keep`, or `replace` (a `skills/feld/` holding the person's files, and a `writable_roots` element that opens the whole spool). `skills/feld/` belongs to Feld when every file in it is a file Feld writes (`SKILL.md` and the parts in `bin/`) and its content is the current version's or matches the digest in the prepare record. If there are files of the person (files with other names, or files whose content the person changed), it is `replace`: the files with other names are kept and Feld's files are written. In `hooks.json`, if Feld's hooks are in the group with the same matcher with the current version's declaration, it is `keep` even if the same group has the person's hooks. The spool path added to `writable_roots` differs per Feld home, and the rest of the content is determined by the feld version alone. The daemon the parts call is determined by `FELD_HOME` in the shell that started Codex (`eval "$(feld env)"`, [cli.md](../../../guide/cli.md#feld-env)). Codex's telemetry is attributed to a Session only through the header in the startup environment, so an `attached` Session's metrics consist of the [Rollout gauges](#rollout-gauges), and the Adapter's `match` answers `telemetryPrepared` as false.

The prepare record is placed owner-only at `$FELD_HOME/prepared/codex.json`. It holds the prepared configuration roots, for each configuration root where spool was added the path of the added element, what was added (elements, keys, tables), and the SHA-256 digests of the files written to `skills/feld/` ([preparing Codex in spool.md](../spool.md#codex-preparation)).

[`feld uninstall`](../../../guide/cli.md#feld-uninstall) reverts the preparation with the Adapter's `restore`. It reverts in the configuration roots the record holds and the CLI's configuration root: it marks as `remove` a `skills/feld/` that belongs to Feld and the hooks in `hooks.json` that point at the parts in `skills/feld/bin/`, and keeps the person's hooks even in the same group. Groups, events, and files that consisted only of Feld's hooks are also removed. A `skills/feld/` holding the person's files is left as `keep`, and `notes` points out those files. From `writable_roots`, only in configuration roots where the record says spool was added, it marks as `remove` the element at the path the record holds. For a record from an earlier version with no path, that element is `spool`. Spool elements the person wrote, and the spool of a home with no record, are kept. The `action` of `restore` is `remove`, and `keep` for a `skills/feld/` holding the person's files. Codex manages the trust hashes in `[hooks.state]`, so `notes` points them out. The configuration root's preparation is shared regardless of `FELD_HOME`, so `feld uninstall` in a home with no record also removes it, and the CLI lists that attach from other Feld homes also stops ([cli.md](../../../guide/cli.md#feld-uninstall)).

### The attach flow

A Codex the person started themselves is attached from the `feld` skill inside Codex, or by the person from a shell outside the sandbox with `feld actor attach --runtime codex --tmux-pane <pane>`. With the skill, the model calls `feld actor attach --self --runtime codex` from the shell. In this flow, only the Adapter's side beside the Runtime is at work. The CLI rejects a call from inside the sandbox and instructs running it outside the sandbox ([guidance when the spool cannot be written in spool.md](../spool.md#guidance-when-the-cli-cannot-write)). The model asks Codex for approval to run outside the sandbox, and with a Codex that does not ask for approval, the person attaches with `--tmux-pane`.

1. The CLI identifies the Placement and the Runtime process. With `--self`, it assembles the Placement from the selector the Provider's `self` answered and identifies the Runtime process from its own ancestor chain; with `--tmux-pane`, it takes the pane's foreground process ([provider/tmux.md](../provider/tmux.md#attach), [provider/herdr.md](../provider/herdr.md#attach)). The Adapter's `match` checks that the candidate is Codex. It is Codex if either the executable name or the basename of argv[0] is `codex`, or if either is `node` and the basename of argv[1] is `codex` or `codex.js` (the npm distribution's `bin/codex` is a symlink to `codex.js`; Node 24 on Linux names the main thread `MainThread`, and the executable name becomes that). It then calls `POST …/attach` with `runtime: codex`. The Session's `execution` has `type` `agent-runtime`, `label` "Codex", and `command` null
2. The CLI saves the Session token from the response in the self index. From then on, this Codex's child processes get the token from the self index ([auth.md](../auth.md#attached-sessions))
3. The CLI delivers the Runtime identity the Adapter's `identity` answered with `POST /v1/self/runtime`. The rollout file's location is the path with the file resolved (the same form the Codex hook gives), and `model` is null
   - `--self`: the thread id is the environment's `CODEX_SESSION_ID` (or `CODEX_THREAD_ID` if unset), and the rollout file is the file under the configuration root's `sessions/` whose name has that id
   - Attach from outside: Codex keeps the thread's rollout file open while running, so among the files open by the Runtime process from step 1 and its descendants (Codex under `node` in the npm distribution), the Adapter takes a file of the form `sessions/<YYYY>/<MM>/<DD>/rollout-<start>-<thread id>.jsonl` whose first record is not a Sub Agent thread's (`thread_source` is not `subagent`), and gets the thread id from the name. If there are several, it takes the one written last
   - When the thread id or rollout file cannot be obtained (before the first input), it delivers nothing and says so in the output
4. The CLI calls the Adapter's `attached` with the token and the identity from step 3, and the Adapter starts the [queue helper](#queue-helper). A Codex waiting for input fires no hook, so from this start the helper inserts Messages into Codex's queue. When re-attaching changes the token, it waits until the old token's helper is rejected and releases the lock, then takes over delivery. When step 3 could not deliver the identity, it does not start the helper and says in the output that the hook firing on the next input starts it
5. The next hook to fire (UserPromptSubmit or Stop) delivers the same identity with `model`, and the Session's `model` is set. The Stop hook sends gauges from its next firing

When Codex runs the thread on the managed app-server, the tools and hooks are children of the app-server, not descendants of the pane's TUI. The environment variables pointing at the pane (`TMUX_PANE` for tmux, `HERDR_PANE_ID` for herdr) are those of the process that started the app-server. In that case the check in step 1 fails, the Adapter's `self` answers the thread, and the CLI proceeds as follows.

- The Adapter's `self` answers `CODEX_SESSION_ID` (or `CODEX_THREAD_ID` if unset) as `{"thread": <id>}`. Core checks with `match` whether a Codex process (the in-process TUI or the app-server) is in the caller's ancestor chain ([auth.md](../auth.md#runtime-adapter-resolution-order)). `CODEX_SESSION_ID` is the thread that started the tree, which in a Sub Agent's tool is the parent's thread, so a Sub Agent gets the parent's token. `notes` points out that when started with `codex --no-daemon`, `--self` finds the pane by itself
- `--self` without a pane rejects the call, showing as candidates the Provider panes with Codex in the foreground in the CLI's cwd, each with a retry command that points at it with `--tmux-pane` (`--herdr-pane` for herdr) ([cli.md](../../../guide/cli.md#feld-actor-attach)). The model shows the candidates to the person, and the person chooses the pane showing the TUI
- With `--self --tmux-pane <pane>` (`--self --herdr-pane <pane>` for herdr), the CLI records the pane's foreground TUI as the Runtime process, writes the token under the thread key ([Runtime key](../auth.md#attached-sessions)), and shows on a `note:` line that the pane was given with `--self`. The Session's lifetime and observation follow the TUI

When the thread changes (`/new`, `/resume`, `/fork`), Codex emits no SessionEnd for the old thread, and the new thread's SessionStart comes on the first input. What follows depends on the token's key.

- Runtime key: the new thread's tools and hooks have no token until the person re-attaches from that thread
- Provider key: the new thread's tools and hooks are also descendants of the same TUI, so they get the old token and keep running as the same Session. The Runtime identity stays at the recorded pair of thread and rollout file, and the new thread's identity the hook delivers is rejected ([`POST /v1/self/runtime`](../../../guide/daemon-api.openapi.yaml)). So `feld session log` reads the old thread's rollout file, the queue helper keeps inserting Messages into the old thread, and Codex takes them in on turns of the old thread that are not shown on screen

Re-attaching to the same TUI is [token recovery](../auth.md#token-recovery), and re-attaching from the new thread aligns the Session with the new thread. The old token is rejected, so the old queue helper exits, and the queue helper started by the re-attach's `attached` takes over delivery to the new thread.

- Runtime key: re-attach from the new thread with `--self --tmux-pane <pane>`. The key is moved to the new thread, and the CLI delivers the new thread's Runtime identity
- Provider key: re-attach from the new thread with `--self`. A `codex --no-daemon` TUI keeps the old thread's rollout file open too, so a re-attach from outside aligns with the thread written last, as in step 3

Codex reads `hooks.json` at startup. When `feld init` has written step 2 of the preparation, the hook and queue helper run from the next Codex started. `feld init` says so in its output. Attach is accepted even for a Codex without the preparation, and the CLI suggests `feld init` on a `note:` line.

## Known limitations

### The sandbox reverts when the app-server restarts

When the launch argv has neither `--no-daemon` nor `-c`, Codex's TUI runs the thread on the managed app-server if one is running. `launch` adds `--no-daemon`, so an `owned` Session's thread runs inside the TUI and is not subject to this limitation. It applies to `attached` Sessions whose Codex runs the thread on the app-server.

- **When it happens**: when the app-server restarts (`codex app-server daemon restart`, auto-update), the threads running on it lose the permission flags given at startup (`--yolo`, `--dangerously-bypass-approvals-and-sandbox`) and run in the directory's default sandbox. That is `workspace-write` following `[sandbox_workspace_write]` in `config.toml` for a trusted directory, and `read-only` otherwise
- **What happens**: the hook and queue helper keep running outside the sandbox. The `feld` the model calls reaches the daemon through the [spool](../spool.md) under `workspace-write`, and fails under `read-only` because it cannot write to the spool. The CLI shows that the sandbox is `read-only` and how to fix it ([guidance when the spool cannot be written in spool.md](../spool.md#guidance-when-the-cli-cannot-write))
- **How to notice**: Permissions in the TUI's `/status` changes. `CODEX_SANDBOX=seatbelt` appears in the tools' environment, `CODEX_SANDBOX_NETWORK_DISABLED=1` also appears if the network is closed, and `CODEX_PERMISSION_PROFILE=:read-only` also appears under `read-only`. `permission_mode` in the hook input does not change
- **How to fix**: start a new thread with `/new`, or exit the TUI and reopen the same thread with `codex resume <thread id> --yolo`. A Codex started with `codex --no-daemon` runs the thread inside the TUI and is not subject to this limitation
