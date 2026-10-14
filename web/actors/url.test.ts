import { expect, test } from "vitest";
import { eventWorkspaceOf, nextSearch, workspaceInSearch } from "./url.ts";

const query = (value: string) => new URLSearchParams(value);

test("a query without a Workspace shows the default one", () => {
  expect(workspaceInSearch(query(""))).toBe("default");
  expect(workspaceInSearch(query("workspace=*"))).toBe(null);
  expect(workspaceInSearch(query("workspace=team-a"))).toBe("team-a");
});

// The Actors page follows the Workspace it shows; the Settings page follows all of them.
test("the event stream follows the page's Workspace", () => {
  expect(eventWorkspaceOf("/ui/", query(""))).toBe("default");
  expect(eventWorkspaceOf("/ui/", query("workspace=team-a"))).toBe("team-a");
  expect(eventWorkspaceOf("/ui/", query("workspace=*"))).toBe(null);
  expect(eventWorkspaceOf("/ui/settings", query(""))).toBe(null);
  expect(eventWorkspaceOf("/ui/settings", query("workspace=team-a"))).toBe(null);
});

// Every Workspace selection becomes a history entry, all Workspaces included, and the rest
// of the query is left alone.
test("a Workspace change pushes a history entry", () => {
  expect(nextSearch(query("scenario=x&workspace=default"), "research", null)).toEqual({ search: query("scenario=x&workspace=research"), replace: false });
  expect(nextSearch(query("workspace=default"), null, null)?.search.toString()).toBe("workspace=*");
  expect(nextSearch(query("workspace=default"), null, null)?.replace).toBe(false);
});

// Opening, switching, and closing Consoles rewrites the entry in place.
test("the inspected Session replaces the history entry", () => {
  expect(nextSearch(query("workspace=default"), "default", "session_1")).toEqual({ search: query("workspace=default&session=session_1"), replace: true });
  expect(nextSearch(query("workspace=default&session=session_1"), "default", "session_2")?.replace).toBe(true);
  expect(nextSearch(query("workspace=default&session=session_1"), "default", null)).toEqual({ search: query("workspace=default"), replace: true });
});

// What the query already shows writes nothing, so selecting the shown Workspace adds no
// entry.
test("an unchanged page writes no query", () => {
  expect(nextSearch(query("workspace=*"), null, null)).toBe(null);
  expect(nextSearch(query(""), "default", null)).toBe(null);
  expect(nextSearch(query("workspace=default&session=s"), "default", "s")).toBe(null);
});
