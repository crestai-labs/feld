# Claude Code Adapter

This document describes how the [Runtime Adapter](../../concepts/glossary.md#runtime-adapter) `claude-code` launches `claude`, what it runs beside the Runtime, and how it projects Claude Code's transcript file onto the Session, the Console, and Sub Agent Sessions. The contract with core follows [plugin.md](../plugin.md), the API follows the Runtime API (`Self`) in the [OpenAPI](../../../guide/daemon-api.openapi.yaml), and the commands follow [`cli.md`](../../../guide/cli.md). Token distribution and resolution are in [auth.md](../auth.md), the meaning of a Message read is in [message-delivery.md](../message-delivery.md), telemetry attribution is in [otel.md](../otel.md), and aggregation into metrics is in [metrics.md](../metrics.md).

## Contents

- [Overview](#overview)
- [The launching side](#the-launching-side)
  - [Launch arguments and environment](#launch-arguments-and-environment)
  - [Per-Session integration files](#per-session-integration-files)
  - [Associating the Runtime identity](#associating-the-runtime-identity)
  - [Workdir trust](#workdir-trust)
  - [Exit input](#exit-input)
  - [Sandbox reads](#sandbox-reads)
- [The side beside the Runtime](#the-side-beside-the-runtime)
  - [hook](#hook)
  - [Plugin Monitor helper](#plugin-monitor-helper)
  - [otelHeadersHelper](#otelheadershelper)
  - [status line helper](#status-line-helper)
- [Projection onto the Console](#projection-onto-the-console)
  - [Projecting transcript entries](#projecting-transcript-entries)
  - [Transcript file state](#transcript-file-state)
  - [Inserting records](#inserting-records)
- [Projecting Sub Agent Sessions](#projecting-sub-agent-sessions)
- [metrics](#metrics)
- [When attached](#when-attached)
  - [Preparing for attach](#preparing-for-attach)
  - [The attach flow](#the-attach-flow)

## Overview

The Adapter consists of two sides. The launching side is `launch`, which the daemon calls when it launches an `owned` Session; it assembles the launch arguments, the environment, and the per-Session integration files. The side beside the Runtime consists of four components: the hook, the Plugin Monitor helper, `otelHeadersHelper`, and the status line helper. Each runs as a child process that Claude Code launches according to its own settings, and calls the daemon directly over the Feld home socket with the Session token and the Collector with the telemetry token ([token exposure prevention in auth.md](../auth.md#preventing-token-exposure)). The directory passed to Claude Code's plugin mechanism (hereafter the Claude Code plugin) is one of the files the Adapter generates and passes to Claude Code.

The Adapter handles only the connection with the Runtime. The role, prompt, model, and tool policy are decided by whoever creates the Actor, through `runtimeSettings`, and the Adapter passes them to Claude Code as they are. What the Adapter delivers to the daemon are facts the Runtime holds (the Runtime-side session id, the transcript file location, `model`, and telemetry), and the daemon keeps these as records separate from the facts Feld records itself (read, spawned, lifecycle).

## The launching side

### Launch arguments and environment

When launching an Actor whose launchSpec `runtime` is `claude-code`, the daemon calls the Adapter's `launch`, and the Adapter assembles the argv and environment passed to the Provider as follows.

1. The argv is `claude` with `--settings <file>` and `--plugin-dir <dir>` pointing to the per-Session integration files. Model and permission choices arrive as the settings file, as written in `runtimeSettings` by whoever creates the Actor
2. The environment holds the launchSpec `env`, with `FELD_API`, `FELD_SESSION_TOKEN`, `FELD_SESSION_ID`, `FELD_ACTOR`, `FELD_WORKSPACE`, and `FELD_HOME` layered on top. The settings file `env` (`runtimeSettings.env`) is layered further on top in the environment of Claude Code and its child processes. Claude Code and its child processes (the hook, the helpers, and the `feld` the model calls from Bash) inherit this and learn their own Session
3. The environment tells Claude Code that Feld is its parent. When the Provider's server was launched from inside Claude Code, the panes that server opens inherit the markers the calling Claude Code places on its child processes (`CLAUDECODE`, `CLAUDE_CODE_CHILD_SESSION`, `CLAUDE_CODE_SESSION_ID`, `CLAUDE_CODE_SESSION_ATTENDED`, `CLAUDE_PID`), so the launch environment cancels them with empty values. Claude Code reads an empty value as absent. It also sets `CLAUDE_CODE_FORCE_SESSION_PERSISTENCE=1` so the Session's transcript file persists whatever the pane inherits
4. `workdir` is passed to the Provider as in the launchSpec. If the daemon's environment has a Claude Code config root (`CLAUDE_CONFIG_DIR`), the Adapter passes it as part of the launch environment. Otherwise Claude Code determines its config root from the environment of the Provider's server (the server's `CLAUDE_CONFIG_DIR`, or `$HOME/.claude` if absent)
5. `OTEL_*` in the environment enables Claude Code's telemetry and points the metrics and logs (event) exporters over OTLP/HTTP at the Collector's loopback endpoint ([otel.md](../otel.md#environment-passed-to-the-runtime)). [`otelHeadersHelper`](#otelheadershelper) attaches the header on every send. When there is no Collector, the settings file `env` disables telemetry (`CLAUDE_CODE_ENABLE_TELEMETRY=0`, `OTEL_METRICS_EXPORTER=none`, `OTEL_LOGS_EXPORTER=none`)

```console
CLAUDE_CODE_ENABLE_TELEMETRY=1
OTEL_METRICS_EXPORTER=otlp
OTEL_LOGS_EXPORTER=otlp
OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:<collector-port>
OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE=delta
OTEL_METRIC_EXPORT_INTERVAL=10000
```

### Per-Session integration files

The Adapter generates the files passed to Claude Code in the per-Session directory core provides (`$FELD_HOME/sessions/<sessionId>/runtime/`). The files are specific to the Session, and the working directory and the person's Claude Code settings stay as they are.

- settings file: carries the entries declared in `runtimeSettings` as they are, and adds the Adapter's own entries (`otelHeadersHelper`, `statusLine`) beside them. The Adapter owns these two top-level keys as the manifest's [`ownedSettings`](../plugin.md#manifest), and rejects them at Actor creation and update if they appear in `runtimeSettings`. `env` carries the `runtimeSettings` `env` with the launch environment's `FELD_*` and `OTEL_*` layered on top (the same name takes Feld's value). The file is owner-only
- Claude Code plugin: holds the manifest, the hook declaration (`hooks/hooks.json`), the monitor declaration (`monitors/monitors.json`), the executables of the four components, and skills such as `/feld attach`. The four components are executables in the Adapter's directory, separate from the `feld` CLI executable. In the plugin of an `owned` Session, `bin/hook` and `bin/monitor` place `FELD_SESSION_ID` in the environment before launching the component. An `attached` Session uses the same plugin ([Preparing for attach](#preparing-for-attach))

The Adapter-owned part of the settings file takes the following form. Claude Code runs the two values in a shell, so the helper path is written as one word with shell quoting (`'...'`).

```json
{
  "otelHeadersHelper": "'<plugin-dir>/bin/otel-headers'",
  "statusLine": { "type": "command", "command": "'<plugin-dir>/bin/status-line'" }
}
```

The Claude Code plugin's `hooks/hooks.json` declares all four events the [hook](#hook) uses.

```json
{
  "hooks": {
    "SessionStart": [ { "hooks": [ { "type": "command", "command": "${CLAUDE_PLUGIN_ROOT}/bin/hook" } ] } ],
    "UserPromptSubmit": [ { "hooks": [ { "type": "command", "command": "${CLAUDE_PLUGIN_ROOT}/bin/hook" } ] } ],
    "PostToolUse": [ { "hooks": [ { "type": "command", "command": "${CLAUDE_PLUGIN_ROOT}/bin/hook" } ] } ],
    "Stop": [ { "hooks": [ { "type": "command", "command": "${CLAUDE_PLUGIN_ROOT}/bin/hook" } ] } ]
  }
}
```

### Associating the Runtime identity

A Session and a Claude Code run are linked by the Runtime identity delivered with `POST /v1/self/runtime`. It carries three things: the Runtime-side session id, the transcript file location, and `model`. The hook takes them from the input Claude Code passes to the hook. Claude Code places `model` in the input only for SessionStart, so the other hooks use the `message.model` of the last response in the transcript file (an `assistant` line, excluding Sub Agent ones and `<synthetic>`) as `model`. Claude Code launches the Stop hook before writing that turn's response to the transcript file, so Stop rereads for up to 2 seconds until it finds the response. In an `attached` Session, `feld actor attach --self` also delivers the session id Claude Code places in its child processes' environment (`CLAUDE_CODE_SESSION_ID`), the transcript file named with that id under the config root's `projects/`, and the `model` of that file's last response ([The attach flow](#the-attach-flow)). If there is no response yet, `model` is null.

- The daemon records the delivered identity on the Session and uses it as the source of `model` in `GET /v1/sessions/{sessionId}` and of the Console and Sub Agent Session projections
- The session id and the transcript file location identify the Runtime and stay fixed once recorded. A different pair is rejected
- `model` is a current fact of the Runtime and takes each non-null value delivered. Null keeps the recorded value
- Resending the same content is idempotent, and hooks other than SessionStart can send it too
- `owned` and `attached` use the same path, and the daemon treats them the same regardless of the Session's ownership

Claude Code starts writing the transcript file after the first input, so it is normal for the file to be absent when the identity arrives. The Console draws this as "the file is absent" in [Transcript file state](#transcript-file-state).

### Workdir trust

In directories marked trusted in `projects` of the config root's `.claude.json` (`$HOME/.claude.json` if there is no `CLAUDE_CONFIG_DIR`) and below them, Claude Code waits for input right away. In any other workdir it first asks whether to trust it (default choice `No, exit`), and runs neither the hook nor the Plugin Monitor helper until it gets an answer. The Admin decides whether to trust a workdir, and the Adapter leaves `.claude.json` to Claude Code; trust follows `.claude.json` regardless of the launchSpec's `trustWorkdir`.

A Session stopped at this prompt has no Runtime identity yet, so the [unlinked mark](../../concepts/glossary.md#unlinked-mark) stays. The Admin trusts the workdir in one of the following ways.

- Choose `Yes, I trust this folder` in the running Session's pane. Enter the pane with the `attach_command` shown by `feld session get`. Once chosen, the SessionStart hook delivers the identity and the mark disappears
- Before launching the Session, launch `claude` once in the workdir (or its parent) and trust it

### Exit input

For `feld session stop`, the Provider delivers the manifest's [`stopInput`](../plugin.md#manifest) to the pane. Claude Code reads `/exit` as a request to exit, and asks how to exit when a task keeps running, such as the Plugin Monitor helper, so `stopInput` consists of `/exit` and an empty input that selects that question's default (stop the tasks and exit). In a Feld Session the helper is running, so the second input is the answer. A Claude Code still starting up drops Enter keys and lines typed before its prompt is ready, so a stop right after launch ends with the sequence the Provider redelivers. The characters of `/exit` typed while the confirmation is shown are ignored, and the following empty input answers the confirmation.

### Sandbox reads

Claude Code's sandbox and permissions can deny reads by path. The settings file adds the following after the arrays `runtimeSettings` declares, separating the model and tools in the Session from the Admin's files ([assumptions and trust boundaries](../auth.md#assumptions-and-trust-boundary)) and from other Sessions' requests, responses, and tokens. The paths are absolute paths of the resolved Feld home.

| key | Added value |
|---|---|
| `sandbox.filesystem.denyRead` | `$FELD_HOME/admin`, `$FELD_HOME/spool`, `$FELD_HOME/tokens` |
| `permissions.deny` | `Read(/<path>/**)` for the same three (absolute paths starting with `//`) |
| `sandbox.filesystem.allowRead`, `sandbox.filesystem.allowWrite` | The Session's spool directory (`$FELD_HOME/spool/<sessionId>`) |

The sandbox denies loopback TCP, so `feld` inside the sandbox reaches the daemon through the [spool](../spool.md) in its own directory. `allowRead`, as the narrower path, wins over `denyRead`, so only its own directory is readable. `feld` in an `owned` Session uses the token in the environment, so it works without reading `tokens/`.

For writes, `allowWrite` opens only its own directory. Whether another Session's directory is writable depends on the user's settings (workdir, `allowWrite`, `additionalDirectories`) ([known limits](../spool.md#known-limitations)).

## The side beside the Runtime

The four components run as child processes of Claude Code and obtain the token by the [resolution order in auth.md](../auth.md#runtime-adapter-resolution-order).

### hook

The hook is the command declared in the Claude Code plugin's `hooks/hooks.json`. Claude Code launches it on each hook event, and it reads the needed fields from the JSON on standard input and delivers them to the daemon.

- SessionStart: delivers the Runtime identity (the Runtime-side session id, the transcript file location, `model`) with `POST /v1/self/runtime`. It also tells the model what the Plugin Monitor helper's notifications mean, through `hookSpecificOutput.additionalContext` on stdout ([Plugin Monitor helper](#plugin-monitor-helper)). It returns this whether or not it has a token, so it also takes effect in an `attached` Session
- UserPromptSubmit, PostToolUse, Stop: resend the same Runtime identity. Their arrival signals that transcript entries were added, and UserPromptSubmit and Stop correspond to [turn](../../concepts/glossary.md#turn) boundaries. A Session whose SessionStart finished before Feld got involved, as with attach, obtains the identity through this resend

The hook exits 0 after it finishes sending to the daemon, and exits 1 when it cannot resolve the Feld home. It also exits 0 when it could not reach the daemon and when it cannot obtain a Session token (before attach), keeping Claude Code moving. In that case the identity stays undelivered on the Feld side, and arrives with a hook firing after the token becomes available.

### Plugin Monitor helper

The helper is a long-lived process that Claude Code launches at the start of the Session through the Claude Code plugin's monitor declaration, and it runs for the whole Session. It long-polls the mailbox with `GET /v1/self/messages`, writes each arriving Message as one line of JSON to its stdout, and after writing records the read with `POST /v1/self/messages/read`. The read flow and the line format are in [message-delivery.md](../message-delivery.md#reading-by-the-helper).

Claude Code receives one line of the monitor's stdout as one notification and passes it to the model's next turn. The notification stays in the transcript file as a Runtime notification, and the Console draws it distinctly from human input. The SessionStart hook's context tells the model that a `feld_message` notification is a Message addressed to it and how to treat it by sender kind ([message-delivery.md](../message-delivery.md#reading-by-the-helper)). The monitor declaration's `description` states that a notification is a Message from the Admin or another Actor, and that `fromKind` indicates the sender kind.

Claude Code truncates a monitor line to 500 characters (UTF-16 code units) before passing it to the model. The helper writes a Message whose line fits in 500 characters in the common form. For a Message that does not fit, it carries `excerpt` (the beginning of the body, as long as fits the line in 500 characters, followed by a trailing `…`) and `show` (a command that prints the whole body) instead of `body`. The model reads the whole body with the `show` command.

```json
{"type":"feld_message","message":{"id":"message_a41b7d","fifo":13,"from":"dispatcher","fromKind":"actor","fromSessionId":"session_0aa4d9","accepted":"2026-09-13T11:50:12+09:00","excerpt":"Please take issue #15. The work location is…","show":"feld message get message_a41b7d"}}
```

Until it obtains a Session token (before attach), the helper retries the resolution order at a fixed interval, and starts long-polling the mailbox once it has one. One helper runs per Session: after obtaining the token, the helper takes a per-Session lock in `$FELD_HOME`, and exits if it cannot. Even if the plugin installed in the person's Claude Code and the plugin the Adapter passes overlap in the same Session, notifications come from only one helper.

If the helper restarts before recording a read, a Message without a read is returned again on the next fetch. Notification is at least once, and each Message has one read record. The helper exits when the Session closes and its token is revoked, and when `$FELD_HOME` disappears.

The installed monitor shim that `feld init` places keeps waiting for the duration of the Session without output when `$FELD_HOME` has no Plugin.

### otelHeadersHelper

Claude Code runs the command written in the settings' `otelHeadersHelper` before sending telemetry, and attaches the JSON on its stdout as OTLP headers. Feld's helper prints the telemetry token derived from the Session token as the header value, and prints empty JSON (`{}`) when no token is available (before attach). The helper obtains the token by the same resolution order as the other components (environment variables in an `owned` Session, an owner-only file in an `attached` Session). The settings file and the environment's `OTEL_*` hold only the destination and the exporter choice.

Claude Code runs `otelHeadersHelper` at startup and every `CLAUDE_CODE_OTEL_HEADERS_HELPER_DEBOUNCE_MS` (default 29 minutes), and uses the previous header in between. The header before attach carries no token, and after attach or a token reissue the previous header remains until the next run; telemetry in that interval has no attribution target. [Preparing for attach](#preparing-for-attach) sets this interval to 1 minute.

The Collector attributes telemetry to the Session by this header. The attribution decision is in [otel.md](../otel.md), and how the metrics `status` is determined is in [metrics.md](../metrics.md#status-and-source).

### status line helper

Claude Code's OTel carries neither the context window nor the account rate limit, so the status line helper extracts them from the status line JSON and sends them as telemetry.

- Claude Code runs the command written in the settings' `statusLine` at session start, on each assistant message, on completion of `/compact`, and so on, passing the status line JSON on standard input
- The helper builds the following [gauges](../metrics.md#adapter-gauges) from the JSON and sends them to the Collector over OTLP/HTTP. The destination is the Collector endpoint kept in `$FELD_HOME`, and the header carries the same telemetry token as `otelHeadersHelper`
- The helper writes the Actor name and the context window / quota usage (hereafter the Feld segment) as one line to standard output and exits 0. It obtains the Actor name by the same resolution as the token (`FELD_ACTOR` in the environment for `owned`, the self index for `attached`). It also writes the same line and exits when it could not reach the Collector and when no token is available (before attach). When no token is available, the Actor name is `Feld`
- With `--chain <command>`, it passes the standard-input JSON as is to `<command>` run with `sh -c`, and appends ` · ` and the Feld segment after the first line of that command's standard output to form one line. When `<command>` does not exit 0 or prints nothing, it prints only the Feld segment. Sending gauges is the same with or without `--chain`

| gauge | Value | status line JSON field |
|---|---|---|
| `feld.claude_code.context_window.size` | tokens | `context_window.context_window_size` |
| `feld.claude_code.context_window.used_percent` | 0–100 | `context_window.used_percentage` |
| `feld.claude_code.rate_limit.used_percent` | 0–100, attributes `window` and account | `rate_limits.<window>.used_percentage` |
| `feld.claude_code.rate_limit.resets_at` | epoch seconds, attributes `window` and account | `rate_limits.<window>.resets_at` |

The account attributes of the rate limit gauges are, when `CLAUDE_CONFIG_DIR/.claude.json` (`$HOME/.claude.json` if unset) has `oauthAccount`, its `accountUuid` as `account.id` and its `emailAddress` as `account.name`.

`<config root>/skills/feld/bin/status-line` can also be called from the person's own status line script. Given the status line JSON on standard input, it sends the gauges and prints the Feld segment as one line, so the script can place that output wherever it likes. `feld init` keeps a `statusLine` pointing to such a script as `keep`.

```sh
#!/bin/sh
input=$(cat)
mine=$(printf '%s' "$input" | ~/.claude/my-statusline.sh)
feld=$(printf '%s' "$input" | ~/.claude/skills/feld/bin/status-line)
printf '%s | %s\n' "$mine" "$feld"
```

`used_percentage` is `null` before the first API response and right after `/compact`. `rate_limits` is present after the first API response in a Claude Code logged in with a Claude.ai subscription, and holds both `five_hour` and `seven_day`.

## Projection onto the Console

### Projecting transcript entries

The Adapter's `transcript` reads the transcript file (JSONL) indicated by the Runtime identity and the Sub Agent transcript files beside it, one line per entry, projects them into the [common form](../plugin.md#common-shape-of-a-transcript-entry), and returns them. `raw` is the line exactly as Claude Code wrote it, and when a line holds multiple blocks, each block becomes one entry. `cursor` is a string built from the entry's time, file, line, and block, and a line appended to the end of any file sorts after a cursor already read. The kind is decided as follows.

- `user` lines: a human prompt is `input`, a Runtime notification (such as a Plugin Monitor helper notification) is `notification`, and a `tool_result` block is `tool-result`; an `is_error` block carries `error`
- `assistant` lines: a `text` block is `response`, a `tool_use` block is `tool-call`, and a line with only `thinking` blocks is `thinking`
- Among `system` lines, local commands and turn duration are `system`
- Snapshots, mode changes, meta lines Claude Code injects, and the like are `other`
- Entries of a Sub Agent transcript file carry that Sub Agent's id in `subAgent`. The entry in the parent transcript that marks a Sub Agent's completion (the `tool_result` of the tool call that launched it for a synchronously called Sub Agent, the completion notification for a Sub Agent called in the background) carries `subAgentEnded`. An unreadable Sub Agent transcript file appears in the sequence as a single entry with `subAgent` and `state` `lost`

### Transcript file state

The [transcript state](../plugin.md#transcript-states) is common. For an open Session, `missing` means Claude Code is waiting for the first input, and the Console shows that the first input creates the transcript file. For a closed Session, either it ended with no input or Claude Code deleted an old transcript.

### Inserting records

The Console inserts the records of `GET /v1/records?session=` into the transcript. The position is decided in this order.

1. If a record has a Message ID (`anchor` for `message.sent`, the Message's ID for `message.read`) and the body of some transcript entry (in the Conversation view, a notification's content and a tool call's input and result; in the All events view, the whole entry) quotes that ID, the record goes beside the first transcript entry that quotes it
2. A record without a quote is placed by time. It goes before the position of the next transcript entry's time

The kinds inserted per view follow the Web UI's [Console](../../../guide/webui.md#console) view. The Conversation view inserts Control Plane request and Message records, and the All events view inserts all 10 kinds. Which Session a Message-related record belongs to is in [message-delivery.md](../message-delivery.md#inserting-into-session-records-and-the-console).

## Projecting Sub Agent Sessions

A Sub Agent that Claude Code launches inside a Session appears as a [Sub Agent Session](../../concepts/glossary.md#sub-agent-session) that core projects from `transcript`'s `subAgent` / `subAgentEnded`.

- `start` is the time of the Sub Agent's first transcript entry, and `parentSession` is the Session that holds the transcript file
- `end` is the time of the transcript entry in the parent Session's transcript that marks the Sub Agent's completion (`subAgentEnded`). Until the completion entry appears, `end` is null and the Sub Agent Session is drawn as running. When the parent Session closes without a completion entry, the parent Session's `end` becomes `end`
- `startKind` is `sub-agent` and `placement` is null. Ownership, lifecycle, and observation belong to the parent Session. The relationship with the parent and the interpretation of the end are in [session-lifecycle.md](../session-lifecycle.md#sub-agent-session)
- `execution` and `model` are the same as the parent Session's
- The Sub Agent's Console draws the Sub Agent's transcript in the same two views

`GET /v1/sessions/{sessionId}` returns the list of the parent Session's Sub Agent Sessions, projected from the transcript. The Web UI draws them inside the parent's band.

## metrics

The metrics in [metrics.md](../metrics.md) come from the following sources. The metrics and logs (event) exporters are enabled in [Launch arguments and environment](#launch-arguments-and-environment), and the context window and quota come from the gauges of the [status line helper](#status-line-helper). The `source` of `reported` is "Claude Code Runtime telemetry" for metrics taken from OTel, and "Claude Code status line" for metrics taken from gauges.

| metric | Source | How the value is taken |
|---|---|---|
| token usage | metric `claude_code.token.usage` | Map the value per `type` to the [four token kinds](../metrics.md#the-four-token-kinds) by the table below |
| estimated cost | metric `claude_code.cost.usage` | Value (USD) |
| API errors | event `claude_code.api_error` | Count of events |
| prompt cache hit rate | token usage | The formula in [four token kinds](../metrics.md#the-four-token-kinds) |
| API request latency p95 | event `claude_code.api_request` | Attribute `duration_ms` |
| tool calls | event `claude_code.tool_result` | Count of events |
| context window utilization | gauge `feld.claude_code.context_window.used_percent` / `.size` | Value |
| active time | metric `claude_code.active_time.total` | Sum over `type` (`user` / `cli`) (seconds) |
| quota | gauge `feld.claude_code.rate_limit.used_percent` / `.resets_at` | Value per attribute `window` (`five_hour` / `seven_day`) |

| Kind | `type` |
|---|---|
| `input` | `input` |
| `output` | `output` |
| `cache_read` | `cacheRead` |
| `cache_creation` | `cacheCreation` |

Tokens and cost are taken from metrics. The tokens and cost of the same request also appear as attributes of event `claude_code.api_request`, and the projection counts only the metrics.

## When attached

### Preparing for attach

To make a Claude Code the person launches themselves attachable, set up the following. For an `owned` Session the Adapter assembles these at launch, so no preparation is needed.

1. Run the daemon with `feld daemon start`
2. Put `eval "$(feld env)"` in the shell config file (`~/.zshrc` for zsh, `~/.bashrc` for bash). [`feld init`](../../../guide/cli.md#feld-init) appends it with consent. `feld env` prints the daemon's `FELD_HOME` and `FELD_API`, plus the same `OTEL_*` as an `owned` Session's [launch environment](#launch-arguments-and-environment) with `CLAUDE_CODE_OTEL_HEADERS_HELPER_DEBOUNCE_MS=60000` added ([cli.md](../../../guide/cli.md#feld-env)). A Claude Code launched from this shell exports telemetry to that daemon's Collector, and its components call that daemon. `CLAUDE_CODE_OTEL_HEADERS_HELPER_DEBOUNCE_MS` makes attach and token reissues show up in the header sooner ([otelHeadersHelper](#otelheadershelper))
3. Place the Claude Code plugin in the config root's `skills/feld/`. The declarations and skills are the same as those passed to an `owned` Session, and the components in `bin/` run `plugins/runtime/claude-code/plugin` under `FELD_HOME` (`~/.feld` if absent) at run time. If that Plugin is absent, `monitor` [waits without output](#plugin-monitor-helper) and the other components exit doing nothing (`otel-headers` prints `{}`, `status-line` prints the output of the `--chain` command, or an empty line if there is none). Claude Code loads this as a plugin from the next Session on, enabling the hook, the Plugin Monitor helper, and the `/feld attach` skill
4. Write `otelHeadersHelper` and `statusLine` into Claude Code's user settings (the config root's `settings.json`), keeping the other entries as they are. `env` belongs to the person, and Feld's telemetry environment comes from the shell
5. Launch `claude` inside a Provider pane (tmux or herdr). `feld actor attach --self` assembles the Placement from the environment the Provider's `self` reads (`TMUX` and `TMUX_PANE` for tmux; `HERDR_ENV=1`, `HERDR_SOCKET_PATH`, and `HERDR_PANE_ID` for herdr)

```json
{
  "otelHeadersHelper": "'<config root>/skills/feld/bin/otel-headers'",
  "statusLine": { "type": "command", "command": "'<config root>/skills/feld/bin/status-line'" }
}
```

Steps 3 and 4 are the Adapter's `prepare`; [`feld init`](../../../guide/cli.md#feld-init) writes them, and `feld actor attach --runtime claude-code` only checks them. What is written depends only on the feld version and is the same from any daemon. The write target is the CLI's config root (`CLAUDE_CONFIG_DIR`, or `$HOME/.claude` if absent). A value that writes that config root's helper path unquoted (including, for `statusLine`, one followed by `--chain`) is also Feld's value, and `write` rewrites it in quoted form.

When something other than Feld is present, entries are handled as follows, and `chain` and `replace` are written after obtaining the person's consent ([plugin.md](../plugin.md#operation)).

- `skills/feld/`: it is Feld's if every file in it is a file Feld writes (the plugin declarations, skills, and the components in `bin/`) and their contents are the current version's or match the prepare record's digests. If it is still the current version, `keep`; if it is Feld's, `write`. If it has the person's files (files with other names, files whose contents were changed by hand), `replace`, which leaves the files with other names and writes Feld's files
- `statusLine`: `keep` if the current command contains Feld's helper (`skills/feld/bin/status-line`, however the config root is written) or the person's script the command points to calls that helper. An already chained value also falls here. `keep` if the prepare record holds a value Feld wrote and the current value was changed by hand. Any other value is `chain`, which keeps the person's command as the helper's `--chain` argument, keeps the other `statusLine` entries (`padding`, `refreshInterval`, and so on), and stores the pre-replacement value in the prepare record. A value without a command or an empty one (including a non-object value) is `replace`
- `otelHeadersHelper`: Claude Code reads only one, so a non-Feld value is `replace`, and the pre-replacement value is stored in the prepare record
- `env`: if the complete set of telemetry environment written by an earlier Feld is present (every key of `feld env` except `FELD_*` holds Feld's value, and `OTEL_EXPORTER_OTLP_ENDPOINT` starts with `http://127.0.0.1:`), `replace`, which removes that set and stores it in the prepare record. When a loopback endpoint exists but the set is incomplete, it is kept and a note reports it

```json
{
  "statusLine": {
    "type": "command",
    "command": "'/Users/alice/.claude/skills/feld/bin/status-line' --chain '~/.claude/statusline.sh'",
    "padding": 0
  }
}
```

The prepare record lives owner-only at `$FELD_HOME/prepared/claude-code.json` and holds, per prepared config root, the keys and values Feld wrote, the pre-replacement values of keys replaced by `chain` and `replace`, and the SHA-256 digests of the files written to `skills/feld/`. The daemon replaces everything under `$FELD_HOME/plugins/` with its own version at startup, so the record lives outside it.

[`feld uninstall`](../../../guide/cli.md#feld-uninstall) reverts the preparation with the Adapter's `restore`. The targets are the config roots the record holds and the CLI's config root; `skills/feld/` is `remove` if it is Feld's ([Preparing for attach](#preparing-for-attach)), and if it has the person's files it is left as `keep` with `notes` pointing to those files. User settings entries are decided from the record and the current value as follows.

- A `statusLine` still at the record's value whose command has `--chain` is `unchain`, which restores the command in the `--chain` argument and keeps the other entries
- A value still at the record's value with a pre-replacement value is `restore`, which returns it to that value (`otelHeadersHelper`, or a `statusLine` that had no command)
- If the record has a pre-replacement `env` set, `restore`, which restores only the keys absent from `env`
- Otherwise, a value still at the record's value is `remove`
- A value changed from the record's value is `keep`
- Of values not in the record, only those containing the Feld helper's path are `remove` (an already chained `statusLine` is `unchain`)

The config root preparation is shared regardless of `FELD_HOME`, so `feld uninstall` in a home without a record also removes it, and the CLI lists that attach also stops for other Feld homes ([cli.md](../../../guide/cli.md#feld-uninstall)).

Claude Code reads the environment's `OTEL_*` and the settings' `otelHeadersHelper` at startup, so a Claude Code exports telemetry to this Session only if it was launched from a shell with this daemon's `feld env`, after `otelHeadersHelper` was written. The Adapter's `match` answers `telemetryPrepared` by whether `OTEL_EXPORTER_OTLP_ENDPOINT` in the Runtime process's environment points to the daemon's Collector (read from `/proc/<pid>/environ` on Linux and from `sysctl` `KERN_PROCARGS2` on macOS; null if unreadable), and `prepare` answers `telemetryPrepared` as false when `otelHeadersHelper` is not Feld's helper. If either is false, the CLI tells the daemon with `runtimeTelemetryPrepared: false`, and those metrics become `unavailable` except context window utilization, with the reason in `source` ([metrics.md](../metrics.md#status-and-source)). The `telemetry` line of `feld actor attach` output shows this state and says to relaunch Claude Code from a shell with `feld env`. Claude Code rereads `statusLine` while running, so the status line gauges also arrive from this Session.

In a prepared Claude Code, the four components run from before attach, and wait without calling the daemon's Session API until a token is available ([hook](#hook), [Plugin Monitor helper](#plugin-monitor-helper), [otelHeadersHelper](#otelheadershelper), [status line helper](#status-line-helper)).

### The attach flow

When the person runs `/feld attach [ACTOR]` inside a Claude Code they launched themselves, the skill calls `feld actor attach [ACTOR] --self --runtime claude-code`, passing the skill's argument as the Actor name. In this flow only the Adapter's side beside the Runtime is at work.

1. The CLI assembles the Placement from the selector the Provider's `self` answers, and identifies the Runtime process from its own chain of parents. The Adapter's `match` checks by executable name and argv that the candidate is Claude Code ([provider/tmux.md](../provider/tmux.md#--self), [provider/herdr.md](../provider/herdr.md#--self)). It then calls `POST …/attach` with `runtime: claude-code`. The Session's `execution` is determined from `runtime`: `type` is `agent-runtime`, `label` is "Claude Code", and `command` is null
2. The CLI saves the Session token from the response to the self index. From then on, this Claude Code's child processes obtain the token from the self index. The procedure is in [auth.md](../auth.md#attached-sessions). If the self index is lost, running `/feld attach` again inside the same Claude Code recovers the token of the same Session ([token recovery](../auth.md#token-recovery))
3. The CLI delivers the Runtime identity answered by the Adapter's `identity` with `POST /v1/self/runtime`. The session id comes from `CLAUDE_CODE_SESSION_ID`, which Claude Code places in its child processes' environment; the transcript file location from the file named with that id under the config root's `projects/`; and `model` from that file's last response. The Console draws the transcript from this point. When the environment has no session id or the transcript file does not exist yet, it delivers nothing and says so in the output
4. Hooks firing afterwards (UserPromptSubmit, PostToolUse, Stop) deliver the same identity with the `model` of the transcript file's last response, keeping `model` current
5. The Plugin Monitor helper obtains the self index token on its next resolution and starts long-polling. `otelHeadersHelper` carries the token in the header from Claude Code's next run of it, and the status line helper sends gauges from its next run

Only telemetry correlated with the token in the header becomes the Session's Telemetry and the source of metrics. In a Claude Code without the environment of preparation step 2, metrics are `unavailable`.

Attach is accepted even for an unprepared Claude Code, and the Session and token are created. The hook, the Plugin Monitor helper, and the status line helper do not run, so the CLI points to `feld init` in a `note:` line, and the `telemetry` line shows `unavailable`.

Claude Code loads plugins at startup. When `feld init` wrote preparation step 3, the components run in an already running Claude Code after `/reload-plugins`, or from the next Claude Code launched. Metrics depend on the environment of step 2 and the `otelHeadersHelper` of step 4 being read at startup, so a Claude Code running since before the preparation delivers them once relaunched. `feld init` states these two points in `note:`.
