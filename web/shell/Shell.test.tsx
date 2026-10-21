// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createQueryClient } from "../api/queries.ts";
import { ActorsPage } from "../actors/ActorsPage.tsx";
import { useActorsStore } from "../actors/store.ts";
import { Shell } from "./Shell.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let page: HTMLElement;
let root: Root;

// The compact layout: every media query of the page matches.
beforeEach(async () => {
  vi.spyOn(window, "matchMedia").mockImplementation((query) => ({
    matches: true, media: query, onchange: null,
    addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, dispatchEvent: () => false,
  }));
  useActorsStore.getState().reset(undefined, Date.parse("2026-09-25T00:30:00Z"));
  page = document.createElement("div");
  document.body.append(page);
  root = createRoot(page);
  await act(async () => root.render(
    <QueryClientProvider client={createQueryClient()}>
      <MemoryRouter initialEntries={["/ui/?scenario=three-days-ago-1h"]}><Shell><ActorsPage /></Shell></MemoryRouter>
    </QueryClientProvider>,
  ));
  await vi.waitFor(() => expect(page.querySelector('.sidebar [data-action="select-workspace"]')).not.toBeNull(), { timeout: 10000 });
});

afterEach(async () => {
  await act(async () => root.unmount());
  page.remove();
  vi.restoreAllMocks();
});

function element(selector: string) {
  const found = page.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`${selector} is drawn`);
  return found;
}

async function click(target: HTMLElement) {
  await act(async () => target.click());
}

const menu = () => element('[data-action="toggle-drawer"]');
const drawerOpen = () => element(".viewport").hasAttribute("data-drawer-open");

test("the drawer holds the Workspace list and closes on Escape, the scrim, and a choice", async () => {
  expect(element("[data-workspace-menu]").closest(".sidebar")).not.toBeNull();
  expect(element("[data-workspace-menu]").hidden).toBe(false);

  menu().focus();
  await click(menu());
  expect(drawerOpen()).toBe(true);
  expect(menu().getAttribute("aria-expanded")).toBe("true");
  expect(element(".app").inert).toBe(true);
  expect(document.activeElement?.getAttribute("aria-checked")).toBe("true");
  await act(async () => { document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
  expect(drawerOpen()).toBe(false);
  expect(document.activeElement).toBe(menu());

  await click(menu());
  await click(element(".topbar__scrim"));
  expect(drawerOpen()).toBe(false);

  // The Workspace switcher opens the drawer, and a Workspace chosen there closes it.
  const switcher = element('[data-action="toggle-workspace-menu"]');
  switcher.focus();
  await click(switcher);
  expect(drawerOpen()).toBe(true);
  expect(switcher.getAttribute("aria-expanded")).toBe("true");
  await click(element('[data-action="all-workspaces"]'));
  expect(drawerOpen()).toBe(false);
  expect(useActorsStore.getState().workspace).toBeNull();
  expect(document.activeElement).toBe(switcher);
});

test("the search button opens the field, which stays open while it holds a query", async () => {
  const heading = element(".feld-agents__heading");
  const toggle = element('[data-action="toggle-search"]');
  expect(heading.classList.contains("is-searching")).toBe(false);
  await click(toggle);
  expect(heading.classList.contains("is-searching")).toBe(true);
  expect(document.activeElement).toBe(element('[data-action="search"]'));

  await act(async () => useActorsStore.getState().setQuery("dispatch"));
  await act(async () => element('[data-action="search"]').blur());
  expect(heading.classList.contains("is-searching")).toBe(true);

  await act(async () => useActorsStore.getState().setQuery(""));
  expect(heading.classList.contains("is-searching")).toBe(false);
});
