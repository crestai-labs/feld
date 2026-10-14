-- Schema 6 names the Session token by the word of `auth.md`.
ALTER TABLE sessions RENAME COLUMN credential TO token;
ALTER TABLE session_credentials RENAME TO session_tokens;
ALTER TABLE session_tokens RENAME COLUMN credential TO token;
ALTER TABLE credential_secrets RENAME TO token_secrets;
