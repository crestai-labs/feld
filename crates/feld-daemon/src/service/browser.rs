use std::sync::{Arc, Mutex};
use std::time::Duration as Pause;

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as Queue, watch};
use tokio::time::{Instant, sleep_until};

use crate::store::{NewBrowser, Store};
use feld_core::id::{fresh_id, fresh_token};
use feld_core::model::{Browser, PairingCode};
use feld_core::time;

/// The 31 letters of a pairing code: digits and capitals without `0`, `1`, `I`, `L`, `O`.
const CODE_ALPHABET: &[u8; 31] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
const CODE_LENGTH: usize = 8;
const CODE_LIFETIME: Duration = Duration::minutes(5);
/// The wait after a failed check before the daemon checks the next code, which bounds guesses
/// to one a second however many requests arrive at once.
const FAILURE_PAUSE: Pause = Pause::from_secs(1);

/// The browser tokens of `auth.md` and the pairing codes a browser exchanges for one.
pub struct BrowserService {
    store: Arc<Store>,
    cookie: String,
    pairing: Mutex<Pairing>,
    /// Checks of codes, one at a time in arrival order: the earliest a check may run.
    checks: Queue<Option<Instant>>,
    revocations: watch::Sender<()>,
}

/// The pending pairing codes, held as hashes in memory only.
#[derive(Default)]
struct Pairing {
    codes: Vec<PendingCode>,
}

struct PendingCode {
    hash: String,
    name: String,
    expires: DateTime<Utc>,
}

/// A valid browser token a request carried.
pub struct BrowserUse {
    pub id: String,
    last_used_at: String,
}

/// A browser token a pairing issued: its record, the value the cookie carries, and the key
/// the page sends beside it.
pub struct Paired {
    pub browser: Browser,
    pub token: String,
    pub key: String,
}

impl BrowserService {
    /// The service of the daemon listening on `port`, whose cookie the port names.
    pub fn new(store: Arc<Store>, port: u16) -> Self {
        Self {
            store,
            cookie: format!("feld_browser_{port}"),
            pairing: Mutex::default(),
            checks: Queue::new(None),
            revocations: watch::Sender::new(()),
        }
    }

    /// The name of the cookie that carries the browser token.
    pub fn cookie(&self) -> &str {
        &self.cookie
    }

    /// Issues a pairing code, which names the browser token it is exchanged for.
    pub fn issue_code(&self, name: &str) -> PairingCode {
        let now = Utc::now();
        // 128 random bits read as base 31: the bias is far below one in 2^80.
        let random = Sha256::digest(fresh_token().as_bytes());
        let mut value = u128::from_le_bytes(random[..16].try_into().expect("16 bytes"));
        let letters: String = (0..CODE_LENGTH)
            .map(|_| {
                let letter = CODE_ALPHABET[(value % 31) as usize];
                value /= 31;
                char::from(letter)
            })
            .collect();
        let expires = now + CODE_LIFETIME;
        let mut pairing = self.pairing.lock().expect("pairing mutex poisoned");
        pairing.codes.retain(|code| code.expires > now);
        pairing.codes.push(PendingCode {
            hash: hash(&letters),
            name: name.to_owned(),
            expires,
        });
        PairingCode {
            code: format!("{}-{}", &letters[..4], &letters[4..]),
            name: name.to_owned(),
            expires_at: time::format(expires),
        }
    }

    /// Exchanges a pairing code for a new browser token, revoking `replacing`, the credential
    /// of the token the browser still holds. `None` when the code is unknown, expired, or used.
    pub async fn pair(&self, code: &str, replacing: Option<&str>) -> Result<Option<Paired>> {
        let mut next_check = self.checks.lock().await;
        if let Some(at) = *next_check {
            sleep_until(at).await;
        }
        let now = Utc::now();
        let normalized: String = code
            .chars()
            .filter(|letter| *letter != '-')
            .map(|letter| letter.to_ascii_uppercase())
            .collect();
        let supplied = hash(&normalized);
        let name = {
            let mut pairing = self.pairing.lock().expect("pairing mutex poisoned");
            pairing.codes.retain(|code| code.expires > now);
            let found = pairing.codes.iter().position(|code| code.hash == supplied);
            *next_check = found.is_none().then(|| Instant::now() + FAILURE_PAUSE);
            match found {
                Some(index) => pairing.codes.swap_remove(index).name,
                None => return Ok(None),
            }
        };
        drop(next_check);
        let token = fresh_token();
        let key = fresh_token();
        let id = fresh_id("browser");
        let replacing = replacing.map(hash);
        let browser = self.store.pair_browser(
            NewBrowser {
                id: &id,
                token_hash: &hash(&credential(&token, &key)),
                name: &name,
            },
            replacing.as_deref(),
            now,
        )?;
        if replacing.is_some() {
            self.revocations.send_replace(());
        }
        Ok(Some(Paired {
            browser,
            token,
            key,
        }))
    }

    /// The valid browser token of `credential`, whose use [`BrowserService::record_use`]
    /// records.
    pub fn authenticate(&self, credential: &str) -> Result<Option<BrowserUse>> {
        Ok(self
            .store
            .authenticate_browser(&hash(credential), Utc::now())?
            .map(|(id, last_used_at)| BrowserUse { id, last_used_at }))
    }

    pub fn record_use(&self, browser: &BrowserUse) -> Result<()> {
        self.store
            .record_browser_use(&browser.id, &browser.last_used_at, Utc::now())
    }

    pub fn list(&self) -> Result<Vec<Browser>> {
        self.store.list_browsers(Utc::now())
    }

    /// Revokes the valid browser token `id`, or answers `None` when there is none.
    pub fn revoke(&self, id: &str) -> Result<Option<Browser>> {
        let revoked = self.store.revoke_browser(id, Utc::now())?;
        if revoked.is_some() {
            self.revocations.send_replace(());
        }
        Ok(revoked)
    }

    /// Whether the browser token `id` is still valid.
    pub fn valid(&self, id: &str) -> Result<bool> {
        self.store.browser_valid(id, Utc::now())
    }

    /// Changes whenever a browser token is revoked.
    pub fn revocations(&self) -> watch::Receiver<()> {
        self.revocations.subscribe()
    }
}

/// What identifies a browser token: the value of its cookie and its key together, so that
/// the cookie alone, which reaches every port of the host, names no token.
pub fn credential(token: &str, key: &str) -> String {
    format!("{token}.{key}")
}

fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
