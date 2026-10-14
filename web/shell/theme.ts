import { create } from "zustand";
import { storedValue, storeValue } from "./storage.ts";
import { usePreferences } from "./preferences.ts";

// The token sets live in styles/shell.css; the Settings page offers the choice.
export const THEMES = ["system", "light", "dark"] as const;
export const DARK_THEMES = ["graphite", "sumi", "slate"] as const;
export type Theme = (typeof THEMES)[number];
export type DarkTheme = (typeof DARK_THEMES)[number];

function themeOf(value: string): Theme {
  return (THEMES as readonly string[]).includes(value) ? (value as Theme) : "system";
}

function darkThemeOf(value: string): DarkTheme {
  return (DARK_THEMES as readonly string[]).includes(value) ? (value as DarkTheme) : DARK_THEMES[0];
}

// The choice is written to <html>, where the stylesheets read it, and to local storage.
function apply(theme: Theme, darkTheme: DarkTheme) {
  const root = document.documentElement;
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
  root.dataset.darkTheme = darkTheme;
  paintThemeColor();
}

// The browser's own UI takes the page ground. The theme-color tags in index.html hold the
// default grounds until the stylesheet loads; from then on both carry the --bg in effect.
function paintThemeColor() {
  const bg = getComputedStyle(document.documentElement).getPropertyValue("--bg").trim();
  if (!bg) return;
  for (const meta of document.querySelectorAll<HTMLMetaElement>('meta[name="theme-color"]')) meta.content = bg;
}

export const useTheme = create<{ theme: Theme; darkTheme: DarkTheme; setTheme(theme: string): void; setDarkTheme(theme: string): void }>((set, get) => ({
  theme: themeOf(storedValue("feld-mock-theme", "system")),
  darkTheme: darkThemeOf(storedValue("feld-mock-dark-theme", DARK_THEMES[0])),
  setTheme(value) {
    const theme = themeOf(value);
    storeValue("feld-mock-theme", theme);
    apply(theme, get().darkTheme);
    set({ theme });
  },
  setDarkTheme(value) {
    const darkTheme = darkThemeOf(value);
    storeValue("feld-mock-dark-theme", darkTheme);
    apply(get().theme, darkTheme);
    set({ darkTheme });
  },
}));

// Applied before the first render, so the page never paints in the wrong theme.
apply(useTheme.getState().theme, useTheme.getState().darkTheme);
window.addEventListener("load", paintThemeColor);
window.matchMedia?.("(prefers-color-scheme: dark)").addEventListener("change", paintThemeColor);

// Resetting every preference returns the theme to its default.
usePreferences.subscribe((state, previous) => {
  if (state.reset === previous.reset || state.reset?.kind !== "all") return;
  const theme = themeOf(storedValue("feld-mock-theme", "system"));
  const darkTheme = darkThemeOf(storedValue("feld-mock-dark-theme", DARK_THEMES[0]));
  apply(theme, darkTheme);
  useTheme.setState({ theme, darkTheme });
});
