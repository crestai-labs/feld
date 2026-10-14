import { create } from "zustand";
import { consoleView, type ConsoleView } from "../shell/preferences.ts";

// The reading state of the Console open now: the view and the rows the reader opened. It
// starts afresh each time a Session's Console opens.
export interface ConsoleReading {
  view: "conversation" | "events";
  // The `data-console-key` of every ledger, Work group, and event the reader opened.
  open: ReadonlySet<string>;
}

// A Console opens in the viewer's default view.
const FIRST_READINGS: Readonly<Record<ConsoleView, ConsoleReading>> = Object.freeze({
  conversation: Object.freeze({ view: "conversation", open: new Set<string>() }),
  events: Object.freeze({ view: "events", open: new Set<string>() }),
});

function firstReading() {
  return FIRST_READINGS[consoleView()];
}

interface ConsoleStore {
  readings: Record<string, ConsoleReading>;
  setView(sessionId: string, view: ConsoleReading["view"]): void;
  setOpen(sessionId: string, key: string, open: boolean): void;
}

export const useConsoleStore = create<ConsoleStore>((set) => {
  function update(sessionId: string, change: (reading: ConsoleReading) => Partial<ConsoleReading> | null) {
    set((state) => {
      const reading = state.readings[sessionId] || firstReading();
      const next = change(reading);
      return next ? { readings: { ...state.readings, [sessionId]: { ...reading, ...next } } } : state;
    });
  }
  return {
    readings: {},
    setView: (sessionId, view) => update(sessionId, (reading) => reading.view === view ? null : { view }),
    setOpen: (sessionId, key, open) => update(sessionId, (reading) => {
      if (reading.open.has(key) === open) return null;
      const keys = new Set(reading.open);
      if (open) keys.add(key);
      else keys.delete(key);
      return { open: keys };
    }),
  };
});

export function useConsoleReading(sessionId: string) {
  return useConsoleStore((state) => state.readings[sessionId] || firstReading());
}

// Whether the reader opened one ledger: a ledger draws again only when its own state changes.
export function useConsoleOpen(sessionId: string, key: string) {
  return useConsoleStore((state) => (state.readings[sessionId] || firstReading()).open.has(key));
}

// Where the reader left each Console: the row at the top of the view, by its position in the
// view, and how far from the top it was. A Console opens at its tail and then keeps its own
// reading position while the reader moves between Sessions, until it closes; it is written
// when the Console closes and read when it opens again, so it stays out of the render
// state. It names a row rather than a scroll offset, since the rows above are laid out at
// an estimated height until they are drawn.
export interface ReadingPlace {
  row: number;
  offset: number;
}

export const readingPlaces = new Map<string, ReadingPlace>();

// A Session's Console opens: it starts from the first reading, at the position it was left.
export function startReading(sessionId: string) {
  useConsoleStore.setState((state) => {
    if (!(sessionId in state.readings)) return state;
    const { [sessionId]: _, ...readings } = state.readings;
    return { readings };
  });
}

// The Console closes: the next one opens at its tail.
export function forgetConsoles() {
  readingPlaces.clear();
  useConsoleStore.setState({ readings: {} });
}
