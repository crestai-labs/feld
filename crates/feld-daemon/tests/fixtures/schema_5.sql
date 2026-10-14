-- The DDL of a store of schema version 5, the oldest version Feld migrates.
CREATE TABLE workspaces (
  id TEXT PRIMARY KEY NOT NULL,
  key TEXT NOT NULL,
  created_at TEXT NOT NULL,
  deleted_at TEXT
);
CREATE UNIQUE INDEX workspace_live_key ON workspaces(key) WHERE deleted_at IS NULL;
CREATE TABLE actors (
  id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id),
  workspace_key TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('agent', 'job')),
  launch_spec TEXT,
  schedule_cron TEXT,
  schedule_timezone TEXT,
  schedule_enabled INTEGER CHECK (schedule_enabled IN (0, 1)),
  schedule_created_at TEXT,
  schedule_paused_at TEXT,
  schedule_enabled_at TEXT,
  schedule_revision INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);
CREATE UNIQUE INDEX actor_live_name ON actors(workspace_id, name) WHERE deleted_at IS NULL;
CREATE TABLE sessions (
  id TEXT PRIMARY KEY NOT NULL,
  actor_id TEXT NOT NULL REFERENCES actors(id),
  generation INTEGER NOT NULL,
  start TEXT NOT NULL,
  end TEXT,
  start_kind TEXT NOT NULL CHECK (start_kind IN ('start', 'replacement', 'attach', 'schedule')),
  ownership TEXT NOT NULL CHECK (ownership IN ('owned', 'attached')),
  end_kind TEXT NOT NULL CHECK (end_kind IN ('running', 'stopped', 'exited', 'unknown')),
  exit_code INTEGER,
  planned TEXT,
  execution TEXT NOT NULL,
  placement TEXT NOT NULL,
  credential TEXT NOT NULL UNIQUE,
  start_settled TEXT,
  start_failed TEXT,
  stop_record TEXT,
  kill_record TEXT,
  observation_kind TEXT,
  observation_at TEXT,
  observation_detail TEXT,
  observation_exit_code INTEGER,
  runtime_identity TEXT,
  telemetry_collector_available INTEGER NOT NULL DEFAULT 1
    CHECK (telemetry_collector_available IN (0, 1)),
  telemetry_runtime_prepared INTEGER NOT NULL DEFAULT 1
    CHECK (telemetry_runtime_prepared IN (0, 1)),
  UNIQUE (actor_id, generation)
);
CREATE UNIQUE INDEX actor_current_session ON sessions(actor_id) WHERE end IS NULL;
CREATE UNIQUE INDEX scheduled_session_tick ON sessions(actor_id, planned)
  WHERE planned IS NOT NULL;
CREATE TABLE restarts (
  session_id TEXT PRIMARY KEY NOT NULL REFERENCES sessions(id),
  spawner_actor_id TEXT REFERENCES actors(id),
  spawner_session_id TEXT,
  at TEXT NOT NULL
);
CREATE TABLE records (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  at TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('session.started', 'session.ended', 'spawned',
    'message.accepted', 'message.sent', 'message.read', 'stop.requested', 'kill.requested',
    'observation', 'schedule.skipped')),
  actor_id TEXT NOT NULL REFERENCES actors(id),
  session_id TEXT REFERENCES sessions(id),
  data TEXT NOT NULL
);
CREATE INDEX records_session ON records(session_id, sequence);
CREATE INDEX records_actor_kind ON records(actor_id, kind);
CREATE UNIQUE INDEX records_spawned_session ON records(session_id) WHERE kind = 'spawned';
CREATE TABLE messages (
  id TEXT PRIMARY KEY NOT NULL,
  fifo INTEGER NOT NULL,
  from_actor_id TEXT REFERENCES actors(id),
  from_session_id TEXT,
  to_actor_id TEXT NOT NULL REFERENCES actors(id),
  to_session_id TEXT,
  accepted TEXT NOT NULL,
  body TEXT NOT NULL,
  anchor TEXT NOT NULL,
  read_session_id TEXT,
  read_at TEXT,
  read_id TEXT UNIQUE,
  deleted_at TEXT,
  UNIQUE (to_actor_id, fifo)
);
CREATE INDEX messages_accepted ON messages(accepted);
CREATE TABLE session_credentials (
  session_id TEXT NOT NULL REFERENCES sessions(id),
  credential TEXT NOT NULL UNIQUE
);
CREATE TABLE credential_secrets (
  kind TEXT NOT NULL,
  secret TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL
);
CREATE TABLE telemetry_batches (
  id TEXT PRIMARY KEY NOT NULL,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  signal TEXT NOT NULL CHECK (signal IN ('metrics', 'logs')),
  content_type TEXT NOT NULL,
  body BLOB NOT NULL,
  digest TEXT NOT NULL,
  received_at TEXT NOT NULL
);
CREATE INDEX telemetry_batches_session ON telemetry_batches(session_id, received_at, id);
