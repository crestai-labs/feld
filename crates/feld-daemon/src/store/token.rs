use std::collections::BTreeSet;
use std::sync::RwLockReadGuard;

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use feld_core::model::{Grant, Requester};
use feld_core::time::now;

use crate::console::Secrets;

use super::Store;

#[derive(Clone, Debug)]
pub struct AuthenticatedSession {
    pub requester: Requester,
    pub session_id: String,
    /// The grants of the Session's Actor.
    pub grants: BTreeSet<Grant>,
}

impl Store {
    pub fn authenticate_session(&self, token: &str) -> Result<Option<AuthenticatedSession>> {
        self.read(|connection| {
            let found = connection
                .query_row(
                    "SELECT s.id, s.actor_id, a.workspace_key, a.name, a.grants
                     FROM sessions s JOIN actors a ON a.id = s.actor_id
                     WHERE s.token = ?1 AND s.end IS NULL",
                    [token],
                    |row| {
                        let session_id = row.get(0)?;
                        let requester = Requester::Actor {
                            id: row.get(1)?,
                            workspace: row.get(2)?,
                            name: row.get(3)?,
                        };
                        Ok((session_id, requester, row.get::<_, String>(4)?))
                    },
                )
                .optional()
                .context("authenticate Session token")?;
            found
                .map(|(session_id, requester, grants)| {
                    Ok(AuthenticatedSession {
                        requester,
                        session_id,
                        grants: serde_json::from_str(&grants).context("decode grants")?,
                    })
                })
                .transpose()
        })
    }

    /// Whether the token was issued to a Session, open or closed.
    pub fn session_token_issued(&self, token: &str) -> Result<bool> {
        self.read(|connection| {
            connection
                .query_row("SELECT 1 FROM sessions WHERE token = ?1", [token], |_| {
                    Ok(())
                })
                .optional()
                .map(|found| found.is_some())
                .context("find the Session of a token")
        })
    }

    /// The closed Session a token was last issued to, which the Runtime API refuses as
    /// session closed.
    pub fn closed_session_for_token(&self, token: &str) -> Result<Option<String>> {
        self.read(|connection| {
            connection
                .query_row(
                    "SELECT id FROM sessions WHERE token = ?1 AND end IS NOT NULL",
                    [token],
                    |row| row.get(0),
                )
                .optional()
                .context("find the closed Session of a token")
        })
    }

    pub fn token_secrets(&self) -> Result<Vec<String>> {
        self.read(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT token FROM sessions
                     UNION SELECT token FROM session_tokens
                     UNION SELECT secret FROM token_secrets",
                )
                .context("prepare token redaction listing")?;
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .context("query token redaction values")?
                .collect::<rusqlite::Result<Vec<_>>>()
                .context("read token redaction values")
        })
    }

    /// Every token value the store holds, as kept in memory since the store opened.
    pub fn secrets(&self) -> RwLockReadGuard<'_, Secrets> {
        self.secrets.read().expect("secrets lock poisoned")
    }

    /// Keeps a token the store now holds for the redaction of responses.
    pub(super) fn keep_secret(&self, secret: &str) {
        self.secrets
            .write()
            .expect("secrets lock poisoned")
            .insert(secret);
    }

    /// Retains a token for redaction after the token is rotated or retired.
    ///
    /// The free-form kind leaves a storage boundary for future internal tokens,
    /// including Collector authentication, without exposing those tokens in models.
    pub fn remember_token_secret(&self, kind: &str, secret: &str) -> Result<()> {
        if secret.is_empty() {
            return Ok(());
        }
        self.write("token retention", |transaction, _| {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO token_secrets (kind, secret, created_at)
                     VALUES (?1, ?2, ?3)",
                    params![kind, secret, now()],
                )
                .context("retain token for redaction")?;
            Ok::<_, anyhow::Error>(())
        })?;
        self.keep_secret(secret);
        Ok(())
    }
}
