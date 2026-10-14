import { expect, test, vi } from "vitest";
import { markPointerFocusedLane, plotClickTarget } from "./interaction.ts";

// Elements by name, enough for the identity the click reads.
const element = (name: string) => ({ name }) as unknown as Element;

test("a click that lands on the plot after a press that captured the pointer reads the element pressed", () => {
  const svg = element("svg");
  const lane = element("lane");
  expect(plotClickTarget(svg, svg, lane)).toBe(lane);
  // A click on a mark of its own, or with no press recorded, reads its own target.
  const bundle = element("bundle");
  expect(plotClickTarget(bundle, svg, lane)).toBe(bundle);
  expect(plotClickTarget(svg, svg, null)).toBe(svg);
});

test("a press on a lane, or on a mark over it, marks the lane as pointer-focused; one elsewhere marks nothing", () => {
  const lane = { setAttribute: vi.fn() };
  const onLane = { closest: (selector: string) => (selector.startsWith(".feld-agents__lane-hit") ? lane : null) } as unknown as Element;
  expect(markPointerFocusedLane(onLane)).toBe(lane);
  expect(lane.setAttribute).toHaveBeenCalledWith("data-pointer-focus", "");
  expect(markPointerFocusedLane({ closest: () => null } as unknown as Element)).toBeNull();
  expect(markPointerFocusedLane(null)).toBeNull();
});
