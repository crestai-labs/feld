# design

Collects, unit by unit, internal behavior that is hard to picture from the [CLI](../../guide/cli.md), [daemon API](../../guide/daemon-api.md), and [Web UI views](../../guide/webui.md), in a form people can read, understand, and review. The list of terms and rules has its source of truth in [`concepts/glossary.md`](../concepts/glossary.md); this directory covers the flows and decisions that involve multiple records and the ordering of times.

- [session-lifecycle.md](session-lifecycle.md): the four paths by which a Session starts, admission, stopping, interpreting an end, and recovery after a daemon restart
- [plugin.md](plugin.md): the Plugin directory and manifest, invoking operations, the contracts of Runtime Adapter and Execution Provider, and the division of work on attach
- [provider/tmux.md](provider/tmux.md): deciding the Placement on tmux, launch, judging observation, attach, stopping, and cleanup
- [provider/herdr.md](provider/herdr.md): the same flow on herdr, and the contrast with tmux
- [message-delivery.md](message-delivery.md): from sending a Message to read, deriving waiting, and insertion into the Console
- [auth.md](auth.md): the four token kinds, browser token pairing with its cookie and key, distributing and resolving the Session token, authorization decisions, and revocation
- [spool.md](spool.md): the path by which the CLI inside a Runtime's sandbox passes requests to the daemon through files, verifying the caller, Codex preparation, and guidance when it cannot write
- [adapter/claude-code.md](adapter/claude-code.md): launching `claude`, the hook / Plugin Monitor helper / `otelHeadersHelper` / status line helper, projection into the Console and Sub Agent Session, and the sources of metrics
- [adapter/codex.md](adapter/codex.md): launching `codex`, the hook / queue helper, projecting the rollout file into the Console and Sub Agent Session, and the sources of metrics
- [otel.md](otel.md): the Collector, attributing telemetry to a Session, and storing Telemetry
- [metrics.md](metrics.md): Runtime-independent metric timing, projection, aggregation, Sub Agent attribution, quota, and the Metrics lane
- [webui.md](webui.md): how the Web UI reads its scope, re-reading on notification, how the Console reads, writes, handling 401, and viewer settings
