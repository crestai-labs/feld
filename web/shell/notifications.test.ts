import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { MESSAGE_WAITING_DELAY, messageNotice, sessionNotice, skipNotice } from "./notifications.ts";

const SINCE = Date.parse("2026-10-03T12:00:00Z");
const AFTER = "2026-10-03T12:05:00Z";
const BEFORE = "2026-10-03T11:55:00Z";

beforeEach(() => {
  vi.stubGlobal("localStorage", { getItem: () => null, setItem() {}, removeItem() {} });
  vi.stubGlobal("navigator", { languages: ["en-US"], language: "en-US" });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function session(fields: { end?: string | null; outcome?: FeldApi.Outcome; startFailed?: string | null; detail?: string }) {
  return {
    id: "session_a", actor: "builder", end: fields.end ?? null, outcome: fields.outcome ?? "running",
    lifecycle: { startSettled: null, startFailed: fields.startFailed ?? null, stop: null, kill: null },
    observation: fields.detail ? { detail: fields.detail } : null,
  } as unknown as FeldApi.Session;
}

test("a Session that ends failed or unknown after the tab opened is a failed Session", () => {
  expect(sessionNotice(session({ end: AFTER, outcome: "failed" }), SINCE)).toMatchObject({
    condition: "session-failed", title: "Session failed · builder", body: "session_a ended with the outcome failed.", tag: "feld:session_a",
  });
  expect(sessionNotice(session({ end: AFTER, outcome: "unknown" }), SINCE)?.condition).toBe("session-failed");
  for (const outcome of ["stopped", "succeeded", "exited", "ended"] as const) {
    expect(sessionNotice(session({ end: AFTER, outcome }), SINCE)).toBeNull();
  }
  expect(sessionNotice(session({ end: BEFORE, outcome: "failed" }), SINCE)).toBeNull();
  expect(sessionNotice(session({ outcome: "running" }), SINCE)).toBeNull();
});

test("a Session that never began is a failed start, not a failed Session", () => {
  expect(sessionNotice(session({ startFailed: AFTER, end: AFTER, outcome: "unknown", detail: "tmux: no server" }), SINCE)).toMatchObject({
    condition: "start-failed", title: "Start failed · builder", body: "session_a never began. tmux: no server",
  });
  expect(sessionNotice(session({ startFailed: AFTER }), SINCE)?.body).toBe("session_a never began.");
  expect(sessionNotice(session({ startFailed: BEFORE, end: AFTER, outcome: "unknown" }), SINCE)).toBeNull();
});

function message(fields: Partial<FeldApi.MessageSummary>) {
  return {
    id: "message_a", from: { type: "admin" }, to: { id: "actor_a", workspace: "default", name: "reviewer" },
    accepted: AFTER, waiting: true, deletedAt: null, preview: "please review", ...fields,
  } as FeldApi.MessageSummary;
}

test("a Message is notified once it has waited a minute", () => {
  const accepted = Date.parse(AFTER);
  expect(messageNotice(message({}), SINCE, accepted + MESSAGE_WAITING_DELAY - 1)).toBeNull();
  expect(messageNotice(message({}), SINCE, accepted + MESSAGE_WAITING_DELAY)).toMatchObject({
    condition: "message-waiting", title: "Message waiting · reviewer", body: "From Admin, unread for a minute: please review", tag: "feld:message_a",
  });
  const from = { type: "actor", id: "actor_b", workspace: "default", name: "developer" } as FeldApi.Requester;
  expect(messageNotice(message({ from }), SINCE, accepted + MESSAGE_WAITING_DELAY)?.body).toBe("From developer, unread for a minute: please review");
  expect(messageNotice(message({ waiting: false }), SINCE, accepted + MESSAGE_WAITING_DELAY)).toBeNull();
  expect(messageNotice(message({ deletedAt: AFTER }), SINCE, accepted + MESSAGE_WAITING_DELAY)).toBeNull();
  expect(messageNotice(message({ accepted: BEFORE }), SINCE, accepted + MESSAGE_WAITING_DELAY)).toBeNull();
});

test("a skipped tick names its planned run and the current Session", () => {
  const record = {
    sequence: 42, at: AFTER, kind: "schedule.skipped", sessionId: "session_b",
    actor: { id: "actor_c", workspace: "default", name: "nightly" },
    data: { planned: "2026-10-03T12:05:00Z", currentSessionId: "session_b" },
  } as FeldApi.FeldRecord;
  expect(skipNotice(record, "UTC")).toEqual({
    condition: "schedule-skipped",
    title: "Schedule tick skipped · nightly",
    body: "The run planned for Oct 3, 12:05 UTC was skipped while session_b was current.",
    tag: "feld:record:42",
  });
});
