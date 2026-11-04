// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import type { DaemonSource } from "../daemon/source.ts";
import { SettingsView } from "./SettingsPage.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const source: DaemonSource = {
  phase: "reading", status: null, lastRead: null, workspaces: null, defaultWorkspace: "default",
  actionError: null, restartedAt: null, now: Date.parse("2026-09-25T00:30:00Z"),
  retry() {}, restart: async () => {}, stop: async () => {}, removeDevice: async () => {},
};
let page: HTMLElement;
let root: Root;

beforeEach(async () => {
  page = document.createElement("div");
  document.body.append(page);
  root = createRoot(page);
  await act(async () => root.render(<SettingsView source={source} />));
});

afterEach(async () => {
  await act(async () => root.unmount());
  page.remove();
});

test("the section select names every section of the nav, grouped by scope, and goes to the one chosen", async () => {
  const select = page.querySelector<HTMLSelectElement>(".feld-settings__nav-select")!;
  const links = [...page.querySelectorAll<HTMLElement>(".feld-settings__nav a")].map((link) => link.dataset.settingsLink);
  expect([...select.options].map((option) => option.value)).toEqual(links);
  expect(select.querySelectorAll("optgroup")).toHaveLength(2);
  const marked = () => page.querySelector('.feld-settings__nav a[aria-current="true"]')?.getAttribute("data-settings-link");
  expect(select.value).toBe(marked());

  await act(async () => {
    select.value = "plugins";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(location.hash).toBe("#settings-plugins");
  expect(select.value).toBe("plugins");
  expect(marked()).toBe("plugins");
});
