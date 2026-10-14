use std::sync::{Arc, RwLockReadGuard};

use anyhow::Result;

use crate::console::Secrets;
use crate::store::{AuthenticatedSession, Store};

/// The Session tokens Feld issued, as the API judges a caller by them.
pub struct TokenService {
    store: Arc<Store>,
}

impl TokenService {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    /// The open Session a token names.
    pub fn authenticate(&self, token: &str) -> Result<Option<AuthenticatedSession>> {
        self.store.authenticate_session(token)
    }

    /// The closed Session a token named, when it named one.
    pub fn closed_session(&self, token: &str) -> Result<Option<String>> {
        self.store.closed_session_for_token(token)
    }

    /// Whether Feld ever issued this token to a Session.
    pub fn issued(&self, token: &str) -> Result<bool> {
        self.store.session_token_issued(token)
    }

    /// Every token value Feld remembers, which no response may carry.
    pub fn secrets(&self) -> RwLockReadGuard<'_, Secrets> {
        self.store.secrets()
    }
}
