// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router";
import { expect, test, vi } from "vitest";
import { createQueryClient } from "../api/queries.ts";
import { ActorsPage } from "../actors/ActorsPage.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// An attached, a spawned (owned), and the orchestrator's Session of the density scenario.
const DENSITY_SESSIONS = ["session_density_orchestrator", "session_density_worker-01", "session_density_worker-06"];

function click(element: Element | null) {
  if (!element) throw new Error("nothing to click");
  return act(async () => element.dispatchEvent(new MouseEvent("click", { bubbles: true })));
}

// The density scenario in fixture mode: each Session opens its Console with Feld's own
// records, and Details shows its Session facts.
test("fixture mode opens the density Sessions' Consoles and Session facts", async () => {
  const errors: unknown[] = [];
  window.addEventListener("error", (event) => errors.push(event.error));
  const page = document.createElement("div");
  document.body.append(page);
  const root = createRoot(page);
  await act(async () => root.render(
    <QueryClientProvider client={createQueryClient()}>
      <MemoryRouter initialEntries={["/ui/?scenario=message-density-30m"]}><ActorsPage /></MemoryRouter>
    </QueryClientProvider>,
  ));
  await vi.waitFor(() => expect(page.querySelector("[data-session]")).not.toBeNull(), { timeout: 10000 });

  for (const id of DENSITY_SESSIONS) {
    await click(page.querySelector(`[data-session="${id}"]`));
    const terminal = await vi.waitFor(() => {
      const found = page.querySelector(".feld-console__terminal");
      expect(found?.textContent).toContain(id);
      return found!;
    });
    expect(terminal.textContent).not.toContain("no longer in the range");
    expect(terminal.querySelectorAll("[data-feld-record]").length, id).toBeGreaterThan(0);

    await click(page.querySelector(`[data-action="open-actor"][data-session-id="${id}"]`));
    const facts = await vi.waitFor(() => {
      const card = document.querySelector('[role="dialog"].is-actor, .feld-agents__inspector.is-actor');
      expect(card?.textContent).toContain(id);
      const found = [...card!.querySelectorAll(".feld-agents__inspector-facts dt")].map((term) => term.textContent);
      expect(found).toContain("Ownership");
      return found;
    });
    expect(facts).toEqual(expect.arrayContaining(["Execution", "Model", "Started", "Ended", "Start kind"]));
  }
  expect(errors).toEqual([]);
  await act(async () => root.unmount());
});
