// Projects a Claude Code transcript file onto the common transcript entry shape the Runtime Adapter
// answers `transcript` with (`docs/spec/design/plugin.md`), so the mock reads a Session
// the way the Web UI does: `cursor`, `at`, `kind`, `text`, `error`, `raw`. The Runtime's own
// line stays in `raw`, which the All events view shows.
import fs from "node:fs";
import path from "node:path";

const [sourcePath, outputPath, sessionKey = "session_CD"] = process.argv.slice(2);
if (!sourcePath || !outputPath) {
  throw new Error("Usage: node build-session-recording.mjs <source.jsonl> <output.js> [mock-session-key]");
}

const privateKeys = new Set([
  "accountUuid",
  "atis",
  "ownerAccountUuid",
  "ownerOrganizationUuid",
  "requestId",
  "signature",
  "toolUseResult",
]);

// An image's base64 payload becomes a transparent 1×1 PNG: the Console still shows an image
// where the transcript has one, and the fixture keeps a small size.
const IMAGE_PLACEHOLDER = { type: "base64", media_type: "image/png", data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=" };

function sanitizedCopy(value) {
  return JSON.parse(JSON.stringify(value, (key, entry) => {
    if (privateKeys.has(key)) return undefined;
    return entry?.type === "image" && entry.source?.type === "base64" ? { ...entry, source: IMAGE_PLACEHOLDER } : entry;
  }));
}

// The fixture is a sample: a transcript file that still names a person, their machine, or
// their account is not one. The source has to be rewritten before it becomes a fixture.
const PERSONAL = [
  ["a home directory other than /Users/alice", /\/(?:Users|home)\/(?!alice\b)[A-Za-z0-9._-]+|-(?:Users|home)-(?!alice\b)[A-Za-z0-9._]+-/],
  ["an e-mail address outside example.com", /[A-Za-z0-9._%+-]+@(?!example\.com\b)[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}\b/],
  ["a claude.ai URL", /claude\.ai\//],
];

function personalIn(record) {
  const text = JSON.stringify(record, (key, entry) => (key === "data" && typeof entry === "string" && entry.length > 4096 ? "" : entry));
  return PERSONAL.filter(([, pattern]) => pattern.test(text)).map(([what, pattern]) => `${what}: ${pattern.exec(text)[0]}`);
}

// The `user` records Claude Code writes for a turn someone or something took, as opposed to
// the lines it keeps for the transcript alone.
const HUMAN_PROMPT_SOURCES = new Set(["typed", "queued", "suggestion_accepted", "sdk"]);

function messageBlocks(record) {
  return Array.isArray(record.message?.content) ? record.message.content : [];
}

// Whether a `user` record carries a turn at all: a prompt someone gave, a notification the
// Runtime delivered, or a tool result. A line Claude Code keeps for the transcript alone —
// injected context, a compact summary, an interruption notice — carries none.
function carriesTurn(record) {
  if (record.isMeta || record.isCompactSummary || record.isVisibleInTranscriptOnly) return false;
  if (messageBlocks(record).some((block) => block.type === "tool_result")) return true;
  const origin = record.origin?.kind;
  if (origin === "task-notification") return true;
  return origin === "human" || HUMAN_PROMPT_SOURCES.has(record.promptSource);
}

function blockResultText(result) {
  if (typeof result.content === "string") return result.content;
  if (result.content === undefined) return "";
  return JSON.stringify(result.content, null, 2);
}

// A tool call reads as its name and then the input it was called with, so the Console can
// name the call in one line and show the rest below it.
function toolCallText(call) {
  const name = call.name || "tool";
  if (call.input === undefined || call.input === null) return name;
  return `${name}\n${JSON.stringify(call.input, null, 2)}`;
}

function projectUser(record) {
  if (!carriesTurn(record)) return [{ kind: "other", text: "" }];
  const results = messageBlocks(record)
    .filter((block) => block.type === "tool_result")
    .map((block) => ({ kind: "tool-result", text: blockResultText(block), error: block.is_error === true }));
  if (results.length) return results;
  if (typeof record.message?.content !== "string") return [{ kind: "other", text: "" }];
  const origin = record.origin?.kind;
  return [{ kind: origin === undefined || origin === "human" ? "input" : "notification", text: record.message.content }];
}

function projectAssistant(record) {
  const projected = messageBlocks(record).flatMap((block) => {
    if (block.type === "text") return [{ kind: "response", text: block.text || "" }];
    if (block.type === "tool_use") return [{ kind: "tool-call", text: toolCallText(block) }];
    return [];
  });
  if (projected.length) return projected;
  const thinking = messageBlocks(record).filter((block) => block.type === "thinking").map((block) => block.thinking);
  return thinking.length ? [{ kind: "thinking", text: thinking.join("\n") }] : [{ kind: "other", text: "" }];
}

// Which kinds one Claude Code record carries, in the order they are shown. A record with
// several blocks of interest becomes several records: an assistant turn that answers and
// then calls a tool is a `response` followed by a `tool-call`.
function projectRecord(record) {
  if (record.type === "user") return projectUser(record);
  if (record.type === "assistant") return projectAssistant(record);
  if (record.type === "system" && record.subtype === "local_command") {
    return [{ kind: "system", text: record.content || "" }];
  }
  if (record.type === "system" && record.subtype === "turn_duration") {
    return [{ kind: "system", text: record.durationMs ? `Turn completed in ${record.durationMs} ms` : "Turn completed" }];
  }
  return [{ kind: "other", text: "" }];
}

function padded(value, width) {
  return String(value).padStart(width, "0");
}

const raw = fs.readFileSync(sourcePath, "utf8")
  .split(/\r?\n/)
  .filter(Boolean)
  .map((line) => sanitizedCopy(JSON.parse(line)));

const personal = raw.flatMap((record, line) => personalIn(record).map((found) => `line ${line + 1}: ${found}`));
if (personal.length) {
  throw new Error(`The transcript file is not a sample yet:\n${personal.slice(0, 20).join("\n")}`);
}

// The cursor orders every record by the instant it carries, so a record appended later
// sorts after everything read before it. Records Claude Code wrote without an instant sort
// with the last instant before them.
const projected = [];
let ordering = 0;
raw.forEach((record, line) => {
  const at = typeof record.timestamp === "string" ? record.timestamp : null;
  const instant = at ? Date.parse(at) : Number.NaN;
  if (Number.isFinite(instant)) ordering = instant;
  projectRecord(record).forEach((item, index) => {
    projected.push({
      cursor: `${padded(ordering, 13)}--${padded(line, 6)}-${padded(index, 2)}`,
      at,
      kind: item.kind,
      text: item.text,
      ...(item.error ? { error: true } : {}),
      source: line,
    });
  });
});

const instants = projected.map((record) => record.at).filter(Boolean);
const recording = {
  id: path.basename(sourcePath, path.extname(sourcePath)),
  sourceName: path.basename(sourcePath),
  startedAt: instants[0] || null,
  endedAt: instants.at(-1) || null,
};

function literal(value) {
  return JSON.stringify(value).replaceAll("\u2028", "\\u2028").replaceAll("\u2029", "\\u2029");
}

const output = `// A sample transcript, generated from a Claude Code JSONL. Do not edit by hand.
(function registerSessionRecording(global) {
  "use strict";
  const ui = global.FeldWebUI = global.FeldWebUI || {};
  ui.data = ui.data || {};
  // The Runtime's own lines, and the common-form records the Runtime Adapter projects from
  // them. \`source\` names the line a record came from and becomes its \`raw\`.
  const raw = ${literal(raw)};
  const records = Object.freeze(${literal(projected)}.map(({ source, ...record }) => Object.freeze({ ...record, raw: raw[source] })));
  ui.data.sessionRecordings = Object.freeze({ ...(ui.data.sessionRecordings || {}), [${literal(sessionKey)}]: Object.freeze({ ...${literal(recording)}, records }) });
})(window);
`;

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, output);
