// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createQueryClient } from "../api/queries.ts";
import { ActorsPage } from "./ActorsPage.tsx";
import { useActorsStore } from "./store.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let page: HTMLElement;
let shell: HTMLElement;
let root: Root;

beforeEach(async () => {
  useActorsStore.getState().reset(undefined, Date.parse("2026-09-25T00:30:00Z"));
  // The app shell around the page: a sibling of an ancestor of the dialog.
  shell = document.createElement("nav");
  page = document.createElement("div");
  document.body.append(shell, page);
  root = createRoot(page);
  await act(async () => root.render(
    <QueryClientProvider client={createQueryClient()}>
      <MemoryRouter initialEntries={["/ui/?scenario=three-days-ago-1h"]}><ActorsPage /></MemoryRouter>
    </QueryClientProvider>,
  ));
  await vi.waitFor(() => expect(page.querySelector(".feld-agents__schedule")).not.toBeNull(), { timeout: 10000 });
});

afterEach(async () => {
  await act(async () => root.unmount());
  page.remove();
  shell.remove();
});

async function click(selector: string) {
  const found = page.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`${selector} is drawn`);
  await act(async () => found.click());
}

// What lies behind the open dialog: the section's other children and the app shell.
function behind() {
  return [...page.querySelector(".feld-agents")!.children].filter((child) => !child.classList.contains("feld-agents__display-modal")).concat(shell);
}

// The elements in the Tab order: focusable, enabled, and outside anything inert.
function tabbable() {
  const focusable = 'a[href], button, input, select, textarea, [tabindex]';
  return [...document.querySelectorAll<HTMLElement>(focusable)]
    .filter((element) => element.tabIndex >= 0 && !element.matches(":disabled") && !element.closest("[inert]"));
}

const DIALOGS: [string, string[]][] = [
  ["Timeline display", ['[data-action="toggle-agent-display-menu"]']],
  ["Timeline guide", ['[data-action="open-timeline-guide"]']],
  ["Metrics display", ['[data-action="toggle-metrics-menu"]']],
  ["New actor", ['[data-action="new-actor"]']],
  ["New workspace", ['[data-action="toggle-workspace-menu"]', '[data-action="new-workspace"]']],
];

// One drawing of the page serves every dialog: each opens, then closes before the next.
test("each dialog leaves the page behind it inert until it closes", async () => {
  for (const [name, openers] of DIALOGS) {
    for (const opener of openers) await click(opener);
    const dialog = page.querySelector('[role="dialog"][aria-modal="true"]')!;
    expect(dialog, name).not.toBeNull();
    expect(dialog.closest("[inert]"), name).toBeNull();
    expect(behind().length, name).toBeGreaterThan(0);
    for (const child of behind()) expect(child.hasAttribute("inert"), name).toBe(true);
    // Every element Tab and Shift+Tab can reach is in the dialog.
    expect(tabbable().filter((element) => !dialog.contains(element)), name).toEqual([]);
    expect(tabbable().length, name).toBeGreaterThan(0);

    await click(".feld-agents__display-scrim");
    expect(page.querySelector('[role="dialog"][aria-modal="true"]'), name).toBeNull();
    expect(document.querySelector("[inert]"), name).toBeNull();
  }
});

test("New actor starts as a Job, changes kind by its two options, and keeps each field's help behind an info mark", async () => {
  await click('[data-action="new-actor"]');
  const form = () => page.querySelector("[data-new-actor]")!;
  expect(form().querySelector('[data-kind="job"]')?.getAttribute("aria-checked")).toBe("true");
  expect(form().querySelector('[data-kind="agent"]')?.getAttribute("aria-checked")).toBe("false");
  expect(form().querySelector('[name="command"]')).not.toBeNull();
  expect(form().querySelector('[name="execution"]')).toBeNull();
  // The help is a popover beside the name, not a line under the field.
  expect(form().querySelector(".feld-agents__field > small")).toBeNull();
  expect(form().querySelectorAll('.feld-agents__field-info [role="tooltip"]').length).toBeGreaterThan(3);
  // The mark is no button, so the label's control stays the field itself.
  expect(form().querySelector(".feld-agents__field-info button")).toBeNull();
  expect(form().querySelector('.feld-agents__field-info [role="button"]')).not.toBeNull();
  // Advanced offers the Provider and no Node: one Node runs everything.
  await click("[data-new-advanced] summary");
  expect(form().querySelector('[name="provider"]')).not.toBeNull();
  expect(form().querySelector('[name="node"]')).toBeNull();

  await click('[data-kind="agent"]');
  expect(useActorsStore.getState().newActor?.kind).toBe("agent");
  expect(form().querySelector('[data-kind="agent"]')?.getAttribute("aria-checked")).toBe("true");
  expect(form().querySelector('[name="runtime"]')).not.toBeNull();
  expect(form().querySelector('[name="command"]')).toBeNull();
});
