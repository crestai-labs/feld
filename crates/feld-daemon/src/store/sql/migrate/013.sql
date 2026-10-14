-- Schema 13 keeps the grants of an Actor.
ALTER TABLE actors ADD COLUMN grants TEXT NOT NULL DEFAULT '[]';
