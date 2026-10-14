use uuid::Uuid;

/// A new id of the kind `prefix` names, such as `session_<hex>`.
pub fn fresh_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

/// A new bearer token.
pub fn fresh_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// The grammar of Workspace keys, Actor names, and Plugin names: `^[a-z0-9][a-z0-9-]*$`.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' => true,
            b'-' => index > 0,
            _ => false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_the_grammar() {
        for key in ["default", "team-2", "team-", "team--", "0"] {
            assert!(valid_key(key), "{key}");
        }
        for key in ["", "-team", "Team", "team_a"] {
            assert!(!valid_key(key), "{key}");
        }
    }
}
