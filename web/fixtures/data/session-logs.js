// Hand-written fixture: the stdout and stderr the Execution Provider captured for the
// program (Job) Sessions. A program Session has no Runtime Adapter and therefore
// no transcript; its Console is this log with Feld's own records placed between the
// lines by instant. Each line is [offset in ms from the Session's start, stream, text].
// Feld does not parse the text: a line printed by `claude -p` inside a script is a
// line of the script's output, nothing more.
(function registerSessionLogs(global) {
  "use strict";
  const ui = global.FeldWebUI = global.FeldWebUI || {};
  ui.data = ui.data || {};
  const s = (seconds) => Math.round(seconds * 1000);
  const m = (minutes, seconds = 0) => Math.round((minutes * 60 + seconds) * 1000);
  ui.data.sessionLogs = Object.freeze({
    ...(ui.data.sessionLogs || {}),
    // dispatcher, run by hand on Aug 30 (feld actor start while the Job's Schedule was paused).
    session_D0: Object.freeze({ source: "stdout and stderr of python3 dispatch.py, captured by the tmux Provider", lines: [
      [s(0.4), "stdout", "dispatch: FELD_ACTOR=dispatcher FELD_WORKSPACE=default"],
      [s(1.2), "stdout", "dispatch: mailbox read → 0 messages waiting"],
      [s(3.8), "stdout", "dispatch: project 10 → 3 open issues, 0 in review"],
      [m(11, 58), "stdout", "dispatch: issue #22 has no assignee; starting developer-01"],
      [m(12, 0.6), "stdout", "feld actor create developer-01 --kind agent --runtime codex --workdir /Users/alice/Dev/feld → session_D1"],
      [m(23, 57), "stdout", "dispatch: PR #41 opened by developer-01; starting reviewer-01 and test-runner"],
      [m(24, 0.9), "stdout", "feld actor create reviewer-01 --kind agent --runtime claude-code --workdir /Users/alice/Dev/feld → session_R0"],
      [m(30, 3), "stderr", "dispatch: gh api rate limit 4 remaining; sleeping 60 s"],
      [m(40, 1.1), "stdout", "feld actor create test-runner --kind agent --runtime codex --workdir /Users/alice/Dev/feld → session_T0"],
      [m(45, 30), "stdout", "dispatch: board updated (3 open, 1 in review); nothing else to do"],
      [m(45, 31), "stdout", "dispatch: exit 0"],
    ] }),
    // dispatcher, the Schedule's 13:43 tick on Sep 2.
    session_D4: Object.freeze({ source: "stdout and stderr of python3 dispatch.py, captured by the tmux Provider", lines: [
      [s(0.5), "stdout", "dispatch: FELD_ACTOR=dispatcher FELD_WORKSPACE=default"],
      [s(1.3), "stdout", "dispatch: mailbox read → 2 messages waiting (FIFO 4..5), deferred to the triage step"],
      [s(4.1), "stdout", "dispatch: project 10 → 1 open issue, 1 in review"],
      [m(2, 40), "stdout", "dispatch: PR #41 still awaiting a human merge; no action"],
      [m(2, 58), "stdout", "dispatch: exit 0"],
    ] }),
    // dispatcher, the 14:13 tick that is still current at the fixture's now: stuck on a
    // GitHub rate limit, which is why the 14:43 tick was skipped.
    session_D3: Object.freeze({ source: "stdout and stderr of python3 dispatch.py, captured by the tmux Provider", lines: [
      [s(0.5), "stdout", "dispatch: FELD_ACTOR=dispatcher FELD_WORKSPACE=default"],
      [s(1.4), "stdout", "dispatch: mailbox read → 2 messages waiting (FIFO 4..5), deferred to the triage step"],
      [s(4.0), "stderr", "dispatch: gh api → HTTP 403 rate limit exceeded; reset in 58 m"],
      [s(4.1), "stdout", "dispatch: retrying every 60 s until the limit resets"],
      [m(1, 4), "stderr", "dispatch: gh api → HTTP 403 (retry 1)"],
      [m(2, 4), "stderr", "dispatch: gh api → HTTP 403 (retry 2)"],
      [m(15, 5), "stderr", "dispatch: gh api → HTTP 403 (retry 15)"],
      [m(30, 6), "stderr", "dispatch: gh api → HTTP 403 (retry 30)"],
      [m(45, 7), "stderr", "dispatch: gh api → HTTP 403 (retry 45)"],
      [m(46, 7), "stderr", "dispatch: gh api → HTTP 403 (retry 46)"],
    ] }),
    // nightly-summary, the 15:27 run that succeeded.
    session_NS: Object.freeze({ source: "stdout and stderr of python3 summarize.py, captured by the Docker Provider", lines: [
      [s(0.6), "stdout", "summarize: FELD_ACTOR=nightly-summary digest=team since=2026-08-29T15:27+09:00"],
      [s(2.4), "stdout", "summarize: collected 41 Session records, 18 Messages, 6 PR events"],
      [m(1, 5), "stdout", "summarize: responses.create gpt-5.4-mini → 1,180 tokens (outline)"],
      [m(5, 12), "stdout", "summarize: responses.create gpt-5.4 → 900 tokens (digest)"],
      [m(8, 44), "stdout", "summarize: wrote digest/2026-08-30.md (2.1 KB)"],
      [m(8, 50), "stdout", "summarize: exit 0"],
    ] }),
    // nightly-summary, the 03:27 run an operator stopped.
    session_NS0: Object.freeze({ source: "stdout and stderr of python3 summarize.py, captured by the Docker Provider", lines: [
      [s(0.6), "stdout", "summarize: FELD_ACTOR=nightly-summary digest=team since=2026-08-29T03:27+09:00"],
      [s(2.5), "stdout", "summarize: collected 12 Session records, 4 Messages, 1 PR event"],
      [m(1, 3), "stdout", "summarize: responses.create gpt-5.4-mini → 310 tokens (outline)"],
      [m(3, 40), "stderr", "summarize: SIGTERM received; aborting before the digest call"],
    ] }),
    // release-check, the 15:20 run that failed with exit 2.
    session_RC1: Object.freeze({ source: "stdout and stderr of release-check, captured by the Docker Provider", lines: [
      [s(0.2), "stdout", "release-check --strict · FELD_ACTOR=release-check"],
      [s(1.1), "stdout", "checking 14 artifacts under /workspace/feld/dist"],
      [m(2, 30), "stdout", "12 artifacts verified (cosign)"],
      [m(3, 20), "stderr", "feld-darwin-arm64.tar.gz: no signature"],
      [m(3, 21), "stderr", "feld-otelcol-darwin-arm64.tar.gz: no signature"],
      [m(3, 22), "stderr", "release blocked: 2 artifacts have no signature"],
      [m(3, 22.5), "stdout", "release-check: exit 2"],
    ] }),
    // pr-checks, a Job without a Schedule spawned once from the CLI for PR #41.
    session_PC: Object.freeze({ source: "stdout and stderr of bash scripts/pr-checks.sh, captured by the tmux Provider", lines: [
      [s(0.3), "stdout", "pr-checks: FELD_ACTOR=pr-checks pr=41"],
      [s(2.1), "stdout", "pr-checks: PR #41 (example-org/feld) → base main, 14 files changed"],
      [s(2.4), "stdout", "pr-checks: cargo fmt --check"],
      [s(6.0), "stdout", "pr-checks: cargo clippy --all-targets"],
      [m(3, 12), "stderr", "warning: unused variable `retries` (src/runtime/monitor.rs:88)"],
      [m(3, 40), "stdout", "pr-checks: cargo test"],
      [m(8, 31), "stdout", "test result: ok. 212 passed; 0 failed; 3 ignored"],
      [m(8, 55), "stdout", "pr-checks: exit 0"],
    ] }),
    // release-check, the 03:20 run whose end the Provider could not prove: the output
    // stops and the container was never observed again.
    session_RC: Object.freeze({ source: "stdout and stderr of release-check, captured by the Docker Provider until the container was lost", lines: [
      [s(0.2), "stdout", "release-check --strict · FELD_ACTOR=release-check"],
      [s(1.0), "stdout", "checking 14 artifacts under /workspace/feld/dist"],
      [m(21, 4), "stdout", "9 artifacts verified (cosign)"],
    ] }),
  });
})(window);
