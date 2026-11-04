-- Schema 16 keeps the labels of each Actor and adds the `label.*` record kinds.
CREATE TABLE actor_labels (
  actor_id TEXT NOT NULL REFERENCES actors(id),
  key TEXT NOT NULL,
  until TEXT,
  reason TEXT,
  set_by TEXT NOT NULL CHECK (set_by IN ('admin', 'actor')),
  set_at TEXT NOT NULL,
  PRIMARY KEY (actor_id, key)
);
CREATE INDEX actor_labels_until ON actor_labels(until) WHERE until IS NOT NULL;
CREATE TABLE records_16 (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  at TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('session.started', 'session.ended', 'spawned',
    'message.accepted', 'message.sent', 'message.read', 'stop.requested', 'kill.requested',
    'observation', 'schedule.skipped', 'schedule.missed', 'label.set', 'label.cleared',
    'label.expired')),
  actor_id TEXT NOT NULL REFERENCES actors(id),
  session_id TEXT REFERENCES sessions(id),
  data TEXT NOT NULL
);
INSERT INTO records_16 (sequence, at, kind, actor_id, session_id, data)
  SELECT sequence, at, kind, actor_id, session_id, data FROM records;
DROP TABLE records;
ALTER TABLE records_16 RENAME TO records;
CREATE INDEX records_session ON records(session_id, sequence);
CREATE INDEX records_actor_kind ON records(actor_id, kind);
CREATE UNIQUE INDEX records_spawned_session ON records(session_id) WHERE kind = 'spawned';
