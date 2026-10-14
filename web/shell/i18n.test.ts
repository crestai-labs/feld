import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { DICTIONARIES, t } from "./i18n.ts";
import { set } from "./preferences.ts";

beforeEach(() => {
  const items = new Map<string, string>();
  vi.stubGlobal("localStorage", { getItem: (key: string) => items.get(key) ?? null, setItem: (key: string, value: string) => items.set(key, value), removeItem: (key: string) => items.delete(key) });
  vi.stubGlobal("navigator", { languages: ["en-US"], language: "en-US" });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

test("a phrase falls back to English, then to its key, and fills its names", () => {
  set("language", "ja");
  expect(t("settings.title")).toBe("設定");
  // Japanese keeps the concept name of the navigation in English.
  expect(t("nav.actors")).toBe("Actors");
  expect(t("no.such.key")).toBe("no.such.key");
  expect(t("daemon.instance", { id: "dmn_1" })).toBe("Instance dmn_1");
  expect(t("daemon.restarted", { at: "15:00" })).toBe("15:00 に再起動しました");
  expect(t("daemon.restarted")).toBe("{at} に再起動しました");
});

// A language holds only English's keys, and each phrase fills the names English's does.
test("every language follows English's keys and names", () => {
  const names = (phrase: string) => [...phrase.matchAll(/\{(\w+)\}/g)].map(([, name]) => name).sort();
  const english = DICTIONARIES.en;
  for (const [language, dictionary] of Object.entries(DICTIONARIES)) {
    for (const [key, phrase] of Object.entries(dictionary)) {
      expect(english[key], `${language} ${key}`).toBeDefined();
      expect(names(phrase), `${language} ${key}`).toEqual(names(english[key]));
    }
  }
});
