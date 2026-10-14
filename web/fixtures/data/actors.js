// Hand-written fixture of the Actors page: the scenarios, the Workspaces, and the records
// of Actors, Sessions, Schedules, skipped ticks, Spawns, Messages, and Metrics lanes, on the
// clock of data/clock.js. components/agents.js derives everything it draws from these.
(function registerActorsFixture(global) {
  "use strict";
  const ui = global.FeldWebUI = global.FeldWebUI || {};
  ui.data = ui.data || {};

  const MINUTE = 60 * 1000;
  const HOUR = 60 * MINUTE;
  const DAY = 24 * HOUR;
  const MOCK_NOW = ui.data.clock.now;

  const HISTORICAL_ANCHOR = Date.parse("2026-08-30T15:00:00+09:00");
  const MESSAGE_DENSITY_ANCHOR = Date.parse("2026-08-29T15:00:00+09:00");
  const MESSAGE_DENSITY_WORKERS = Object.freeze(Array.from({ length: 34 }, (_unused, index) => `worker-${String(index + 1).padStart(2, "0")}`));

  // The scenarios `?scenario=` selects: a historical range of `duration` centred on
  // `anchor`. `pinJobs: false` opens with every Job unpinned, so the rows hold only the
  // Actors the scenario is about.
  const VARIANTS = Object.freeze({
    "three-days-ago-1h": Object.freeze({
      anchor: HISTORICAL_ANCHOR,
      duration: HOUR,
    }),
    "three-days-ago-1d": Object.freeze({
      anchor: HISTORICAL_ANCHOR,
      duration: DAY,
    }),
    "message-density-30m": Object.freeze({
      anchor: MESSAGE_DENSITY_ANCHOR,
      duration: 30 * MINUTE,
      pinJobs: false,
    }),
  });

  // Workspaces of the fixture. An Actor's name is unique within its Workspace; its id is
  // unique across the mock, and a fixture Actor's id is its name.
  const WORKSPACES = ["default", "home", "research"];
  const DEFAULT_WORKSPACE = "default";
  const FELD_WORKDIR = "/Users/alice/Dev/feld";

  // The fixture arrays below are mutable so the New actor form and the Job modal can show their
  // mock acceptance as a visible change; a reload restores the fixture.
  //
  // `launchSpec` is the Actor's launch recipe: `runtime` (a Coding Agent Runtime) or
  // `command` (argv), plus `workdir`, an optional `env`, `runtimeSettings` for a Runtime, `provider`, and `node`. `feld actor create` gives
  // one; `feld actor update` changes it. An Actor that only `feld actor attach` created has none,
  // so Feld cannot start a Session for it until one is given. `grants` are what the Admin
  // lets the Actor's Sessions do to the other Actors.
  const ACTORS = [
    { id: "dispatcher", workspace: "default", kind: "job", launchSpec: { command: ["python3", "dispatch.py"], workdir: FELD_WORKDIR, env: { GH_CONFIG_DIR: "/Users/alice/.config/gh-dispatcher", GIT_AUTHOR_NAME: "dispatcher" }, provider: "tmux", node: "Local Node" } },
    // Its launchSpec was updated between the Codex run on tmux and the Claude Code run in
    // Docker: a Session's execution is a copy taken when the Session starts.
    { id: "developer-01", workspace: "default", kind: "agent", launchSpec: { runtime: "claude-code", runtimeSettings: { model: "opus", env: { GITHUB_TOKEN: "ghp_fixture", GIT_AUTHOR_NAME: "developer-01" } }, workdir: "/workspace/feld", env: { NPM_TOKEN: "npm_fixture", CI: "1" }, provider: "docker", node: "Build Node" }, grants: ["read", "manage-agent"] },
    // Agents the dispatcher spawns per PR: Coding Agent Sessions that report back over
    // the mailbox and then ask Feld to stop them (a requested end, so no attention).
    { id: "reviewer-01", workspace: "default", kind: "agent", launchSpec: { runtime: "claude-code", workdir: FELD_WORKDIR, provider: "tmux", node: "Local Node" } },
    { id: "test-runner", workspace: "default", kind: "agent", launchSpec: { runtime: "codex", workdir: FELD_WORKDIR, provider: "tmux", node: "Local Node" } },
    { id: "nightly-summary", workspace: "default", kind: "job", launchSpec: { command: ["python3", "summarize.py", "--digest", "team"], workdir: "/workspace/jobs", provider: "docker", node: "Batch Node" } },
    { id: "planner-01", workspace: "default", kind: "agent", launchSpec: { runtime: "claude-code", workdir: "/Users/alice/Dev/my-project", provider: "tmux", node: "Local Node" } },
    // An Actor of another Workspace, spawned and messaged from the default Workspace
    // with `--workspace research`: the records carry both Workspaces.
    { id: "researcher", workspace: "research", kind: "agent", launchSpec: { runtime: "claude-code", workdir: "/workspace/research", provider: "docker", node: "Sandbox Node" } },
    { id: "docs-writer", workspace: "default", kind: "agent", launchSpec: { runtime: "codex", workdir: "/Users/alice/Dev/my-project", provider: "tmux", node: "Local Node" } },
    { id: "release-check", workspace: "default", kind: "job", launchSpec: { command: ["release-check", "--strict"], workdir: "/workspace/feld", provider: "docker", node: "Build Node" } },
    // A Job without a Schedule: Feld decides no start time for it, so its Sessions start
    // only when the Admin or another Session spawns or starts it.
    { id: "pr-checks", workspace: "default", kind: "job", launchSpec: { command: ["bash", "scripts/pr-checks.sh", "41"], workdir: FELD_WORKDIR, provider: "tmux", node: "Local Node" } },
    // A Job whose declared working directory was removed after the declaration: the
    // Provider refuses the launch, so its Session never begins.
    { id: "stale-checkout", workspace: "default", kind: "job", launchSpec: { command: ["bash", "scripts/smoke.sh"], workdir: "/Users/alice/Dev/feld-issue-12", provider: "tmux", node: "Local Node" } },
    // The person's own Claude Code in another checkout, registered with `/feld attach`
    // into its own Workspace; no launchSpec, so no Start.
    { id: "home-assistant-1", workspace: "home", kind: "agent", launchSpec: null },
    { id: "live-operator", workspace: "default", kind: "agent", launchSpec: { runtime: "claude-code", workdir: FELD_WORKDIR, provider: "tmux", node: "Local Node" } },
    { id: "current-dev", workspace: "default", kind: "agent", launchSpec: { runtime: "claude-code", workdir: FELD_WORKDIR, provider: "tmux", node: "Local Node" } },
    // Agents placed by the two other Providers: a Docker Sandboxes sandbox and a Herdr pane.
    { id: "sandbox-dev", workspace: "default", kind: "agent", launchSpec: { runtime: "claude-code", workdir: "/workspace/feld", provider: "docker-sandbox", node: "Sandbox Node" } },
    { id: "herdr-dev", workspace: "default", kind: "agent", launchSpec: { runtime: "codex", workdir: "/Users/alice/Dev/my-project", provider: "herdr", node: "Local Node" } },
    // An Agent whose current Session the Provider can no longer find (observed `lost`):
    // it stays current until the next start request or a stop / kill closes it.
    { id: "bench-runner", workspace: "default", kind: "agent", launchSpec: { runtime: "claude-code", workdir: "/workspace/bench", provider: "docker", node: "Build Node" } },
    // Agents the person registered from inside their own Claude Code runs with
    // `/feld attach`, which names them after the working directory with a suffix: the
    // first attach in a checkout is `<cwd basename>-1`, a second attach while it is current
    // is `<cwd basename>-2`, and so on; once an Actor's Session has ended, the next attach
    // stacks a new generation on the lowest free suffix rather than declaring another
    // Actor. Three ran at once on Aug 30, so three Actors exist. Attach creates no
    // launchSpec.
    { id: "my-project-1", workspace: "default", kind: "agent", launchSpec: null },
    { id: "my-project-2", workspace: "default", kind: "agent", launchSpec: null },
    { id: "my-project-3", workspace: "default", kind: "agent", launchSpec: null },
    // A Codex pane the person started by hand and attached from outside:
    // `feld actor attach codex-dev --runtime codex --tmux-pane %47`.
    { id: "codex-dev", workspace: "default", kind: "agent", launchSpec: null },
    // A dedicated high-density scenario: one orchestrator exchanges short bursts with
    // 34 workers. Their Sessions live on Aug 29, outside both ordinary preview ranges.
    { id: "orchestrator", workspace: "default", kind: "agent", launchSpec: null },
    ...MESSAGE_DENSITY_WORKERS.map((id) => ({ id, workspace: "default", kind: "agent", launchSpec: null })),
  ];

  // Metrics lanes: the Sessions' metrics (see the Session metrics fixture below)
  // aggregated per Runtime by each metric's own rule and shown as one lane per metric:
  // additive metrics (tokens, cost, counts, active time) are summed over the Sessions of a
  // bin; the cache hit rate, the latency p95, and the context window follow their own rule,
  // and each `derivation` says which. Every value is Runtime telemetry that Claude Code or
  // Codex itself emits through its OpenTelemetry export (metric and event names are quoted
  // verbatim), or a gauge a part beside the Runtime sends to the same Collector: the Claude
  // Code status line helper reads its values from the status line, the Codex hook reads its
  // own from the rollout file.
  // Sub Agent Sessions are not counted: their telemetry is already in the parent Session.
  // The quota lanes are the exception: an account's rate limit belongs to the account, not
  // to any Session; Sessions only report it, so it is never part of a Session's metrics and
  // appears only here. Control Plane facts (current Sessions, mailbox traffic) stay in the
  // Actors table and are not mixed into these lanes. A series without a value carries its
  // `status` (`n/a` or `unavailable`) and the `source` that explains it; a series whose
  // samples stop at `until` has none past it, and one with `gaps` has empty bins inside
  // each [start, end) span, where no Session reported. None is plotted as zero, and each lane
  // reading says which case it is. The product draws the lanes from the per-Session
  // metric bins of the Session metrics reads and the quota lanes from the quota read; the
  // mock draws them from the periodic generator parameters below (metricValueAt in
  // components/agents.js).
  const CLAUDE_QUOTA_SOURCE = "Claude Code status line gauges feld.claude_code.rate_limit.used_percent and .resets_at";
  const CODEX_QUOTA_SOURCE = "Codex rollout gauges feld.codex.rate_limit.used_percent and .resets_at";
  const CLAUDE_WINDOW_SOURCE = "a Claude.ai subscription reports both windows under the names five_hour and seven_day";
  const CODEX_WINDOW_SOURCE = "a ChatGPT subscription reports rate_limits.primary and, on the plans that have one, .secondary; each window is named by its own window_minutes, so 10080 is seven_day";
  const CODEX_CONTEXT_SOURCE = "Codex rollout gauges feld.codex.context_window.used_percent and .size";
  const CODEX_ATTACHED_SOURCE = "Codex telemetry is attributed only in owned Sessions";
  // One lane holds one Runtime and one window, because the two Runtimes report different
  // windows: Claude Code reports five_hour and seven_day together, and this fixture's
  // Codex reports seven_day only. Every reported window shows by
  // default. A lane holds one series per account, since a Node can run Sessions under
  // several accounts and each has its own window and reset; the lane lists every account
  // and draws the ones it shows.
  function claudeQuota(account, window, generator) {
    return {
      provider: "Claude Code",
      account,
      tone: "claude",
      source: `${CLAUDE_QUOTA_SOURCE}{window=${window}} · ${CLAUDE_WINDOW_SOURCE}`,
      showResets: true,
      ...generator,
    };
  }
  // The other lanes hold one series per account as well: Feld groups a Runtime's Sessions
  // by the account each runs under (CLAUDE_ACCOUNT_BY_ACTOR below). A summed metric splits
  // the Runtime's usage by each account's share; a rate, a latency, or a level keeps its
  // range with a phase of its own.
  const CLAUDE_ACCOUNT_SHARES = Object.freeze([["team@example.com", .4], ["personal@example.com", .3], ["research@example.com", .2], ["review-bot@example.com", .1]]);
  function claudeAccounts(series, summed) {
    return CLAUDE_ACCOUNT_SHARES.map(([account, share], index) => ({
      ...series,
      account,
      ...(summed ? { minimum: series.minimum * share, span: series.span * share, variation: series.variation * share } : {}),
      phase: series.phase + index * .9,
    }));
  }
  const QUOTA_DERIVATION = "Feld groups the reports of this Runtime and window by the account of the reporting Session, one series per account, each with its latest reporting Session. Reset markers are the reported resets_at";
  const COUNT_DERIVATION = "summed over every Session of the account in the bin; a Session whose token usage is reported counts 0 when none arrived";

  const METRICS = Object.freeze([
    {
      id: "quota-claude-5h",
      name: "Claude Code quota · 5h",
      caption: "Claude Code · 5-hour window · % used",
      unit: "%",
      maximum: 100,
      kind: "quota",
      defaultVisible: true,
      derivation: QUOTA_DERIVATION,
      series: [
        claudeQuota("team@example.com", "five_hour", { period: 5 * HOUR, resetAnchor: HISTORICAL_ANCHOR, minimum: 7, span: 78, variation: 5, phase: .2 }),
        claudeQuota("personal@example.com", "five_hour", { period: 5 * HOUR, resetAnchor: HISTORICAL_ANCHOR + 95 * MINUTE, minimum: 4, span: 64, variation: 4, phase: 1.1 }),
        claudeQuota("research@example.com", "five_hour", { period: 5 * HOUR, resetAnchor: HISTORICAL_ANCHOR - 70 * MINUTE, minimum: 2, span: 41, variation: 3, phase: 2.3 }),
        claudeQuota("review-bot@example.com", "five_hour", { period: 5 * HOUR, resetAnchor: HISTORICAL_ANCHOR + 40 * MINUTE, minimum: 1, span: 22, variation: 2, phase: .6 }),
      ],
    },
    {
      id: "quota-claude-7d",
      name: "Claude Code quota · 7d",
      caption: "Claude Code · 7-day window · % used",
      unit: "%",
      maximum: 100,
      kind: "quota",
      defaultVisible: true,
      derivation: QUOTA_DERIVATION,
      series: [
        claudeQuota("team@example.com", "seven_day", { period: 7 * DAY, resetAnchor: HISTORICAL_ANCHOR - 12 * MINUTE, minimum: 3, span: 81, variation: 3, phase: 1.4 }),
        claudeQuota("personal@example.com", "seven_day", { period: 7 * DAY, resetAnchor: HISTORICAL_ANCHOR - 2 * DAY, minimum: 2, span: 58, variation: 2, phase: .4 }),
        claudeQuota("research@example.com", "seven_day", { period: 7 * DAY, resetAnchor: HISTORICAL_ANCHOR + 1 * DAY, minimum: 1, span: 37, variation: 2, phase: 2 }),
        claudeQuota("review-bot@example.com", "seven_day", { period: 7 * DAY, resetAnchor: HISTORICAL_ANCHOR - 3 * DAY, minimum: 1, span: 19, variation: 1, phase: 1 }),
      ],
    },
    {
      id: "quota-codex-7d",
      name: "Codex quota · 7d",
      caption: "Codex · 7-day window · % used",
      unit: "%",
      maximum: 100,
      kind: "quota",
      defaultVisible: true,
      derivation: QUOTA_DERIVATION,
      series: [
        {
          provider: "Codex",
          account: "team@example.com",
          tone: "codex",
          source: `${CODEX_QUOTA_SOURCE}{window=seven_day} · ${CODEX_WINDOW_SOURCE}`,
          period: 7 * DAY,
          resetAnchor: HISTORICAL_ANCHOR - 3 * HOUR,
          minimum: 2,
          span: 54,
          variation: 3,
          phase: .8,
          showResets: true,
        },
      ],
    },
    {
      id: "token-usage",
      name: "Token usage",
      caption: "Claude Code · Codex · tokens/min",
      unit: " tok/min",
      maximum: 2600,
      defaultVisible: true,
      derivation: "Feld maps each Runtime's tokens to input, output, cache read, and cache creation, sums them over every Session of the account in the bin, and divides by the bin duration",
      series: [
        ...claudeAccounts({
          provider: "Claude Code",
          tone: "claude",
          source: "OTel metric claude_code.token.usage{type=input|output|cacheRead|cacheCreation}",
          period: 72 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR - 7 * MINUTE,
          minimum: 420,
          span: 1240,
          variation: 310,
          phase: .8,
        }, true),
        {
          provider: "Codex",
          account: "team@example.com",
          tone: "codex",
          source: "OTel histogram codex.turn.token_usage, sum per token_type: input = input − cached_input, cache read = cached_input, cache creation = cache_write_input, output = output",
          gaps: [[HISTORICAL_ANCHOR - 20 * MINUTE, HISTORICAL_ANCHOR - 8 * MINUTE]],
          period: 58 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR + 4 * MINUTE,
          minimum: 610,
          span: 1510,
          variation: 260,
          phase: 2.6,
        },
      ],
    },
    {
      id: "estimated-cost",
      name: "Estimated cost",
      caption: "Claude Code · Codex · USD/hour",
      prefix: "$",
      unit: "/h",
      decimals: 2,
      maximum: 6,
      defaultVisible: true,
      derivation: "The Runtime's own estimate, summed over every Session of the account in the bin; not a billing record",
      series: [
        ...claudeAccounts({
          provider: "Claude Code",
          tone: "claude",
          source: "OTel metric claude_code.cost.usage (USD)",
          gaps: [[HISTORICAL_ANCHOR + 5 * MINUTE, HISTORICAL_ANCHOR + 17 * MINUTE]],
          period: 66 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR - 9 * MINUTE,
          minimum: .8,
          span: 2.9,
          variation: .7,
          phase: 1.3,
        }, true),
        {
          provider: "Codex",
          account: "team@example.com",
          tone: "codex",
          source: "OTel metric codex.turn.cost_microusd ÷ 1,000,000 (USD)",
          until: MOCK_NOW - 2 * HOUR,
          period: 54 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR + 6 * MINUTE,
          minimum: 1.1,
          span: 3.4,
          variation: .6,
          phase: 3.9,
        },
      ],
    },
    {
      id: "api-errors",
      name: "API errors",
      caption: "Claude Code · Codex · errors/hour",
      unit: "/h",
      maximum: 12,
      defaultVisible: true,
      derivation: `Failed model API requests ${COUNT_DERIVATION}`,
      series: [
        ...claudeAccounts({
          provider: "Claude Code",
          tone: "claude",
          source: "OTel event claude_code.api_error, counted",
          period: 3 * HOUR,
          resetAnchor: HISTORICAL_ANCHOR - 44 * MINUTE,
          minimum: 0,
          span: 0,
          variation: 0,
          phase: 5.2,
        }, true),
        { provider: "Codex", tone: "codex", status: "unavailable", source: CODEX_ATTACHED_SOURCE },
      ],
    },
    {
      id: "cache-hit-rate",
      name: "Prompt cache hit rate",
      caption: "Claude Code · Codex · derived",
      unit: "%",
      maximum: 100,
      defaultVisible: false,
      derivation: "Feld sums each token kind over every Session of the account in the bin, then takes cache_read / (input + cache_read + cache_creation); a bin whose denominator is 0 is empty",
      series: [
        ...claudeAccounts({
          provider: "Claude Code",
          tone: "claude",
          source: "Token usage from OTel metric claude_code.token.usage",
          period: 2 * HOUR,
          resetAnchor: HISTORICAL_ANCHOR + 27 * MINUTE,
          minimum: 54,
          span: 31,
          variation: 8,
          phase: 1.1,
        }, false),
        {
          provider: "Codex",
          account: "team@example.com",
          tone: "codex",
          source: "Token usage from OTel histogram codex.turn.token_usage",
          period: 95 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR - 15 * MINUTE,
          minimum: 38,
          span: 36,
          variation: 9,
          phase: 3.3,
        },
      ],
    },
    {
      id: "request-latency",
      name: "API request latency p95",
      caption: "Claude Code · Codex · ms",
      unit: " ms",
      maximum: 3200,
      defaultVisible: false,
      derivation: "Feld computes p95 from every latency observation of every Session of the account in the bin, interpolating within the bucket where the Runtime reports a histogram",
      series: [
        ...claudeAccounts({
          provider: "Claude Code",
          tone: "claude",
          source: "OTel event claude_code.api_request{duration_ms}",
          period: 53 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR + 9 * MINUTE,
          minimum: 620,
          span: 1680,
          variation: 480,
          phase: 2.9,
        }, false),
        {
          provider: "Codex",
          account: "team@example.com",
          tone: "codex",
          source: "OTel histogram codex.turn.ttft.duration_ms (time to first token of a turn)",
          period: 47 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR - 4 * MINUTE,
          minimum: 540,
          span: 1450,
          variation: 400,
          phase: 1.4,
        },
      ],
    },
    {
      id: "tool-calls",
      name: "Tool calls",
      caption: "Claude Code · Codex · calls/min",
      unit: "/min",
      maximum: 12,
      defaultVisible: false,
      derivation: `Tool calls ${COUNT_DERIVATION}`,
      series: [
        ...claudeAccounts({
          provider: "Claude Code",
          tone: "claude",
          source: "OTel event claude_code.tool_result, counted",
          period: 41 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR - 2 * MINUTE,
          minimum: 1.5,
          span: 6,
          variation: 1.8,
          phase: 4.4,
        }, true),
        {
          provider: "Codex",
          account: "team@example.com",
          tone: "codex",
          source: "OTel metric codex.tool.call, summed",
          period: 36 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR + 11 * MINUTE,
          minimum: 2,
          span: 5.5,
          variation: 1.6,
          phase: 1.9,
        },
      ],
    },
    {
      id: "context-utilization",
      name: "Context window utilization",
      caption: "Claude Code · Codex · % of window",
      unit: "%",
      maximum: 100,
      defaultVisible: false,
      derivation: "Feld takes each Session's latest value in the bin and shows the largest of the account's Sessions, with the Session it came from",
      series: [
        ...claudeAccounts({
          provider: "Claude Code",
          tone: "claude",
          source: "Claude Code status line gauges feld.claude_code.context_window.used_percent and .size",
          period: 84 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR - 21 * MINUTE,
          minimum: 24,
          span: 58,
          variation: 7,
          phase: .4,
        }, false),
        {
          provider: "Codex",
          account: "team@example.com",
          tone: "codex",
          source: CODEX_CONTEXT_SOURCE,
          period: 66 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR - 8 * MINUTE,
          minimum: 18,
          span: 61,
          variation: 6,
          phase: 2.2,
        },
      ],
    },
    {
      id: "active-time",
      name: "Active time",
      caption: "Claude Code only · active seconds/min",
      unit: " s/min",
      maximum: 180,
      defaultVisible: false,
      derivation: "Feld sums the active seconds of every Session of the account in the bin and divides by the bin duration; with several Sessions it exceeds 60 s/min",
      series: [
        ...claudeAccounts({
          provider: "Claude Code",
          tone: "claude",
          source: "OTel metric claude_code.active_time.total, type=user and type=cli summed (seconds)",
          period: 49 * MINUTE,
          resetAnchor: HISTORICAL_ANCHOR - 6 * MINUTE,
          minimum: 40,
          span: 80,
          variation: 22,
          phase: 3.7,
        }, true),
        { provider: "Codex", tone: "codex", status: "n/a", source: "Not provided by Codex" },
      ],
    },
  ]);

  function at(dayOffset, hour, minute) {
    const start = Date.parse("2026-08-30T00:00:00+09:00");
    return start + dayOffset * DAY + hour * HOUR + minute * MINUTE;
  }

  // Placement records mirror what the Execution Binding commits for one Session: the
  // Execution Provider, the Node, the working directory, and the Provider's own blob.
  // Feld reads two things out of that blob, both answered by the Provider: `summary`,
  // how its resource reads, and `attachCommand`, how a person reaches it. The page shows
  // those and never looks inside, so no Provider's own axes appear here.
  // `placement: null` marks a Runtime-internal Sub Agent, for which Feld records no
  // Binding at all.
  function placementOf(provider, node, workdir, summary, attachCommand = null) {
    return { provider, node, workdir, summary, attachCommand };
  }

  const FELD_SOCKET = Object.freeze({ kind: "name", value: "feld" });
  const USER_SOCKET = Object.freeze({ kind: "path", value: "/private/tmp/tmux-501/default" });

  // tmux reads its resource as the unit, the stable ID it recorded (`$1`, `@7`, `%3`), the
  // name tmux answers for that ID when the Placement is read, and whether Feld `created`
  // it or attached to one that already `existing`ly ran. Its attach command puts a
  // terminal on the same stable ID.
  function tmuxPlacement(node, workdir, socket, unit, id, name, origin = "created") {
    const selector = socket.kind === "path" ? `-S ${socket.value}` : `-L ${socket.value}`;
    return placementOf("tmux", node, workdir, `${unit} ${id} (${name}, ${origin})`, `tmux ${selector} attach-session -t '${id}'`);
  }

  // An attached Session runs in a pane that already existed. Feld never tears an existing
  // resource down.
  function existingPanePlacement(node, workdir, socket, pane, name) {
    return tmuxPlacement(node, workdir, socket, "pane", pane, name, "existing");
  }

  function dockerPlacement(node, workdir, container) {
    return placementOf("docker", node, workdir, `container ${container} (created)`);
  }

  function dockerSandboxPlacement(node, workdir, sandbox) {
    return placementOf("docker-sandbox", node, workdir, `sandbox ${sandbox} (created)`);
  }

  function herdrPlacement(node, workdir, workspace, pane) {
    return placementOf("herdr", node, workdir, `pane ${pane} in workspace ${workspace} (created)`);
  }

  // ---- Session metrics ------------------------------------------------------------------
  // Runtime telemetry attributed to a Session. Feld stores it as the Session's Telemetry
  // under the Session's token, apart from the Session record, and projects it into one
  // series per metric when read; the mock keeps that projection inline. Each metric carries its
  // own `status` and the `source` that explains it, decided in this order:
  //   n/a          a program Session (no Runtime Adapter attributes telemetry to it, even
  //                when the program calls a model itself); a Sub Agent Session, whose
  //                telemetry is counted in its parent Session; or a Runtime that does not
  //                provide the metric
  //   unavailable  no usable Runtime telemetry correlated with the Session (an attached
  //                Codex, whose export header was fixed at launch without a token,
  //                keeps only the gauges its hook reads out of the rollout file)
  //   reported     telemetry exists; `samples` are spaced evenly over the Session
  // Only `reported` Sessions enter an aggregate, so the band histogram, the navigator, and
  // the Metrics lanes never count a Sub Agent Session on top of its parent.
  // Token usage samples are [input, output, cache_read, cache_creation] per interval; the
  // total is derived at read time, never stored. Context window samples are the used
  // percentage of the reported window `size`: it rises as the conversation grows and drops
  // where the Runtime compacted it. Account rate limits (5-hour / 7-day quota) belong to
  // the account, not to a Session, and appear only in the Metrics lanes below the timeline.

  function claudeMetrics(tokenSamples, contextSamples) {
    return {
      tokens: { status: "reported", source: "Claude Code Runtime telemetry · claude_code.token.usage", samples: tokenSamples },
      context: { status: "reported", source: "Claude Code status line · feld.claude_code.context_window.used_percent", size: 200000, samples: contextSamples },
    };
  }

  function codexMetrics(tokenSamples, contextSamples = tokenSamples.map((_, interval) => 6 + interval * 7)) {
    return {
      tokens: { status: "reported", source: "Codex Runtime telemetry · codex.turn.token_usage", samples: tokenSamples },
      context: codexContext(contextSamples),
    };
  }

  // A Codex a person started exports without a token, so only what the hook reads out
  // of the rollout file reaches its Session.
  function attachedCodexMetrics(contextSamples) {
    return {
      tokens: { status: "unavailable", source: CODEX_ATTACHED_SOURCE },
      context: codexContext(contextSamples),
    };
  }

  function codexContext(samples) {
    return { status: "reported", source: `Codex rollout · ${CODEX_CONTEXT_SOURCE}`, size: 258400, samples };
  }

  function unavailableMetrics(source, contextSource = source) {
    return { tokens: { status: "unavailable", source }, context: { status: "unavailable", source: contextSource } };
  }

  function notApplicableMetrics(source) {
    return Object.freeze({ tokens: Object.freeze({ status: "n/a", source }), context: Object.freeze({ status: "n/a", source }) });
  }

  // A program execution has no Runtime Adapter, so nothing is attributed to it even when
  // the program calls a model itself (nightly-summary does).
  const PROGRAM_METRICS = notApplicableMetrics("Not applicable: a program execution has no Runtime Adapter attributing telemetry to it");
  // A Sub Agent runs inside its parent's Runtime process with the parent's exporter and
  // token, so its telemetry is in the parent Session's metrics.
  const SUB_AGENT_METRICS = notApplicableMetrics("Included in the parent Session");

  // Session evidence is the Provider `observation`: the latest kind, the instant it was
  // last observed (`at`), and, where the Timeline reads it, the instant the kind was
  // recorded (`since`). Feld writes an `observation` record only when the kind changes
  // (the first `running` at `lifecycle.startSettled` included); `observations` lists those
  // changes where the fixture keeps more than the derived first and last. Feld keeps
  // no Session-originated report and derives no interpretation of its own beyond the
  // outcome below.
  //
  // `ownership` is a Session fact fixed at start: `owned` for a start, replacement, or
  // schedule start, `attached` for an attach. It decides whether Feld can stop or kill
  // the Session and how an unrequested end is read.
  //
  // Lifecycle end is two Control Plane facts, not one outcome. `endKind` says how the
  // Session ended: `running` (no end), `stopped` (ended after a recorded `feld session stop` or
  // `feld session kill`, kept in `lifecycle.stop` / `lifecycle.kill`), `exited` (ended with no
  // such request), or `unknown` (the observation could not prove the end; `end` is then
  // the instant Feld stopped tracking it, at the next start request or a stop / kill).
  // `exitCode` is the recorded exit status where the Provider reported one; null means it
  // reported none. What that pair means is derived at render time from the Session's
  // ownership and the Actor's kind (see outcomeFor): the same `exited` with code 0 is a
  // normal completion for an owned Job, an unrequested exit for an owned Agent, and
  // simply the end of an attached Session, which Feld could not have stopped.
  // A program Session (Job) has no Runtime Adapter: Feld records the argv it
  // launched, the stdout and stderr the Provider captured (its Console is a log view), and
  // the exit code. Model and tokens are N/A even when the program calls a model itself.
  // An attached Session records no command: Feld did not start it. An owned agent-runtime
  // Session may record none either: its Runtime Adapter builds the command.
  const SESSIONS = [
    // dispatcher was created paused on Aug 30 for the Project 10 kickoff and run by hand
    // once (Start session; its first generation, so startKind start); it was resumed on
    // Sep 2 (see SCHEDULES).
    { id: "session_D0", actor: "dispatcher", start: at(0, 14, 38), end: at(0, 15, 24), startKind: "start", ownership: "owned", endKind: "exited", exitCode: 0, execution: { type: "program", label: "Python program", command: "python3 dispatch.py", ai: false }, model: null, placement: tmuxPlacement("Local Node", FELD_WORKDIR, FELD_SOCKET, "session", "$10", "feld-default-dispatcher-1"), metrics: PROGRAM_METRICS, observation: { kind: "gone", at: at(0, 15, 24) } },
    // The first tick after the resume (13:43) ran and ended on its own; the next (14:13) is
    // still current at now, so the 14:43 tick was skipped (see SCHEDULE_SKIPS).
    { id: "session_D4", actor: "dispatcher", start: MOCK_NOW - 77 * MINUTE, planned: MOCK_NOW - 77 * MINUTE, end: MOCK_NOW - 74 * MINUTE, startKind: "schedule", ownership: "owned", endKind: "exited", exitCode: 0, execution: { type: "program", label: "Python program", command: "python3 dispatch.py", ai: false }, model: null, placement: tmuxPlacement("Local Node", FELD_WORKDIR, FELD_SOCKET, "session", "$20", "feld-default-dispatcher-3"), metrics: PROGRAM_METRICS, observation: { kind: "gone", at: MOCK_NOW - 74 * MINUTE }, lifecycle: { startSettled: MOCK_NOW - 77 * MINUTE + 410 } },
    { id: "session_D3", actor: "dispatcher", start: MOCK_NOW - 47 * MINUTE + 380, planned: MOCK_NOW - 47 * MINUTE, end: null, startKind: "schedule", ownership: "owned", endKind: "running", exitCode: null, execution: { type: "program", label: "Python program", command: "python3 dispatch.py", ai: false }, model: null, placement: tmuxPlacement("Local Node", FELD_WORKDIR, FELD_SOCKET, "session", "$21", "feld-default-dispatcher-4"), metrics: PROGRAM_METRICS, observation: { kind: "running", at: MOCK_NOW - MINUTE }, lifecycle: { startSettled: MOCK_NOW - 47 * MINUTE + 380 + 390 } },
    // An Agent whose process ended with no stop or kill recorded: Codex crashed. The exit
    // code is kept, but for an Agent the unrequested exit itself is what needs attention.
    // Its token usage includes the tokens of its two Sub Agents below.
    { id: "session_D1", actor: "developer-01", start: at(0, 14, 50), end: at(0, 15, 8), startKind: "start", ownership: "owned", endKind: "exited", exitCode: 1, execution: { type: "agent-runtime", label: "Codex", command: "codex", ai: true }, model: ["Codex", "codex-sol-high"], placement: tmuxPlacement("Local Node", FELD_WORKDIR, FELD_SOCKET, "window", "@4", "feld:1"), metrics: codexMetrics([[37, 17, 48, 8], [81, 38, 102, 19], [176, 75, 206, 38], [244, 100, 269, 52], [412, 173, 448, 87], [513, 219, 541, 107], [378, 178, 393, 81], [61, 28, 77, 14]]), observation: { kind: "gone", at: at(0, 15, 8) } },
    // Runtime-internal Sub Agents of that Codex Session: their start, end, and parent are
    // the Runtime's transcript. Feld records no lifecycle, Provider observation, or
    // Binding for them, so startKind is sub-agent and ownership, endKind, and observation are null.
    // Their telemetry arrives in the parent's Telemetry, so their metrics are N/A.
    { id: "session_D1_S1", actor: "developer-01", subAgent: "explore-tests", parentSession: "session_D1", start: at(0, 14, 55), end: at(0, 15, 4), startKind: "sub-agent", ownership: null, endKind: null, exitCode: null, execution: { type: "agent-runtime", label: "Codex · Runtime-internal Sub Agent", command: "runtime internal", ai: true }, model: ["Codex", "codex-sol-high"], placement: null, metrics: SUB_AGENT_METRICS, observation: null },
    { id: "session_D1_S2", actor: "developer-01", subAgent: "explore-api", parentSession: "session_D1", start: at(0, 14, 58), end: at(0, 15, 6), startKind: "sub-agent", ownership: null, endKind: null, exitCode: null, execution: { type: "agent-runtime", label: "Codex · Runtime-internal Sub Agent", command: "runtime internal", ai: true }, model: ["Codex", "codex-sol-high"], placement: null, metrics: SUB_AGENT_METRICS, observation: null },
    // session_D2 follows one recorded run (dev-issue-22, 2026-09-09): its
    // Console shows that run's Claude Code transcript file, and its records below keep the
    // run's instants relative to the accepted start, which is placed at 15:12:00.
    // `recordingOrigin` is the run's instant that coincides with `start`, so the Console can
    // show the recording on the fixture clock without editing the transcript file.
    { id: "session_D2", actor: "developer-01", start: at(0, 15, 12), end: at(0, 15, 12) + 274754, startKind: "replacement", ownership: "owned", endKind: "stopped", exitCode: 129, execution: { type: "agent-runtime", label: "Claude Code", command: "claude", ai: true }, model: ["Claude Code", "claude-fable-5-1"], placement: dockerPlacement("Build Node", "/workspace/feld", "7c1e4b9a02d3"), metrics: claudeMetrics([[33, 14, 64, 9], [86, 37, 163, 24], [72, 31, 137, 20], [114, 49, 215, 32], [50, 21, 95, 14]], [21, 33, 47, 61, 72, 38, 46]), observation: { kind: "gone", at: at(0, 15, 12) + 274754 }, recordingOrigin: "2026-09-09T01:54:25.741Z", lifecycle: { startSettled: at(0, 15, 12) + 46, stop: { accepted: at(0, 15, 12) + 273955, requesterSessionId: null, attempt: "Docker accepted a stop of the Session's container", observed: "the container has exited and the process table no longer lists the Runtime" } } },
    // Agents spawned per PR that asked Feld to stop them when done: the Session's own
    // `feld session stop` is a recorded request (its requester is the Session itself), so the
    // end is `stopped`, not an unrequested exit.
    { id: "session_R0", actor: "reviewer-01", start: at(0, 15, 2), end: at(0, 15, 41), startKind: "start", ownership: "owned", endKind: "stopped", exitCode: 0, execution: { type: "agent-runtime", label: "Claude Code", command: "claude", ai: true }, model: ["Claude Code", "claude-sonnet"], placement: tmuxPlacement("Local Node", FELD_WORKDIR, FELD_SOCKET, "window", "@5", "feld:2"), metrics: claudeMetrics([[21, 9, 35, 5], [39, 18, 63, 10], [72, 33, 116, 19], [63, 29, 102, 16], [102, 47, 164, 27], [81, 37, 131, 21], [57, 26, 92, 15], [36, 16, 59, 9]], [11, 18, 26, 35, 44, 52, 61, 69]), observation: { kind: "gone", at: at(0, 15, 41) }, lifecycle: { stop: { accepted: at(0, 15, 40) + 30000, requesterSessionId: "session_R0", attempt: "tmux sent /exit to the pane", observed: "tmux and the process table both show that this exact incarnation no longer exists" } } },
    { id: "session_T0", actor: "test-runner", start: at(0, 15, 18), end: at(0, 15, 46), startKind: "start", ownership: "owned", endKind: "stopped", exitCode: 0, execution: { type: "agent-runtime", label: "Codex", command: "codex", ai: true }, model: ["Codex", "codex-terra"], placement: tmuxPlacement("Local Node", FELD_WORKDIR, FELD_SOCKET, "window", "@6", "feld:3"), metrics: codexMetrics([[21, 12, 23, 4], [32, 18, 33, 7], [43, 24, 44, 9], [93, 52, 95, 20], [64, 36, 66, 14], [108, 60, 108, 24], [57, 32, 59, 12]]), observation: { kind: "gone", at: at(0, 15, 46) }, lifecycle: { stop: { accepted: at(0, 15, 45) + 30000, requesterSessionId: "session_T0", attempt: "tmux sent /quit to the pane", observed: "tmux and the process table both show that this exact incarnation no longer exists" } } },
    // nightly-summary is a program that calls a model itself (`ai: true`); with no
    // Runtime Adapter its model and tokens are still N/A. This earlier scheduled run was
    // stopped by an operator: the exit code (143, SIGTERM) is recorded but a requested
    // end is `stopped`, not `failed`.
    { id: "session_NS0", actor: "nightly-summary", start: at(0, 3, 27), planned: at(0, 3, 27), end: at(0, 3, 31), startKind: "schedule", ownership: "owned", endKind: "stopped", exitCode: 143, execution: { type: "program", label: "Python program", command: "python3 summarize.py --digest team", ai: true }, model: null, placement: dockerPlacement("Batch Node", "/workspace/jobs", "e91b7d4c5a20"), metrics: PROGRAM_METRICS, observation: { kind: "gone", at: at(0, 3, 31) }, lifecycle: { stop: { accepted: at(0, 3, 30) + 40000, requesterSessionId: null, attempt: "docker accepted a stop of the run's container", observed: "the container has exited and the process table no longer lists the program" } } },
    { id: "session_NS", actor: "nightly-summary", start: at(0, 15, 27), planned: at(0, 15, 27), end: at(0, 15, 36), startKind: "schedule", ownership: "owned", endKind: "exited", exitCode: 0, execution: { type: "program", label: "Python program", command: "python3 summarize.py --digest team", ai: true }, model: null, placement: dockerPlacement("Batch Node", "/workspace/jobs", "a4d8c2f17b63"), metrics: PROGRAM_METRICS, observation: { kind: "gone", at: at(0, 15, 36) } },
    // Agents that ended the ordinary way: a `feld session stop` was recorded before the process ended.
    { id: "session_P0", actor: "planner-01", start: at(0, 5, 30), end: at(0, 8, 10), startKind: "start", ownership: "owned", endKind: "stopped", exitCode: 0, execution: { type: "agent-runtime", label: "Claude Code", command: "claude", ai: true }, model: ["Claude Code", "claude-opus"], placement: tmuxPlacement("Local Node", "/Users/alice/Dev/my-project", USER_SOCKET, "window", "@14", "main:5"), metrics: claudeMetrics([[33, 23, 64, 10], [46, 32, 88, 14], [67, 46, 127, 20], [57, 39, 107, 17], [101, 70, 188, 31], [80, 55, 151, 24], [114, 79, 212, 35], [72, 50, 136, 22]], [14, 24, 36, 49, 61, 73, 84, 41, 53, 64, 76, 83]), observation: { kind: "gone", at: at(0, 8, 10) }, lifecycle: { stop: { accepted: at(0, 8, 9) + 30000, requesterSessionId: null, attempt: "tmux sent /exit to the pane", observed: "tmux and the process table both show that this exact incarnation no longer exists" } } },
    // Spawned from the person's attached Session (session_FP2 of my-project-2), then
    // stopped by the same person.
    { id: "session_RS", actor: "researcher", start: at(0, 16, 50), end: at(0, 18, 20), startKind: "start", ownership: "owned", endKind: "stopped", exitCode: 0, execution: { type: "agent-runtime", label: "Claude Code", command: "claude", ai: true }, model: ["Claude Code", "claude-opus"], placement: dockerPlacement("Sandbox Node", "/workspace/research", "5b2f9e7d3c41"), metrics: claudeMetrics([[30, 16, 46, 8], [63, 33, 98, 16], [54, 28, 84, 14], [96, 51, 148, 25], [135, 72, 207, 36], [114, 60, 176, 30], [156, 83, 240, 41], [99, 52, 153, 26]], [10, 22, 35, 47, 58, 70, 81, 36, 49, 62]), observation: { kind: "gone", at: at(0, 18, 20) }, lifecycle: { stop: { accepted: at(0, 18, 19) + 40000, requesterSessionId: "session_FP2", attempt: "docker accepted a stop of the Session's container", observed: "the container has exited and the process table no longer lists the Runtime" } } },
    // An Agent that exited cleanly (code 0) with no stop recorded, for example `/quit` typed
    // into Codex. For an Agent that is still an unrequested exit and its band needs attention.
    { id: "session_W0", actor: "docs-writer", start: at(0, 20, 0), end: at(0, 22, 40), startKind: "start", ownership: "owned", endKind: "exited", exitCode: 0, execution: { type: "agent-runtime", label: "Codex", command: null, ai: true }, model: ["Codex", "codex-terra"], placement: tmuxPlacement("Local Node", "/Users/alice/Dev/my-project", USER_SOCKET, "pane", "%31", "main:2"), metrics: codexMetrics([[25, 17, 32, 6], [51, 35, 62, 12], [73, 50, 89, 18], [60, 41, 74, 15], [118, 81, 142, 29], [92, 63, 112, 23], [131, 90, 157, 32], [83, 57, 100, 20]]), observation: { kind: "gone", at: at(0, 22, 40) } },
    // The next generation of docs-writer: `feld session stop` sent /quit, Codex did not end, and
    // `feld session kill` two minutes later did (exit 137). Both requests are recorded; the end
    // is `stopped`.
    { id: "session_W1", actor: "docs-writer", start: at(0, 23, 10), end: at(0, 23, 52), startKind: "replacement", ownership: "owned", endKind: "stopped", exitCode: 137, execution: { type: "agent-runtime", label: "Codex", command: "codex", ai: true }, model: ["Codex", "codex-terra"], placement: tmuxPlacement("Local Node", "/Users/alice/Dev/my-project", USER_SOCKET, "pane", "%32", "main:2"), metrics: codexMetrics([[22, 15, 30, 5], [48, 33, 60, 11], [70, 48, 85, 17], [58, 40, 71, 14], [95, 66, 116, 24], [74, 51, 90, 18]]), observation: { kind: "gone", at: at(0, 23, 52) }, lifecycle: { startSettled: at(0, 23, 10) + 520, stop: { accepted: at(0, 23, 49) + 30000, requesterSessionId: null, attempt: "tmux sent /quit to the pane", observed: "the pane and its process are still present" }, kill: { accepted: at(0, 23, 51) + 40000, requesterSessionId: null, attempt: "tmux killed the pane's process (SIGKILL)", observed: "tmux and the process table both show that this exact incarnation no longer exists" } } },
    // A native binary on a Schedule: token usage is not applicable. The Provider lost its
    // container at 04:05 and the Session stayed current, `lost`, until the 15:20 tick was
    // admitted: Feld then closed it as `unknown` (`end` is that instant) and started
    // session_RC1.
    { id: "session_RC", actor: "release-check", start: at(0, 3, 20), planned: at(0, 3, 20), end: at(0, 15, 20), startKind: "schedule", ownership: "owned", endKind: "unknown", exitCode: null, execution: { type: "program", label: "Native program", command: "release-check --strict", ai: false }, model: null, placement: dockerPlacement("Build Node", "/workspace/feld", "d06a3e8b1f92"), metrics: PROGRAM_METRICS, observation: { kind: "lost", since: at(0, 4, 5), at: at(0, 15, 19) }, observations: [{ kind: "running", at: at(0, 3, 20) + 700 }, { kind: "lost", at: at(0, 4, 5) }], lifecycle: { startSettled: at(0, 3, 20) + 700 } },
    // The next scheduled run ended on its own with exit 2: for a Job that is `failed`.
    { id: "session_RC1", actor: "release-check", start: at(0, 15, 20) + 512, planned: at(0, 15, 20), end: at(0, 15, 24), startKind: "schedule", ownership: "owned", endKind: "exited", exitCode: 2, execution: { type: "program", label: "Native program", command: "release-check --strict", ai: false }, model: null, placement: dockerPlacement("Build Node", "/workspace/feld", "38f7c1a9e6d4"), metrics: PROGRAM_METRICS, observation: { kind: "gone", at: at(0, 15, 24) } },
    // A Job without a Schedule, spawned once from the CLI for PR #41: no planned instant,
    // no tick, and the run ended on its own with exit 0, which for an owned Job is
    // `succeeded`. Its Console is the captured stdout and stderr (data/session-logs.js).
    { id: "session_PC", actor: "pr-checks", start: at(0, 15, 6), end: at(0, 15, 15), startKind: "start", ownership: "owned", endKind: "exited", exitCode: 0, execution: { type: "program", label: "Shell program", command: "bash scripts/pr-checks.sh 41", ai: false }, model: null, placement: tmuxPlacement("Local Node", FELD_WORKDIR, FELD_SOCKET, "session", "$11", "feld-default-pr-checks-1"), metrics: PROGRAM_METRICS, observation: { kind: "gone", at: at(0, 15, 15) }, lifecycle: { startSettled: at(0, 15, 6) + 350 } },
    // A start that never began: the Provider answered the launch with `gone`, its own
    // account of why, and no resource, so Feld recorded `lifecycle.startFailed`. The
    // Console names the row `Start failed` and carries the Provider's detail, rather than
    // folding it into a generic `gone`.
    { id: "session_SC", actor: "stale-checkout", start: at(0, 15, 24), end: at(0, 15, 24) + 240, startKind: "start", ownership: "owned", endKind: "exited", exitCode: null, execution: { type: "program", label: "Shell program", command: "bash scripts/smoke.sh", ai: false }, model: null, placement: placementOf("tmux", "Local Node", "/Users/alice/Dev/feld-issue-12", "session pending"), metrics: PROGRAM_METRICS, observation: { kind: "gone", at: at(0, 15, 24) + 240, detail: "invalid tmux provider request: prepared workdir must still be an existing absolute directory" }, lifecycle: { startFailed: at(0, 15, 24) + 240 } },
    // The person's own Claude Code in another checkout, left open for days and registered
    // with `/feld attach`, so the Session is `attached`: Feld neither started it nor holds
    // the authority to stop it, and if it ends it is `ended`. It was started without the
    // telemetry export, so its token usage is unavailable rather than N/A.
    { id: "session_HA", actor: "home-assistant-1", start: at(-2, 9, 0), end: null, startKind: "attach", ownership: "attached", endKind: "running", exitCode: null, execution: { type: "agent-runtime", label: "Claude Code", command: null, ai: true }, model: ["Claude Code", "claude-sonnet"], placement: existingPanePlacement("Local Node", "/Users/alice/Dev/home-assistant", USER_SOCKET, "%15", "main:1"), metrics: unavailableMetrics("No correlated Runtime usage record"), observation: { kind: "running", at: MOCK_NOW - 4 * MINUTE } },
    // Stale observation: the last Provider observation is old, but Feld infers nothing from that.
    { id: "session_LO", actor: "live-operator", start: MOCK_NOW - 42 * MINUTE, end: null, startKind: "start", ownership: "owned", endKind: "running", exitCode: null, execution: { type: "agent-runtime", label: "Claude Code", command: "claude", ai: true }, model: ["Claude Code", "claude-opus"], placement: tmuxPlacement("Local Node", FELD_WORKDIR, FELD_SOCKET, "session", "$12", "feld-default-live-operator-1"), metrics: claudeMetrics([[25, 12, 46, 7], [42, 21, 75, 12], [72, 36, 132, 20], [61, 30, 112, 17], [103, 51, 187, 29], [86, 43, 157, 24], [137, 68, 246, 39], [120, 60, 216, 34]], [9, 17, 26, 36, 45, 53, 62, 70]), observation: { kind: "running", at: MOCK_NOW - 38 * MINUTE } },
    // A current Session with a fresh Provider observation; its band's attention comes only
    // from unread mail (message_status_check), never from an age threshold.
    { id: "session_CD", actor: "current-dev", start: MOCK_NOW - 2 * HOUR, end: null, startKind: "start", ownership: "owned", endKind: "running", exitCode: null, execution: { type: "agent-runtime", label: "Claude Code", command: "claude", ai: true }, model: ["Claude Code", "claude-opus"], placement: tmuxPlacement("Local Node", FELD_WORKDIR, USER_SOCKET, "window", "@17", "main:6"), metrics: claudeMetrics([[31, 16, 64, 9], [59, 32, 121, 18], [49, 26, 100, 15], [93, 50, 189, 28], [80, 43, 163, 24], [135, 72, 272, 41], [119, 64, 241, 36], [153, 82, 308, 47]], [12, 25, 38, 52, 66, 79, 88, 42, 55, 67, 78, 86]), observation: { kind: "running", at: MOCK_NOW - 2 * MINUTE } },
    // One Session on each of the two other Providers. Their Bindings record the unit the
    // Provider created (a sandbox name, a Herdr workspace and pane) instead of tmux axes.
    { id: "session_SD", actor: "sandbox-dev", start: at(0, 15, 9), end: at(0, 15, 38), startKind: "start", ownership: "owned", endKind: "stopped", exitCode: 0, execution: { type: "agent-runtime", label: "Claude Code", command: "claude", ai: true }, model: ["Claude Code", "claude-sonnet"], placement: dockerSandboxPlacement("Sandbox Node", "/workspace/feld", "feld-sandbox-dev-1"), metrics: claudeMetrics([[40, 18, 62, 10], [70, 30, 110, 20], [90, 40, 150, 25], [85, 36, 140, 22], [120, 50, 190, 30], [100, 44, 160, 26], [60, 26, 96, 16]], [10, 19, 30, 42, 55, 66, 74]), observation: { kind: "gone", at: at(0, 15, 38) }, lifecycle: { startSettled: at(0, 15, 9) + 2400, stop: { accepted: at(0, 15, 37) + 30000, requesterSessionId: null, attempt: "Docker Sandboxes accepted a stop of the Session's sandbox", observed: "the sandbox has stopped and the process table no longer lists the Runtime" } } },
    { id: "session_HD", actor: "herdr-dev", start: at(0, 15, 16), end: at(0, 15, 55), startKind: "start", ownership: "owned", endKind: "stopped", exitCode: 0, execution: { type: "agent-runtime", label: "Codex", command: "codex", ai: true }, model: ["Codex", "codex-terra"], placement: herdrPlacement("Local Node", "/Users/alice/Dev/my-project", "my-project", "pane-3"), metrics: codexMetrics([[50, 24, 54, 12], [80, 40, 90, 20], [110, 52, 120, 28], [95, 46, 104, 25], [140, 66, 150, 34], [125, 60, 136, 29], [90, 42, 98, 20], [55, 26, 60, 14]]), observation: { kind: "gone", at: at(0, 15, 55) }, lifecycle: { startSettled: at(0, 15, 16) + 620, stop: { accepted: at(0, 15, 54) + 30000, requesterSessionId: null, attempt: "Herdr accepted a close of the Session's pane", observed: "Herdr reports the pane closed and the process table no longer lists the Runtime" } } },
    // A current Session whose container the Provider stopped finding 25 minutes ago:
    // Feld infers nothing from that, keeps the Session current and `lost`, and shows it
    // as needing attention until the next start request or a kill closes it.
    { id: "session_BR", actor: "bench-runner", start: MOCK_NOW - 5 * HOUR, end: null, startKind: "start", ownership: "owned", endKind: "running", exitCode: null, execution: { type: "agent-runtime", label: "Claude Code", command: "claude", ai: true }, model: ["Claude Code", "claude-sonnet"], placement: dockerPlacement("Build Node", "/workspace/bench", "9f3c7a1d5e08"), metrics: claudeMetrics([[30, 14, 52, 8], [66, 31, 118, 18], [58, 27, 104, 16], [104, 49, 186, 29], [88, 41, 158, 24], [126, 59, 226, 35], [70, 33, 125, 19], [21, 10, 38, 6]], [11, 24, 37, 51, 64, 76, 85, 88]), observation: { kind: "lost", since: MOCK_NOW - 25 * MINUTE, at: MOCK_NOW - MINUTE }, observations: [{ kind: "running", at: MOCK_NOW - 5 * HOUR + 1900 }, { kind: "lost", at: MOCK_NOW - 25 * MINUTE }], lifecycle: { startSettled: MOCK_NOW - 5 * HOUR + 1900 } },
    // A Codex pane the person started by hand and attached from outside with
    // `feld actor attach codex-dev --runtime codex --tmux-pane %47`: the execution is
    // agent-runtime by the declared runtime and its command is unknown to Feld. The
    // Codex Runtime Adapter delivered the Runtime identity (model) from a hook fired after
    // the attach. Codex built its telemetry header once at launch, before the attach, so
    // the header carries no telemetry token and the metrics are unavailable.
    { id: "session_CX", actor: "codex-dev", start: MOCK_NOW - 90 * MINUTE, end: null, startKind: "attach", ownership: "attached", attachCommand: "feld actor attach codex-dev --runtime codex --tmux-pane %47", endKind: "running", exitCode: null, execution: { type: "agent-runtime", label: "Codex", command: null, ai: true }, model: ["Codex", "codex-terra"], placement: existingPanePlacement("Local Node", "/Users/alice/Dev/my-project", USER_SOCKET, "%47", "main:9"), metrics: attachedCodexMetrics([14, 22, 29, 35, 41, 48, 54, 61]), observation: { kind: "running", at: MOCK_NOW - 2 * MINUTE } },
    // Attached Sessions: the person's own Claude Code runs in the my-project checkout,
    // each registered from inside with `/feld attach` (`feld actor attach --runtime claude-code --self`). None was
    // started by Feld, Feld cannot stop them, and each ends when the person closes it.
    // Three overlap on Aug 30 (13:40–17:25), so the attach naming rule gave them
    // my-project-1, -2, and -3 in order of attach. The two current now stacked new
    // generations on my-project-1 and -2, whose Aug 30 Sessions had ended.
    { id: "session_FP1", actor: "my-project-1", start: at(0, 13, 40), end: at(0, 15, 5), startKind: "attach", ownership: "attached", endKind: "exited", exitCode: null, execution: { type: "agent-runtime", label: "Claude Code", command: null, ai: true }, model: ["Claude Code", "claude-opus"], placement: existingPanePlacement("Local Node", "/Users/alice/Dev/my-project", USER_SOCKET, "%33", "main:3"), metrics: claudeMetrics([[42, 22, 65, 11], [78, 41, 121, 20], [63, 33, 98, 16], [114, 60, 176, 30], [99, 52, 153, 26], [87, 46, 134, 23], [57, 30, 88, 15], [36, 19, 56, 9]], [15, 26, 38, 49, 60, 71, 80, 87]), observation: { kind: "gone", at: at(0, 15, 5) } },
    { id: "session_FP2", actor: "my-project-2", start: at(0, 14, 20), end: at(0, 17, 25), startKind: "attach", ownership: "attached", endKind: "exited", exitCode: null, execution: { type: "agent-runtime", label: "Claude Code", command: null, ai: true }, model: ["Claude Code", "claude-opus"], placement: existingPanePlacement("Local Node", "/Users/alice/Dev/my-project", USER_SOCKET, "%34", "main:3"), metrics: claudeMetrics([[25, 12, 46, 7], [50, 25, 91, 14], [72, 36, 132, 20], [67, 33, 121, 19], [114, 57, 207, 32], [98, 49, 175, 28], [84, 42, 150, 24], [61, 30, 112, 17], [44, 22, 82, 12]], [11, 22, 34, 46, 58, 69, 81, 90, 44, 56, 67, 79]), observation: { kind: "gone", at: at(0, 17, 25) } },
    // The transcript of session_FP2 names Sub Agents from before the attach (14:20). The
    // Timeline draws them from the attach: survey-docs ended before it and is not drawn.
    { id: "session_FP2_S1", actor: "my-project-2", subAgent: "survey-docs", parentSession: "session_FP2", start: at(0, 14, 2), end: at(0, 14, 12), startKind: "sub-agent", ownership: null, endKind: null, exitCode: null, execution: { type: "agent-runtime", label: "Claude Code · Runtime-internal Sub Agent", command: "runtime internal", ai: true }, model: ["Claude Code", "claude-opus"], placement: null, metrics: SUB_AGENT_METRICS, observation: null },
    { id: "session_FP2_S2", actor: "my-project-2", subAgent: "draft-notes", parentSession: "session_FP2", start: at(0, 14, 10), end: at(0, 14, 31), startKind: "sub-agent", ownership: null, endKind: null, exitCode: null, execution: { type: "agent-runtime", label: "Claude Code · Runtime-internal Sub Agent", command: "runtime internal", ai: true }, model: ["Claude Code", "claude-opus"], placement: null, metrics: SUB_AGENT_METRICS, observation: null },
    { id: "session_FP3", actor: "my-project-3", start: at(0, 14, 58), end: at(0, 15, 22), startKind: "attach", ownership: "attached", endKind: "exited", exitCode: null, execution: { type: "agent-runtime", label: "Claude Code", command: null, ai: true }, model: ["Claude Code", "claude-sonnet"], placement: existingPanePlacement("Local Node", "/Users/alice/Dev/my-project", USER_SOCKET, "%36", "main:4"), metrics: claudeMetrics([[20, 9, 27, 4], [51, 24, 63, 12], [78, 36, 98, 18], [61, 28, 77, 14], [30, 14, 39, 7]], [10, 19, 29, 40, 51]), observation: { kind: "gone", at: at(0, 15, 22) } },
    { id: "session_FP4", actor: "my-project-1", start: MOCK_NOW - 3 * HOUR, end: null, startKind: "attach", ownership: "attached", endKind: "running", exitCode: null, execution: { type: "agent-runtime", label: "Claude Code", command: null, ai: true }, model: ["Claude Code", "claude-opus"], placement: existingPanePlacement("Local Node", "/Users/alice/Dev/my-project", USER_SOCKET, "%41", "main:7"), metrics: claudeMetrics([[28, 13, 61, 8], [62, 28, 131, 19], [52, 24, 108, 16], [93, 43, 196, 28], [75, 34, 158, 23], [117, 54, 243, 36], [98, 45, 207, 30], [80, 37, 169, 24]], [14, 27, 41, 55, 68, 82, 91, 39, 52, 65, 77, 85]), observation: { kind: "running", at: MOCK_NOW - 3 * MINUTE } },
    // session_FP5 shows a transcript file still being written: its last entry is a Runtime
    // notification with no response after it yet, two minutes before now. The file was
    // created at the first input, 43 minutes after the attach.
    { id: "session_FP5", actor: "my-project-2", start: MOCK_NOW - 50 * MINUTE, recordingOrigin: "2026-09-09T01:10:59.738Z", end: null, startKind: "attach", ownership: "attached", endKind: "running", exitCode: null, execution: { type: "agent-runtime", label: "Claude Code", command: null, ai: true }, model: ["Claude Code", "claude-fable-5-1"], placement: existingPanePlacement("Local Node", "/Users/alice/Dev/my-project", USER_SOCKET, "%43", "main:8"), metrics: claudeMetrics([[24, 12, 38, 6], [57, 30, 88, 15], [78, 41, 121, 20], [102, 54, 157, 27], [126, 67, 194, 33]], [12, 23, 35, 46, 58]), observation: { kind: "running", at: MOCK_NOW - MINUTE } },
  ];

  // Every density Session overlaps the whole 30-minute scenario. The orchestrator spawns
  // worker-06 to worker-10 within 13 seconds, just before it sends them their work packages:
  // their Spawns and those Messages share a time bin. Feld started those Sessions, so they
  // are owned and end on the orchestrator's stop. The other density Sessions are attached
  // so the fixture can stay compact: the mock needs lane and Message geometry, not
  // lifecycle or transcript detail for these synthetic Actors.
  const DENSITY_SESSION_START = MESSAGE_DENSITY_ANCHOR - 14 * MINUTE;
  const DENSITY_SESSION_END = MESSAGE_DENSITY_ANCHOR + 14 * MINUTE;
  const DENSITY_SPAWNED = MESSAGE_DENSITY_WORKERS.slice(5, 10);
  const densitySpawnAt = (actor) => MESSAGE_DENSITY_ANCHOR - 9 * MINUTE + 2000 + DENSITY_SPAWNED.indexOf(actor) * 2600;
  ["orchestrator", ...MESSAGE_DENSITY_WORKERS].forEach((actor, index) => {
    const spawned = DENSITY_SPAWNED.includes(actor);
    const end = DENSITY_SESSION_END - index * 25;
    const codex = index % 3 === 0;
    SESSIONS.push({
      id: `session_density_${actor}`,
      actor,
      start: spawned ? densitySpawnAt(actor) : DENSITY_SESSION_START + index * 25,
      end,
      startKind: spawned ? "start" : "attach",
      ownership: spawned ? "owned" : "attached",
      endKind: spawned ? "stopped" : "exited",
      exitCode: spawned ? 0 : null,
      execution: { type: "agent-runtime", label: codex ? "Codex" : "Claude Code", command: spawned ? (codex ? "codex" : "claude") : null, ai: true },
      model: codex ? ["Codex", "codex-terra"] : ["Claude Code", "claude-sonnet"],
      placement: spawned
        ? tmuxPlacement("Local Node", FELD_WORKDIR, FELD_SOCKET, "window", `@density-${index + 1}`, `feld:density-${index + 1}`)
        : existingPanePlacement("Local Node", FELD_WORKDIR, USER_SOCKET, `%density-${index + 1}`, `density:${index + 1}`),
      metrics: unavailableMetrics("Density scenario: telemetry omitted"),
      observation: { kind: "gone", at: end },
      ...(spawned ? { lifecycle: { stop: { accepted: end - 2000, requesterSessionId: "session_density_orchestrator", attempt: "tmux sent /exit to the pane", observed: "tmux and the process table both show that this exact incarnation no longer exists" } } } : {}),
    });
  });

  // The account each Runtime Session runs under. A Runtime's rate limit belongs to an
  // account, and one Node can run Sessions under several (a person's own subscription, a
  // team's, a bot's). Feld learns it from the Session's Telemetry (docs/spec/design/
  // metrics.md#quota) and groups the quota reports by it.
  const CLAUDE_ACCOUNT_BY_ACTOR = Object.freeze({
    "my-project-1": "personal@example.com",
    "my-project-2": "personal@example.com",
    "my-project-3": "personal@example.com",
    researcher: "research@example.com",
    "bench-runner": "research@example.com",
    "reviewer-01": "review-bot@example.com",
    "sandbox-dev": "review-bot@example.com",
  });
  SESSIONS.forEach((session) => {
    if (session.model?.[0] === "Claude Code") session.account = CLAUDE_ACCOUNT_BY_ACTOR[session.actor] || "team@example.com";
    else if (session.model?.[0] === "Codex") session.account = "team@example.com";
  });

  // A Schedule is part of a Job's record when the Job has one: a cron expression, its
  // timezone, and whether it is enabled. The timezone is an IANA name, resolved from the
  // daemon's local timezone when the declaration omits it and stored at creation. Each
  // tick starts a Session from the Actor's launchSpec. Everything else shown about it
  // (next run, recent runs) is derived at render time from this record and the Session
  // history. Only `job` Actors carry one, and a Job need not: a Job without a Schedule
  // (pr-checks) starts only when spawned or started, and Feld decides no start time
  // for it. An Agent never carries one: an Agent loop runs until stopped, so its ticks
  // would only ever fire after an unrequested exit, which would make the schedule a
  // restart policy in disguise. `pausedAt` and `enabledAt` are the accepted instants of
  // the latest pause and resume requests.
  const SCHEDULES = [
    { actor: "release-check", cron: "20 3,15 * * *", timezone: "Asia/Tokyo", enabled: true },
    // Paused by the operator on Sep 1; the record stays, no tick fires until resumed.
    { actor: "nightly-summary", cron: "27 3,15 * * *", timezone: "Asia/Tokyo", enabled: false, pausedAt: Date.parse("2026-09-01T10:38:00+09:00") },
    // Created paused on Aug 30 for the Project 10 kickoff and run by hand once (session_D0),
    // then resumed 80 minutes before the fixture's now: its ticks since are 13:43
    // (session_D4), 14:13 (session_D3, still current), and 14:43 (skipped while session_D3 ran).
    { actor: "dispatcher", cron: "13,43 * * * *", timezone: "Asia/Tokyo", enabled: true, enabledAt: MOCK_NOW - 80 * MINUTE },
  ];

  // `schedule.skipped` is the one Schedule-specific event Feld records: at a planned
  // tick the Actor's current Session was still running (or unobserved), so no spawn was
  // submitted. Ticks that ran are Session records with startKind schedule. The record
  // names the Session that was current at the tick, so it appears in that Session's
  // trace and Console.
  const SCHEDULE_SKIPS = [
    { id: "skip_dispatcher_1443", actor: "dispatcher", planned: MOCK_NOW - 17 * MINUTE, reason: "previous session current", previousSession: "session_D3" },
  ];

  // A Spawn record names the spawner and the target as Workspace-qualified Actor
  // references, the Session it created, the instant, and `spawnerSessionId`: the exact
  // Session whose token authenticated the request, saved when it was accepted.
  const ACTIONS = Object.freeze([
    { id: "action_spawn_D1", spawner: "dispatcher", spawnerWorkspace: "default", spawnerSessionId: "session_D0", target: "developer-01", targetWorkspace: "default", session: "session_D1", at: at(0, 14, 50) },
    { id: "action_spawn_R0", spawner: "dispatcher", spawnerWorkspace: "default", spawnerSessionId: "session_D0", target: "reviewer-01", targetWorkspace: "default", session: "session_R0", at: at(0, 15, 2) },
    { id: "action_spawn_T0", spawner: "dispatcher", spawnerWorkspace: "default", spawnerSessionId: "session_D0", target: "test-runner", targetWorkspace: "default", session: "session_T0", at: at(0, 15, 18) },
    // Spawned from the attached Session session_FP2 into another Workspace
    // (`feld actor create researcher --kind agent --workspace research ...`).
    { id: "action_spawn_RS", spawner: "my-project-2", spawnerWorkspace: "default", spawnerSessionId: "session_FP2", target: "researcher", targetWorkspace: "research", session: "session_RS", at: at(0, 16, 50) },
    ...DENSITY_SPAWNED.map((worker) => ({ id: `action_spawn_density_${worker}`, spawner: "orchestrator", spawnerWorkspace: "default", spawnerSessionId: "session_density_orchestrator", target: worker, targetWorkspace: "default", session: `session_density_${worker}`, at: densitySpawnAt(worker) })),
  ]);

  // Each Message keeps the product's delivery facts apart and never promotes one into
  // another: `accepted` is the instant Feld persisted it and acknowledged the sender;
  // `fromSessionId` is the exact sender Session saved at acceptance (null when the
  // Admin sent it); `toSessionId` is the recipient's Session
  // current at acceptance (null when none was), which is where the accepted record
  // appears in a trace or Console; `read` is the one authenticated mailbox read by the
  // recipient's exact Session (null when none is recorded), with its own `readId` and the
  // Message's integer `fifo` position in the mailbox. A Plugin Monitor helper long-polling
  // the mailbox and the Session's own `feld self message recv` record the same read; the route is
  // not distinguished. Semantic processing is always not_claimed and is not stored.
  // "Waiting" is not a stored state: it is derived at query time as accepted with no
  // read recorded. Feld never starts a Session to drain a mailbox, so a Message to an
  // Actor with no current Session waits until a later Session reads it.
  // `anchor` is the Message ID that the sender's transcript file quotes because
  // `feld message send` printed it; the Message ID is likewise quoted by the reader's transcript
  // file from the Monitor's event line or `feld self message recv`, and the read is recorded only
  // after that hand-off. The Console uses them to place the record beside that
  // transcript entry; it is a text match made at render time.
  const DENSITY_MESSAGES = [];
  MESSAGE_DENSITY_WORKERS.forEach((worker, index) => {
    const workerSessionId = `session_density_${worker}`;
    const waveStart = MESSAGE_DENSITY_ANCHOR - 9 * MINUTE + Math.floor(index / 5) * 15 * 1000;
    for (let part = 0; part < 3; part += 1) {
      const accepted = waveStart + (index % 5) * 1400 + part * 280;
      const waiting = index % 7 === 0 && part === 2;
      DENSITY_MESSAGES.push({
        id: `message_density_task_${index + 1}_${part + 1}`,
        from: "orchestrator",
        fromWorkspace: "default",
        fromSessionId: "session_density_orchestrator",
        to: worker,
        toWorkspace: "default",
        toSessionId: workerSessionId,
        accepted,
        read: waiting ? null : { readId: `read_density_task_${index + 1}_${part + 1}`, sessionId: workerSessionId, at: accepted + 40 + part * 9, fifo: part + 1 },
        anchor: null,
        body: `[${part + 1}/3] Work package for ${worker}`,
      });
    }
    const reportAccepted = MESSAGE_DENSITY_ANCHOR + 2 * MINUTE + index * 7100;
    DENSITY_MESSAGES.push({
      id: `message_density_report_${index + 1}`,
      from: worker,
      fromWorkspace: "default",
      fromSessionId: workerSessionId,
      to: "orchestrator",
      toWorkspace: "default",
      toSessionId: "session_density_orchestrator",
      accepted: reportAccepted,
      read: { readId: `read_density_report_${index + 1}`, sessionId: "session_density_orchestrator", at: reportAccepted + 65, fifo: index + 1 },
      anchor: null,
      body: `${worker}: work package complete`,
    });
  });

  const MESSAGES = Object.freeze([
    ...DENSITY_MESSAGES,
    { id: "message_review_request", from: "developer-01", fromWorkspace: "default", fromSessionId: "session_D1", to: "reviewer-01", toWorkspace: "default", toSessionId: "session_R0", accepted: at(0, 15, 5), read: { readId: "read_4c1d9e02b7a5f38e6d0c2b91a7e4f560", sessionId: "session_R0", at: at(0, 15, 6), fifo: 1 }, anchor: null, body: "Please review the implementation and call out blocking issues." },
    // The inbound Message of the dev-issue-22 run: accepted 10.078 s after the
    // start, read by the Session's Plugin Monitor helper 3 ms later.
    { id: "message_conflict_task", from: "Admin", fromWorkspace: null, fromSessionId: null, to: "developer-01", toWorkspace: "default", toSessionId: "session_D2", accepted: at(0, 15, 12) + 10078, read: { readId: "read_ad4926ad85ad7c63e267a0551823c4dd", sessionId: "session_D2", at: at(0, 15, 12) + 10081, fifo: 3 }, anchor: null, body: [
      "dev-issue-22: libra PR #41 (cursor paging for catalogue search) needs another pass.",
      "",
      "The on-loan filter from PR #35 reached main after your last merge and the two now collide in README.md. Bring `issue-22` up to date with main, keep the PR's README section short, and report back with `feld message send dispatcher` once `make check` is green.",
    ].join("\n") },
    // The outbound report of that run, accepted while `feld message send` was running.
    { id: "message_merge_report", from: "developer-01", fromWorkspace: "default", fromSessionId: "session_D2", to: "dispatcher", toWorkspace: "default", toSessionId: "session_D0", accepted: at(0, 15, 12) + 261528, read: null, anchor: "msg_f893843602d0099b2d0204d7118b6f33", body: [
      "## Outcome",
      "",
      "- RESULT: DONE",
      "- TICKET: example-org/libra#22",
      "- PULL REQUEST: https://github.com/example-org/libra/pull/41",
      "- CHECKS: `env -u LIBRA_DB_URL make check` is green (exit 0); GitHub says the PR is MERGEABLE. CI has not finished, so the merge state still reads **UNSTABLE**.",
      "",
      "## What changed",
      "",
      "- Merged main into `issue-22` without rewriting history; the only conflict was README.md.",
      "- Kept the PR's short README and moved main's on-loan filter notes to docs/search-api.md.",
      "- The cursor still orders by (sort key, book_id); 612 tests pass.",
      "",
      "## Next",
      "",
      "Waiting for the next message from dispatcher.",
    ].join("\n") },
    // Accepted after session_D2 ended: no Session was current, so no Session's trace
    // carries it, and no Session has read it.
    { id: "message_review_result", from: "reviewer-01", fromWorkspace: "default", fromSessionId: "session_R0", to: "developer-01", toWorkspace: "default", toSessionId: null, accepted: at(0, 15, 20), read: null, anchor: null, body: "Approved with two non-blocking suggestions." },
    // Accepted one minute after session_D0 ended: likewise waiting with no Session.
    { id: "message_test_result", from: "test-runner", fromWorkspace: "default", fromSessionId: "session_T0", to: "dispatcher", toWorkspace: "default", toSessionId: null, accepted: at(0, 15, 25), read: null, anchor: null, body: "Verification passed; artifacts are ready for inspection." },
    // Waiting does not depend on the Actor having a Session: current-dev is running but
    // has not read this mailbox position yet.
    { id: "message_status_check", from: "Admin", fromWorkspace: null, fromSessionId: null, to: "current-dev", toWorkspace: "default", toSessionId: "session_CD", accepted: MOCK_NOW - 12 * MINUTE, read: null, anchor: null, body: "Status check: report progress on the migration before continuing.\n\nName the tables already moved, the ones left, and whether the dry run against the staging copy still passes." },
    // Sent by the person from their attached Session session_FP2 to an Actor of another
    // Workspace (`feld message send researcher --workspace research ...`).
    { id: "message_research_brief", from: "my-project-2", fromWorkspace: "default", fromSessionId: "session_FP2", to: "researcher", toWorkspace: "research", toSessionId: "session_RS", accepted: at(0, 17, 5), read: { readId: "read_7a92e0c4d1b6f5a3e8c2d09b4f6a1e73", sessionId: "session_RS", at: at(0, 17, 6), fifo: 1 }, anchor: null, body: "Focus on how other control planes record Session exit codes; skip UI frameworks." },
  ]);

  // The programs the fixture Node has on its PATH, so that a Command naming anything else
  // shows what the New actor screen reports for a start that fails the moment it is made.
  const NODE_PROGRAMS = ["bash", "sh", "echo", "printf", "sleep", "python3", "node", "make", "cargo", "git"];

  ui.data.actors = Object.freeze({
    scenarios: VARIANTS,
    workspaces: WORKSPACES,
    defaultWorkspace: DEFAULT_WORKSPACE,
    actors: ACTORS,
    sessions: SESSIONS,
    schedules: SCHEDULES,
    scheduleSkips: SCHEDULE_SKIPS,
    actions: ACTIONS,
    messages: MESSAGES,
    metricLanes: METRICS,
    nodePrograms: NODE_PROGRAMS,
    // The mock acceptance of the New actor form builds its Session's Binding and metrics
    // with the same helpers as the records above.
    placement: Object.freeze({ tmux: tmuxPlacement, docker: dockerPlacement, dockerSandbox: dockerSandboxPlacement, herdr: herdrPlacement, feldSocket: FELD_SOCKET }),
    sessionMetrics: Object.freeze({ unavailable: unavailableMetrics, program: PROGRAM_METRICS }),
  });
})(window);
