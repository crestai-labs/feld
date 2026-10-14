-- Schema 7 names the stored telemetry and the ID of its request by the words of `otel.md`.
ALTER TABLE telemetry_batches RENAME TO telemetries;
ALTER TABLE telemetries RENAME COLUMN id TO request_id;
DROP INDEX telemetry_batches_session;
CREATE INDEX telemetries_session ON telemetries(session_id, received_at, request_id);
