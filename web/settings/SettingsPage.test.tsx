// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, expect, test } from "vitest";
import type { DaemonSource } from "../daemon/source.ts";
import { loadFixture } from "../fixtures/load.ts";
import { SettingsView } from "./SettingsPage.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const source: DaemonSource = {
  phase: "reading", status: null, lastRead: null, workspaces: null, defaultWorkspace: "default",
  actionError: null, restartedAt: null, now: Date.parse("2026-09-25T00:30:00Z"), operate: true,
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

// The fixture's running daemon, for a caller that operates or only reads.
async function drawRunning(operate: boolean) {
  await act(async () => root.render(<MemoryRouter><SettingsView source={{ ...source, ...loadFixture("three-days-ago-1h").daemon("running"), operate }} /></MemoryRouter>));
}

test("a caller that operates signs out and restarts or stops the daemon", async () => {
  await drawRunning(true);
  expect(page.querySelector('[data-settings-action="remove-device"]')).not.toBeNull();
  expect(page.querySelector('[data-settings-action="sign-in"]')).toBeNull();
  expect(page.querySelectorAll('[data-settings-action="confirm-daemon"]')).toHaveLength(2);
});

test("a caller that only reads is offered Sign in in place of Sign out, and no daemon control", async () => {
  await drawRunning(false);
  const device = page.querySelector("#settings-device")!;
  expect(device.querySelector('[data-settings-action="remove-device"]')).toBeNull();
  expect(device.querySelector('[data-settings-action="sign-in"]')!.getAttribute("href")).toBe("/ui/pair");
  expect(device.textContent).toContain("This browser reads Feld without signing in.");
  expect(page.querySelector('[data-settings-action="confirm-daemon"]')).toBeNull();
  // The daemon's facts stay.
  expect(page.querySelector("#settings-daemon .feld-settings__facts")).not.toBeNull();
});
