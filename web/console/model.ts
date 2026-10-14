// The Console's derivations, free of React and of the daemon: how the pages it read join
// into one column, how Feld's own records are placed among the Runtime's, and what the
// Actors page reads back from a Session's column. Everything here is the same for a read
// Session and for a fixture one.
import { conversationItems, lastTurnRecord, toolName } from "../lib/runtime-records.ts";

export type ConsoleKind = "transcript" | "output";
export type ConsoleRow = FeldApi.TranscriptEntry | FeldApi.OutputLine;
export type ConsolePage = FeldApi.TranscriptPage | FeldApi.OutputPage;

export const LIVE_LOG_SOURCE = "stdout and stderr of the program, captured by the Execution Provider";

// A program Session has no Runtime Adapter, so no transcript: its Console reads the
// captured output instead.
export function consoleKindOf(session: FeldConsoleSession): ConsoleKind {
  return session.console === "log" ? "output" : "transcript";
}

// ---- The pages held -----------------------------------------------------------------------

// The pages the Console read of one Session, oldest first, each with the cursor it was
// read with. A page read with `before` sits in front of the ones held; one read with
// `after` behind them.
export interface ConsolePages {
  pages: ConsolePage[];
  pageParams: FeldConsoleCursor[];
}

// What the Console holds of one Session's reads.
export interface ConsoleRead {
  rows: ConsoleRow[];
  hasOlder: boolean;
  previousBefore: string | number | null;
  nextAfter: string | number | null;
  // The daemon's own word for what it found: pending, missing, lost, or readable.
  state: FeldApi.ConsoleState | null;
  detail: string | null;
  // The last read's failure; a later read that answers clears it.
  error: string | null;
  // Whether any read of this Session has answered, successfully or not.
  loaded: boolean;
}

function rowsOfPage(page: ConsolePage): ConsoleRow[] {
  return "records" in page ? page.records || [] : page.lines || [];
}

// A transcript entry is identified by the cursor its Adapter gave it, a captured output
// line by its own sequence; either way a page adds only what is not held already.
export function rowKey(row: ConsoleRow) {
  return "cursor" in row ? row.cursor : row.sequence;
}

// The rows of every page held, in order, each once.
export function joinPages(pages: readonly ConsolePage[]): ConsoleRow[] {
  const held = new Set<string | number>();
  const rows: ConsoleRow[] = [];
  for (const page of pages) {
    for (const row of rowsOfPage(page)) {
      const key = rowKey(row);
      if (held.has(key)) continue;
      held.add(key);
      rows.push(row);
    }
  }
  return rows;
}

export function isBefore(cursor: FeldConsoleCursor | undefined) {
  return cursor?.before !== undefined;
}

// Where a forward read continues: the cursor the newest page that names one gave. A page
// read with `before` names none, so it is passed over.
export function nextAfterOf({ pages, pageParams }: ConsolePages): string | number | null {
  for (let index = pages.length - 1; index >= 0; index -= 1) {
    if (isBefore(pageParams[index])) break;
    const after = pages[index].nextAfter;
    if (after !== null && after !== undefined) return after;
  }
  return null;
}

// The page before the ones held, when the first of them says there is one.
export function previousBeforeOf(pages: readonly ConsolePage[]): string | number | null {
  const first = pages[0];
  if (!first?.hasOlder) return null;
  return first.previousBefore ?? null;
}

export function readOf(data: ConsolePages | undefined, error: Error | null | undefined): ConsoleRead {
  const pages = data?.pages || [];
  const last = pages.at(-1) as Partial<FeldApi.TranscriptPage> | undefined;
  return {
    rows: joinPages(pages),
    hasOlder: Boolean(pages[0]?.hasOlder),
    previousBefore: previousBeforeOf(pages),
    nextAfter: data ? nextAfterOf(data) : null,
    state: last?.state || null,
    detail: last?.detail || null,
    error: error ? error.message || String(error) : null,
    loaded: Boolean(data) || Boolean(error),
  };
}

// A forward read of a running Session answers every second, usually with nothing new.
// Such a page is dropped rather than held, so a Console left open for hours holds as many
// pages as it read rows, not as many as it asked for.
export function withoutEmptyTail<T extends ConsolePages>(data: T): T {
  const index = data.pages.length - 1;
  if (index < 1) return data;
  const page = data.pages[index] as ConsolePage & Partial<FeldApi.TranscriptPage>;
  const previous = data.pages[index - 1] as ConsolePage & Partial<FeldApi.TranscriptPage>;
  const cursor = data.pageParams[index];
  if (cursor?.after === undefined || rowsOfPage(page).length) return data;
  if ((page.nextAfter ?? null) !== null && page.nextAfter !== cursor.after) return data;
  if (page.state !== previous.state || page.detail !== previous.detail) return data;
  return { ...data, pages: data.pages.slice(0, index), pageParams: data.pageParams.slice(0, index) };
}

// ---- The Session as the Console knows it --------------------------------------------------

// What the daemon found where the transcript file should be, in the Console's own terms. A
// readable file clears the empty-Console notice; `lost` reads as a source that cannot be
// reached at all, and says why. Before this Session's own read has answered, the range
// reads' word stands: they carry the state as of their read, the Console's read is newer.
export function consoleSessionOf(session: FeldConsoleSession, read: ConsoleRead | null): FeldConsoleSession {
  const known: FeldConsoleSession = { ...session, placement: session.placement || session.runtime };
  if (!read || !read.loaded) return known;
  delete known.sourceUnavailable;
  // A read that failed with nothing to show is a source Feld could not reach, and is
  // said so rather than left to read as a transcript file that does not exist.
  if (read.error && !read.rows.length) {
    known.sourceUnavailable = `Feld could not read it: ${read.error}`;
    return known;
  }
  if (consoleKindOf(session) !== "transcript" || !read.state) return known;
  if (read.state === "readable") delete known.console;
  else if (read.state === "missing") known.console = "missing";
  else if (read.state === "lost") {
    known.console = "none";
    known.sourceUnavailable = read.detail || "the transcript file's location can no longer be reached";
  } else known.console = "none";
  return known;
}

// The page held, in the shape a fixture recording has: the transcript file's own lines with
// their sequence. `startedAt` is left out on purpose — a read Session's records carry
// their real instants, so no recording clock is laid over them.
export function recordingOf(id: string, records: FeldApi.TranscriptEntry[], hasOlder: boolean): FeldConsoleRecording {
  return {
    id,
    records,
    // Whether records before the page held exist; the Adapter answers with a cursor
    // rather than a count, so the views say what is held and that more is above it.
    hasOlder,
  };
}

// The lines held, as the log view reads them: the offset from the Session's start, the
// stream, the text, and the line's own sequence so a line keeps its identity when an
// earlier page is added above it.
export function logOf(lines: FeldApi.OutputLine[], sessionStart: number): FeldConsoleLog {
  return {
    source: LIVE_LOG_SOURCE,
    lines: lines.map((line) => [Date.parse(line.timestamp) - sessionStart, line.stream, line.text, line.sequence]),
  };
}

// The lines held after the line of sequence `last` (null: after an empty output): what
// the output gained at its tail. A page read above the lines held adds none.
export function linesAfter(lines: FeldConsoleLog["lines"], last: number | null) {
  return lines.filter(([, , , sequence]) => sequence !== undefined && (last === null || sequence > last)).length;
}

// The recording a Session shows: none for a Session without a readable transcript file.
// A fixture Session may name another Session's recording, and falls back to the shared one.
export function recordingFromFixture(session: FeldConsoleSession, recordings: Readonly<Record<string, FeldConsoleRecording>>) {
  if (session.sourceUnavailable || session.console) return null;
  return recordings[session.recording || session.id] || recordings.shared_short || null;
}

// The captured stdout and stderr of a program Session, or null when it has no such
// record (a Runtime Session, or a program whose output the fixture does not keep).
export function logFromFixture(session: FeldConsoleSession, logs: Readonly<Record<string, FeldConsoleLog>>) {
  if (session.console !== "log") return null;
  return logs[session.id] || null;
}

// ---- Instants ------------------------------------------------------------------------------

// The time column follows the page's timezone and stays 24-hour, like a terminal's log.
const recordingClocks = new Map<string, Intl.DateTimeFormat>();

function recordingClock(timeZone: string) {
  if (!recordingClocks.has(timeZone)) {
    recordingClocks.set(timeZone, new Intl.DateTimeFormat("en-GB", { timeZone, hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false }));
  }
  return recordingClocks.get(timeZone)!;
}

// Runtime instants are shown on the fixture clock: the caller names which recording
// instant coincides with the Session's start, and every displayed instant is shifted by
// that difference. Order and gaps are unchanged; the raw record keeps its own timestamp.
export function timeOffset(timeOrigin: FeldConsoleTimeOrigin | null | undefined) {
  if (!timeOrigin) return 0;
  return timeOrigin.sessionInstant - timeOrigin.recordingInstant;
}

export function displayedInstant(timestamp: string | null, offset: number) {
  return timestamp ? Date.parse(timestamp) + offset : null;
}

export function recordingTime(timestamp: string | null, offset: number, timeZone: string) {
  const instant = displayedInstant(timestamp, offset);
  if (instant === null) return "--:--:--";
  return recordingClock(timeZone).format(new Date(instant));
}

export function instantTime(instant: number, timeZone: string) {
  return recordingClock(timeZone).format(new Date(instant));
}

// ---- A record's text ----------------------------------------------------------------------

export interface ImageSource {
  media_type: string;
  data: string;
}

// The image a block carries: Claude Code writes the model's `source` form and the tool's own
// `file` form, often of the same image in one line. A block without a base64 payload has
// nothing to draw.
function imageOf(entry: any): ImageSource | null {
  const { source, file } = entry;
  if (typeof source?.media_type === "string" && typeof source.data === "string") return { media_type: source.media_type, data: source.data };
  if (typeof file?.type === "string" && typeof file.base64 === "string") return { media_type: file.type, data: file.base64 };
  return null;
}

export function imagesIn(value: unknown) {
  const images: ImageSource[] = [];
  function visit(entry: any) {
    if (!entry || typeof entry !== "object") return;
    if (entry.type === "image") {
      const image = imageOf(entry);
      if (image && !images.some((held) => held.data === image.data)) images.push(image);
      return;
    }
    Object.values(entry).forEach((child) => {
      if (Array.isArray(child)) child.forEach(visit);
      else if (child && typeof child === "object") visit(child);
    });
  }
  visit(value);
  return images;
}

export function imageCaption(source: ImageSource) {
  return `${source.media_type} · ${Math.round(source.data.length * .75 / 1024).toLocaleString("en-US")} KB`;
}

export function jsonForDisplay(value: unknown) {
  return JSON.stringify(value, (key: string, entry: unknown) => {
    if ((key === "data" || key === "base64") && typeof entry === "string") {
      return `[base64 image payload · ${entry.length.toLocaleString("en-US")} characters]`;
    }
    return entry;
  }, 2);
}

// What is derived from one transcript entry is kept with it: an entry keeps its identity
// from one read to the next, so it is derived once however often its rows are drawn.
function perEntry<T>(derive: (record: FeldApi.TranscriptEntry) => T) {
  const held = new WeakMap<FeldApi.TranscriptEntry, T>();
  return (record: FeldApi.TranscriptEntry) => {
    if (!held.has(record)) held.set(record, derive(record));
    return held.get(record)!;
  };
}

export const entryImages = perEntry((record) => imagesIn(record.raw));
export const entryJson = perEntry((record) => jsonForDisplay(record.raw));
export const entryBlocks = perEntry((record) => messageBlocks(record.text));
const entryRawText = perEntry((record) => JSON.stringify(record.raw ?? {}));

// Whether an entry quotes a record ID, kept with the entry: a record is looked for in every
// entry again whenever a page arrives, and each entry's text is searched for it once.
function quoting(text: (record: FeldApi.TranscriptEntry) => string) {
  const found = new WeakMap<FeldApi.TranscriptEntry, Map<string, boolean>>();
  return (record: FeldApi.TranscriptEntry, anchor: string) => {
    let known = found.get(record);
    if (!known) found.set(record, known = new Map());
    if (!known.has(anchor)) known.set(anchor, text(record).includes(anchor));
    return known.get(anchor)!;
  };
}

const textQuotes = quoting((record) => record.text);
const rawQuotes = quoting(entryRawText);

// One line that says what a record was about: its text, on one line, cut where a
// summary stops being readable.
export function oneLine(text: unknown) {
  const line = String(text || "").replace(/\s+/g, " ").trim();
  return line.length > 120 ? `${line.slice(0, 119)}…` : line;
}

// A notification the Runtime retains usually wraps its headline in <summary>; one that
// does not is named by its first line.
export function notificationTitle(text: string) {
  const summary = String(text).match(/<summary>([\s\S]*?)<\/summary>/);
  if (summary) return summary[1].trim();
  return String(text).split("\n").map((line) => line.trim()).find(Boolean) || "Runtime notification";
}

// ---- Message bodies ------------------------------------------------------------------------
// A message is shown as written, with the little markdown a Runtime's answer leans on:
// inline code, bold, headings, fenced blocks, and tables.

export type InlinePart = string | { code: InlinePart[] } | { strong: InlinePart[] };
export type MessageBlock =
  | { kind: "plain"; parts: InlinePart[] }
  | { kind: "heading"; level: number; parts: InlinePart[] }
  | { kind: "code"; text: string }
  | { kind: "table"; headings: InlinePart[][]; body: InlinePart[][][] };

// Inline code is found first and bold after it, over the text with the code marked, so
// bold can hold code and code can hold bold. The marks are private-use characters, which
// neither pattern matches.
const CODE_OPEN = "\uE000";
const CODE_CLOSE = "\uE001";
const STRONG_OPEN = "\uE002";
const STRONG_CLOSE = "\uE003";

// Whether a span opens every code mark it closes, so bold never crosses a code edge.
function balanced(span: string) {
  let depth = 0;
  for (const character of span) {
    if (character === CODE_OPEN) depth += 1;
    else if (character === CODE_CLOSE && --depth < 0) return false;
  }
  return depth === 0;
}

export function inlineParts(value: string): InlinePart[] {
  const marked = value
    .replace(/`([^`\n]+)`/g, `${CODE_OPEN}$1${CODE_CLOSE}`)
    .replace(/\*\*([^*\n]+)\*\*/g, (whole, inner: string) => balanced(inner) ? `${STRONG_OPEN}${inner}${STRONG_CLOSE}` : whole);
  const root: InlinePart[] = [];
  const stack: InlinePart[][] = [root];
  let text = "";
  const flush = () => {
    if (text) stack.at(-1)!.push(text);
    text = "";
  };
  for (const character of marked) {
    if (character === CODE_OPEN || character === STRONG_OPEN) {
      flush();
      const parts: InlinePart[] = [];
      stack.at(-1)!.push(character === CODE_OPEN ? { code: parts } : { strong: parts });
      stack.push(parts);
    } else if (character === CODE_CLOSE || character === STRONG_CLOSE) {
      flush();
      stack.pop();
    } else {
      text += character;
    }
  }
  flush();
  return root;
}

function tableOf(lines: string[]): MessageBlock {
  const rows = lines.map((line) => line.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((cell) => cell.trim()));
  return { kind: "table", headings: rows[0].map(inlineParts), body: rows.slice(2).map((row) => row.map(inlineParts)) };
}

export function messageBlocks(value: string): MessageBlock[] {
  const lines = String(value).split("\n");
  const blocks: MessageBlock[] = [];
  let plain: string[] = [];

  function flushPlain() {
    if (!plain.length) return;
    blocks.push({ kind: "plain", parts: inlineParts(plain.join("\n")) });
    plain = [];
  }

  for (let index = 0; index < lines.length;) {
    // A heading stands on its own: the blank lines around it are its margins.
    const heading = lines[index].match(/^ {0,3}(#{1,4})\s+(.+?)\s*#*\s*$/);
    if (heading) {
      while (plain.length && !plain.at(-1)!.trim()) plain.pop();
      flushPlain();
      blocks.push({ kind: "heading", level: heading[1].length, parts: inlineParts(heading[2]) });
      index += 1;
      while (index < lines.length && !lines[index].trim()) index += 1;
      continue;
    }

    const fence = lines[index].match(/^\s*```\s*([^\s`]*)\s*$/);
    if (fence) {
      flushPlain();
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !/^\s*```\s*$/.test(lines[index])) {
        code.push(lines[index]);
        index += 1;
      }
      index += 1;
      blocks.push({ kind: "code", text: code.join("\n") });
      continue;
    }

    const tableStart = /^\s*\|.*\|\s*$/.test(lines[index])
      && index + 1 < lines.length
      && /^\s*\|?\s*:?-{3,}:?(?:\s*\|\s*:?-{3,}:?)+\s*\|?\s*$/.test(lines[index + 1]);
    if (tableStart) {
      flushPlain();
      const block: string[] = [];
      while (index < lines.length && lines[index].trim()) {
        block.push(lines[index]);
        index += 1;
      }
      blocks.push(tableOf(block));
      continue;
    }

    plain.push(lines[index]);
    index += 1;
  }
  flushPlain();
  return blocks;
}

// ---- Feld's records among the Runtime's ----------------------------------------------------

// One row of the All events view before Feld's records are merged in.
export interface EventItem {
  kind: "event";
  record: FeldApi.TranscriptEntry;
  ordinal: number;
}

// One of Feld's records placed among the transcript entries, with the entry it quotes.
export interface FeldItem {
  kind: "feld";
  record: FeldConsoleRecord;
  host: FeldConversationItem | null;
}

export interface LogLineItem {
  kind: "line";
  at: number;
  stream: FeldApi.OutputLine["stream"];
  text: string;
  index: number;
}

// Whether a transcript entry quotes a record ID: a notification's content, a tool call's
// input or the result that followed it, or in All events the whole entry as written.
export function quotesAnchor(item: FeldConversationItem | EventItem, anchor: string) {
  if (item.kind === "event") return rawQuotes(item.record, anchor);
  if (item.kind === "notification") return textQuotes(item.record, anchor);
  if (item.kind === "tool-call") return textQuotes(item.record, anchor) || Boolean(item.result && textQuotes(item.result, anchor));
  return false;
}

export function runtimeAnchorLabel(host: FeldConversationItem) {
  if (host.kind === "notification") return `Notification retained by the Runtime`;
  return `${toolName(host.record)} call`;
}

// Interleave Feld's records with the Runtime's. A record whose ID the Runtime file quotes
// is placed at that transcript entry and shown with it; any other record is placed by its
// instant. Order comes from the records; nothing here claims that one caused the other.
export function mergeFeldRecords<T extends FeldConversationItem | EventItem>(items: T[], feldRecords: FeldConsoleRecord[], offset: number) {
  const hosts = items.map((item) => ({ item, feldRecords: [] as FeldConsoleRecord[] }));
  const unplaced: FeldConsoleRecord[] = [];
  for (const record of feldRecords) {
    const host = record.anchor ? hosts.find((candidate) => quotesAnchor(candidate.item, record.anchor!)) : null;
    if (host) host.feldRecords.push(record);
    else unplaced.push(record);
  }
  unplaced.sort((left, right) => left.at - right.at);
  const merged: (T | FeldItem)[] = [];
  for (const host of hosts) {
    const instant = displayedInstant(host.item.record.at, offset);
    while (unplaced.length && unplaced[0].at < (instant as number)) merged.push({ kind: "feld", record: unplaced.shift()!, host: null });
    if (host.feldRecords.length && host.item.kind !== "event") {
      merged.push({ kind: "feld", record: host.feldRecords[0], host: host.item as FeldConversationItem });
    } else {
      merged.push(host.item);
      host.feldRecords.forEach((record) => merged.push({ kind: "feld", record, host: null }));
    }
  }
  while (unplaced.length) merged.push({ kind: "feld", record: unplaced.shift()!, host: null });
  return merged;
}

export function conversationRecords(feldRecords: FeldConsoleRecord[]) {
  return feldRecords.filter((record) => record.conversation);
}

// Reasoning with no text is left to All events: the Runtime records it with its content
// withheld.
function hiddenInConversation(item: FeldConversationItem) {
  return item.kind === "thinking" && !item.record.text.trim();
}

// Where the Conversation view shows a transcript entry a Console is opened or marked at
// (`runtime:<cursor>`): reasoning it leaves out is shown by the row before it, or the row
// after it when nothing is shown before it. Any other ID is its own place.
export function conversationFocus(recording: FeldConsoleRecording, recordId: string) {
  if (!recordId.startsWith("runtime:")) return recordId;
  const items = conversationItems(recording.records);
  const index = items.findIndex((item) => item.record.cursor === recordId.slice("runtime:".length));
  if (index === -1 || !hiddenInConversation(items[index])) return recordId;
  const shown = (item: FeldConversationItem) => !hiddenInConversation(item);
  const near = items.slice(0, index).findLast(shown) ?? items.slice(index + 1).find(shown);
  return near ? `runtime:${near.record.cursor}` : recordId;
}

// The Conversation view's rows: Feld's conversation records merged in, and each run of
// tool calls and results gathered into one Work group.
export type ConversationRow =
  | { kind: "work"; items: FeldConversationItem[] }
  | FeldItem
  | FeldConversationItem;

export function conversationRows(recording: FeldConsoleRecording, feldRecords: FeldConsoleRecord[], offset: number): ConversationRow[] {
  const entries = conversationItems(recording.records).filter((item) => !hiddenInConversation(item));
  const items = mergeFeldRecords(entries, conversationRecords(feldRecords), offset);
  const rows: ConversationRow[] = [];
  let tools: FeldConversationItem[] = [];
  function flushTools() {
    if (!tools.length) return;
    rows.push({ kind: "work", items: tools });
    tools = [];
  }
  items.forEach((item) => {
    if (item.kind === "tool-call" || item.kind === "tool-result") {
      tools.push(item as FeldConversationItem);
      return;
    }
    flushTools();
    rows.push(item);
  });
  flushTools();
  return rows;
}

// The All events view lists every transcript entry and every record: a Feld
// record whose ID a transcript entry quotes sits at that record, the rest by instant.
export function eventRows(recording: FeldConsoleRecording, feldRecords: FeldConsoleRecord[], offset: number) {
  const items = recording.records.map((record, index): EventItem => ({ kind: "event", record, ordinal: index + 1 }));
  return mergeFeldRecords(items, feldRecords, offset).map((item): EventItem | FeldItem => item.kind === "feld" ? { kind: "feld", record: item.record, host: null } : item);
}

// A program Session's lines on the fixture clock (the Session's start plus each line's
// offset). A read line carries its own sequence, so it keeps its identity when an earlier
// page is added above it; a fixture line is identified by its position.
export function logLinesFor(log: FeldConsoleLog, sessionStart: number) {
  return log.lines.map(([offset, stream, text, sequence], index): LogLineItem => ({ kind: "line", at: sessionStart + offset, stream, text, index: sequence === undefined ? index : sequence }));
}

export function mergeFeldRecordsIntoLog(lines: LogLineItem[], feldRecords: FeldConsoleRecord[]) {
  const records = feldRecords.map((record): FeldItem & { at: number } => ({ kind: "feld", record, host: null, at: record.at }));
  // At the same instant a record precedes a log line; records keep the
  // order feldRecordsFor gave them.
  return [...lines, ...records].sort((left, right) => left.at - right.at || (left.kind === right.kind ? 0 : left.kind === "feld" ? -1 : 1));
}

// ---- Whether a row shows what it showed ------------------------------------------------------
// The rows are derived again whenever a page arrives, as new objects; a row that shows the
// same records as before is not drawn again.

function sameEntry(left: FeldConversationItem, right: FeldConversationItem) {
  return left.kind === right.kind && left.record === right.record && left.result === right.result;
}

export function sameFeldItem(left: Pick<FeldItem, "record" | "host">, right: Pick<FeldItem, "record" | "host">) {
  return left.record === right.record && (left.host === right.host || (left.host !== null && right.host !== null && sameEntry(left.host, right.host)));
}

export function sameConversationRow(left: ConversationRow, right: ConversationRow) {
  if (left === right) return true;
  if (left.kind === "work") return right.kind === "work" && left.items.length === right.items.length && left.items.every((item, index) => sameEntry(item, right.items[index]));
  if (left.kind === "feld") return right.kind === "feld" && sameFeldItem(left, right);
  return right.kind !== "work" && right.kind !== "feld" && sameEntry(left, right);
}

export function sameEventItem(left: EventItem, right: EventItem) {
  return left.record === right.record && left.ordinal === right.ordinal;
}

export function sameLogLine(left: LogLineItem, right: LogLineItem) {
  return left.at === right.at && left.stream === right.stream && left.text === right.text && left.index === right.index;
}

// ---- What the All events view calls a record -----------------------------------------------

// What one record is called in the All events view, and the tone it is drawn in. Both
// follow the record's kind alone.
const EVENT_TITLES: Partial<Record<FeldApi.TranscriptKind, string>> = {
  input: "Human prompt",
  notification: "Runtime notification",
  response: "Runtime response",
  thinking: "Runtime reasoning",
  "tool-result": "Tool result",
  system: "Runtime line",
  other: "Transcript entry · no turn",
};
const EVENT_TONES: Partial<Record<FeldApi.TranscriptKind, string>> = {
  input: "human",
  response: "claude",
  thinking: "claude",
  "tool-call": "tool",
  "tool-result": "tool",
  other: "context",
};

export function eventTitle(record: FeldApi.TranscriptEntry) {
  if (record.kind === "tool-call") return `${toolName(record)} tool call`;
  return EVENT_TITLES[record.kind] || record.kind;
}

export function eventKind(record: FeldApi.TranscriptEntry) {
  return EVENT_TONES[record.kind] || "system";
}

// What is held of the record column: so many records, and whether more sit above them.
export function recordCountLabel(recording: FeldConsoleRecording) {
  const held = recording.records.length;
  return recording.hasOlder ? `${held}+` : String(held);
}

// ---- What the Actors page reads back ----------------------------------------------------------

// Where one record ID is quoted in a Session's transcript file, for the Message
// record's transcript entry fact; null when the Session has no readable transcript file or
// the file does not quote the ID.
export function runtimeRecordIn(recording: FeldConsoleRecording | null, anchor: string, timeOrigin: FeldConsoleTimeOrigin | null): FeldConsoleRuntimeAnchor | null {
  if (!recording) return null;
  const offset = timeOffset(timeOrigin);
  const host = conversationItems(recording.records).find((item) => quotesAnchor(item, anchor));
  if (!host) return null;
  return { cursor: host.record.cursor, at: displayedInstant(host.record.at, offset), label: runtimeAnchorLabel(host) };
}

// The last transcript entry that carries a turn, on the fixture clock, with the tool a
// call names and whether its result is held; null when the Session has no readable
// records.
export function lastRuntimeRecordIn(recording: FeldConsoleRecording | null, timeOrigin: FeldConsoleTimeOrigin | null): FeldConsoleLastRecord | null {
  if (!recording) return null;
  const last = lastTurnRecord(recording.records);
  if (!last) return null;
  const call = last.kind === "tool-call"
    ? conversationItems(recording.records).find((item) => item.record === last)
    : null;
  return {
    kind: last.kind,
    label: eventTitle(last),
    tool: call ? toolName(last) : undefined,
    hasResult: Boolean(call?.result),
    cursor: last.cursor,
    at: displayedInstant(last.at, timeOffset(timeOrigin)),
    recordId: `runtime:${last.cursor}`,
  };
}

// The last stdout or stderr line of a program Session, on the fixture clock.
export function lastLogLineIn(log: FeldConsoleLog | null, sessionStart: number): FeldConsoleLastRecord | null {
  if (!log || !log.lines.length) return null;
  const line = logLinesFor(log, sessionStart).at(-1)!;
  return { kind: line.stream, label: `${line.stream} line`, at: line.at, recordId: `runtime:line-${line.index}` };
}

// The recording a read Session shows: what is held of its transcript, unless the Console's
// own read (or, before it answers, the range reads) says there is no readable file.
export function recordingFromRead(session: FeldConsoleSession, read: ConsoleRead | null): FeldConsoleRecording | null {
  if (session.sourceUnavailable || session.console || !read?.rows.length) return null;
  return recordingOf(session.id, read.rows as FeldApi.TranscriptEntry[], read.hasOlder);
}

// The captured output a read program Session shows, once it has any.
export function logFromRead(session: FeldConsoleSession, read: ConsoleRead | null, sessionStart: number | null): FeldConsoleLog | null {
  if (session.console !== "log" || !read?.rows.length || !Number.isFinite(sessionStart)) return null;
  return logOf(read.rows as FeldApi.OutputLine[], sessionStart as number);
}
