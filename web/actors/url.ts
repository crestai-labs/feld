import { searchWithWorkspace, workspaceOfSearch } from "../shell/workspace-url.ts";
import { DEFAULT_WORKSPACE_KEY } from "./snapshot.ts";

// The Workspace a query shows: the one it names, all of them (null), or the default one
// when it names none.
export function workspaceInSearch(search: URLSearchParams) {
  const workspace = workspaceOfSearch(search);
  return workspace === undefined ? DEFAULT_WORKSPACE_KEY : workspace;
}

// The Workspace the page's event stream and browser notifications follow: the one the
// Actors page shows, and all of them (null) on the other pages.
export function eventWorkspaceOf(pathname: string, search: URLSearchParams) {
  return pathname === "/ui/" ? workspaceInSearch(search) : null;
}

// The query that shows the page's Workspace and inspected Session, and whether it replaces
// the history entry, or null when the query already shows them. A Workspace change is a
// new history entry, so back and forward walk the Workspaces visited; the inspected
// Session is written in place, so reading Consoles does not fill the history.
export function nextSearch(search: URLSearchParams, workspace: string | null, inspectedSessionId: string | null) {
  const workspaceChanged = workspaceInSearch(search) !== workspace;
  const next = workspaceChanged ? searchWithWorkspace(search, workspace) : new URLSearchParams(search);
  if (inspectedSessionId) next.set("session", inspectedSessionId);
  else next.delete("session");
  if (next.toString() === search.toString()) return null;
  return { search: next, replace: !workspaceChanged };
}
