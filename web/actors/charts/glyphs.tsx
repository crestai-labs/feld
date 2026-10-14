// The glyph of every Timeline symbol: the mark as the Timeline draws it, at the size of an
// entry of the Timeline guide.

import type { ReactNode } from "react";
import { arrowheadPath, mailboxMarkerPath } from "../model.ts";
import type { ActorState } from "../types.ts";

const SESSION_TO_END = <line className="feld-agents__session" x1="2" y1="10" x2="20" y2="10" />;
const SESSION_FROM_START = <line className="feld-agents__session" x1="6" y1="10" x2="24" y2="10" />;
const CROSS = "M 16.5 6.5 L 23.5 13.5 M 23.5 6.5 L 16.5 13.5";

// The glyph beside an Actor's name, 16 × 16. Inside, whether the Actor has a current
// Session; the three states differ in shape as well as in colour. The outer ring is the
// Schedule: solid green while it fires, grey and dotted while it is paused, absent when the
// Actor has none.
const ACTOR_STATE_CORES: Record<ActorState["key"], ReactNode> = {
  running: <><circle cx="8" cy="8" r="4.6" /><circle cx="8" cy="8" r="2.2" fill="currentColor" stroke="none" /></>,
  stopped: <><circle cx="8" cy="8" r="4.6" /><rect x="6.2" y="6.2" width="3.6" height="3.6" rx=".7" fill="currentColor" stroke="none" /></>,
  none: <circle cx="8" cy="8" r="4.6" strokeDasharray="2 1.8" />,
};

export function actorStateGlyph(key: ActorState["key"], schedule: ActorState["schedule"]) {
  return (
    <g className={`feld-agents__state-glyph is-${key}`}>
      {schedule ? <circle className={`feld-agents__schedule-ring is-${schedule}`} cx="8" cy="8" r="7.1" /> : null}
      {ACTOR_STATE_CORES[key]}
    </g>
  );
}

const GLYPHS: Readonly<Record<string, ReactNode>> = Object.freeze({
  "actor-running": <g transform="translate(5 0)">{actorStateGlyph("running", null)}</g>,
  "actor-stopped": <g transform="translate(5 0)">{actorStateGlyph("stopped", null)}</g>,
  "actor-none": <g transform="translate(5 0)">{actorStateGlyph("none", null)}</g>,
  "actor-scheduled": <g transform="translate(5 0)">{actorStateGlyph("stopped", "scheduled")}</g>,
  "actor-paused": <g transform="translate(5 0)">{actorStateGlyph("stopped", "paused")}</g>,
  session: <line className="feld-agents__session" x1="2" y1="10" x2="24" y2="10" />,
  "sub-agent": (
    <>
      <line className="feld-agents__session" x1="2" y1="5" x2="24" y2="5" />
      <path className="feld-agents__session is-sub-agent" d="M 9 5 C 9 9, 11 12, 15 12 L 24 12" />
      <circle className="feld-agents__sub-agent-fork" cx="9" cy="5" r="2.5" />
    </>
  ),
  usage: (
    <>
      <line className="feld-agents__session" x1="2" y1="12" x2="24" y2="12" />
      <rect className="feld-agents__token-bar" x="4" y="7" width="3.6" height="3" />
      <rect className="feld-agents__token-bar" x="9" y="4" width="3.6" height="6" />
      <rect className="feld-agents__token-bar" x="14" y="6" width="3.6" height="4" />
      <rect className="feld-agents__token-bar" x="19" y="3" width="3.6" height="7" />
    </>
  ),
  continues: (
    <>
      <line className="feld-agents__session" x1="8" y1="10" x2="24" y2="10" />
      <path className="feld-agents__continuation" d="M 8 6 L 3.5 10 L 8 14" />
    </>
  ),
  started: (
    <>
      {SESSION_FROM_START}
      <circle className="feld-agents__event-fill" cx="6" cy="10" r="3.4" />
    </>
  ),
  attached: (
    <>
      {SESSION_FROM_START}
      <circle className="feld-agents__event" cx="6" cy="10" r="3.4" />
    </>
  ),
  replacement: (
    <>
      {SESSION_FROM_START}
      <rect className="feld-agents__event" x="2.6" y="6.6" width="6.8" height="6.8" transform="rotate(45 6 10)" />
    </>
  ),
  skipped: (
    <>
      <line className="feld-agents__session" x1="2" y1="10" x2="24" y2="10" />
      <g className="feld-agents__skip"><circle cx="13" cy="10" r="4.2" /><path d="M 10.5 12.5 L 15.5 7.5" /></g>
    </>
  ),
  running: <line className="feld-agents__session" x1="2" y1="10" x2="24" y2="10" />,
  succeeded: (
    <>
      {SESSION_TO_END}
      <path className="feld-agents__event is-succeeded" d="M 20 5 L 20 15" />
    </>
  ),
  failed: (
    <>
      {SESSION_TO_END}
      <path className="feld-agents__event is-attention is-failed" d={CROSS} />
    </>
  ),
  exited: (
    <>
      {SESSION_TO_END}
      <path className="feld-agents__event is-attention is-exited" d={CROSS} />
    </>
  ),
  stopped: (
    <>
      {SESSION_TO_END}
      <circle className="feld-agents__event-fill" cx="20" cy="10" r="3.4" />
    </>
  ),
  unknown: (
    <>
      <line className="feld-agents__session" x1="2" y1="10" x2="10" y2="10" />
      <line className="feld-agents__session is-uncertain" x1="10" y1="10" x2="20" y2="10" />
      <circle className="feld-agents__event" cx="20" cy="10" r="3.4" />
    </>
  ),
  spawn: (
    <>
      <line className="feld-agents__relation-link" x1="13" y1="2" x2="13" y2="14" />
      <path className="feld-agents__relation-arrow" d={arrowheadPath(13, 2, 14)} />
    </>
  ),
  message: (
    <>
      <line className="feld-agents__relation-link" x1="13" y1="2" x2="13" y2="14" />
      <path className="feld-agents__relation-arrow" d={arrowheadPath(13, 2, 14)} />
    </>
  ),
  waiting: (
    <>
      <line className="feld-agents__relation-link" x1="13" y1="2" x2="13" y2="14" />
      <path className="feld-agents__mailbox-marker" d={mailboxMarkerPath(13, 2, 14)} />
    </>
  ),
});

export function LegendGlyph({ symbol }: { symbol: string }) {
  return (
    <svg className="feld-agents__legend-glyph" viewBox="0 0 26 16" aria-hidden="true">
      {GLYPHS[symbol]}
    </svg>
  );
}

// The kind glyph: one of two marks beside an Actor's name, and in the New actor form's
// Kind choice. A Control Plane fact declared at creation, so it is always shown and never
// inferred.
export const KIND_GLYPHS: Record<string, ReactNode> = {
  // A robot's head: an Agent loop acts on its own until asked to stop.
  agent: <><rect x="2.2" y="4.2" width="7.6" height="5.6" rx="1.4"></rect><path d="M6 4.2V2.4M4.9 2.2h2.2M4.4 6.8v1.2M7.6 6.8v1.2M1 6.6v1.6M11 6.6v1.6"></path></>,
  // A script file with a prompt: a Job is a bounded program that runs and ends.
  job: <><path d="M7.4 1.3H3.6a1.3 1.3 0 0 0-1.3 1.3v6.8a1.3 1.3 0 0 0 1.3 1.3h4.8a1.3 1.3 0 0 0 1.3-1.3V3.6Z"></path><path d="M7.4 1.3v2.3h2.3"></path><path d="m4.1 5.4 1.4 1.3-1.4 1.3M6.3 8.6h1.7"></path></>,
};
