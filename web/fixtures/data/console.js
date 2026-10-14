// Hand-written fixture of the Console: one entry per Session of data/actors.js, naming the
// recording (data/session-*.js) or the captured output (data/session-logs.js) it shows.
(function registerConsoleFixture(global) {
  "use strict";
  const ui = global.FeldWebUI = global.FeldWebUI || {};
  ui.data = ui.data || {};

  // `console: "log"` marks a program Session: no Runtime Adapter, so no transcript; the
  // Console shows the stdout and stderr the Provider captured (data/session-logs.js).
  // `console: "none"` is a Session whose Runtime Adapter has not delivered the record
  // file's location, and `console: "missing"` one whose location arrived but whose file
  // does not exist (no input yet, or removed after the Session); both show only Feld's own
  // records. A Claude Code Session without a recording of its own shows the one `recording`
  // names, or the shared exercise recording. `placement` is the Console header's Placement
  // summary. `hasOlder` marks a Session whose Console holds only the tail of its records,
  // as a long Session's first read does; the records before it are read at the top. The
  // array is mutable so components/console.js can register a Session the
  // Actors component creates.
  ui.data.consoleSessions = [
    { id: "session_D0", actor: "dispatcher", model: "N/A", placement: "tmux · Local Node", console: "log" },
    { id: "session_D4", actor: "dispatcher", model: "N/A", placement: "tmux · Local Node", console: "log" },
    { id: "session_D3", actor: "dispatcher", model: "N/A", placement: "tmux · Local Node", console: "log" },
    { id: "session_D1", actor: "developer-01", model: "codex-sol-high", placement: "tmux · Local Node", console: "none" },
    { id: "session_D1_S1", actor: "developer-01 / explore-tests", model: "codex-sol-high", placement: "Not tracked · inside session_D1", console: "none" },
    { id: "session_D1_S2", actor: "developer-01 / explore-api", model: "codex-sol-high", placement: "Not tracked · inside session_D1", console: "none" },
    { id: "session_D2", actor: "developer-01", model: "claude-fable-5-1", placement: "docker · Build Node" },
    { id: "session_R0", actor: "reviewer-01", model: "claude-sonnet", placement: "tmux · Local Node", hasOlder: true },
    { id: "session_T0", actor: "test-runner", model: "codex-terra", placement: "tmux · Local Node", console: "none" },
    { id: "session_NS", actor: "nightly-summary", model: "N/A", placement: "docker · Batch Node", console: "log" },
    { id: "session_P0", actor: "planner-01", model: "claude-opus", placement: "tmux · Local Node" },
    { id: "session_RS", actor: "researcher", model: "claude-opus", placement: "docker · Sandbox Node", sourceUnavailable: "the Sandbox Node container that ran this Session is gone, so its output cannot be read" },
    { id: "session_W0", actor: "docs-writer", model: "codex-terra", placement: "tmux · Local Node", console: "none" },
    { id: "session_W1", actor: "docs-writer", model: "codex-terra", placement: "tmux · Local Node", console: "none" },
    { id: "session_RC", actor: "release-check", model: "N/A", placement: "docker · Build Node", console: "log" },
    { id: "session_HA", actor: "home-assistant-1", model: "claude-sonnet", placement: "tmux · Local Node" },
    { id: "session_LO", actor: "live-operator", model: "claude-opus", placement: "tmux · Local Node" },
    { id: "session_CD", actor: "current-dev", model: "claude-opus", placement: "tmux · Local Node" },
    { id: "session_SD", actor: "sandbox-dev", model: "claude-sonnet", placement: "docker-sandbox · Sandbox Node" },
    { id: "session_HD", actor: "herdr-dev", model: "codex-terra", placement: "herdr · Local Node", console: "none" },
    { id: "session_BR", actor: "bench-runner", model: "us.anthropic.claude-sonnet-5-20260801-v1:0", placement: "docker · Build Node · container 9f3c7a1d5e08 (created)" },
    { id: "session_NS0", actor: "nightly-summary", model: "N/A", placement: "docker · Batch Node", console: "log" },
    { id: "session_RC1", actor: "release-check", model: "N/A", placement: "docker · Build Node", console: "log" },
    { id: "session_PC", actor: "pr-checks", model: "N/A", placement: "tmux · Local Node", console: "log" },
    // The launch was refused, so the program never ran and the Provider captured nothing.
    { id: "session_SC", actor: "stale-checkout", model: "N/A", placement: "tmux · Local Node", console: "log" },
    // Attached Sessions: the person's own Claude Code, registered from inside with
    // `/feld attach`, which named the Actors my-project-1 to -3, and a Codex pane
    // attached from outside.
    { id: "session_FP1", actor: "my-project-1", model: "claude-opus", placement: "tmux · Local Node" },
    { id: "session_FP2", actor: "my-project-2", model: "claude-opus", placement: "tmux · Local Node" },
    { id: "session_FP2_S1", actor: "my-project-2 / survey-docs", model: "claude-opus", placement: "Not tracked · inside session_FP2", console: "none" },
    { id: "session_FP2_S2", actor: "my-project-2 / draft-notes", model: "claude-opus", placement: "Not tracked · inside session_FP2", console: "none" },
    { id: "session_FP3", actor: "my-project-3", model: "claude-sonnet", placement: "tmux · Local Node" },
    { id: "session_FP4", actor: "my-project-1", model: "claude-opus", placement: "tmux · Local Node" },
    { id: "session_FP5", actor: "my-project-2", model: "claude-fable-5-1", placement: "tmux · Local Node", recording: "session_D2" },
    { id: "session_CX", actor: "codex-dev", model: "codex-terra", placement: "tmux · Local Node", console: "none" },
    // The message-density scenario's synthetic Sessions carry no transcript: their Console
    // shows Feld's own records.
    ...ui.data.actors.sessions
      .filter((session) => session.id.startsWith("session_density_"))
      .map((session) => ({ id: session.id, actor: session.actor, model: session.model[1], placement: `${session.placement.provider} · ${session.placement.node}`, console: "none" })),
  ];
})(window);
