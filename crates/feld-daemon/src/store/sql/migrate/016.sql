-- Schema 16 records when an Actor is archived; an archived Actor leaves the name uniqueness.
ALTER TABLE actors ADD COLUMN archived_at TEXT;
DROP INDEX actor_live_name;
CREATE UNIQUE INDEX actor_live_name ON actors(workspace_id, name)
  WHERE archived_at IS NULL AND deleted_at IS NULL;
