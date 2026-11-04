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

/// The grammar of Actor patterns: an Actor name in which `*` stands for any run of characters.
pub fn valid_actor_pattern(pattern: &str) -> bool {
    !pattern.is_empty()
        && pattern.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' | b'*' => true,
            b'-' => index > 0,
            _ => false,
        })
}

/// The grammar of the senders an Actor accepts: an Actor name, or `workspace/actor`.
pub fn valid_sender(sender: &str) -> bool {
    match sender.split_once('/') {
        Some((workspace, name)) => valid_key(workspace) && valid_key(name),
        None => valid_key(sender),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_patterns_are_names_with_globs() {
        for pattern in ["*", "*-leader", "dev-*", "lead"] {
            assert!(valid_actor_pattern(pattern), "{pattern}");
        }
        for pattern in ["", "-x", "a/b", "a?b", "[a]", "Lead"] {
            assert!(!valid_actor_pattern(pattern), "{pattern}");
        }
    }

    #[test]
    fn senders_are_names_or_qualified_names() {
        for sender in ["lead", "team-b/lead"] {
            assert!(valid_sender(sender), "{sender}");
        }
        for sender in ["", "a/b/c", "/lead", "team-b/", "team-b/*"] {
            assert!(!valid_sender(sender), "{sender}");
        }
    }

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
