-- Schema 11 keeps that a Runtime holds a Message in a queue of its own.
ALTER TABLE messages ADD COLUMN queued_session_id TEXT;
ALTER TABLE messages ADD COLUMN queued_at TEXT;
