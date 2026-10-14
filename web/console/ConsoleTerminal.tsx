import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { replaceEqualDeep } from "@tanstack/react-query";
import { useConsoleData } from "./data.ts";
import {
  conversationFocus,
  conversationRecords,
  conversationRows,
  eventRows,
  isBefore,
  linesAfter,
  logLinesFor,
  mergeFeldRecordsIntoLog,
  recordCountLabel,
  timeOffset,
} from "./model.ts";
import {
  ConversationRowView,
  conversationRowKey,
  EventRow,
  FeldRecordRow,
  LogLine,
  RowsContext,
  type RowContext,
} from "./rows.tsx";
import { readingPlaces, useConsoleReading, useConsoleStore } from "./store.ts";

// How near the top a reader has to be for the page before to be read.
const OLDER_THRESHOLD = 160;
// A reader already at the foot is kept there when the tail grows.
const BOTTOM_THRESHOLD = 48;

// The read-only Console of one Session for the card over the Actors Timeline: identity,
// transcript, and footer. `status` is the outcome key the Actors page concluded from the Session's
// lifecycle facts and its Actor's kind; `controls` is drawn beside the Console's own.
// `focus` is a record the Console was opened at (a Feld record ID, or `runtime:<cursor>`):
// once its row is drawn the Console opens it and scrolls to it, then calls `onFocused`.
// `mark` is the record that stays marked while the Console is open.
export interface ConsoleTerminalProps extends Omit<FeldConsoleTerminalOptions, "controls"> {
  session: FeldConsoleSession | null;
  controls?: ReactNode;
  focus?: string | null;
  onFocused?: () => void;
  mark?: string | null;
  onOpenConsole?: (sessionId: string, recordId: string | undefined) => void;
  onOpenJob?: (agentId: string | undefined) => void;
  // The zone the time column is shown in: the Actors page's.
  timeZone: string;
}

export function ConsoleTerminal(props: ConsoleTerminalProps) {
  if (!props.session) {
    return <div className="feld-console__terminal"><p className="feld-console__unavailable">This Session is no longer in the range that was read.</p></div>;
  }
  // Another Session is another Console, with its own reading position.
  return <Terminal key={props.session.id} {...props} session={props.session} />;
}

// The Actors page derives its model again with every read, so what it hands the Console is a
// new object each time: the one held is kept for as long as the new one equals it.
function useSettled<T>(value: T): T {
  const held = useRef(value);
  held.current = replaceEqualDeep(held.current, value);
  return held.current;
}

function offsetIn(element: Element, scroller: HTMLElement) {
  return element.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
}

function rowsShown(scroller: HTMLElement) {
  return [...scroller.querySelectorAll("[data-recording-panel]:not([hidden]) > [data-recording-item]")];
}

// The row the reader sees at the top of the view, and where it is: a row of the panel, not
// one drawn inside it. The probes step past the panel's padding and the gaps between rows.
function topRowIn(scroller: HTMLElement) {
  const box = scroller.getBoundingClientRect();
  for (let y = box.top + 1; y < Math.min(box.bottom, box.top + 80); y += 16) {
    const row = document.elementFromPoint?.(box.left + box.width / 2, y)?.closest("[data-recording-panel] > [data-recording-item]");
    if (row && scroller.contains(row)) return { row, offset: offsetIn(row, scroller) };
  }
  return null;
}

// The head of a paged column: the records before the page held are read when the reader
// reaches the top or presses this.
function Older({ hasOlder, loading, onRead }: { hasOlder: boolean; loading: boolean; onRead: () => void }) {
  if (!hasOlder) return null;
  return <button className="feld-console__older" type="button" data-loading={loading ? "" : undefined} disabled={loading} onClick={onRead}>{loading ? "Reading earlier records…" : "Earlier records · scroll up to read them"}</button>;
}

function Terminal({ session: given, controls, actorLabel, feldRecords: givenRecords, timeOrigin, sessionStart, status, statusLabel, statusTitle, timeZone, focus = null, onFocused, mark = null, onOpenConsole, onOpenJob }: ConsoleTerminalProps & { session: FeldConsoleSession }) {
  const live = status === "running";
  const feldRecords = useSettled(givenRecords);
  const data = useConsoleData(useSettled(given), live, sessionStart);
  const { session, recording, log } = data;
  const id = session.id;
  const reading = useConsoleReading(id);
  const { setView, setOpen } = useConsoleStore.getState();
  const offset = timeOffset(timeOrigin);
  const unavailable = Boolean(session.sourceUnavailable);
  const view = recording ? reading.view : log ? "log" : null;
  const inEvents = reading.view === "events";

  const terminalRef = useRef<HTMLDivElement>(null);
  const transcriptRef = useRef<HTMLDivElement>(null);

  // ---- The rows of the view shown ----
  const conversation = useMemo(() => recording && !inEvents ? conversationRows(recording, feldRecords, offset) : null, [recording, inEvents, feldRecords, offset]);
  const events = useMemo(() => recording && inEvents ? eventRows(recording, feldRecords, offset) : null, [recording, inEvents, feldRecords, offset]);
  const logRows = useMemo(() => log ? mergeFeldRecordsIntoLog(logLinesFor(log, sessionStart), feldRecords) : null, [log, feldRecords, sessionStart]);

  // The rows reach the page's handlers through a ref: the page hands new ones with every
  // drawing, and the rows are not drawn again for them.
  const handlers = useRef({ onOpenConsole, onOpenJob });
  handlers.current = { onOpenConsole, onOpenJob };
  const rows: RowContext = useMemo(() => ({
    sessionId: id,
    mark,
    timeZone,
    onOpenConsole: (sessionId, recordId) => handlers.current.onOpenConsole?.(sessionId, recordId),
    onOpenJob: (agentId) => handlers.current.onOpenJob?.(agentId),
  }), [id, mark, timeZone]);
  // The Conversation marks the row that shows the marked entry.
  const conversationMark = useMemo(() => recording && mark ? conversationFocus(recording, mark) : mark, [recording, mark]);
  const conversationContext: RowContext = useMemo(() => ({ ...rows, mark: conversationMark }), [rows, conversationMark]);

  // ---- The reading position ----
  // The Console opens where the reader left it, or at its tail. A reader at the foot
  // stays there as the tail grows; a page added above keeps the rows under the eye; the
  // page before is read when the reader reaches the top, or when the rows held do not
  // reach past it.
  const position = useRef({
    placed: false,
    fromBottom: 0,
    head: data.head,
    content: null as unknown,
    // A row held at the reader's last scroll, and where it was.
    anchor: null as { row: Element | null; offset: number; height: number; top: number } | null,
    // The row at the top of the view, and where it was.
    top: null as { row: Element; offset: number } | null,
  });
  const content = recording || log;
  const loadOlder = data.loadOlder;
  // A row away from the viewport is laid out at an estimated height and takes its own once
  // it comes near (`content-visibility`), so the column grows after it was placed: a reader
  // at the foot is kept there as it does, and any other reader keeps the row at the top of
  // the view where it was.
  const growth = useRef<ResizeObserver | null>(null);
  useEffect(() => () => growth.current?.disconnect(), []);

  useLayoutEffect(() => {
    const transcript = transcriptRef.current;
    const scroller = transcript;
    if (!scroller) return;
    const held = position.current;
    growth.current ??= new ResizeObserver(() => {
      if (held.fromBottom <= BOTTOM_THRESHOLD) scroller.scrollTop = scroller.scrollHeight;
      else if (held.top?.row.isConnected) scroller.scrollTop += offsetIn(held.top.row, scroller) - held.top.offset;
    });
    for (const panel of scroller.children) growth.current.observe(panel);
    if (!held.placed) {
      held.placed = true;
      const place = readingPlaces.get(id);
      const row = place ? rowsShown(scroller)[place.row] : undefined;
      if (row && place) {
        scroller.scrollTop += offsetIn(row, scroller) - place.offset;
        held.top = { row, offset: place.offset };
      } else {
        scroller.scrollTop = scroller.scrollHeight;
      }
    } else if (data.head !== held.head && isBefore(data.head) && held.anchor) {
      const { row, offset: was, height, top } = held.anchor;
      scroller.scrollTop = row?.isConnected ? scroller.scrollTop + offsetIn(row, scroller) - was : top + (scroller.scrollHeight - height);
    } else if ((content !== held.content) && held.fromBottom <= BOTTOM_THRESHOLD) {
      scroller.scrollTop = scroller.scrollHeight;
    }
    held.head = data.head;
    held.content = content;
    held.fromBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
    // A column too short to scroll raises no scroll, and a read asked for while another
    // was in flight is dropped: each drawing asks again while the reader is at the top,
    // until a read of the page before fails.
    if (data.hasOlder && !data.loadingOlder && !data.olderFailed && scroller.scrollTop <= OLDER_THRESHOLD) loadOlder();
  });

  // Where the reader leaves the Console is where it opens again; a reader who leaves it at
  // its foot finds it at its tail.
  useLayoutEffect(() => {
    const scroller = transcriptRef.current;
    return () => {
      const { fromBottom, top } = position.current;
      const row = scroller && top && fromBottom > BOTTOM_THRESHOLD ? rowsShown(scroller).indexOf(top.row) : -1;
      if (row !== -1) readingPlaces.set(id, { row, offset: offsetIn(top!.row, scroller!) });
      else readingPlaces.delete(id);
    };
  }, [id]);

  // Only the Console's own scrolling region counts: a table or a code block scrolled
  // sideways inside the transcript is not the reader reaching the top of it.
  useEffect(() => {
    const transcript = transcriptRef.current;
    if (!transcript) return;
    function onScroll(event: Event) {
      const scroller = transcript!;
      if (event.currentTarget !== scroller) return;
      const held = position.current;
      held.fromBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
      held.top = topRowIn(scroller);
      // Any row held now stays where it is when a page is added above it, so the first
      // one is measured on every scroll: the page asked for below can arrive after more.
      const row = terminalRef.current?.querySelector("[data-recording-panel]:not([hidden]) > [data-recording-item]:not([hidden])") || null;
      held.anchor = { row, offset: row ? offsetIn(row, scroller) : 0, height: scroller.scrollHeight, top: scroller.scrollTop };
      if (scroller.scrollTop <= OLDER_THRESHOLD) loadOlder();
    }
    transcript.addEventListener("scroll", onScroll);
    return () => transcript.removeEventListener("scroll", onScroll);
  }, [id, loadOlder]);

  // A Console opened at a record shows it: a transcript entry in the view shown, with the
  // Work group that holds a tool call opened; a Feld record with its ledger opened. The
  // row may not be drawn yet — the read that holds it can still be on its way — so this
  // runs again with every change until it is, and once for each record it is opened at.
  const focused = useRef<string | null>(null);
  useLayoutEffect(() => {
    if (!focus) focused.current = null;
    if (!focus || focused.current === focus) return;
    const terminal = terminalRef.current;
    const scroller = transcriptRef.current;
    if (!terminal || !scroller) return;
    let row: Element | null;
    if (focus.startsWith("runtime:")) {
      const shown = (conversation ? conversationFocus(recording!, focus) : focus).slice("runtime:".length);
      // A tool call is drawn once the Work group that holds it is open.
      const work = conversation?.find((candidate) => candidate.kind === "work" && candidate.items.some((item) => item.record.cursor === shown));
      if (work && !reading.open.has(conversationRowKey(work))) {
        setOpen(id, conversationRowKey(work), true);
        return;
      }
      row = terminal.querySelector(`[data-recording-panel]:not([hidden]) [data-runtime-record="${CSS.escape(shown)}"]`);
    } else {
      row = terminal.querySelector(`[data-recording-panel]:not([hidden]) [data-feld-record="${CSS.escape(focus)}"]`)
        || terminal.querySelector(`[data-feld-record="${CSS.escape(focus)}"]`);
      if (row && !reading.open.has(`feld-${focus}`)) {
        setOpen(id, `feld-${focus}`, true);
        return;
      }
    }
    if (!row) return;
    focused.current = focus;
    scroller.scrollTop = Math.max(0, offsetIn(row, scroller) + scroller.scrollTop - 56);
    onFocused?.();
  });

  // ---- What a live program Session's output gained ----
  // The transcript itself is read on demand: a page added above it is not news. Only the
  // lines added at the tail are told, as a count.
  // The tail is the sequence of the last line read: null once an empty output is read,
  // undefined until the first read answers.
  const program = session.console === "log";
  const tail = useRef<number | null | undefined>(undefined);
  const [added, setAdded] = useState<{ count: number; told: number } | null>(null);
  useEffect(() => {
    if (!program || !data.loaded) return;
    const lines = log?.lines || [];
    const count = live && tail.current !== undefined ? linesAfter(lines, tail.current) : 0;
    tail.current = lines.at(-1)?.[3] ?? tail.current ?? null;
    if (count) setAdded((was) => ({ count, told: (was?.told ?? 0) + 1 }));
  }, [program, data.loaded, log, live]);

  // ---- The markup ----
  const footer = recording ? "Recorded transcript" : log ? live ? "Output so far" : "Recorded output" : unavailable ? "Source unavailable" : session.console === "log" ? "No output captured" : session.console === "missing" && !live ? "No transcript file" : "No transcript file yet";
  const feldCount = `${feldRecords.length} record${feldRecords.length === 1 ? "" : "s"}${recording ? ` (${conversationRecords(feldRecords).length} in Conversation)` : ""}`;
  const emptyNotice = unavailable
    ? `Console source unavailable: ${session.sourceUnavailable}.`
    : session.console === "log"
      ? "No output has been captured for this program Session."
      : session.console === "missing"
        ? live
          ? `The transcript file's location has arrived; ${session.runtimeName || "the Runtime"} creates the file at the first input.`
          : "The transcript file's location arrived, but no file exists there: the Session ended without input, or the Runtime removed the transcript."
        : "The Runtime Adapter has not delivered the transcript file's location yet; Feld shows the transcript once it has it for this exact Session.";
  const held = recording ? recordCountLabel(recording) : null;
  const counts = recording ? `${held} transcript ${held === "1" ? "entry" : "entries"}` : log ? `${log.lines.length} line${log.lines.length === 1 ? "" : "s"}` : "0 transcript entries";
  const older = <Older hasOlder={data.hasOlder} loading={data.loadingOlder} onRead={loadOlder} />;

  let chrome: ReactNode = null;
  let body: ReactNode;
  if (recording) {
    chrome = (
      <div className="feld-console__recording-chrome">
        <div className="feld-console__recording-controls">
          <div className="feld-console__recording-tabs" role="group" aria-label="Transcript detail">
            <button type="button" aria-pressed={!inEvents} data-recording-action="view" data-view="conversation" onClick={() => setView(id, "conversation")}>Conversation</button>
            <button type="button" aria-pressed={inEvents} data-recording-action="view" data-view="events" onClick={() => setView(id, "events")}>All events <span>{recordCountLabel(recording)}</span></button>
          </div>
        </div>
      </div>
    );
    // The view not shown keeps its panel and draws none of its rows.
    body = (
      <>
        <div className="feld-console__recording-panel" data-recording-panel="conversation" hidden={inEvents}>
          {conversation ? older : null}
          <RowsContext value={conversationContext}>
            {conversation?.map((row) => <ConversationRowView key={conversationRowKey(row)} row={row} offset={offset} />)}
          </RowsContext>
        </div>
        <div className="feld-console__recording-panel" data-recording-panel="events" hidden={!inEvents}>
          {events ? older : null}
          {events?.map((item) => item.kind === "feld"
            ? <FeldRecordRow key={`feld-${item.record.id}`} item={item} offset={offset} />
            : <EventRow key={`event-${item.record.cursor}`} item={item} offset={offset} />)}
        </div>
      </>
    );
  } else if (log) {
    // A program Session's Console: the captured stdout and stderr, with Feld's own
    // records placed between them by instant. Every line is shown as printed; nothing in
    // it is read as a Feld fact, and no tools stand over it.
    body = (
      <div className="feld-console__recording-panel feld-console__log" data-recording-panel="log">
        {older}
        {logRows!.map((item) => item.kind === "feld"
          ? <FeldRecordRow key={`feld-${item.record.id}`} item={item} offset={0} />
          : <LogLine key={`line-${item.index}`} line={item} />)}
      </div>
    );
  } else {
    // A Session with no readable transcript file still has Feld's own records, all of them.
    body = (
      <>
        <p className="feld-console__unavailable">{emptyNotice}</p>
        <div className="feld-console__recording-panel" data-recording-panel="events">
          {feldRecords.map((record) => <FeldRecordRow key={`feld-${record.id}`} item={{ record, host: null }} offset={0} />)}
        </div>
      </>
    );
  }

  return (
    <RowsContext value={rows}>
      <div
        ref={terminalRef}
        className={`feld-console__terminal${recording ? " has-recording" : ""}${log ? " has-log" : ""}`}
        data-console-session={id}
        data-recording-id={recording ? recording.id : log ? `log-${id}` : undefined}
        data-recording-view={view || undefined}
      >
        <div className="feld-console__terminal-bar">
          <div className="feld-console__identity">
            <span className={`feld-console__live-dot is-${status}`} aria-hidden="true"></span>
            <div><strong>{actorLabel || session.actor}</strong><small>{id}</small></div>
            <span className={`feld-console__status is-${status}`} title={statusTitle}>{live ? "Live" : statusLabel}</span>
          </div>
          <dl>
            <div><dt>Model</dt><dd title={session.model}>{session.model}</dd></div>
            <div><dt>Placement</dt><dd title={session.placement}>{session.placement}</dd></div>
          </dl>
          <div className="feld-console__terminal-actions">
            {controls}
          </div>
        </div>
        {chrome}
        <div ref={transcriptRef} className="feld-console__transcript is-recording" role="log" aria-live="off" tabIndex={0}>
          {body}
        </div>
        {program && live
          ? <div className="feld-console__announcement" role="status">{added ? <span key={added.told}>{`${added.count} line${added.count === 1 ? "" : "s"} added`}</span> : null}</div>
          : null}
        <div className="feld-console__read-only"><span>{footer}</span><span>{`${counts} · ${feldCount}`}</span></div>
      </div>
    </RowsContext>
  );
}
