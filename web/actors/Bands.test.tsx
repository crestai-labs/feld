// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { beforeEach, expect, test, vi } from "vitest";
import { createQueryClient } from "../api/queries.ts";
import { ActorsPage } from "./ActorsPage.tsx";
import { targetConfirmed, selectActorFrom } from "./Bands.tsx";
import { useActorsStore } from "./store.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Each drawing of the bands pane derives its Actors once, through the model's bandAgents.
const bands = vi.hoisted(() => ({ draws: 0 }));
vi.mock("./model.ts", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./model.ts")>();
  return {
    ...actual,
    createModel: (...args: Parameters<typeof actual.createModel>) => {
      const model = actual.createModel(...args);
      return { ...model, bandAgents: (state: Parameters<typeof model.bandAgents>[0]) => { bands.draws += 1; return model.bandAgents(state); } };
    },
  };
});

const store = () => useActorsStore.getState();

beforeEach(() => {
  store().reset(undefined, Date.parse("2026-09-25T00:30:00Z"));
});

// An Actor row inside the bands pane, with a lane per Actor in the Timeline beside it.
function row(clicked: { closest: (selector: string) => Element | null }) {
  const lanes = ["default/dev", "default/rev"].map((id) => {
    const lane = { getAttribute: () => id, setAttribute: vi.fn(), focus: vi.fn(), closest: () => lane };
    return lane;
  });
  const pane = { querySelectorAll: () => lanes };
  return { lanes, event: { target: clicked as unknown as Element, currentTarget: { closest: () => pane } as unknown as Element } };
}

const cell = { closest: () => null };

test("a click on an Actor's row selects it and focuses its lane, so the next Escape clears it", () => {
  const { lanes, event } = row(cell);
  selectActorFrom(event, "default/rev");
  expect(store().focusedActorId).toBe("default/rev");
  expect(lanes[1].focus).toHaveBeenCalledWith({ preventScroll: true });
  expect(lanes[0].focus).not.toHaveBeenCalled();
  // The pointer focused it, so the lane draws no focus outline.
  expect(lanes[1].setAttribute).toHaveBeenCalledWith("data-pointer-focus", "");
  // The lane is inside the page, whose Escape closes what is open and then clears the selection.
  expect(store().escapeAll()).toBeNull();
  expect(store().focusedActorId).toBeNull();
  selectActorFrom(event, "default/rev");
  selectActorFrom(event, "default/rev");
  expect(store().focusedActorId).toBeNull();
});

test("a click on a control of the row leaves the selection as it is", () => {
  const { lanes, event } = row({ closest: () => ({}) as Element });
  selectActorFrom(event, "default/rev");
  expect(store().focusedActorId).toBeNull();
  expect(lanes[1].focus).not.toHaveBeenCalled();
});

test("Kill stands confirmed only for the Session it was confirmed for", () => {
  expect(targetConfirmed(null, "session_A")).toBe(false);
  expect(targetConfirmed("session_A", "session_A")).toBe(true);
  // A live update made session_B the current one while the menu stayed open.
  expect(targetConfirmed("session_A", "session_B")).toBe(false);
  expect(targetConfirmed("session_A", null)).toBe(false);
});

test("typing in the composer or resizing the inspector leaves the bands pane as drawn", async () => {
  const page = document.createElement("div");
  document.body.append(page);
  const root = createRoot(page);
  await act(async () => root.render(
    <QueryClientProvider client={createQueryClient()}>
      <MemoryRouter initialEntries={["/ui/?scenario=message-density-30m"]}><ActorsPage /></MemoryRouter>
    </QueryClientProvider>,
  ));
  await vi.waitFor(() => expect(page.querySelector("[data-session]")).not.toBeNull(), { timeout: 10000 });
  const agentId = page.querySelector(".feld-agents__actor-name")!.getAttribute("data-agent-id")!;
  await act(async () => store().openComposer(agentId));
  await vi.waitFor(() => expect(page.querySelector("[data-composer] textarea")).not.toBeNull());

  bands.draws = 0;
  for (const text of ["h", "he", "hel", "hell", "hello"]) await act(async () => store().setComposeDraft(text));
  for (const width of [640, 660, 680]) await act(async () => store().setInspectorWidth(width));
  expect(bands.draws).toBe(0);

  // A key the bands pane reads still draws it again.
  await act(async () => store().setQuery("dev"));
  expect(bands.draws).toBeGreaterThan(0);
  await act(async () => root.unmount());
});
