import { create } from "zustand";

// The viewer's preferences: what one browser stores for itself in localStorage. The Settings page edits them and every
// page reads them as it draws. Feld's own configuration is the daemon's and is read from it.

export const LANGUAGES = ["system", "en", "ja", "zh-Hans"] as const;
export const HOUR_CYCLES = ["h23", "h12"] as const;
export const CONSOLE_VIEWS = ["conversation", "events"] as const;
// Conditions a Feld tab can raise a browser notification for.
export const NOTIFICATIONS = ["message-waiting", "session-failed", "start-failed", "schedule-skipped"] as const;
// The zones the time zone menu names first; any other IANA zone the browser knows follows.
export const COMMON_ZONES = ["UTC", "Asia/Tokyo", "Asia/Shanghai", "Europe/London", "Europe/Berlin", "America/New_York", "America/Los_Angeles"] as const;
// The abbreviations a reader expects where Intl only offers an offset.
const ZONE_ABBREVIATIONS: Record<string, string> = { "Asia/Tokyo": "JST", "Asia/Shanghai": "CST", UTC: "UTC" };

export type Language = Exclude<(typeof LANGUAGES)[number], "system">;
export type HourCycle = (typeof HOUR_CYCLES)[number];
export type ConsoleView = (typeof CONSOLE_VIEWS)[number];

// Every stored key, with its fallback. shell/theme.ts applies and writes the two theme keys.
const KEYS = {
  theme: ["feld-mock-theme", "system"],
  darkTheme: ["feld-mock-dark-theme", "graphite"],
  language: ["feld-mock-language", "system"],
  timeZone: ["feld-mock-time-zone", "daemon"],
  hourCycle: ["feld-mock-hour-cycle", "h23"],
  consoleView: ["feld-mock-console-view", "conversation"],
  notifications: ["feld-mock-notifications", "message-waiting,session-failed,start-failed"],
} as const;
export type PreferenceName = keyof typeof KEYS;

// Layout the reader set by dragging; Reset layout clears these.
const LAYOUT_KEYS = ["feld-mock-actor-width", "feld-mock-actor-column-widths", "feld-mock-sidebar-width", "feld-mock-sidebar", "feld-mock-metrics-window", "feld-mock-actor-order"];
// The Actor pins and Metrics lanes the reader set on the Actors page; Reset all returns them to the default.
const PIN_KEYS = ["feld-mock-actor-pins", "feld-mock-metric-lanes"];

// Each write moves the revision, so a component that draws a preference subscribes to it.
// `reset` names the last reset, for the parts of the page that hold a stored value in state.
export const usePreferences = create<{ revision: number; reset: { kind: "layout" | "all"; revision: number } | null }>(() => ({ revision: 0, reset: null }));

function bump(reset?: "layout" | "all") {
  usePreferences.setState((state) => ({ revision: state.revision + 1, reset: reset ? { kind: reset, revision: state.revision + 1 } : state.reset }));
}

// An empty value is a value: no notification chosen is stored as "".
function read(key: string, fallback: string) {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch (_error) {
    return fallback;
  }
}

function write(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch (_error) {
    // Local storage can be unavailable in privacy modes; the page keeps its defaults.
  }
}

export function get(name: PreferenceName) {
  const [key, fallback] = KEYS[name];
  return read(key, fallback);
}

export function set(name: PreferenceName, value: string) {
  write(KEYS[name][0], value);
  bump();
}

function oneOf<T extends string>(options: readonly T[], value: string, fallback: T): T {
  return (options as readonly string[]).includes(value) ? value as T : fallback;
}

export function browserTimeZone() {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch (_error) {
    return "UTC";
  }
}

export function isTimeZone(name: string) {
  try {
    new Intl.DateTimeFormat("en-US", { timeZone: name });
    return true;
  } catch (_error) {
    return false;
  }
}

// "daemon" and "browser" follow those clocks; anything else is an IANA name. Until the
// daemon's zone is known, "daemon" follows the browser.
export function timeZone(daemonZone: string | null) {
  const choice = get("timeZone");
  const daemon = daemonZone || browserTimeZone();
  if (choice === "daemon") return daemon;
  if (choice === "browser") return browserTimeZone();
  return isTimeZone(choice) ? choice : daemon;
}

// The short name printed after instants, taken at the given instant so it follows DST.
export function zoneLabel(zone: string, at: number) {
  if (ZONE_ABBREVIATIONS[zone]) return ZONE_ABBREVIATIONS[zone];
  // en-US names American zones and en-GB European ones; an offset is the last resort.
  let offset: string | null = null;
  for (const locale of ["en-US", "en-GB"]) {
    try {
      const name = new Intl.DateTimeFormat(locale, { timeZone: zone, timeZoneName: "short" })
        .formatToParts(at).find((item) => item.type === "timeZoneName")?.value;
      if (name && !name.startsWith("GMT") && !name.startsWith("UTC")) return name;
      offset = offset || name || null;
    } catch (_error) {
      return zone;
    }
  }
  return offset || zone;
}

export function hourCycle(): HourCycle {
  return oneOf(HOUR_CYCLES, get("hourCycle"), "h23");
}

export function consoleView(): ConsoleView {
  return oneOf(CONSOLE_VIEWS, get("consoleView"), "conversation");
}

// System follows the browser's first language Feld has a dictionary for.
export function systemLanguage(): Language {
  for (const tag of navigator.languages?.length ? navigator.languages : [navigator.language || "en"]) {
    const lower = String(tag).toLowerCase();
    if (lower.startsWith("ja")) return "ja";
    if (lower.startsWith("zh")) return "zh-Hans";
    if (lower.startsWith("en")) return "en";
  }
  return "en";
}

export function language(): Language {
  const choice = oneOf(LANGUAGES, get("language"), "system");
  return choice === "system" ? systemLanguage() : choice;
}

export function notifications() {
  return new Set(get("notifications").split(",").filter((name) => (NOTIFICATIONS as readonly string[]).includes(name)));
}

export function resetLayout() {
  for (const key of LAYOUT_KEYS) write(key, null);
  bump("layout");
}

export function resetAll() {
  for (const [key] of Object.values(KEYS)) write(key, null);
  for (const key of PIN_KEYS) write(key, null);
  for (const key of LAYOUT_KEYS) write(key, null);
  bump("all");
}
