-- Schema 14 keeps the senders an Actor accepts Messages from.
ALTER TABLE actors ADD COLUMN accept_from TEXT;
