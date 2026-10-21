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

const store = () => useActorsStore.getState();
let page: HTMLElement;
let root: Root;

beforeEach(async () => {
  store().reset(undefined, Date.parse("2026-09-25T00:30:00Z"));
  page = document.createElement("div");
  document.body.append(page);
  root = createRoot(page);
  await act(async () => root.render(
    <QueryClientProvider client={createQueryClient()}>
      <MemoryRouter initialEntries={["/ui/?scenario=three-days-ago-1h"]}><ActorsPage /></MemoryRouter>
    </QueryClientProvider>,
  ));
  await vi.waitFor(() => expect(page.querySelector(".feld-agents__session-hit")).not.toBeNull(), { timeout: 10000 });
});

afterEach(async () => {
  await act(async () => root.unmount());
  page.remove();
});

function element(selector: string) {
  const found = page.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`${selector} is drawn`);
  return found;
}

async function press(target: Element, key: string) {
  await act(async () => { target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true })); });
}

async function click(target: HTMLElement) {
  await act(async () => target.click());
}

async function openConsole() {
  const hit = element(".feld-agents__session-hit");
  hit.focus();
  await press(hit, "Enter");
  expect(document.activeElement).toBe(element(".feld-agents__console .feld-console__transcript"));
  return hit.getAttribute("data-session")!;
}

async function openComposer() {
  const menu = element('[data-action="toggle-agent-menu"]');
  const agentId = menu.getAttribute("data-agent-id")!;
  await click(menu);
  await click(element(`.feld-agents__action-menu [data-action="send-message"][data-agent-id="${agentId}"]`));
  expect(document.activeElement).toBe(element("[data-composer] textarea"));
  return agentId;
}

const sessionHit = (sessionId: string) => `.feld-agents__session-hit[data-session="${sessionId}"]`;
const actionsMenu = (agentId: string) => `[data-action="toggle-agent-menu"][data-agent-id="${agentId}"]`;

test("a Console closed with Escape or × returns the focus to its Session on the Timeline", async () => {
  let sessionId = await openConsole();
  await press(document.activeElement!, "Escape");
  expect(page.querySelector(".feld-agents__console")).toBeNull();
  expect(document.activeElement).toBe(element(sessionHit(sessionId)));

  // Closed by the keyboard, the Session's line takes the focus with its outline.
  expect(element(sessionHit(sessionId)).hasAttribute("data-pointer-focus")).toBe(false);

  sessionId = await openConsole();
  const close = element('[aria-label="Close Console"]');
  await act(async () => { close.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, composed: true })); });
  await click(close);
  expect(page.querySelector(".feld-agents__console")).toBeNull();
  expect(document.activeElement).toBe(element(sessionHit(sessionId)));
  // Closed by a pointer, it takes the focus without one, until a key moves the focus on.
  expect(element(sessionHit(sessionId)).hasAttribute("data-pointer-focus")).toBe(true);
  await press(document.activeElement!, "Tab");
  expect(element(sessionHit(sessionId)).hasAttribute("data-pointer-focus")).toBe(false);
});

test("the composer closed with Escape, × or Cancel returns the focus to its Actor's Actions menu", async () => {
  for (const close of ["Escape", "×", "Cancel"]) {
    const agentId = await openComposer();
    if (close === "Escape") await press(document.activeElement!, "Escape");
    else if (close === "×") await click(element('[aria-label="Close Message composer"]'));
    else await click(element('[data-composer] [data-action="close-inspector"]'));
    expect(page.querySelector("[data-composer]")).toBeNull();
    expect(document.activeElement).toBe(element(actionsMenu(agentId)));
  }
});

test("Escape closes the composer while its Message is being sent", async () => {
  const agentId = await openComposer();
  await act(async () => store().setComposeDraft("hello"));
  const send = element('[data-composer] [type="submit"]');
  send.focus();
  await click(send);
  // The fixture page has no daemon, so the send is held in flight by hand.
  await act(async () => useActorsStore.setState({ composeSending: { [agentId]: true } }));
  expect(send.hasAttribute("disabled")).toBe(true);
  expect(document.activeElement).toBe(element("[data-composer] textarea"));
  await press(document.activeElement!, "Escape");
  expect(page.querySelector("[data-composer]")).toBeNull();
  expect(document.activeElement).toBe(element(actionsMenu(agentId)));
});

test("a card closed while a search hides its Actor leaves the focus on the search field", async () => {
  await openConsole();
  await act(async () => store().setQuery("no actor matches this"));
  expect(page.querySelector(".feld-agents__session-hit")).toBeNull();
  await click(element('[aria-label="Close Console"]'));
  expect(document.activeElement).toBe(element('[data-action="search"]'));

  await act(async () => store().setQuery(""));
  await openComposer();
  await act(async () => store().setQuery("no actor matches this"));
  expect(page.querySelector('[data-action="toggle-agent-menu"]')).toBeNull();
  await press(document.activeElement!, "Escape");
  expect(document.activeElement).toBe(element('[data-action="search"]'));
});

test("Escape in the Workspace menu closes only the menu and returns the focus to its button", async () => {
  await openConsole();
  const toggle = element('[data-action="toggle-workspace-menu"]');
  // A click focuses nothing here, as in Safari, so opening the menu has to move the focus.
  await click(toggle);
  expect(element("[data-workspace-menu]").hidden).toBe(false);
  expect(document.activeElement).toBe(toggle);
  await press(document.activeElement!, "Escape");
  expect(element("[data-workspace-menu]").hidden).toBe(true);
  expect(page.querySelector(".feld-agents__console")).not.toBeNull();
  expect(document.activeElement).toBe(toggle);

  await click(toggle);
  const item = element('[data-workspace-menu] [data-action="all-workspaces"]');
  item.focus();
  await press(item, "Escape");
  expect(element("[data-workspace-menu]").hidden).toBe(true);
  expect(page.querySelector(".feld-agents__console")).not.toBeNull();
  expect(document.activeElement).toBe(toggle);
});

test("the focus leaving the Workspace menu closes it, so the next Escape closes the Console", async () => {
  await openConsole();
  await click(element('[data-action="toggle-workspace-menu"]'));
  element('[data-workspace-menu] [data-action="all-workspaces"]').focus();
  await act(async () => element('[data-action="search"]').focus());
  expect(element("[data-workspace-menu]").hidden).toBe(true);
  await press(document.activeElement!, "Escape");
  expect(page.querySelector(".feld-agents__console")).toBeNull();
});

test("the Session opened again closes its Console, and the Actor's name pressed again closes its card", async () => {
  const sessionId = await openConsole();
  const hit = element(sessionHit(sessionId));
  await press(hit, "Enter");
  expect(page.querySelector(".feld-agents__console")).toBeNull();
  expect(store().inspectedSessionId).toBeNull();
  expect(document.activeElement).toBe(element(sessionHit(sessionId)));

  const name = element('[data-action="open-actor"]');
  await click(name);
  expect(page.querySelector(".feld-agents__inspector.is-actor")).not.toBeNull();
  expect(store().focusedActorId).toBe(name.getAttribute("data-agent-id"));
  await click(element('[data-action="open-actor"]'));
  expect(page.querySelector(".feld-agents__inspector.is-actor")).toBeNull();
  // The lane's selection came with the card and goes with it.
  expect(store().focusedActorId).toBeNull();
});

test("a range picker closes when the pointer goes down outside it, and stays while it goes down inside", async () => {
  async function pressAt(target: Element) {
    await act(async () => { target.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, composed: true })); });
  }
  await click(element('[data-action="toggle-picker"]'));
  const picker = element(".feld-agents__picker");
  await pressAt(element(".feld-agents__picker h2"));
  expect(page.querySelector(".feld-agents__picker")).toBe(picker);
  await pressAt(element(".feld-agents__bands"));
  expect(page.querySelector(".feld-agents__picker")).toBeNull();
  expect(store().pickerOpen).toBe(false);

  await click(element('[data-action="toggle-metrics-picker"]'));
  expect(page.querySelector(".feld-agents__metrics-picker")).not.toBeNull();
  await pressAt(document.body);
  expect(page.querySelector(".feld-agents__metrics-picker")).toBeNull();
});

test("on the compact layout the range picker shows the Quick ranges and opens the Historical range from Custom…", async () => {
  await click(element('[data-action="toggle-picker"]'));
  expect(page.querySelector('[data-action="toggle-custom-range"]')).toBeNull();
  expect(page.querySelector('[data-action="apply-picker"]')).not.toBeNull();
  await click(element('[data-action="close-picker"]'));

  await act(async () => store().setCompactLayout(true));
  await click(element('[data-action="toggle-picker"]'));
  const custom = element('[data-action="toggle-custom-range"]');
  expect(page.querySelectorAll('[data-action="quick-live"]').length).toBeGreaterThan(0);
  expect(page.querySelector('[data-action="apply-picker"]')).toBeNull();
  expect(custom.getAttribute("aria-expanded")).toBe("false");
  await click(custom);
  expect(custom.getAttribute("aria-expanded")).toBe("true");
  expect(page.querySelector('[data-action="apply-picker"]')).not.toBeNull();
});

test("a Job's Schedule line opens its Actor card at the Job section, whose edit form Escape closes first", async () => {
  const line = element(".feld-agents__schedule[data-agent-id]");
  const agentId = line.getAttribute("data-agent-id")!;
  await click(line);
  const card = element(".feld-agents__inspector.is-actor");
  expect(document.activeElement).toBe(card);
  expect(card.querySelector("[data-job-schedule]")?.textContent).toContain("Schedule");
  expect(card.querySelectorAll('[data-action="pause-job"], [data-action="resume-job"]')).toHaveLength(1);
  expect(page.querySelector('[role="dialog"][aria-modal="true"]')).toBeNull();

  await click(element('[data-action="edit-actor"]'));
  // The Job runs, so its name stays and the focus starts at the Schedule.
  expect((element('[data-edit-form] [name="name"]') as HTMLInputElement).disabled).toBe(true);
  expect(element("[data-rename-blocked]").textContent).toContain("is running");
  expect(document.activeElement).toBe(element('[data-edit-form] [name="cron"]'));
  expect(store().cardView).toMatchObject({ agentId, editing: true });
  await press(document.activeElement!, "Escape");
  expect(store().cardView).toMatchObject({ agentId, editing: false });
  expect(page.querySelector(".feld-agents__inspector.is-actor")).not.toBeNull();
  expect(document.activeElement).toBe(element('[data-action="edit-actor"]'));
  await press(document.activeElement!, "Escape");
  expect(page.querySelector(".feld-agents__inspector.is-actor")).toBeNull();
});

test("a skipped tick opens the Job's card with that tick highlighted among its runs", async () => {
  const skip = element(".feld-agents__unread.is-skip");
  await click(skip);
  const highlighted = element(".feld-agents__run.is-skip.is-highlighted");
  expect(highlighted.getAttribute("data-skip-row")).toBe(store().cardView?.skipId);
  expect(element(".feld-agents__inspector.is-actor").contains(highlighted)).toBe(true);
});

test("an Agent's Actions menu opens its card at the launch spec and grants, with its secrets masked in the card and the edit form", async () => {
  const agentId = "default/developer-01";
  await click(element(actionsMenu(agentId)));
  await click(element(`.feld-agents__action-menu [data-action="open-agent"][data-agent-id="${agentId}"]`));
  const card = element(".feld-agents__inspector.is-actor");
  expect(card.textContent).toContain("read, manage-agent");
  expect(element("[data-runtime-settings]").textContent).toContain('"GITHUB_TOKEN": "••••••••"');
  expect(card.textContent).not.toContain("ghp_fixture");
  expect(element("[data-launch-drift]").textContent).toMatch(/ran with|Same launch spec/);

  await click(element('[data-action="edit-actor"]'));
  const settings = element('[data-edit-form] [name="settings"]') as HTMLTextAreaElement;
  expect(settings.value).toContain('"model": "opus"');
  expect(settings.value).not.toContain("ghp_fixture");
  const env = (element('[data-edit-form] [name="env"]') as HTMLTextAreaElement).value;
  expect(env).toContain("NPM_TOKEN=••••••••");
  expect(env).toContain("CI=1");
  expect((element('[data-edit-form] [name="name"]') as HTMLInputElement).value).toBe("developer-01");
});

test("a Job without a Schedule gets Edit, whose form changes its name and launch spec", async () => {
  const agentId = "default/pr-checks";
  await click(element(`[data-action="open-actor"][data-agent-id="${agentId}"]`));
  expect(element("[data-job-schedule]").textContent).toBe("Schedule");
  expect(page.querySelectorAll('[data-action="pause-job"], [data-action="resume-job"]')).toHaveLength(0);

  await click(element('[data-action="edit-actor"]'));
  expect(page.querySelector('[data-edit-form] [name="cron"]')).toBeNull();
  const name = element('[data-edit-form] [name="name"]') as HTMLInputElement;
  expect(name.disabled).toBe(false);
  expect(document.activeElement).toBe(name);
  expect((element('[data-edit-form] [name="workdir"]') as HTMLInputElement).value).not.toBe("");
  expect(store().cardView).toMatchObject({ agentId, editing: true });
});
