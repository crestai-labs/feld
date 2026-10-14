# Authentication and authorization in Feld

This document describes how the daemon distinguishes its callers with four token kinds, and how the Session token serves as the basis for a Message's `from` and a spawned record's `spawner`. Common rules for each API are in [the common rules of `daemon-api.md`](../../guide/daemon-api.md#common-conventions), and identification of CLI callers is in [the common rules of `cli.md`](../../guide/cli.md#common-conventions); this document describes the behavior behind them.

## Contents

- [Assumptions and trust boundary](#assumptions-and-trust-boundary)
- [Accepted hosts](#accepted-hosts)
- [Token kinds](#token-kinds)
  - [local token](#local-token)
  - [Session token](#session-token)
  - [internal token](#internal-token)
  - [browser token](#browser-token)
- [pairing](#pairing)
- [cookie and key](#cookie-and-key)
- [Distributing the Session token](#distributing-the-session-token)
  - [owned Sessions](#owned-sessions)
  - [attached Sessions](#attached-sessions)
  - [Token recovery](#token-recovery)
  - [Runtime Adapter resolution order](#runtime-adapter-resolution-order)
- [Authorization decisions](#authorization-decisions)
- [Preventing token exposure](#preventing-token-exposure)
- [Token revocation](#token-revocation)

## Assumptions and trust boundary

The daemon listens on the same Port on loopback (`127.0.0.1`, and `[::1]` where IPv6 is available) and on the socket in Feld home (`$FELD_HOME/daemon.sock`); within the same host it relies on the OS user boundary. Browsers on other devices come through an HTTPS proxy that the user sets up (`tailscale serve`). Exposure is limited to the tailnet. The daemon learns the name the proxy exposes externally only through the user's permission ([Accepted hosts](#accepted-hosts)), and it does not identify callers by headers the proxy adds (other processes on the same host can forge them).

The daemon owns the source-of-truth store, and every request from outside crosses the API acceptance boundary before it becomes a record. Callers fall into four groups, each of which reaches the daemon with its own token.

- CLI and scripts to the daemon: [Admin](../concepts/glossary.md#admin) operations. Called with the local token
- Web UI to the daemon: Admin operations. Whether on the daemon's device or another device, the browser token is carried by a cookie and a key
- From inside a Session to the daemon: calls from the Session's child processes (the `feld` CLI, the Runtime Adapter's hooks / helpers). Called with the Session token. The CLI inside a Runtime sandbox goes through the [spool](spool.md) instead of TCP, and the spool accepts only the Session token. A Session is outside the daemon even when it runs under the same OS account, and the facts a Session reports stay within the scope the Session token attributes
- Collector to the daemon: forwarding Runtime telemetry. Called with the internal token

The local token and the browser token have authority across all Workspaces. A Session token points to one Session, and since the Session's Actor has one Workspace, the token belongs to that Workspace. The authority of a Session token is limited to operations on itself and operations that the Actor's grants open within that Workspace ([Authorization decisions](#authorization-decisions)).

Feld home (`$FELD_HOME`) is an owner-only directory, split into `admin/`, which only Admin reads, and the rest, which Sessions read (`daemon.json`, `spool/`, `self-index/`, `tokens/`, `plugins/`, `sessions/`, and so on). `admin/` holds the local token (`api-token`), the Collector configuration (`collector.json`), the store (`feld.db` with `-wal` and `-shm`), and the pairing page of [`feld ui`](../../guide/cli.md#feld-ui) (`ui-pair/`). The Session environment contains `FELD_HOME`, and the Runtime sandbox reads Feld home ([spool](spool.md)). A sandbox that can restrict reads by path denies `$FELD_HOME/admin`, separating processes inside the Session from the Admin token and the store (per-Runtime Adapter settings are in [Claude Code](adapter/claude-code.md#sandbox-reads) and [Codex](adapter/codex.md#feld-inside-the-sandbox)). At startup, the daemon moves these items, if placed directly under home, into `admin/`.

Session token authorization is a guardrail. In a Runtime without a sandbox, and inside a sandbox that allows network without denying reads of `admin/`, a Session's child process can read the local token and call over TCP as Admin. The hard wall that confines the model inside a Session is built on the Runtime sandbox side (closing reads of `$FELD_HOME/admin` and the network, and opening only the [spool](spool.md)).

## Accepted hosts

The daemon answers only requests whose destination name is in a list (protection against DNS rebinding). The list consists of the following two.

- Loopback names (`127.0.0.1`, `localhost`, `[::1]`). The Port is limited to the Port the daemon listens on
- Proxy names that the user permits with `feld daemon start --allow-host <name>` ([cli.md](../../guide/cli.md#feld-daemon-start)). Any Port

For every request arriving at the API listener (including Web UI serving and pairing), the daemon checks `Host` (or, if absent, the authority of the request URI) and, if present, every name in `X-Forwarded-Host`. If any one is not in the list, it returns 403 without reading the request. The body is human-readable plain text that shows the following.

- A name that is not permitted: the name and the command that permits it (`feld daemon restart --allow-host <name>`)
- A loopback name with a Port other than the daemon's: the URL of the daemon's Web UI (`http://127.0.0.1:<port>/ui/`)
- A value that cannot be read as a name: the value

`tailscale serve` puts the name the browser opened (`<host>.<tailnet>.ts.net`, plus the Port if it is not the default) in both `Host` and `X-Forwarded-Host` when sending to the daemon.

## Token kinds

### local token

Used by the CLI and scripts. The daemon issues a new one at each startup and saves it owner-only in `$FELD_HOME/admin/api-token`. On a daemon restart (`feld daemon restart` and `POST /v1/daemon/restart`), the new daemon takes over the token of the daemon being stopped. The CLI resolves its location from `--home` / `FELD_HOME` and reads it. It allows all Admin operations ([Authorization decisions](#authorization-decisions)).

### Session token

A token that uniquely points to one Session, issued by the daemon when the Session starts or is attached. Every operation called from inside a Session uses it, and the hooks and helpers of the [Runtime Adapter](../concepts/glossary.md#runtime-adapter) use the same token.

The token is the basis of identity. The daemon determines a Message's `from` / `fromSessionId`, a spawned record's `spawner` / `spawnerSessionId`, stop / kill's `requesterSessionId`, and a mailbox read's `sessionId` from the Session token attached to the request. The request body and environment variables play no part in identity, and the caller has no room to declare these.

It is valid only while the Session has no `end` and it is the last token issued to that Session. Requests with the token of a closed Session are rejected, and the Runtime API and requests that determine `spawner` / `from` / the requester return session closed ([Token revocation](#token-revocation)).

Runtime telemetry headers carry a telemetry token instead of the Session token. The telemetry token is a value derived one-way from the Session token (the hex SHA-256 of the string `feld-telemetry:` followed by the Session token), and it is valid for the same period as the Session token. The daemon uses it only for telemetry attribution and does not accept it as an API Bearer ([`otel.md`](otel.md#the-telemetry-token-header)).

### internal token

Used by the Collector for `POST /v1/self/telemetry` and `POST /v1/self/telemetry/token`. The daemon issues it at each startup and places it owner-only only in the Collector configuration (`$FELD_HOME/admin/collector.json`). It shows that the Collector is a component of Feld, and it opens only the telemetry endpoints. Telemetry is attributed to a Session by the telemetry token in the telemetry header ([Authorization decisions](#authorization-decisions)).

### browser token

Used by the Web UI in the browser. It is issued only in exchange for a code through [pairing](#pairing), one per browser. The token consists of two values: the cookie value and the key. The daemon hashes the combined value and saves it in the store, so the token stays valid across daemon start / stop. The two values reach the browser only once, in the pairing response (the cookie value via Set-Cookie, the key in the body), and afterward the daemon accepts the token only when the [cookie and key](#cookie-and-key) are both present. A browser token presented with `Authorization: Bearer` is not accepted.

For each token the daemon keeps an `id` (`browser_<hex>`), a name, the issue time, and the last-used time. The name is given when the pairing code is issued and serves as a marker for listing and revocation. Duplicates are allowed.

It expires at the earlier of 7 days after last use and 30 days after issue. The caller is recorded as Admin, and the allowed operations are the same as the local token except for pairing management and Sign out ([Authorization decisions](#authorization-decisions)).

## pairing

pairing is the procedure that exchanges a short-lived code issued on the daemon's device for a browser token in the browser. A person carries only the code; the browser token never appears on screen, and the cookie value is never exposed to JS. On user-facing surfaces (Web UI, CLI output, README), this authentication is called Sign in, and `POST /v1/ui/unpair` is called Sign out.

1. A local token caller (`feld ui pair`, `feld ui`) issues a pairing code. The code is 8 characters from 31 hard-to-misread characters (digits and uppercase letters, excluding `0`, `1`, `I`, `L`, `O`), shown in the form `K7QM-4XPD`. It is valid for 5 minutes and can be used once. The daemon keeps the code hashed only in memory, and it disappears when the daemon stops
2. When the Web UI is opened in a browser and there is no browser token or a 401 occurs, the pairing screen appears at any address. A person enters the code. `feld ui` opens `/ui/pair#code=<code>`, and the Web UI tries a light read without showing the form. If a valid browser token exists, it moves to the Actors page without using the code
3. If the read returns 401, the Web UI sends the code to `POST /v1/ui/pair`. Matching ignores case and hyphens. The daemon consumes the code, issues a browser token, and returns the cookie value via Set-Cookie and the key in the response body. The Web UI places the key in `localStorage`. If the request carries a valid browser token's cookie and key, the daemon revokes that token before issuing, so re-pairing in the same browser keeps a single token
4. Before moving on, the Web UI tries a read to confirm the browser token works. On 401, it shows on the pairing screen that the browser did not save the cookie or the key
5. Subsequent reads, writes, and SSE pass with the cookie and key

When `POST /v1/ui/pair` fails 5 times in a row, the daemon invalidates all codes valid at that point. A person issues a new code on the daemon's device. A success resets the count.

## cookie and key

The browser token cookie takes the following form.

- Name: `feld_browser_<port>`. `<port>` is the Port the daemon listens on. Cookies are not separated by Port, so the name separates them to keep daemons running on the same host from mixing
- Attributes: `HttpOnly`, `SameSite=Strict`, `Path=/`, `Max-Age=2592000` (30 days from issue). HTTPS requests also get `Secure`. The daemon judges expiry from the token's record
- Where `Secure` applies: from other devices, traffic goes through the HTTPS proxy. `tailscale serve` uses a `*.ts.net` certificate; enable HTTPS certificates in the tailnet admin console. On `http://127.0.0.1` on the daemon's device, `Secure` is omitted so that browsers including Safari can save the cookie

The key compensates for the reach of the cookie. Cookies are not separated by Port, so the browser also sends the cookie to servers on other Ports of the same host. The Web UI places the key in `localStorage` of its own origin (scheme, host, Port) and attaches it to every request to the daemon (including SSE) in the `X-Feld-Browser-Key` header. The origin of a server on another Port cannot read the key. The daemon passes a request as a browser token only when both the cookie and the key are present and the combined value points to a valid token. On Sign out, the Web UI deletes the key.

If a request has an `Authorization` header, the daemon decides by it alone and ignores the cookie and key. For requests authenticated by a browser token other than `GET` and `HEAD`, and for `POST /v1/ui/pair`, the daemon checks `Origin` in addition to `SameSite=Strict` (CSRF protection). If the host and Port of `Origin` do not match the request destination (`X-Forwarded-Host` if present, otherwise `Host`), including when `Origin` is absent, it returns 403. An omitted Port is filled with the scheme's default (80 for `http`, 443 for `https`) before comparison.

## Distributing the Session token

### owned Sessions

For a Session that Feld starts, Feld passes the token at startup as the environment variable `FELD_SESSION_TOKEN`, and the Runtime Adapter attaches the telemetry token to the telemetry header ([otel.md](otel.md#the-telemetry-token-header)). The list of variables is in [plugin.md](plugin.md#what-core-provides). The Session's child processes inherit this environment, so both the `feld` CLI and the Runtime Adapter's components know their own Session from the same variable.

### attached Sessions

For a Session attached to an already running execution, the environment belongs to the execution side, so the CLI that ran attach places the token in a file, and subsequent child processes find it.

1. The response of `POST …/attach` includes the Session token
2. The CLI that ran attach (whether `--self` or `--<provider>-<key>`) saves the token owner-only under `$FELD_HOME`
3. The CLI writes, into a self index named by a key that core builds, the token's location, the incarnation of the Runtime process, and the Session's Actor name. There are two keys: one built from the provider name and the Provider's selector (the Provider key), and one built from the Runtime Adapter name and the Adapter's `self` selector when the Runtime runs its Runtime-side session outside the Runtime process (the Runtime key) ([plugin.md](plugin.md#division-of-work-on-attach)). Identification of the Runtime process is described per Provider in [provider/tmux.md](provider/tmux.md#attach) and [provider/herdr.md](provider/herdr.md#attach). When attaches to the same key overlap, the index is replaced with the new content and the previous token file is removed. At the same time, the CLI removes other self indexes whose Runtime process has ended along with their token files, and other self indexes of the same Session, so the Session's token is found only under the key the last attach wrote
4. A Session's child process computes the same key from the selectors that the Provider's and Runtime Adapter's `self` answer from its own environment, looks up the self index, and obtains the token when the conditions of the [resolution order](#runtime-adapter-resolution-order) are met

The self index is a cache on the Session side, separate from the daemon's records. The daemon hands over the token value only in the attach response, so when it is lost, it is recovered through the [token recovery](#token-recovery) procedure.

### Token recovery

Even if the token file or self index is lost, the token can be recovered for the same Session while the Runtime is running. No new generation is created; the same Session continues.

1. A person attaches again from inside the same Runtime (`/feld attach` for Claude Code). If the Actor name is omitted, it becomes the name of the Actor whose current Session has that Runtime process ([cli.md](../../guide/cli.md))
2. The Provider identifies the Runtime process, and the daemon matches its pid and start time against the Runtime process recorded in the current Session ([provider/tmux.md](provider/tmux.md#attach), [provider/herdr.md](provider/herdr.md#attach))
3. If it matches the current Session of the specified Actor, the daemon reissues a token for that Session, invalidates the old token, and removes the recorded Runtime identity. The response returns the existing Session and the new token. If it matches the current Session of a different Actor, the request is rejected
4. The CLI rewrites the new token owner-only to the same token file and replaces the self index along with the incarnation of the Runtime process. The Runtime key is built from the current Runtime-side session, so re-attaching after the Runtime-side session changes within the same Runtime process (such as a new Codex thread) moves the key to the new session
5. The Runtime's child processes obtain the new token in subsequent resolutions. The Runtime identity becomes the next one delivered with the new token (delivered by attach, or otherwise by the next hook). Components that keep running with the old token (such as the Plugin Monitor helper) redo the [resolution order](#runtime-adapter-resolution-order) when a request with the old token is rejected, continue with the new token if they obtain it, and exit otherwise. Components that received the token through the environment from their launcher (the Codex queue helper) exit when rejected, and a component with the new token takes over delivery ([the Codex attach flow](adapter/codex.md#the-attach-flow))

In an `owned` Session the token is in the Runtime's environment, so this procedure applies to `attached` Sessions. The CLI rejects `feld actor attach --self` from a call whose environment has `FELD_SESSION_TOKEN`.

### Runtime Adapter resolution order

The `feld` CLI and each component of the Runtime Adapter (hooks and helpers) resolve the token in the same order.

1. The environment variable `FELD_SESSION_TOKEN`
2. Otherwise, the self index of the Provider key. It is looked up for each Provider whose `self` answers a selector, and the token is adopted only when the Runtime process recorded by the self index (matching pid and start time) is in its own ancestor chain (parent, its parent, ...)
3. Otherwise, the self index of the Runtime key. It is looked up for each Runtime Adapter whose `self` answers a selector, and the token is adopted only when the Runtime process recorded by the self index is running (matching pid and start time) and a process that the Runtime Adapter's `match` accepts is in its own ancestor chain

The matching in 2 and 3 reads the process table. When the sandbox denies that, the CLI passes the candidates to the daemon through the [spool](spool.md) without matching, and the daemon matches them under the same conditions ([Caller matching](spool.md#caller-verification)). `feld self token` prints the resolution result without calling the daemon, so when it cannot read the process table it answers with 1 only, and if that is absent it shows the reason and exits with exit code 1. The Runtime Adapter's components run outside the Runtime sandbox.

In an `owned` Session 1 applies, and in an `attached` Session 2 or 3 applies, so callers need only one implementation. The self index key is built from the selectors that the Provider's and Runtime Adapter's `self` answer, and the bundled Runtime Adapter components call the same implementation as the `feld` CLI in-process ([plugin.md](plugin.md#what-core-provides)). When no token can be adopted, the `feld` CLI uses the local token and is treated as Admin. Runtime Adapter components act as the Session, so they wait without calling Session APIs until they obtain a token. How each component waits is described in [`adapter/claude-code.md`](adapter/claude-code.md#the-side-beside-the-runtime) and [`adapter/codex.md`](adapter/codex.md#the-side-beside-the-runtime). Through ancestor-chain matching, only descendants of the attached Runtime process use the Provider key's token. Through the `match` check in 3, only processes that belong to the attached Runtime-side session and have that Runtime's process in their ancestor chain use the Runtime key's token. The key is not tied to the incarnation of the process that runs the session (the Codex app-server restarts on auto-update). Another session of the same Runtime looks up a different key and obtains no token. A child process of a Runtime started later in the same pane has no token until that Runtime is attached. Hook / helper settings are described in each Runtime Adapter's document.

## Authorization decisions

For each operation, this section describes which tokens can call it and what the token determines. The local token and the browser token together are called the Admin token. The two allow the same operations except for pairing management and Sign out, and the caller becomes `Admin` with either. The Admin token can call every operation a Session token can call, except the Runtime API (`/v1/self/*`) and the [spool](spool.md) path. The Runtime API returns 403 to the Admin token.

A Session token can call operations on itself, and operations on other Actors are opened only within the same Workspace by the [grants](../concepts/glossary.md#grants) of the Session's Actor. The daemon reads the grants from the store on each request and decides the same way on the TCP and [spool](spool.md) paths. Requests it does not allow return 403.

Operations a Session token can call without grants:

- The Runtime API (`/v1/self/*`, except the telemetry endpoints called with the internal token). The target is the Session the token points to. Mailbox retrieval and read recording are limited to the mailbox of that Session's Actor, and the `sessionId` of a read record is determined from the token. Both `feld self message recv` and the Runtime Adapter's helper go through the same path
- Sending a Message (`POST …/messages`). `from` becomes that Session's Actor. Recipients are limited to Actors in the same Workspace whose [acceptFrom](../concepts/glossary.md#acceptfrom) accepts that Session's Actor. This is checked in the same transaction that places the Message
- Its own Actor, and all information of its Session (including transcript, output, metrics, and records)
- stop / kill of its own Session
- Reading Messages its own Actor sent and Messages addressed to its own Actor. The Message list is also narrowed to these Messages
- Lists and states of Actors and Sessions in the same Workspace (reading and listing Workspaces, Actors, and Sessions, re-observing Sessions, SSE). These exclude transcript, output, metrics, records, and Message bodies. Another Actor's `launchSpec` and its Session's `execution.command` may hold secrets in `env` or arguments, so they are null in responses
- `GET /v1/runtimes`, `GET /v1/providers`, daemon status (`GET /v1/daemon/status`)

Lists and SSE of Workspaces, Sessions, and records are narrowed to the Session's Workspace for a Session token.

Operations on other Actors in the same Workspace opened by grants:

- `read`: transcript, output, metrics, records, Message bodies
- `control`: start, restart, stop, kill
- `manage-agent` / `manage-job`: creating (including replacing a declaration with the same name), updating (including acceptFrom), and deleting Agents / Jobs that have no grants and no `trustWorkdir`. Operations on Actors that have grants or `trustWorkdir` return 403, preventing escalation by rewriting the launchSpec. That the target's kind is one the grant handles, and that the target has no grants and no `trustWorkdir`, are checked in the same transaction as the write

Operations only the Admin token can call (403 for a Session token):

- Creating and deleting Workspaces, deleting Messages, operations on other Workspaces (including sending Messages), Runtime metrics (`GET /v1/runtimes/{runtime}/metrics`, which aggregates Sessions of all Workspaces)
- Adding and removing an Actor's `grants` (creation and update that include `grants`)
- Granting `trustWorkdir` in a launchSpec (creation and update that include `trustWorkdir: true`)
- attach (`POST …/attach`). The response includes the issued Session token
- Stopping (`POST /v1/daemon/shutdown`) and restarting (`POST /v1/daemon/restart`) the daemon
- Pairing management (issuing pairing codes `POST /v1/ui/pairing-codes`, listing browser tokens `GET /v1/ui/browsers` and revoking them `DELETE /v1/ui/browsers/{browser}`): local token only. Also 403 for the browser token

What the token determines, and operations determined by token kind:

- `spawner` of start and restart: the Session's Actor for a Session token, `Admin` for the Admin token
- stop / kill: among authorized requests, only `owned` Sessions accept them. `attached` Sessions have no stop API. `requesterSessionId` is that Session for a Session token, null for the Admin token
- A Message's `from`: the Session's Actor for a Session token, `Admin` for the Admin token
- telemetry (`POST /v1/self/telemetry`) and telemetry token verification (`POST /v1/self/telemetry/token`): internal token only. Telemetry is attributed to a Session by the telemetry token that the Runtime Adapter attached to the telemetry header, and telemetry without a token is discarded. Attribution details are in [`otel.md`](otel.md), and aggregation is in [`metrics.md`](metrics.md)
- pairing (`POST /v1/ui/pair`): requires no token and decides by the pairing code
- Requests on the [spool](spool.md) path: Session token only. The local token, browser token, and internal token return 403
- Sign out (`POST /v1/ui/unpair`): browser token only. It revokes that token and deletes the cookie

The daemon derives the Actor and Workspace of the Session a Session token points to from records.

## Preventing token exposure

Only the daemon receives the Session token. While the daemon is stopped, another user on the same host can take the Port, so only the `feld` CLI, after confirming via `daemon.lock` and `daemon.json` that the daemon is running, sends the Session token to the Port. The Runtime Adapter's hooks and helpers send the Session token to the socket in Feld home. Feld home is owner-only, so only the daemon answers on the socket. When the Feld home path exceeds the socket path length limit, the daemon places the socket in a directory usable only by that user inside the temporary directory (`feld-<uid>`, mode `700`) and writes its location to `daemon.json`. If `feld-<uid>` belongs to another user or is open to other users, startup stops. Runtime telemetry sends only the telemetry token to the Collector's Port.

The browser token value appears only in the pairing response (the cookie value via Set-Cookie, the key in the body), and the pairing code appears only in the issue response. The daemon keeps only hashes of both and does not write the `Cookie` header, the `X-Feld-Browser-Key` header, or codes to logs.

Among daemon responses, the Session token value appears only in the attach response; all other reads, Console, records, Session logs, and `get` mask the value. `FELD_SESSION_TOKEN` passed as the Session environment is masked the same way, and displays of `execution` and Placement show only what was launched and where.

The environment of an `owned` Session is kept only in the Execution Provider's memory. Other users on the same machine can read command lines with `ps`, and Runtimes inside other Sessions' sandboxes can read and write the temporary directory, so the Provider passes the environment only to that pane's launch script over a Unix domain socket ([Passing the launch environment](plugin.md#passing-the-launch-environment)). The launch script loads the environment and then starts the program ([tmux](provider/tmux.md#launch), [herdr](provider/herdr.md#launch)).

In an `owned` Session the token is in the Runtime's process environment and readable by child processes of the same Session. This is by design, so that a Session's child processes act as the Session. The Session token represents the Session's identity, and it fixes the sender identity (a spawned record's `spawner`, a Message's `from`, stop / kill's `requesterSessionId`), the readable mailbox (that of the Session's Actor), and the destination of Runtime identification (that Session). Everything else it can operate on follows [Authorization decisions](#authorization-decisions). In an `attached` Session the token file and self index are placed owner-only and, like the local token, are readable by processes of the same OS user. The integration files of an `owned` Session are placed in a per-Session owner-only directory and include the token as the environment passed to the Runtime (`env` in the Claude Code settings file). Integration files written to a person's configuration root (attach preparation) do not include the token, and the token is resolved at run time through the self index.

## Token revocation

A Session token is valid under two conditions: "the Session has no `end`" and "it is the last token issued to the Session".

- When a Session closes (whether `stopped`, `exited`, or `unknown`), its token becomes invalid from the moment it closes, and requests under [the common rules of `daemon-api.md`](../../guide/daemon-api.md#common-conventions) are rejected as session closed
- When the generation advances, the previous Session is closed, so the previous token is invalid, and a new token is issued to the new Session. For `owned` it is passed as the new environment; for `attached` the CLI that ran attach replaces the self index with the new location
- When the next start passes admission while the observation remains `lost` / `mismatched`, the previous Session closes as `unknown`, and the previous token becomes invalid at the same time. Requests from an old incarnation that may still be alive stop here
- When a token is reissued through [token recovery](#token-recovery), the Session continues without closing, and the old token of the same Session becomes invalid
- An event stream (`GET /v1/events`) opened with a Session token that has become invalid is closed by the daemon before sending the next event. Even without events, it closes within 1 minute

The rules for when a Session closes are in [`session-lifecycle.md`](session-lifecycle.md).

A browser token becomes invalid when it passes its [expiry](#browser-token), when a local token caller revokes it (`feld browser revoke`), when that browser signs out, and when the same browser pairs again. Requests with an invalid browser token return 401, and the daemon closes event streams (`GET /v1/events`) opened with that token (immediately on revocation, within 1 minute after expiry). The Web UI returns to the pairing screen. The browser token does not depend on daemon start / stop or on regeneration of the local token.

The internal token is reissued at each daemon startup, and the previous daemon's token becomes invalid from the next startup. The local token works the same way, except that a restart carries it over, and it becomes invalid with `feld daemon start` after `feld daemon stop`.
