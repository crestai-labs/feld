import { expect, test } from "vitest";
import {
  consoleSessionOf,
  conversationFocus,
  conversationRows,
  eventRows,
  imageCaption,
  imagesIn,
  inlineParts,
  joinPages,
  jsonForDisplay,
  lastLogLineIn,
  lastRuntimeRecordIn,
  linesAfter,
  logFromRead,
  logLinesFor,
  logOf,
  mergeFeldRecords,
  mergeFeldRecordsIntoLog,
  messageBlocks,
  nextAfterOf,
  readOf,
  recordCountLabel,
  recordingFromFixture,
  recordingFromRead,
  recordingOf,
  runtimeRecordIn,
  withoutEmptyTail,
  type ConsolePage,
  type EventItem,
} from "./model.ts";

const at = (seconds: number) => new Date(Date.UTC(2026, 8, 15, 1, 0, seconds)).toISOString();
const instant = (seconds: number) => Date.parse(at(seconds));

function entry(cursor: string, kind: FeldApi.TranscriptKind, text = "", seconds = 0): FeldApi.TranscriptEntry {
  return { cursor, at: at(seconds), kind, text, raw: { cursor } };
}

function transcriptPage(records: FeldApi.TranscriptEntry[], fields: Partial<FeldApi.TranscriptPage> = {}): FeldApi.TranscriptPage {
  return { session: "s", state: "readable", detail: null, after: null, nextAfter: records.at(-1)?.cursor ?? null, previousBefore: records[0]?.cursor ?? null, hasOlder: false, records, ...fields };
}

function outputLine(sequence: number, seconds: number, stream: FeldApi.OutputLine["stream"] = "stdout"): FeldApi.OutputLine {
  return { sequence, stream, timestamp: at(seconds), text: `line ${sequence}` };
}

function feldRecord(id: string, seconds: number, fields: Partial<FeldConsoleRecord> = {}): FeldConsoleRecord {
  return { id, type: "message.read", kind: "message", at: instant(seconds), title: id, summary: "", anchor: null, conversation: true, facts: [], links: [], ...fields };
}

test("pages join into one column, each row once", () => {
  const older = transcriptPage([entry("a", "input"), entry("b", "response")], { hasOlder: true, previousBefore: "a", nextAfter: null });
  const tail = transcriptPage([entry("b", "response"), entry("c", "input")]);
  const appended = transcriptPage([entry("c", "input"), entry("d", "response")]);
  expect(joinPages([older, tail, appended]).map((row) => "cursor" in row && row.cursor)).toEqual(["a", "b", "c", "d"]);

  const lines: FeldApi.OutputPage = { session: "s", after: 0, nextAfter: 2, previousBefore: 1, hasOlder: false, lines: [outputLine(1, 1), outputLine(2, 2)] };
  const more: FeldApi.OutputPage = { ...lines, after: 2, nextAfter: 3, lines: [outputLine(2, 2), outputLine(3, 3)] };
  expect(joinPages([lines, more]).map((row) => "sequence" in row && row.sequence)).toEqual([1, 2, 3]);
});

test("a forward read continues from the newest cursor a forward page named", () => {
  const before: FeldConsoleCursor = { before: "c", limit: 200 };
  const first: FeldConsoleCursor = { limit: 200 };
  const olderPage = transcriptPage([entry("a", "input")], { nextAfter: null });
  const tail = transcriptPage([entry("c", "input")], { nextAfter: "c" });
  const empty = transcriptPage([], { nextAfter: null });
  expect(nextAfterOf({ pages: [olderPage, tail, empty], pageParams: [before, first, { after: "c" }] })).toBe("c");
  // A page read with `before` names no forward cursor, even when it carries one.
  expect(nextAfterOf({ pages: [transcriptPage([entry("a", "input")], { nextAfter: "x" })], pageParams: [before] })).toBe(null);
  expect(nextAfterOf({ pages: [], pageParams: [] })).toBe(null);
});

test("an empty forward page is dropped unless it moves the cursor or the state", () => {
  const tail = transcriptPage([entry("a", "input")]);
  const pages = (last: ConsolePage) => ({ pages: [tail, last], pageParams: [{ limit: 200 }, { after: "a", limit: 200 }] as FeldConsoleCursor[] });
  const dropped = withoutEmptyTail(pages(transcriptPage([], { nextAfter: "a" })));
  expect(dropped.pages).toEqual([tail]);
  expect(dropped.pageParams).toHaveLength(1);
  const moved = pages(transcriptPage([], { nextAfter: "b" }));
  expect(withoutEmptyTail(moved)).toBe(moved);
  const lost = pages(transcriptPage([], { nextAfter: "a", state: "lost", detail: "gone" }));
  expect(withoutEmptyTail(lost)).toBe(lost);
  const grew = pages(transcriptPage([entry("b", "response")]));
  expect(withoutEmptyTail(grew)).toBe(grew);
  // The first page is kept even when empty.
  const only = { pages: [transcriptPage([], { nextAfter: null })], pageParams: [{ limit: 200 }] };
  expect(withoutEmptyTail(only)).toBe(only);
});

test("the Console's own read settles what the range reads said of the transcript file", () => {
  const session: FeldConsoleSession = { id: "s", actor: "a", runtime: "tmux · Local", console: "none" };
  // Before its own read answers, the range reads' word stands.
  expect(consoleSessionOf(session, null)).toEqual({ ...session, placement: "tmux · Local" });
  expect(consoleSessionOf(session, readOf(undefined, null)).console).toBe("none");

  const readable = readOf({ pages: [transcriptPage([entry("a", "input")])], pageParams: [{}] }, null);
  expect(consoleSessionOf(session, readable).console).toBeUndefined();
  const missing = readOf({ pages: [transcriptPage([], { state: "missing" })], pageParams: [{}] }, null);
  expect(consoleSessionOf(session, missing).console).toBe("missing");
  const lost = readOf({ pages: [transcriptPage([], { state: "lost", detail: "moved" })], pageParams: [{}] }, null);
  expect(consoleSessionOf(session, lost)).toMatchObject({ console: "none", sourceUnavailable: "moved" });
  // A read that failed with nothing to show is a source Feld could not reach.
  const failed = readOf(undefined, new Error("503 Service Unavailable"));
  expect(consoleSessionOf(session, failed).sourceUnavailable).toBe("Feld could not read it: 503 Service Unavailable");
  // One that failed after rows were held keeps showing them.
  const stale = readOf({ pages: [transcriptPage([entry("a", "input")])], pageParams: [{}] }, new Error("timeout"));
  const known = consoleSessionOf(session, stale);
  expect(known.sourceUnavailable).toBeUndefined();
  expect(recordingFromRead(known, stale)?.records).toHaveLength(1);
  // The range reads' lost state stands until the Console's own read says otherwise.
  expect(recordingFromRead({ ...session, console: undefined, sourceUnavailable: "gone" }, readable)).toBe(null);
});

test("a recording counts what it holds, and whether more sits above it", () => {
  const records = [entry("a", "input"), entry("b", "tool-call", "Read\n{}"), entry("c", "response")];
  expect(recordCountLabel(recordingOf("s", records, true))).toBe("3+");
  expect(recordCountLabel(recordingOf("s", records, false))).toBe("3");
});

test("a record's images are the ones it carries a payload for, once each", () => {
  // Claude Code writes a Read of a PNG twice in one line: the block the model saw and the
  // tool's own result.
  const raw = {
    message: { content: [{ type: "tool_result", content: [{ type: "image", source: { type: "base64", media_type: "image/png", data: "AAAA" } }] }] },
    toolUseResult: { type: "image", file: { base64: "AAAA", type: "image/png" } },
  };
  expect(imagesIn(raw)).toEqual([{ media_type: "image/png", data: "AAAA" }]);
  expect(imagesIn({ toolUseResult: { type: "image", file: { base64: "BBBB", type: "image/jpeg" } } })).toEqual([{ media_type: "image/jpeg", data: "BBBB" }]);
  expect(imagesIn({ content: [{ type: "image" }, { type: "image", source: { type: "url", url: "https://example.com/a.png" } }] })).toEqual([]);
  expect(imagesIn(raw).map(imageCaption)).toEqual(["image/png · 0 KB"]);
  expect(jsonForDisplay(raw)).not.toContain("AAAA");
});

test("a Feld record sits at the transcript entry that quotes its ID, the rest by instant", () => {
  const items = [
    { kind: "input" as const, record: entry("a", "input", "hello", 10), result: null },
    { kind: "notification" as const, record: entry("b", "notification", "<summary>mail</summary> msg_1", 20), result: null },
    { kind: "tool-call" as const, record: entry("c", "tool-call", "Bash\nfeld message send", 30), result: entry("d", "tool-result", "sent msg_2", 31) },
  ];
  const quoted = feldRecord("msg_1", 25, { anchor: "msg_1" });
  const sent = feldRecord("msg_2", 5, { anchor: "msg_2" });
  const early = feldRecord("early", 1);
  const late = feldRecord("late", 40);
  const between = feldRecord("between", 15);
  const merged = mergeFeldRecords(items, [late, quoted, between, sent, early], 0);
  expect(merged.map((item) => item.kind === "feld" ? `feld:${item.record.id}${item.host ? `@${item.host.record.cursor}` : ""}` : item.record.cursor))
    .toEqual(["feld:early", "a", "feld:between", "feld:msg_1@b", "feld:msg_2@c", "feld:late"]);

  // In All events the quoting entry stays, and the record follows it.
  const events: EventItem[] = [{ kind: "event", record: { ...entry("b", "notification", "", 20), raw: { text: "msg_1" } }, ordinal: 1 }];
  const inEvents = mergeFeldRecords(events, [quoted], 0);
  expect(inEvents.map((item) => item.kind)).toEqual(["event", "feld"]);

  // Instants are compared on the fixture clock.
  const shifted = mergeFeldRecords([items[0]], [feldRecord("x", 12)], 5000);
  expect(shifted.map((item) => item.kind)).toEqual(["feld", "input"]);
});

test("the Conversation view gathers tool runs into Work groups and keeps only conversation records", () => {
  const recording = recordingOf("s", [
    entry("a", "input", "go", 1),
    entry("b", "tool-call", "Read\n{}", 2),
    entry("c", "tool-result", "ok", 3),
    entry("d", "tool-call", "Bash\nls", 4),
    entry("e", "response", "done", 5),
    entry("f", "other", "", 6),
  ], false);
  const rows = conversationRows(recording, [feldRecord("obs", 4.5, { conversation: false }), feldRecord("read", 4.5)], 0);
  expect(rows.map((row) => row.kind === "work" ? `work:${row.items.map((item) => item.record.cursor).join(",")}` : row.kind === "feld" ? `feld:${row.record.id}` : row.record.cursor))
    .toEqual(["a", "work:b,d", "feld:read", "e"]);
  // All events lists every entry, `other` included, and every record.
  expect(eventRows(recording, [feldRecord("obs", 4.5, { conversation: false })], 0).map((row) => row.kind === "event" ? row.ordinal : row.record.id))
    .toEqual([1, 2, 3, 4, "obs", 5, 6]);
});

test("the Conversation view leaves reasoning with no text to All events", () => {
  const recording = recordingOf("s", [
    entry("a", "input", "go", 1),
    entry("b", "thinking", "", 2),
    entry("c", "tool-call", "Read\n{}", 3),
    entry("d", "thinking", "  \n", 4),
    entry("e", "tool-call", "Bash\nls", 5),
    entry("f", "thinking", "The file is short.", 6),
    entry("g", "response", "done", 7),
  ], false);
  // An empty reasoning between two calls leaves them in one Work group.
  expect(conversationRows(recording, [], 0).map((row) => row.kind === "work" ? `work:${row.items.map((item) => item.record.cursor).join(",")}` : row.kind === "feld" ? `feld:${row.record.id}` : row.record.cursor))
    .toEqual(["a", "work:c,e", "f", "g"]);
  expect(eventRows(recording, [], 0).map((row) => row.kind === "event" ? row.ordinal : row.record.id)).toEqual([1, 2, 3, 4, 5, 6, 7]);
});

test("a Console opened at reasoning with no text shows the Conversation row next to it", () => {
  const recording = recordingOf("s", [
    entry("a", "thinking", "", 1),
    entry("b", "input", "go", 2),
    entry("c", "tool-call", "Read\n{}", 3),
    entry("d", "tool-result", "ok", 4),
    entry("e", "thinking", "", 5),
  ], false);
  // The last entry is the empty reasoning; the Conversation shows it at the call before it.
  expect(lastRuntimeRecordIn(recording, null)?.recordId).toBe("runtime:e");
  expect(conversationFocus(recording, "runtime:e")).toBe("runtime:c");
  // Nothing shown before it: the row after it.
  expect(conversationFocus(recording, "runtime:a")).toBe("runtime:b");
  // Entries the Conversation shows, entries not held, and Feld record IDs are their own place.
  expect(conversationFocus(recording, "runtime:b")).toBe("runtime:b");
  expect(conversationFocus(recording, "runtime:z")).toBe("runtime:z");
  expect(conversationFocus(recording, "message_1")).toBe("message_1");
  expect(conversationFocus(recordingOf("s", [entry("a", "thinking", "", 1)], false), "runtime:a")).toBe("runtime:a");
});

test("a record precedes a log line at the same instant", () => {
  const log = logOf([outputLine(7, 1), outputLine(8, 2, "stderr")], instant(0));
  expect(log.lines).toEqual([[1000, "stdout", "line 7", 7], [2000, "stderr", "line 8", 8]]);
  const lines = logLinesFor(log, instant(0));
  expect(lines.map((line) => line.index)).toEqual([7, 8]);
  const merged = mergeFeldRecordsIntoLog(lines, [feldRecord("r1", 2), feldRecord("r0", 0)]);
  expect(merged.map((item) => item.kind === "feld" ? item.record.id : item.index)).toEqual(["r0", 7, "r1", 8]);
  // A fixture line has no sequence and is known by its position.
  expect(logLinesFor({ source: "", lines: [[0, "stdout", "x"]] }, 0)[0].index).toBe(0);
});

test("the Actors page reads the last turn, the last line, and the entry that quotes an ID", () => {
  const recording = recordingOf("s", [
    entry("a", "input", "go", 1),
    entry("b", "tool-call", "Bash\nfeld message send", 2),
    entry("c", "system", "", 3),
  ], false);
  const origin = { recordingInstant: instant(0), sessionInstant: instant(100) };
  expect(lastRuntimeRecordIn(recording, origin)).toEqual({ kind: "tool-call", label: "Bash tool call", tool: "Bash", hasResult: false, cursor: "b", at: instant(102), recordId: "runtime:b" });
  expect(lastRuntimeRecordIn(null, null)).toBe(null);
  expect(lastRuntimeRecordIn(recordingOf("s", [entry("c", "system")], false), null)).toBe(null);

  const answered = recordingOf("s", [entry("b", "tool-call", "Bash\nsend", 2), entry("r", "tool-result", "msg_9", 3)], false);
  expect(runtimeRecordIn(answered, "msg_9", null)).toEqual({ cursor: "b", at: instant(2), label: "Bash call" });
  expect(runtimeRecordIn(answered, "msg_0", null)).toBe(null);

  const log = logFromRead({ id: "p", actor: "job", console: "log" }, readOf({ pages: [{ session: "p", after: 0, nextAfter: 4, previousBefore: 3, hasOlder: true, lines: [outputLine(3, 5), outputLine(4, 6, "stderr")] }], pageParams: [{}] }, null), instant(0));
  expect(lastLogLineIn(log, instant(0))).toEqual({ kind: "stderr", label: "stderr line", at: instant(6), recordId: "runtime:line-4" });
  expect(lastLogLineIn(null, 0)).toBe(null);
});

test("a fixture Session may name another Session's recording", () => {
  const shared = recordingOf("shared", [], false);
  const own = recordingOf("own", [], false);
  const recordings = { shared_short: shared, session_A: own };
  expect(recordingFromFixture({ id: "session_B", actor: "a", recording: "session_A" }, recordings)).toBe(own);
  expect(recordingFromFixture({ id: "session_C", actor: "a" }, recordings)).toBe(shared);
  expect(recordingFromFixture({ id: "session_A", actor: "a", console: "missing" }, recordings)).toBe(null);
});

test("a message body keeps fenced blocks, tables, inline code, and bold", () => {
  expect(inlineParts("run `ls **x**` then **stop**")).toEqual(["run ", { code: ["ls ", { strong: ["x"] }] }, " then ", { strong: ["stop"] }]);
  expect(inlineParts("**in `src/a.rs` here**")).toEqual([{ strong: ["in ", { code: ["src/a.rs"] }, " here"] }]);
  const blocks = messageBlocks("Intro\n```sh\nls\n```\n| a | b |\n| --- | --- |\n| `1` | 2 |\n\nEnd");
  expect(blocks).toEqual([
    { kind: "plain", parts: ["Intro"] },
    { kind: "code", text: "ls" },
    { kind: "table", headings: [["a"], ["b"]], body: [[[{ code: ["1"] }], ["2"]]] },
    { kind: "plain", parts: ["\nEnd"] },
  ]);
});

test("a heading is its own block and takes the blank lines around it as its margins", () => {
  expect(messageBlocks("## Outcome\n\n- RESULT: DONE\n\n### `make check`\nok\n#not a heading")).toEqual([
    { kind: "heading", level: 2, parts: ["Outcome"] },
    { kind: "plain", parts: ["- RESULT: DONE"] },
    { kind: "heading", level: 3, parts: [{ code: ["make check"] }] },
    { kind: "plain", parts: ["ok\n#not a heading"] },
  ]);
});

test("only the lines added at the tail count as gained", () => {
  const line = (sequence: number): FeldConsoleLog["lines"][number] => [0, "stdout", `line ${sequence}`, sequence];
  const held = [line(4), line(5)];
  expect(linesAfter([...held, line(6), line(7)], 5)).toBe(2);
  expect(linesAfter([line(1), line(2), line(3), ...held], 5)).toBe(0);
  expect(linesAfter(held, null)).toBe(2);
});
