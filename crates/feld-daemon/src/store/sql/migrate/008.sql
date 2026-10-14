-- Schema 8 adds the `schedule.missed` record kind.
CREATE TABLE records_8 (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  at TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('session.started', 'session.ended', 'spawned',
    'message.accepted', 'message.sent', 'message.read', 'stop.requested', 'kill.requested',
    'observation', 'schedule.skipped', 'schedule.missed')),
  actor_id TEXT NOT NULL REFERENCES actors(id),
  session_id TEXT REFERENCES sessions(id),
  data TEXT NOT NULL
);
INSERT INTO records_8 (sequence, at, kind, actor_id, session_id, data)
  SELECT sequence, at, kind, actor_id, session_id, data FROM records;
DROP TABLE records;
ALTER TABLE records_8 RENAME TO records;
CREATE INDEX records_session ON records(session_id, sequence);
CREATE INDEX records_actor_kind ON records(actor_id, kind);
CREATE UNIQUE INDEX records_spawned_session ON records(session_id) WHERE kind = 'spawned';
