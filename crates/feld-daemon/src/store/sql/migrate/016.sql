-- Schema 16 keeps the Actors of other Workspaces that a Workspace accepts Messages from.
CREATE TABLE workspace_accept_from (
  workspace_id TEXT NOT NULL REFERENCES workspaces(id),
  from_workspace TEXT NOT NULL,
  actor TEXT NOT NULL,
  PRIMARY KEY (workspace_id, from_workspace, actor)
);
