// The rows of a Console: messages, tool calls, Work groups, notifications, Feld's own
// records, All events entries, and program output lines. A program output row also has
// the text a search matches, which is the text the row shows.
import { createContext, memo, useContext, useMemo, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { readMessage } from "../api/client.ts";
import { keys } from "../api/queries.ts";
import { toolInput, toolName } from "../lib/runtime-records.ts";
import {
  eventKind,
  eventTitle,
  entryBlocks,
  entryImages,
  entryJson,
  imageCaption,
  instantTime,
  messageBlocks,
  notificationTitle,
  oneLine,
  recordingTime,
  runtimeAnchorLabel,
  sameConversationRow,
  sameEventItem,
  sameFeldItem,
  sameLogLine,
  type ConversationRow,
  type EventItem,
  type FeldItem,
  type InlinePart,
  type LogLineItem,
  type MessageBlock,
} from "./model.ts";
import { useConsoleOpen, useConsoleStore } from "./store.ts";

// What every row of one Console shares: the Session whose rows the reader opens, the record
// the reader was sent to, the timezone its times are shown in, and where a record's links lead.
export interface RowContext {
  sessionId: string;
  mark: string | null;
  timeZone: string;
  onOpenConsole?: (sessionId: string, recordId: string | undefined) => void;
  onOpenJob?: (agentId: string | undefined) => void;
}

export const RowsContext = createContext<RowContext>({ sessionId: "", mark: null, timeZone: "UTC" });

// A ledger, a Work group, or an event the reader opened stays open while the Console is
// read again, and when it is opened again later: its state is kept by key, per Session.
// What it opens onto is drawn while it is open.
function Ledger({ consoleKey, className, summary, children, ...rest }: { consoleKey: string; className: string; summary: ReactNode; children: ReactNode; "data-recording-item"?: string; "data-runtime-record"?: string }) {
  const { sessionId } = useContext(RowsContext);
  const open = useConsoleOpen(sessionId, consoleKey);
  return (
    <details className={className} data-console-key={consoleKey} open={open} onToggle={(event) => useConsoleStore.getState().setOpen(sessionId, consoleKey, event.currentTarget.open)} {...rest}>
      {summary}
      {open ? children : null}
    </details>
  );
}

// The class a transcript entry's row carries while the reader was sent to it.
function useFocused(className: string, cursor: string) {
  const { mark } = useContext(RowsContext);
  return mark === `runtime:${cursor}` ? `${className} feld-console__runtime-focus` : className;
}

// ---- Images ------------------------------------------------------------------------------

function ImageGallery({ record, label }: { record: FeldApi.TranscriptEntry; label: string }) {
  const images = entryImages(record);
  if (!images.length) return null;
  return (
    <div className="feld-console__recording-images">
      {images.map((source, index) => (
        <figure key={index}>
          <img src={`data:${source.media_type};base64,${source.data}`} alt={`${label} ${index + 1}`} />
          <figcaption>{imageCaption(source)}</figcaption>
        </figure>
      ))}
    </div>
  );
}

// ---- Tool calls ------------------------------------------------------------------------------

// A tool call and, when the records have it, the result that followed it. A call with no
// result yet is not a failure and not a completion: the Runtime wrote the call and
// nothing after it, which is what the row says.
function ToolResult({ result, name }: { result: FeldApi.TranscriptEntry | null; name: string }) {
  if (!result) return <p className="feld-console__tool-pending">No result is recorded for this call yet.</p>;
  return <><ImageGallery record={result} label={`${name} result`} /><pre>{result.text}</pre></>;
}

function toolState(item: FeldConversationItem) {
  const failed = Boolean(item.result?.error);
  return { failed, state: !item.result ? "No result recorded" : failed ? "Failed" : "Completed", icon: failed ? "!" : item.result ? "↗" : "…" };
}

export function Tool({ item }: { item: FeldConversationItem }) {
  return item.kind === "tool-result" ? <OrphanResult item={item} /> : <ToolCall item={item} />;
}

function ToolCall({ item }: { item: FeldConversationItem }) {
  const { record, result } = item;
  const name = toolName(record);
  const { failed, state, icon } = toolState(item);
  return (
    <Ledger className={useFocused(`feld-console__tool${failed ? " is-error" : ""}${result ? "" : " is-pending"}`, record.cursor)} consoleKey={`tool-${record.cursor}`} data-runtime-record={record.cursor} summary={
      <summary>
        <span className="feld-console__tool-icon" aria-hidden="true">{icon}</span>
        <strong>{name}</strong>
        <span>{oneLine(toolInput(record)) || "No arguments"}</span>
        <small>{state}</small>
      </summary>
    }>
      <div className="feld-console__tool-detail">
        <section><h4>Input</h4><pre>{toolInput(record)}</pre></section>
        <section><h4>Result</h4><ToolResult result={result} name={name} /></section>
      </div>
    </Ledger>
  );
}

// A tool result whose call is above the page held stands on its own.
function OrphanResult({ item }: { item: FeldConversationItem }) {
  const record = item.record;
  const failed = Boolean(record.error);
  return (
    <Ledger className={useFocused(`feld-console__tool${failed ? " is-error" : ""}`, record.cursor)} consoleKey={`result-${record.cursor}`} data-runtime-record={record.cursor} summary={
      <summary>
        <span className="feld-console__tool-icon" aria-hidden="true">{failed ? "!" : "↗"}</span>
        <strong>Tool result</strong>
        <span>{oneLine(record.text) || "Empty result"}</span>
        <small>{failed ? "Failed · no call held" : "No call held"}</small>
      </summary>
    }>
      <div className="feld-console__tool-detail">
        <section><h4>Result</h4><ImageGallery record={record} label="Tool result" /><pre>{record.text}</pre></section>
      </div>
    </Ledger>
  );
}

// A Work group's count: its calls, and the results whose call is above the page held.
function workCount(items: FeldConversationItem[]) {
  const counted = (count: number, noun: string) => count ? `${count} ${noun}${count === 1 ? "" : "s"}` : null;
  const calls = items.filter((item) => item.kind === "tool-call").length;
  return [counted(calls, "tool call"), counted(items.length - calls, "tool result")].filter(Boolean).join(" · ");
}

// A run of tool calls, one line in the conversation that opens onto the calls. The line
// adds the calls that failed, or those with no result yet; nothing once every call completed.
function WorkGroup({ items }: { items: FeldConversationItem[] }) {
  const failures = items.filter((item) => item.result?.error || (item.kind === "tool-result" && item.record.error)).length;
  const pending = items.filter((item) => item.kind === "tool-call" && !item.result).length;
  const state = failures ? `${failures} failed` : pending ? `${pending} without a result yet` : null;
  return (
    <Ledger className={`feld-console__work${failures ? " is-error" : ""}`} consoleKey={`work-${items[0].record.cursor}`} data-recording-item="" summary={
      <summary>
        <span>Work · {workCount(items)}</span>
        {state ? <small>· {state}</small> : null}
      </summary>
    }>
      <div className="feld-console__work-list">{items.map((item) => <Tool key={item.record.cursor} item={item} />)}</div>
    </Ledger>
  );
}

// ---- Messages ---------------------------------------------------------------------------------

function Inline({ parts }: { parts: InlinePart[] }) {
  return parts.map((part, index) => typeof part === "string"
    ? part
    : "code" in part
      ? <code key={index}><Inline parts={part.code} /></code>
      : <strong key={index}><Inline parts={part.strong} /></strong>);
}

function MessageBody({ record }: { record: FeldApi.TranscriptEntry }) {
  return <Blocks blocks={entryBlocks(record)} />;
}

function Blocks({ blocks }: { blocks: MessageBlock[] }) {
  return blocks.map((block, index) => {
    if (block.kind === "plain") return <Inline key={index} parts={block.parts} />;
    if (block.kind === "heading") return <p key={index} className="feld-console__heading" data-level={block.level}><Inline parts={block.parts} /></p>;
    if (block.kind === "code") {
      return <pre key={index} className="feld-console__formatted-block" role="region" aria-label="Preformatted content in conversation" tabIndex={0}><code>{block.text}</code></pre>;
    }
    return (
      <div key={index} className="feld-console__table-scroll" role="region" aria-label="Table in conversation" tabIndex={0}>
        <table>
          <thead><tr>{block.headings.map((cell, column) => <th key={column} scope="col"><Inline parts={cell} /></th>)}</tr></thead>
          <tbody>{block.body.map((row, line) => <tr key={line}>{row.map((cell, column) => <td key={column}><Inline parts={cell} /></td>)}</tr>)}</tbody>
        </table>
      </div>
    );
  });
}

function Message({ item, offset }: { item: FeldConversationItem; offset: number }) {
  const human = item.kind === "input";
  const { timeZone } = useContext(RowsContext);
  return (
    <article className={useFocused(`feld-console__message is-${human ? "human" : "claude"}`, item.record.cursor)} data-recording-item="" data-runtime-record={item.record.cursor}>
      <header>
        <span className="feld-console__speaker" aria-hidden="true">{human ? "Y" : "C"}</span>
        <strong>{human ? "You" : "Assistant"}</strong>
        <time>{recordingTime(item.record.at, offset, timeZone)}</time>
      </header>
      <div className="feld-console__message-body"><MessageBody record={item.record} /></div>
    </article>
  );
}

// ---- Notifications and reasoning --------------------------------------------------------------

function Notification({ item, offset }: { item: FeldConversationItem; offset: number }) {
  const record = item.record;
  const { timeZone } = useContext(RowsContext);
  return (
    <Ledger className={useFocused("feld-console__notification", record.cursor)} consoleKey={`note-${record.cursor}`} data-recording-item="" data-runtime-record={record.cursor} summary={
      <summary>
        <span className="feld-console__notification-mark" aria-hidden="true">!</span>
        <strong>Runtime notification</strong>
        <span>{notificationTitle(record.text)}</span>
        <small>{recordingTime(record.at, offset, timeZone)}</small>
      </summary>
    }>
      <pre>{record.text}</pre>
    </Ledger>
  );
}

// The Runtime's reasoning, kept out of the way: one line that opens onto what the model
// reasoned, rather than something said to the reader.
function Thinking({ item }: { item: FeldConversationItem }) {
  const record = item.record;
  return (
    <Ledger className={useFocused("feld-console__reasoning", record.cursor)} consoleKey={`thinking-${record.cursor}`} data-recording-item="" data-runtime-record={record.cursor} summary={<summary>Reasoning</summary>}>
      <pre>{record.text}</pre>
    </Ledger>
  );
}

function SystemLine({ item, offset }: { item: FeldConversationItem; offset: number }) {
  const { timeZone } = useContext(RowsContext);
  return (
    <div className={useFocused("feld-console__recording-system", item.record.cursor)} data-recording-item="" data-runtime-record={item.record.cursor}>
      <time>{recordingTime(item.record.at, offset, timeZone)}</time><span>System</span><pre>{item.record.text}</pre>
    </div>
  );
}

// ---- Feld's own records ------------------------------------------------------------------------

function FeldMark() {
  return <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 2.5h10M3 8h7M3 13.5h4" strokeLinecap="round"></path></svg>;
}

// The ledger omits the transcript entry fact when that record is shown inside the row.
function factsOf(record: FeldConsoleRecord, host: FeldConversationItem | null) {
  return record.facts.filter((fact) => !(host && fact.key === "runtime"));
}

// Where a record leads: the other Console a Spawn or a Message names, or the Job a
// schedule fired. Navigation, so it stands in the header rather than in the ledger.
function FeldLinks({ record }: { record: FeldConsoleRecord }) {
  const rows = useContext(RowsContext);
  if (!record.links.length) return null;
  return (
    <span className="feld-console__feld-links">
      {record.links.map((link, index) => link.sessionId
        ? <button key={index} type="button" data-action="open-console" data-session-id={link.sessionId} data-record-id={link.recordId} onClick={() => rows.onOpenConsole?.(link.sessionId!, link.recordId)}>{link.label} <span aria-hidden="true">→</span></button>
        : <button key={index} type="button" data-action="open-job" data-agent-id={link.agentId} onClick={() => rows.onOpenJob?.(link.agentId)}>{link.label} <span aria-hidden="true">→</span></button>)}
    </span>
  );
}

// A Message record's envelope line: the other party of this Session, as a mail names
// who it is for or who it is from. The Session of theirs is in the ledger.
function Envelope({ envelope }: { envelope: FeldConsoleEnvelope }) {
  return <p className="feld-console__feld-envelope"><span>{envelope.role}</span><strong>{envelope.actor}</strong></p>;
}

// A body longer than this is shown from its head, and opens whole on request.
const LONG_MESSAGE_LINES = 12;
const LONG_MESSAGE_CHARACTERS = 1200;

// A Message record's text, whole. A fixture holds the body; the daemon's list carries the
// first 48 characters alone, so the row reads the Message once and shows the preview
// until it answers. A long body is clipped until the reader opens it, and stays open
// while the Console is read again, like a ledger.
function MessageText({ envelope, preview }: { envelope: FeldConsoleEnvelope; preview: string | undefined }) {
  const { sessionId } = useContext(RowsContext);
  const openKey = `message-text-${envelope.id}`;
  const open = useConsoleOpen(sessionId, openKey);
  const held = envelope.body;
  const query = useQuery({
    queryKey: keys.message(envelope.id),
    queryFn: ({ signal }) => readMessage(envelope.id, signal),
    enabled: held === null,
    staleTime: Infinity,
    retry: 1,
  });
  const whole = held ?? query.data?.message.body ?? null;
  const text = whole ?? preview ?? "";
  const blocks = useMemo(() => messageBlocks(text), [text]);
  const lineCount = text.split("\n").length;
  const long = lineCount > LONG_MESSAGE_LINES || text.length > LONG_MESSAGE_CHARACTERS;
  const clipped = long && !open;
  return (
    <div className={`feld-console__feld-text${clipped ? " is-clipped" : ""}`}>
      <blockquote className={`feld-console__feld-body${whole === null ? " is-preview" : ""}`}>
        <Blocks blocks={blocks} />
        {whole === null
          ? <small className="feld-console__feld-body-note">{query.error ? `Preview · the whole text could not be read: ${query.error.message}` : "Preview · reading the whole text"}</small>
          : null}
      </blockquote>
      {long
        ? <button type="button" className="feld-console__feld-more" aria-expanded={!clipped} onClick={() => useConsoleStore.getState().setOpen(sessionId, openKey, clipped)}>
            {clipped ? `Show the whole text · ${lineCount} lines` : "Show less"}
          </button>
        : null}
    </div>
  );
}

// One record row: what happened, in a line, and on request its ledger: the record's
// type and ID, the transcript entry it was matched to (or the statement that it was
// placed by instant alone), and its facts. The ledger stays open while the Console is
// read again, per Session.
type FeldRecordRowProps = { item: Pick<FeldItem, "record" | "host">; offset: number };

export const FeldRecordRow = memo(function FeldRecordRow({ item, offset }: FeldRecordRowProps) {
  const { record, host } = item;
  const { mark, timeZone, sessionId } = useContext(RowsContext);
  const facts = factsOf(record, host);
  const factsKey = `feld-${record.id}`;
  const open = useConsoleOpen(sessionId, factsKey);
  return (
    <article className={`feld-console__feld is-${record.kind}${mark === record.id ? " is-focused" : ""}`} data-recording-item="" data-feld-record={record.id}>
      <header>
        <span className="feld-console__feld-mark" aria-hidden="true"><FeldMark /></span>
        <strong>{record.title}</strong>
        {record.summary ? <span className="feld-console__feld-summary">{record.summary}</span> : null}
        <span className="feld-console__feld-tail">
          <FeldLinks record={record} />
          <button type="button" className="feld-console__feld-facts" aria-expanded={open} data-console-key={factsKey} onClick={() => useConsoleStore.getState().setOpen(sessionId, factsKey, !open)}>
            {facts.length} {facts.length === 1 ? "fact" : "facts"}
          </button>
          <time>{instantTime(record.at, timeZone)}</time>
        </span>
      </header>
      {record.message ? <Envelope envelope={record.message} /> : null}
      {record.message
        ? <MessageText envelope={record.message} preview={record.body} />
        : record.body ? <blockquote className="feld-console__feld-body">{record.body}</blockquote> : null}
      {host ? host.kind === "notification" ? <Notification item={host} offset={offset} /> : <Tool item={host} /> : null}
      {open
        ? <div className="feld-console__feld-detail">
            <p className="feld-console__feld-record"><span className="feld-console__feld-id" aria-hidden="true"></span><strong>Record · {record.type}</strong><code>{record.id}</code></p>
            {host
              ? <p className="feld-console__feld-anchor"><span>{runtimeAnchorLabel(host)}</span> quotes <code>{record.anchor}</code> · matched by ID at render time</p>
              : <p className="feld-console__feld-anchor is-instant"><span>Placed by instant</span> · this record has no ID a transcript entry could quote</p>}
            <dl className="feld-console__ledger">
              {facts.map((fact, index) => (
                <div key={index}>
                  <dt>{fact.label}</dt>
                  <dd><strong>{fact.value}</strong><span>{fact.detail}</span><small>{fact.provenance}</small></dd>
                </div>
              ))}
            </dl>
          </div>
        : null}
    </article>
  );
}, (left: FeldRecordRowProps, right: FeldRecordRowProps) => left.offset === right.offset && sameFeldItem(left.item, right.item));

// ---- The views' rows ------------------------------------------------------------------------------

type ConversationRowProps = { row: ConversationRow; offset: number };

export const ConversationRowView = memo(function ConversationRowView({ row, offset }: ConversationRowProps) {
  if (row.kind === "work") return <WorkGroup items={row.items} />;
  if (row.kind === "feld") return <FeldRecordRow item={row} offset={offset} />;
  if (row.kind === "input" || row.kind === "response") return <Message item={row} offset={offset} />;
  if (row.kind === "notification") return <Notification item={row} offset={offset} />;
  if (row.kind === "thinking") return <Thinking item={row} />;
  if (row.kind === "system") return <SystemLine item={row} offset={offset} />;
  return null;
}, (left: ConversationRowProps, right: ConversationRowProps) => left.offset === right.offset && sameConversationRow(left.row, right.row));

export function conversationRowKey(row: ConversationRow) {
  if (row.kind === "work") return `work-${row.items[0].record.cursor}`;
  if (row.kind === "feld") return `feld-${row.record.id}`;
  return `entry-${row.record.cursor}`;
}

type EventRowProps = { item: EventItem; offset: number };

export const EventRow = memo(function EventRow({ item, offset }: EventRowProps) {
  const { record, ordinal } = item;
  const kind = eventKind(record);
  const { timeZone } = useContext(RowsContext);
  return (
    <Ledger className={useFocused(`feld-console__event is-${kind}`, record.cursor)} consoleKey={`event-${record.cursor}`} data-recording-item="" data-runtime-record={record.cursor} summary={
      <summary>
        <code>{String(ordinal).padStart(3, "0")}</code>
        <time>{recordingTime(record.at, offset, timeZone)}</time>
        <span className="feld-console__event-kind">{kind}</span>
        <strong>{eventTitle(record)}</strong>
      </summary>
    }>
      <EventDetail record={record} ordinal={ordinal} />
    </Ledger>
  );
}, (left: EventRowProps, right: EventRowProps) => left.offset === right.offset && sameEventItem(left.item, right.item));

function EventDetail({ record, ordinal }: { record: FeldApi.TranscriptEntry; ordinal: number }) {
  return <div className="feld-console__event-detail"><ImageGallery record={record} label={`Event ${ordinal}`} /><pre>{entryJson(record)}</pre></div>;
}

// ---- Program output -----------------------------------------------------------------------------

type LogLineProps = { line: LogLineItem };

export const LogLine = memo(function LogLine({ line }: LogLineProps) {
  const { mark, timeZone } = useContext(RowsContext);
  const id = `line-${line.index}`;
  return (
    <div className={`feld-console__log-line is-${line.stream}${mark === `runtime:${id}` ? " feld-console__runtime-focus" : ""}`} data-recording-item="" data-runtime-record={id}>
      <time>{instantTime(line.at, timeZone)}</time>
      <span className="feld-console__log-stream" title={line.stream === "stderr" ? "Written to standard error" : "Written to standard output"}>{line.stream}</span>
      <pre>{line.text}</pre>
    </div>
  );
}, (left: LogLineProps, right: LogLineProps) => sameLogLine(left.line, right.line));
