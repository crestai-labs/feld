// @vitest-environment happy-dom
import { expect, test } from "vitest";
import { laneKeyTarget } from "./laneKeys.ts";

// Two lanes; the hits of a lane are out of time order in the DOM, as the Sessions, the
// skipped ticks, and the bundles are drawn in layers of their own.
function timeline() {
  document.body.innerHTML = `<svg data-timeline="">
    <rect class="feld-agents__lane-hit" data-focus-actor="a" tabindex="0"/>
    <rect class="feld-agents__lane-hit" data-focus-actor="b" tabindex="0"/>
    <line class="feld-agents__session-hit" id="a2" data-lane="a" data-at="200" tabindex="-1"/>
    <line class="feld-agents__session-hit" id="b1" data-lane="b" data-at="90" tabindex="-1"/>
    <line class="feld-agents__session-hit" id="b2" data-lane="b" data-at="400" tabindex="-1"/>
    <circle class="feld-agents__skip-hit" id="a1" data-lane="a" data-at="40" tabindex="-1"/>
    <line class="feld-agents__relation-hit" id="a3" data-lane="a" data-at="300" tabindex="-1"/>
  </svg>`;
  const svg = document.querySelector("svg")!;
  const at = (selector: string) => svg.querySelector(selector)!;
  return { svg, at, laneA: at('[data-focus-actor="a"]'), laneB: at('[data-focus-actor="b"]') };
}

test("Left and Right move through a lane in time order, from its lane hit", () => {
  const { svg, at, laneA } = timeline();
  expect(laneKeyTarget(svg, laneA, "ArrowRight", false)).toBe(at("#a1"));
  expect(laneKeyTarget(svg, at("#a1"), "ArrowRight", false)).toBe(at("#a2"));
  expect(laneKeyTarget(svg, at("#a2"), "ArrowRight", false)).toBe(at("#a3"));
  // The ends of a lane keep the focus.
  expect(laneKeyTarget(svg, at("#a3"), "ArrowRight", false)).toBe(at("#a3"));
  expect(laneKeyTarget(svg, at("#a1"), "ArrowLeft", false)).toBe(laneA);
  expect(laneKeyTarget(svg, laneA, "ArrowLeft", false)).toBe(laneA);
});

test("Up and Down move to the hit nearest in time on the neighbouring lane", () => {
  const { svg, at, laneA, laneB } = timeline();
  expect(laneKeyTarget(svg, at("#a1"), "ArrowDown", false)).toBe(at("#b1"));
  expect(laneKeyTarget(svg, at("#a3"), "ArrowDown", false)).toBe(at("#b2"));
  expect(laneKeyTarget(svg, at("#b1"), "ArrowUp", false)).toBe(at("#a1"));
  expect(laneKeyTarget(svg, laneA, "ArrowDown", false)).toBe(laneB);
  expect(laneKeyTarget(svg, at("#b2"), "ArrowDown", false)).toBe(at("#b2"));
});

test("each lane is one Tab stop", () => {
  const { svg, at, laneA, laneB } = timeline();
  expect([...svg.querySelectorAll('[tabindex="0"]')]).toEqual([laneA, laneB]);
  // From a lane hit, Tab follows the page.
  expect(laneKeyTarget(svg, laneA, "Tab", false)).toBeNull();
  expect(laneKeyTarget(svg, at("#a2"), "Tab", false)).toBe(laneB);
  expect(laneKeyTarget(svg, at("#a2"), "Tab", true)).toBe(laneA);
  // From the last lane, Tab leaves the timeline.
  expect(laneKeyTarget(svg, at("#b1"), "Tab", false)).toBeNull();
  expect(laneKeyTarget(svg, at("#b1"), "Tab", true)).toBe(laneB);
});

test("Enter and the other keys stay with the page", () => {
  const { svg, at } = timeline();
  expect(laneKeyTarget(svg, at("#a1"), "Enter", false)).toBeNull();
  expect(laneKeyTarget(svg, at("#a1"), "Escape", false)).toBeNull();
});
