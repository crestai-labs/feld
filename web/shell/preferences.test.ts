import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as prefs from "./preferences.ts";

// A localStorage of this test's own, and a browser whose languages each test chooses.
class MemoryStorage {
  items = new Map<string, string>();
  getItem(key: string) { return this.items.get(key) ?? null; }
  setItem(key: string, value: string) { this.items.set(key, String(value)); }
  removeItem(key: string) { this.items.delete(key); }
  entries() { return Object.fromEntries(this.items); }
}

let storage: MemoryStorage;

function useLanguages(languages: string[]) {
  vi.stubGlobal("navigator", { languages, language: languages[0] });
}

beforeEach(() => {
  storage = new MemoryStorage();
  vi.stubGlobal("localStorage", storage);
  useLanguages(["en-US"]);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

const NAMES: prefs.PreferenceName[] = ["theme", "darkTheme", "language", "timeZone", "hourCycle", "consoleView", "notifications"];
const LAYOUT = { "feld-mock-actor-width": "320", "feld-mock-actor-column-widths": "{\"status\":240}", "feld-mock-sidebar-width": "260", "feld-mock-sidebar": "expanded", "feld-mock-metrics-window": "24h", "feld-mock-actor-order": "[]" };

test("every preference has its key and fallback", () => {
  expect(Object.fromEntries(NAMES.map((name) => [name, prefs.get(name)]))).toEqual({
    theme: "system", darkTheme: "graphite", language: "system", timeZone: "daemon", hourCycle: "h23", consoleView: "conversation",
    notifications: "message-waiting,session-failed,start-failed",
  });
  for (const name of NAMES) prefs.set(name, `web-${name}`);
  expect(storage.entries()).toEqual({
    "feld-mock-theme": "web-theme", "feld-mock-dark-theme": "web-darkTheme", "feld-mock-language": "web-language",
    "feld-mock-time-zone": "web-timeZone", "feld-mock-hour-cycle": "web-hourCycle", "feld-mock-console-view": "web-consoleView",
    "feld-mock-notifications": "web-notifications",
  });
});

test("no notification chosen reads as none, not as the default", () => {
  prefs.set("notifications", "");
  expect(prefs.notifications().size).toBe(0);
  prefs.set("notifications", "session-failed,bogus,schedule-skipped");
  expect([...prefs.notifications()]).toEqual(["session-failed", "schedule-skipped"]);
});

test("the time zone follows the daemon, the browser, or a zone by name", () => {
  const browser = prefs.browserTimeZone();
  expect(prefs.timeZone("Asia/Tokyo")).toBe("Asia/Tokyo");
  // The daemon's zone is not read yet.
  expect(prefs.timeZone(null)).toBe(browser);
  prefs.set("timeZone", "browser");
  expect(prefs.timeZone("Asia/Tokyo")).toBe(browser);
  prefs.set("timeZone", "Europe/Berlin");
  expect(prefs.timeZone("Asia/Tokyo")).toBe("Europe/Berlin");
  prefs.set("timeZone", "Not/AZone");
  expect(prefs.timeZone("Asia/Tokyo")).toBe("Asia/Tokyo");
  expect(prefs.timeZone(null)).toBe(browser);
});

test("a stored value outside the choices reads as the default", () => {
  prefs.set("hourCycle", "h24");
  prefs.set("consoleView", "raw");
  prefs.set("language", "fr");
  expect([prefs.hourCycle(), prefs.consoleView(), prefs.language()]).toEqual(["h23", "conversation", "en"]);
  prefs.set("hourCycle", "h12");
  prefs.set("consoleView", "events");
  prefs.set("language", "zh-Hans");
  expect([prefs.hourCycle(), prefs.consoleView(), prefs.language()]).toEqual(["h12", "events", "zh-Hans"]);
});

test("System takes the browser's first language Feld has words for", () => {
  useLanguages(["fr-FR", "ja-JP", "en-US"]);
  expect(prefs.systemLanguage()).toBe("ja");
  expect(prefs.language()).toBe("ja");
  useLanguages(["zh-TW"]);
  expect(prefs.systemLanguage()).toBe("zh-Hans");
  useLanguages(["de-DE"]);
  expect(prefs.systemLanguage()).toBe("en");
});

test("Reset layout clears the dragged layout and keeps pins and preferences", () => {
  for (const [key, value] of Object.entries(LAYOUT)) storage.setItem(key, value);
  storage.setItem("feld-mock-actor-pins", "{}");
  prefs.set("language", "ja");
  prefs.resetLayout();
  expect(storage.entries()).toEqual({ "feld-mock-actor-pins": "{}", "feld-mock-language": "ja" });
  expect(prefs.usePreferences.getState().reset?.kind).toBe("layout");
});

test("Reset all clears preferences, pins, and layout", () => {
  for (const [key, value] of Object.entries(LAYOUT)) storage.setItem(key, value);
  storage.setItem("feld-mock-actor-pins", "{}");
  for (const name of NAMES) prefs.set(name, "x");
  const before = prefs.usePreferences.getState().revision;
  prefs.resetAll();
  expect(storage.entries()).toEqual({});
  expect(prefs.usePreferences.getState().reset).toEqual({ kind: "all", revision: before + 1 });
});
