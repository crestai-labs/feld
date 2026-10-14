# Runtime telemetry collection and attribution (OTel)

This document describes how the Collector carries telemetry that a Runtime emits over OTLP to the daemon, and how the daemon attributes it to a Session by the telemetry token and stores it as [Telemetry](../concepts/glossary.md#telemetry). The rules for deriving metrics, quota, and the Metrics lane from stored Telemetry are in [metrics.md](metrics.md), and the API is in the Runtime API (`Self`, `Telemetry`) of the [OpenAPI](../../guide/daemon-api.openapi.yaml).

## Contents

- [End-to-end path](#end-to-end-path)
- [Collector](#collector)
  - [Startup and listen](#startup-and-listen)
  - [Environment passed to the Runtime](#environment-passed-to-the-runtime)
  - [Forwarding to the daemon](#forwarding-to-the-daemon)
- [Attribution to a Session](#attribution-to-a-session)
  - [The telemetry token header](#the-telemetry-token-header)
  - [attached Sessions](#attached-sessions)
  - [Closed Sessions](#closed-sessions)
- [Storing Telemetry](#storing-telemetry)

## End-to-end path

Telemetry flows in the following order. Each stage has only its own role, and the daemon makes the attribution and storage decisions.

1. The Runtime exports telemetry with an OTLP exporter. The destination is the Collector's loopback endpoint that Feld passed at startup, and the request header carries the telemetry token attached by the Runtime Adapter
2. The Collector receives the request, verifies the telemetry token in the header, keeps it as the request's metadata, assigns an ID to the request, and puts it in a per-token queue
3. The Collector calls `POST /v1/self/telemetry` with the internal token and forwards the telemetry and the telemetry token to the daemon
4. The daemon resolves the telemetry token to a Session, attributes the telemetry as is to the Session, stores it as Telemetry, and returns an acknowledge to the Collector
5. Reads of `GET /v1/sessions/{sessionId}/metrics` and `GET /v1/runtimes/{runtime}/metrics` project the stored Telemetry into per-metric series ([metrics.md](metrics.md))

Telemetry affects only metrics and quota. A Session's lifecycle and outcome are determined from the Provider's observation and the Control Plane's records ([session-lifecycle.md](session-lifecycle.md)).

## Collector

### Startup and listen

The Collector is a child process managed by the daemon, one per daemon. At each startup the daemon issues a new internal token, generates the Collector configuration owner-only, then starts the Collector, and stops the Collector when the daemon stops. The Collector is an OTLP/HTTP receiver limited to loopback, listening on per-signal paths (`/v1/logs`, `/v1/metrics`).

The receiver endpoint is kept under `$FELD_HOME`, and the same endpoint is used across daemon restarts. Sessions started before a restart have the same endpoint in their environment, so export resumes once the receiver listens again.

The Collector only forwards; the daemon holds the source-of-truth state and client authority. When the Collector stops or fails to start, the daemon restarts it after an interval. Even when the Collector cannot start, the daemon runs fully apart from the Collector and starts new Sessions without telemetry. The metrics of `agent-runtime` Sessions started during that time are `unavailable`, and `source` holds the reason.

### Environment passed to the Runtime

In an `owned` Session, the Runtime Adapter's `launch` includes in the Runtime's environment or configuration the exporter enablement, the destination endpoint, the protocol, the export interval, and, when the Runtime allows choosing it, the temporality ([time and temporality in metrics.md](metrics.md#time-and-temporality)). For Claude Code this is `OTEL_*` in the environment ([launch arguments and environment](adapter/claude-code.md#launch-arguments-and-environment)); for Codex it is the `otel` configuration passed with `-c` and `OTEL_METRIC_EXPORT_INTERVAL` in the environment ([per-Session configuration](adapter/codex.md#per-session-configuration)). Whether the endpoint includes the receiver path is decided by the Runtime's exporter, and Codex receives paths down to the per-signal level. For a Runtime a person starts themselves, `feld env` passes as shell environment the same `OTEL_*` as the Claude Code launch environment ([cli.md](../../guide/cli.md#feld-env)). A Runtime started from that shell exports to this daemon's Collector. How to attach the header containing the token is described in [The telemetry token header](#the-telemetry-token-header).

The other environment variables for calling Feld (`FELD_API`, `FELD_SESSION_TOKEN`, and so on) are listed in [plugin.md](plugin.md#what-core-provides).

### Forwarding to the daemon

For each export request, the Collector does the following.

- It verifies the telemetry token in the header with the daemon by calling `POST /v1/self/telemetry/token` with the internal token, and caches the answer for a fixed time. Requests with the same token wait for the answer of a single query, and there are upper limits on the number of concurrent queries and on the number of requests waiting for an answer. Requests whose token cannot be resolved to a Session without `end` are discarded with 401, and when the daemon does not answer or a limit is exceeded, it returns 503
- It carries the received request's telemetry token header together with the telemetry as that request's metadata. One request holds the token of only one Session
- It assigns an opaque ID to the request and presents the same ID on every retry
- It calls `POST /v1/self/telemetry` with the internal token and sends the telemetry, the ID, and the telemetry token. The destination is limited to the daemon on loopback
- It waits for the daemon's acknowledge and retries failed requests from an in-memory queue. The queue is split per telemetry token, and there is an upper limit on the number of requests one token can occupy. Requests over the limit get 503, so even if one token's exports overflow, other tokens' exports still enter the queue

The daemon returns the acknowledge after storing the Telemetry. A retry with the same ID returns an acknowledge for the already stored Telemetry, and a request with the same ID but different content is rejected. The queue exists only in memory and the retry period has an upper limit, so telemetry arriving while the Collector is stopped, or while the daemon is stopped longer than the retry period, can be lost. Metrics delivery is best-effort.

## Attribution to a Session

### The telemetry token header

The header carries, instead of the Session token, the [telemetry token](auth.md#session-token) derived from the Session token. The Runtime Adapter defines how to attach the header to fit the Runtime's mechanism. In Claude Code, [`otelHeadersHelper`](adapter/claude-code.md#otelheadershelper), which runs beside the Runtime, returns the telemetry token on every send; in Codex, the startup environment that `launch` placed attaches the same token for the lifetime of the process ([the telemetry header](adapter/codex.md#telemetry-header)).

When the daemon receives `POST /v1/self/telemetry`, it authenticates the Collector with the internal token and determines the Session to attribute to by the telemetry's telemetry token. Only telemetry whose telemetry token matches one derived from the Session token of a Session that is not closed is stored as Telemetry. The internal token opens the telemetry endpoints, and the only basis for attribution is the telemetry token ([Authorization decisions in auth.md](auth.md#authorization-decisions)).

### attached Sessions

The path is the same for `attached` Sessions. The CLI that ran attach places the Session token in the self index, and the Runtimes that can carry the telemetry token in the header are those whose header is attached by a component that obtains the token from the self index. In Claude Code this is `otelHeadersHelper`; `OTEL_*` comes from a shell with `feld env` applied, and `otelHeadersHelper` comes from user settings ([attach preparation](adapter/claude-code.md#preparing-for-attach)). The Codex header is fixed by the startup environment, so the Runtime telemetry of an `attached` Session has no attribution target, and only the [rollout gauge](adapter/codex.md#rollout-gauges) arrives. Telemetry without a token is discarded.

### Closed Sessions

The validity conditions of the Session token are as in [auth.md](auth.md#token-revocation). A replacement starts when the previous Session is closed, or when a `lost` / `mismatched` previous Session is closed in the same transaction as the next Session ([session-lifecycle.md](session-lifecycle.md#transaction-that-closes-the-previous-session)), so the token of a Session that is no longer current is invalid. A request with a new ID carrying the telemetry token of a closed Session, including telemetry generated before `end`, is rejected as unattributed with 422 `telemetry is unattributed`, and the Collector removes rejected requests from retry. A retry of a request stored before closing returns an acknowledge for the stored Telemetry, as in [the resend rules](#forwarding-to-the-daemon).

## Storing Telemetry

The daemon stores telemetry as received, tied to the Session and the request ID, as Telemetry. This is the only stored form of telemetry, and it is kept separate from Session records. Only Telemetry is stored, and all aggregated values are derived at read time ([metrics.md](metrics.md)).

Storage has the following limits.

- The total amount of Telemetry per Session is up to 64 MiB. The size of one Telemetry entry is its body length plus 1 KiB. Beyond the limit, Telemetry is discarded starting from the oldest received time
- Telemetry of a closed Session is deleted 30 days after the Session's `end`. The daemon does this at startup and every hour
