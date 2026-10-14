-- Schema 10 keeps the instant the latest observation kind was recorded, which the `observation`
-- records hold for the Sessions before it.
ALTER TABLE sessions ADD COLUMN observation_since TEXT;
UPDATE sessions SET observation_since = (
  SELECT at FROM records
  WHERE records.session_id = sessions.id AND records.kind = 'observation'
  ORDER BY sequence DESC LIMIT 1
) WHERE observation_kind IS NOT NULL;
