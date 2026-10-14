use serde::{Deserialize, Serialize};

/// A browser token as the daemon lists it: everything but its value.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Browser {
    pub id: String,
    pub name: String,
    pub paired_at: String,
    pub last_used_at: String,
    /// `last_used_at` plus 7 days or `paired_at` plus 30 days, whichever comes first.
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

/// A pairing code, which a browser exchanges for a browser token.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingCode {
    pub code: String,
    pub name: String,
    pub expires_at: String,
}
