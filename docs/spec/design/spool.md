# Path from inside the sandbox (spool)

This document describes the path by which a process running inside a Runtime's sandbox (`feld`, called by the model from a shell) passes requests to the daemon through files instead of TCP. The meaning of requests, authorization, and responses follow [`daemon-api.md`](../../guide/daemon-api.md) and [auth.md](auth.md); this document covers how requests are carried, and the verification and guidance specific to that transport.

## Contents

- [Overview](#overview)
- [Choosing the path](#choosing-the-path)
- [Spool directories](#spool-directories)
- [Request and response files](#request-and-response-files)
  - [Names and states](#names-and-states)
  - [What happens to the files](#what-happens-to-the-files)
  - [Request format](#request-format)
  - [Response format](#response-format)
- [Processing in the daemon](#processing-in-the-daemon)
  - [Watching](#watching)
  - [Acceptance](#acceptance)
  - [Caller verification](#caller-verification)
  - [Cancellation](#cancellation)
- [How the CLI waits](#how-the-cli-waits)
- [When the daemon is stopped](#when-the-daemon-is-stopped)
- [Cleanup](#cleanup)
- [Codex preparation](#codex-preparation)
- [Guidance when the CLI cannot write](#guidance-when-the-cli-cannot-write)
- [The sandbox hole](#the-sandbox-hole)
  - [Known limitations](#known-limitations)

## Overview

Requests reach the daemon by three paths, and all are processed by the same handler.

| Path | Used by | Accepted tokens |
|---|---|---|
| TCP (the API listener) | the CLI and scripts outside the sandbox, the Web UI, the Collector | all [four kinds](auth.md#token-kinds) |
| the Feld home socket (`daemon.sock`, [auth.md](auth.md#assumptions-and-trust-boundary)) | Runtime Adapter hooks and helpers | all [four kinds](auth.md#token-kinds) |
| spool | the CLI inside the sandbox | Session tokens only |

The CLI inside the sandbox writes an HTTP request as a single file into its own Session's directory in the spool, and the daemon returns the response as a file in the same directory. The CLI's per-command implementation is the same over TCP and the spool; only the layer below the HTTP client changes. The sandbox needs only write access to its own directory in the spool and read access to the Feld home excluding `admin/` ([Assumptions and trust boundaries](auth.md#assumptions-and-trust-boundary)). Runtime Adapter hooks and helpers run outside the Runtime's sandbox ([Codex](adapter/codex.md#the-side-beside-the-runtime)), so they use the Feld home socket.

The spool path carries a single request-response round trip. The event stream (`GET /v1/events`) and serving the Web UI use TCP only.

## Choosing the path

The CLI starts with TCP and, when the sandbox denies something the TCP path needs, performs the rest of that command over the spool.

- Connecting to the daemon's API is denied with `EPERM` / `EACCES`
- While resolving the Session token, reading the process table (the parent chain and start times) is denied
- The Session token is not resolved, and reading the local token (`admin/`) used to ask the daemon for its status is denied

A refused connection (`ECONNREFUSED`) or a timeout does not switch the path; it is an error of not reaching Feld (exit code 2). With a sandbox setting that allows network (Codex `network_access: true`), TCP passes as is, and an `owned` Session runs with the token in its environment, so TCP suffices. A setting that allows writing to the spool and a setting that allows network can be used together, and the CLI uses whichever passes.

## Spool directories

The spool (`$FELD_HOME/spool/`) is divided into directories, and each directory determines which Session's requests it can carry. When the sandbox's write access is limited to that Session's directory, it cannot rewrite or withdraw other Sessions' requests or forge their responses. Sandboxes that are not so limited are listed in [Known limitations](#known-limitations).

| Directory | Used by Session | How it is passed to the sandbox |
|---|---|---|
| `spool/<sessionId>/` | that `owned` Session | the Runtime Adapter's `launch` makes it a writable path of that Session's sandbox ([Codex](adapter/codex.md#launch-arguments-and-environment), [Claude Code](adapter/claude-code.md#sandbox-reads)) |
| `spool/shared/` | `attached` Sessions | the sandbox of a Runtime started by a person is fixed before attach, so [Codex preparation](#codex-preparation) puts it in the global configuration |

- The daemon creates `spool/` and `spool/shared/` on every start and sets their mode to `700`. `feld init` starts the daemon, so they exist after preparation
- The daemon creates `spool/<sessionId>/` with mode `700` before starting the Session, and when the Session ends, removes it together with the files inside once no request awaiting a response (`<id>.request`, `<id>.taken`, an `<id>.response` the CLI has not read) remains. In its scan every second, the daemon also creates the directories of open `owned` Sessions and removes the directories of ended Sessions
- When the CLI carries the token from its environment (`FELD_SESSION_TOKEN`), it writes to the directory named by `FELD_SESSION_ID` in its environment; when it carries a token from the self index, it writes to `spool/shared/`. launch puts both into the environment of an `owned` Session
- Before reading a file in the spool, the daemon checks that it is a regular file, that its owner has the same uid as the daemon, and that it is not a symlink (`O_NOFOLLOW`); files that fail these checks are not read and are passed to [Cleanup](#cleanup)
- The CLI creates files with mode `600`

## Request and response files

### Names and states

Each request is named by an id the CLI generates (32 hex digits of a 128-bit random number). Files are written under a temporary name and then put in place by rename, so the other side never reads a partially written file.

Before writing the request file, the CLI takes an exclusive lock (`flock`) on that file and keeps it open until it finishes reading the response. The lock stays on the same file after the rename, and the OS releases it when the CLI process ends (including `SIGKILL`). The daemon learns whether the CLI that wrote the request is still waiting by whether it can take the lock. The lock is attached to the file, so it works the same regardless of PID namespace.

| File | Written by | Meaning |
|---|---|---|
| `<id>.request` | CLI | A request awaiting processing |
| `<id>.taken` | daemon | A request being processed. The daemon takes it by renaming `<id>.request` |
| `<id>.response` | daemon | The response. The CLI reads and removes it |
| `<id>.withdrawn` | CLI | A request withdrawn before processing. The CLI withdraws it by renaming `<id>.request` |
| `<id>.<kind>.tmp` | both | Partially written. The name before the rename |

For `<id>.request`, exactly one of the daemon's rename to `<id>.taken` and the CLI's rename to `<id>.withdrawn` succeeds. A request the daemon takes is processed exactly once, and a request the CLI withdraws is not processed.

### What happens to the files

| Case | Flow and final state |
|---|---|
| Responded | The daemon writes `<id>.response`, then removes `<id>.taken`. The CLI reads and removes `<id>.response` |
| The CLI gave up before processing | The CLI turns `<id>.request` into `<id>.withdrawn` and removes it |
| The CLI ended during processing | The daemon drops the handler through [Cancellation](#cancellation) and removes `<id>.taken`. If the handler finished first, its result is final, and [Cleanup](#cleanup) removes `<id>.response` |
| The CLI terminated abnormally before processing | The daemon can take the lock at the moment it takes the request, so it removes `<id>.taken` without processing |
| The daemon stopped or restarted | [When the daemon is stopped](#when-the-daemon-is-stopped) |

### Request format

A single UTF-8 JSON object.

```json
{
  "version": 1,
  "method": "POST",
  "path": "/v1/workspaces/default/actors/reviewer/messages?wait",
  "contentType": "application/json",
  "body": "{\"body\":\"…\"}",
  "credentials": [
    { "source": "provider", "plugin": "tmux", "token": "<Session token>" },
    { "source": "runtime", "plugin": "codex", "token": "<Session token>" }
  ],
  "caller": { "pid": 48213, "pidNamespace": null }
}
```

- `method`, `path` (including the query), `contentType`, and `body` (the HTTP body as a string; null if absent) have the same values as the HTTP request sent over TCP
- `credentials` lists the Session token candidates the CLI found, in the [resolution order in auth.md](auth.md#runtime-adapter-resolution-order). `source` is one of `environment` (the environment variable `FELD_SESSION_TOKEN`), `provider` (the self index under the Provider's key), or `runtime` (the self index under the Runtime's key), and `plugin` is the name of the Plugin that answered the self index key. If `environment` is present, it is the only entry. On the spool path, the CLI does not verify self index candidates; the daemon does so in [Caller verification](#caller-verification)
- `caller` holds the CLI's own pid and, on Linux, the CLI's PID namespace (the value of the `/proc/self/ns/pid` link; null on macOS)

### Response format

```json
{ "version": 1, "status": 200, "contentType": "application/json", "body": "{…}" }
```

`status`, `contentType`, and `body` have the same values as the HTTP response returned over TCP, and the CLI reads them in the same way as a TCP response.

## Processing in the daemon

### Watching

The daemon watches each directory of the spool with the OS file notification (FSEvents on macOS, inotify on Linux) and picks up the appearance of `<id>.request` within a few ms. To keep making progress even if notifications are missed, it scans each directory every second.

Each request is processed in its own task, so a long-waiting request (`feld self message recv --wait`) never blocks other requests. As with TCP, the order between requests from different processes is not defined. A single CLI waits for a response before writing its next request, so its requests are processed in that order.

### Acceptance

The daemon takes `<id>.request` by renaming it to `<id>.taken` and decides in the following order. If it can take the lock on the request file, the CLI that wrote it has ended, so the daemon removes `<id>.taken` without writing a response. Otherwise, if it rejects the request, it writes that response to `<id>.response`.

1. Format: a file whose size exceeds the API body limit plus 64 KiB is 413; a file that cannot be read as JSON, or whose `version` is unknown, is 400. Only the form of the file is judged here
2. Route: a path (including the query) that does not start with `/` is 400. Paths of the event stream and the Web UI are 400 (`this API path is not available over the spool`)
3. Token: if `credentials` contains anything other than a Session token (a local token, browser token, or internal token), 403 (`the spool accepts Session tokens only`). Session token validity follows [Token revocation](auth.md#token-revocation)
4. Verification: each entry of `credentials` is put through [Caller verification](#caller-verification) in order. If none passes, 401 (the same text as the CLI's `No Session token: run this inside an attached or owned Feld Session`)
5. Directory: among the tokens that passed verification, the first one belonging to the Session of the directory where the request was placed (for `spool/<sessionId>/`, that Session's token; for `spool/shared/`, a token whose `source` is `provider` or `runtime`) becomes the request's token. If there is none, 403 (`the Session token does not belong to this directory of the spool`)

A request that passes is handed to the API router as an HTTP request carrying that token in `Authorization`, and as with TCP, the router checks the body and query after the token decision. [Accepted Host](auth.md#accepted-hosts) is a check on requests that arrive at the listener and is not applied to spool requests. Authorization is the same as TCP regardless of path ([Authorization decisions](auth.md#authorization-decisions)).

### Caller verification

Each candidate in `credentials` passes when the daemon finds the same conditions satisfied that the CLI checks on the TCP path.

| `source` | Condition |
|---|---|
| `environment` | None (the token in an `owned` Session's environment) |
| `provider` | The token's Session is `attached`, the recorded Runtime process (pid and start time) is running, and it is in the caller's parent chain |
| `runtime` | The token's Session is `attached`, the recorded Runtime process is running, and the caller's parent chain contains a process that the `match` of the `plugin`'s Runtime Adapter accepts |

The daemon is outside the sandbox, so it can read the process table. The caller is the pid in `caller`, and the daemon walks its parent chain.

The pid is self-reported, and the daemon does not confirm which process wrote the request file (the file reveals only its owner's uid). Verification guards against mixing up the token of an ended Runtime or of a different pane; it does not shut out processes that lack a token. From inside the sandbox, the self index token files are also readable, so a process holding such a token can forge its report. The boundary of the spool path lies in accepting only Session tokens and in each directory accepting only its own Session's token ([The sandbox hole](#the-sandbox-hole)).

The Linux sandbox (bubblewrap) is a separate PID namespace, and the pid in `caller` is the pid inside the sandbox. If `pidNamespace` matches its own, the daemon uses that pid; otherwise it scans `/proc` for a process whose `/proc/<pid>/ns/pid` matches `pidNamespace` and whose last `NSpid` value in `/proc/<pid>/status` is the reported pid, and uses that host pid as the caller. If none is found, `provider` and `runtime` candidates do not pass.

### Cancellation

The daemon tries the lock on the file of each request being processed every second, and when it can take it (the CLI has ended), it cancels that request. The handler of a cancelled request is dropped midway, as when a TCP connection is closed, and no response is written. If the handler finished first, its result (including side effects) is final, the same as a TCP connection closed before the response was received. On SIGINT / SIGTERM, the CLI withdraws the request by turning `<id>.request` into `<id>.withdrawn` and then exits. If the request has already been taken, it simply exits, and the release of the lock acts as the cancellation.

Even if `feld self message recv --wait` is cancelled, the read is recorded by the CLI in a separate request after receiving the response, so the Message is delivered again on the next retrieval ([message-delivery.md](message-delivery.md#reading-by-feld-self-message-recv)).

## How the CLI waits

- The CLI waits for `<id>.response` to appear by polling. The interval starts at 2 ms, doubles, and stops growing at 100 ms. After reading the response, the CLI removes it
- The upper bound on waiting is the same as TCP. A daemon status request gives up after 5 seconds ([cli.md exit codes](../../guide/cli.md#common-conventions)), and long-waiting requests have no upper bound
- While waiting, the CLI checks every second that the daemon is alive via `daemon.lock`

## When the daemon is stopped

- Before writing the request: as on the TCP path, the CLI checks whether the daemon is running via `daemon.lock` and `daemon.json`, and if it is not, exits without writing the request, with the same text as the TCP path
- When the daemon stops: it writes no response for requests being processed and leaves `<id>.taken` in place. It also writes no response for handlers that finish during the shutdown
- The daemon stopped while the CLI was waiting: the CLI withdraws the request by turning `<id>.request` into `<id>.withdrawn`. If the withdrawal succeeds, the request was not processed, so it exits with exit code 2 and `the Feld daemon stopped before taking the request; nothing was applied`. If the request has already been taken, it leaves `<id>.taken` to the daemon and exits with exit code 2 and `the Feld daemon stopped while handling the request; it may have been applied`
- A new daemon started: on start, it scans the spool and takes any remaining `<id>.request` through the [Acceptance](#acceptance) steps (it does not process a request whose lock it can take). A remaining `<id>.taken` is one the previous daemon stopped in the middle of processing, so the daemon writes 503 (`the Feld daemon restarted while handling the request; it may have been applied`) to `<id>.response` and then removes `<id>.taken`. A CLI that keeps waiting through `feld daemon restart` receives this as the response

## Cleanup

The daemon removes the following in its scan every second.

- In each directory, `<id>.response`, `<id>.withdrawn`, `.tmp`, and files passed over without reading, that are older than 10 minutes
- Entries directly under `spool/`, other than Session directories and `spool/shared/`, that are older than 10 minutes
- The directories of ended Sessions ([Spool directories](#spool-directories))

`<id>.request` and `<id>.taken` disappear through the steps in [Acceptance](#acceptance) and [What happens to the files](#what-happens-to-the-files) and are outside the scope of cleanup.

## Codex preparation

So that the `workspace-write` sandbox of a Codex started and attached by a person can write to the spool, the Codex Adapter's [`prepare`](plugin.md#operation) adds `$FELD_HOME/spool/shared` to `writable_roots` under `[sandbox_workspace_write]` in `config.toml` of the CLI's configuration root (`CODEX_HOME`, or `$HOME/.codex` if unset) ([Codex attach preparation](adapter/codex.md#preparing-for-attach)). For the sandbox of an `owned` Session, `launch` passes that Session's directory ([launch arguments and environment](adapter/codex.md#launch-arguments-and-environment)).

- [`feld init`](../../guide/cli.md#feld-init) writes it, and `feld actor attach --runtime codex` checks it. Because it is an entry that loosens the sandbox, `feld init` asks for consent before writing it
- Because it is a global setting, it applies to every Codex a person starts. The configuration root of an `owned` Session is determined by the daemon's environment ([launch arguments and environment](adapter/codex.md#launch-arguments-and-environment)), so when the configuration roots of the daemon and the CLI differ, `prepare` indicates this in `daemonNotes`. The daemon records in `daemon.json` the values of the environment variables that determine configuration roots (`HOME`, `CODEX_HOME`, `CLAUDE_CONFIG_DIR`), and `prepare` compares them with the running daemon's values
- One element is added per Feld home. If `spool/shared` is already covered by an element of `writable_roots` (`spool/shared` or a directory above it), the result is `keep`. However, an element that is `$FELD_HOME/spool` itself opens every Session's directory, so it is replaced with `spool/shared` via `replace`. If `spool/shared` already exists as a person's element, only the `spool` element is removed. Other elements of `writable_roots`, other keys, comments, and symlink target files are kept as they are
- [`feld uninstall`](../../guide/cli.md#feld-uninstall) uses `restore` to remove only the spool element of this Feld home that `prepare` added. The prepare record keeps, per configuration root, the path of the added element and the keys and tables created for it, and `restore` also removes the created keys and tables once they become empty

## Guidance when the CLI cannot write

When the CLI cannot write a request to the spool (`EPERM`, `EACCES`, `EROFS`), it gives a different message for each cause. The clues are the environment and configuration readable by the CLI inside the sandbox. Runtime hooks run outside the sandbox, and the `permission_mode` in their input does not reflect the thread's sandbox, so the CLI does the detection. Only a CLI that writes to `spool/shared/` uses `config.toml` as a clue; launch passes the directory of an `owned` Session.

| Clue | Cause | Fix shown |
|---|---|---|
| `CODEX_PERMISSION_PROFILE=:read-only` | The sandbox is `read-only`, including when it reverted on an app-server restart ([Known constraints](adapter/codex.md#the-sandbox-reverts-when-the-app-server-restarts)) | Start a new thread with `/new`, or quit Codex and run `codex resume <thread id> --sandbox workspace-write`. The thread id is shown from `CODEX_THREAD_ID` in the environment |
| `CODEX_SANDBOX` is set, and `writable_roots` in `config.toml` of the configuration root lacks `spool/shared` | Not prepared | Run `feld init --runtime codex` outside the sandbox and restart Codex |
| `CODEX_SANDBOX` is set, and `config.toml` has `spool/shared` | `writable_roots` was replaced by `-c` at Codex startup, or the sandbox is `read-only` | Show both: add the spool path to `-c sandbox_workspace_write.writable_roots`, and, if `read-only`, the fix in the first row |
| None of the above | Unknown sandbox | The spool path and the OS error, and that writing to the spool must be allowed |

When the spool directory does not exist (`ENOENT`), a daemon of a version that does not create the spool is running, so the CLI tells the user to run `feld daemon restart` outside the sandbox.

When an Admin operation (such as `feld actor attach`, which requires a local token) is invoked over the spool path, the CLI does not write the request and tells the user to run that command outside the sandbox. For `feld actor attach --self`, it adds `feld actor attach <name> --workspace <workspace> --runtime codex --tmux-pane <pane> --tmux-socket <socket>` (for tmux), with the selector items answered by the Provider's `self` turned into `--<provider>-<key>`. `<name>` becomes the Actor name when one is given.

## The sandbox hole

A Runtime sandbox that reaches Feld is not closed within the scope of Feld's operations. Operations callable with a Session token are equally callable over the spool path, and the daemon starts Jobs opened by grants outside the sandbox.

- The hole is a single point, Feld, and the sandbox can keep external communication closed
- The hole is opened by the `feld init` preparation the user consented to; Feld never loosens the sandbox silently
- The spool path accepts only Session tokens, so the hole is limited to "operations as a Session" and does not reach Admin operations. Even if a local token is readable from inside the sandbox, the spool does not accept it
- Operations as a Session are limited to operations on itself and operations that the grants an Admin gave to the Actor open within the same Workspace ([Authorization decisions](auth.md#authorization-decisions)). A Session of an Actor without grants can neither start, stop, nor modify other Actors
- Operations are kept in the record, so it is possible to trace afterward which Session requested what
- A request placed in `spool/<sessionId>/` is accepted only with that Session's token. Even if another Session's token is readable, operating as that Session from one's own directory is not possible
- The sandbox of an `owned` Session cannot read other Sessions' directories, `admin/`, or `tokens/`, thanks to settings in which the Runtime Adapter adds read denials ([Codex](adapter/codex.md#per-session-configuration), [Claude Code](adapter/claude-code.md#sandbox-reads))

A sandbox that allows network (Codex `network_access: true`) has another hole outside Feld. The [tmux](provider/tmux.md) and [herdr](provider/herdr.md) Providers start the Runtime in a pane outside the sandbox, and the pane's environment holds the path of the server's Unix socket (`TMUX`, `HERDR_SOCKET_PATH`). A sandbox that allows network can connect to that socket, and an Agent inside the sandbox can send input to the pane to run commands in a shell outside the sandbox. If the sandbox closes network, the connection is refused, and the CLI reaches Feld via the spool. Feld reports this combination before startup, leaving the user's sandbox settings unchanged.

- The `sandbox` check of [`feld doctor`](../../guide/cli.md#feld-doctor) is `warn` when Codex's `config.toml` has `network_access: true` and the tmux or herdr Provider is installed
- `feld actor create` / `update` report it in a `note:` line when a Codex Actor runs a `workspace-write` sandbox that allows network on tmux or herdr (determined by `runtimeSettings`, or the value in `config.toml` if absent). For `create`, which starts a Session, the report comes before the startup is requested

### Known limitations

The following sandboxes reach other Sessions' requests and responses.

- `attached` Sessions share `spool/shared/`, and the sandbox of a Runtime a person starts follows that person's settings, so `attached` Sessions can read and write each other's requests and responses
- In an `owned` Claude Code Session, launch adds its own directory to `allowWrite`. Whether it can write to other Sessions' directories depends on the user's settings (workdir, `allowWrite`, `additionalDirectories`) ([Claude Code](adapter/claude-code.md#sandbox-reads))
- A sandbox that lists a path above the spool (such as `$HOME`) as writable and does not deny the spool can write to every directory of the spool
- A Codex Session whose `runtimeSettings` declares `default_permissions` or `permissions` keeps the declared profile, and Feld does not deny reads
