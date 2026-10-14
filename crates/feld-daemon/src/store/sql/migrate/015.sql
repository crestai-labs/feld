-- Schema 15 keeps the bytes of Telemetry each Session counts toward its limit: the body and
-- 1024 bytes of row overhead for each Telemetry.
ALTER TABLE sessions ADD COLUMN telemetry_bytes INTEGER NOT NULL DEFAULT 0;
UPDATE sessions SET telemetry_bytes =
  (SELECT COALESCE(SUM(length(body) + 1024), 0) FROM telemetries WHERE session_id = sessions.id);
