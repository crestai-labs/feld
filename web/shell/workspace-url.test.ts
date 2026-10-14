import { expect, test } from "vitest";
import { pageQuery, searchWithWorkspace, workspaceOfSearch } from "./workspace-url.ts";

const workspaceOf = (query: string) => workspaceOfSearch(new URLSearchParams(query));

// A deep link opens on the Workspace it names; a star or an empty value opens on all of
// them, and no query at all leaves the page on its default Workspace.
test("the query names the Workspace shown", () => {
  expect(workspaceOf("?workspace=team-a")).toBe("team-a");
  expect(workspaceOf("?workspace=*")).toBe(null);
  expect(workspaceOf("?workspace=")).toBe(null);
  expect(workspaceOf("")).toBe(undefined);
});

// A selection writes the query that shows it, and the rest of the query is left alone.
test("a selection becomes the query", () => {
  const search = new URLSearchParams("?scenario=three-days-ago-1d&workspace=default");
  expect(searchWithWorkspace(search, "research").toString()).toBe("scenario=three-days-ago-1d&workspace=research");
  expect(searchWithWorkspace(search, null).toString()).toBe("scenario=three-days-ago-1d&workspace=*");
  expect(workspaceOfSearch(searchWithWorkspace(search, null))).toBe(null);
});

// A link to another page keeps the Workspace and the fixture, and leaves the rest of the
// query behind.
test("a page link carries the Workspace and the fixture", () => {
  expect(pageQuery(new URLSearchParams("?workspace=team-a&session=s1"))).toBe("?workspace=team-a");
  expect(pageQuery(new URLSearchParams("?workspace=*"))).toBe("?workspace=*");
  expect(pageQuery(new URLSearchParams("?scenario=three-days-ago-1d&session=s1&state=error&workspace=*"))).toBe("?workspace=*&scenario=three-days-ago-1d&state=error");
  expect(pageQuery(new URLSearchParams("?session=s1"))).toBe("");
});
