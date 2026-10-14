// The keyboard over the timeline's hits. Each lane is one Tab stop, its lane hit. The
// Sessions, skipped ticks, and bundles of a lane carry `data-lane` and `data-at` (their x)
// and are reached with the arrow keys: Left and Right in time order from the lane hit,
// Up and Down to the hit nearest in time on the neighbouring lane. Tab from a hit moves
// to the next lane, Shift+Tab back to its own lane hit.

const LANE_HIT = ".feld-agents__lane-hit[data-focus-actor]";

function laneOf(hit: Element) {
  return hit.getAttribute("data-focus-actor") ?? hit.getAttribute("data-lane");
}

function isLaneHit(hit: Element) {
  return hit.matches(LANE_HIT);
}

function atOf(hit: Element) {
  return isLaneHit(hit) ? -Infinity : Number(hit.getAttribute("data-at"));
}

// The lane's hits in time order, its lane hit first.
function laneHits(svg: Element, laneHit: Element) {
  const lane = CSS.escape(laneHit.getAttribute("data-focus-actor")!);
  const hits = [...svg.querySelectorAll(`[data-lane="${lane}"]`)].sort((a, b) => atOf(a) - atOf(b));
  return [laneHit, ...hits];
}

// The hit `key` moves the focus to from `from`; null leaves the key to the browser.
export function laneKeyTarget(svg: Element, from: Element, key: string, shiftKey: boolean): Element | null {
  const lane = laneOf(from);
  if (lane === null) return null;
  const lanes = [...svg.querySelectorAll(LANE_HIT)];
  const index = lanes.findIndex((hit) => laneOf(hit) === lane);
  if (index < 0) return null;
  if (key === "Tab") {
    if (isLaneHit(from)) return null;
    return shiftKey ? lanes[index] : lanes[index + 1] ?? null;
  }
  if (key === "ArrowLeft" || key === "ArrowRight") {
    const hits = laneHits(svg, lanes[index]);
    return hits[hits.indexOf(from) + (key === "ArrowRight" ? 1 : -1)] ?? from;
  }
  if (key === "ArrowUp" || key === "ArrowDown") {
    const next = lanes[index + (key === "ArrowDown" ? 1 : -1)];
    if (!next) return from;
    if (isLaneHit(from)) return next;
    const at = atOf(from);
    return laneHits(svg, next).slice(1).reduce<Element>((nearest, hit) => (
      isLaneHit(nearest) || Math.abs(atOf(hit) - at) < Math.abs(atOf(nearest) - at) ? hit : nearest
    ), next);
  }
  return null;
}
