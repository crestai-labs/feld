use sha2::{Digest, Sha256};

/// The token a Runtime's telemetry carries in place of its Session token: derived one way
/// from it, so that whoever reads the telemetry's header learns nothing the daemon API
/// accepts, and the daemon attributes the telemetry to the Session all the same.
pub fn telemetry_token(session_token: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"feld-telemetry:");
    hash.update(session_token.as_bytes());
    format!("{:x}", hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_telemetry_token_is_fixed_by_its_session_token_and_differs_from_it() {
        let token = telemetry_token("feld_session_secret");
        assert_eq!(token, telemetry_token("feld_session_secret"));
        assert_ne!(token, telemetry_token("feld_session_other"));
        assert_ne!(token, "feld_session_secret");
        assert_eq!(token.len(), 64);
    }
}
