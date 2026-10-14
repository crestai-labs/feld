// All Workspaces in the `workspace` query; a Workspace key never holds a star.
export const ALL_WORKSPACES = "*";

// The page opens on the Workspace of `?workspace=`, on all of them with `?workspace=*` (or
// an empty value), and on the default one (undefined) with no query at all.
export function workspaceOfSearch(search: URLSearchParams): string | null | undefined {
  const value = search.get("workspace");
  if (value === null) return undefined;
  return value && value !== ALL_WORKSPACES ? value : null;
}

// The query that shows a Workspace, or all of them, with the rest of the query kept.
export function searchWithWorkspace(search: URLSearchParams, workspace: string | null) {
  const next = new URLSearchParams(search);
  next.set("workspace", workspace === null ? ALL_WORKSPACES : workspace);
  return next;
}

// The query a link to another page carries: the Workspace and the fixture mode's
// scenario and state, so that page and the way back show the same Workspace and fixture.
const PAGE_QUERY_KEYS = ["workspace", "scenario", "state"];

export function pageQuery(search: URLSearchParams) {
  const next = new URLSearchParams();
  for (const key of PAGE_QUERY_KEYS) {
    const value = search.get(key);
    if (value !== null) next.set(key, value);
  }
  const query = next.toString();
  return query ? `?${query}` : "";
}
