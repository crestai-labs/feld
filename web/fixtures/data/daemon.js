// Hand-written fixture of the daemon the pages read, on the clock of data/clock.js.
// `?state=` of settings.html picks what the page shows: `running` reads `status`, `degraded`
// reads `degradedStatus`, and `unreachable` keeps `lastRead` and the last known `status`.
// `?state=error` of index.html reads `readFailure`, and `?state=refusing` of both pages
// answers the requests with `refusals`. The pairing screen trades the codes of `pairing`.
(function registerDaemonFixture(global) {
  "use strict";
  const ui = global.FeldWebUI = global.FeldWebUI || {};
  ui.data = ui.data || {};

  // GET /v1/daemon/status, with the installed plugins of GET /v1/runtimes and
  // GET /v1/providers. Codex and Herdr are left uninstalled so the Settings page shows both
  // states of the bundled plugins.
  const DAEMON_STATUS = {
    state: "running",
    version: "0.5.0",
    instanceId: "dmn_7f3a91c2",
    pid: 48213,
    home: "/Users/alice/.feld",
    api: "http://127.0.0.1:7381",
    webUi: "http://127.0.0.1:7381/ui/",
    otlp: "http://127.0.0.1:7382",
    startedAt: "2026-09-01T09:12:45+09:00",
    serverTime: "2026-09-02T15:00:02+09:00",
    historyStart: "2026-08-12T10:24:10+09:00",
    timezone: "Asia/Tokyo",
    runtimes: [
      { name: "claude-code", label: "Claude Code" },
    ],
    providers: [
      { name: "tmux", label: "tmux" },
    ],
    nodes: ["Local Node", "Build Node", "Batch Node", "Sandbox Node"],
    restored: { actors: 12, runningSessions: 3, schedules: 2 },
  };

  ui.data.daemon = Object.freeze({
    status: DAEMON_STATUS,
    // The Collector is not running.
    degradedStatus: { ...DAEMON_STATUS, otlp: null },
    // Running Sessions counted from the Session read, per provider and Node.
    runningSessions: [
      { provider: "tmux", node: "Local Node", count: 3 },
    ],
    // The last successful read before the daemon became unreachable.
    lastRead: "2026-09-02T14:52:18+09:00",
    // The message of a failed read.
    readFailure: "503 Service Unavailable",
    // The message of each refused request: the daemon's restart and stop from Settings, and
    // the Actions menu's Start, Stop, Kill, and Delete, with `{actor}` and `{session}` filled in.
    refusals: Object.freeze({
      restart: "daemon cannot restart: start daemon restart: Permission denied (os error 13)",
      shutdown: "daemon cannot shut down",
      start: "Actor '{actor}' changed while its launch was being prepared; retry the start",
      stop: "Session '{session}' is already closed",
      kill: "Session '{session}' is already closed",
      delete: "Actor '{actor}' has a running current Session '{session}'; stop it before the delete",
    }),
    // POST /v1/ui/pair. `codes` are the pairing codes `feld ui pair` issued and the daemon
    // still holds; each works once.
    pairing: Object.freeze({
      codes: Object.freeze(["K7QM-4XPD"]),
    }),
  });
})(window);
