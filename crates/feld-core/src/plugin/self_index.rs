//! The names of the entries of the self index.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// The name of the self index entry of one place, which core derives from the Provider's
/// name and the selector its `attach` or `self` answered for that place. The selector is
/// encoded with its keys in name order, so the order a Plugin answers them in is free.
pub fn self_index_key(provider: &str, selector: &Value) -> String {
    let mut hash = Sha256::new();
    hash.update(provider.as_bytes());
    hash.update(b"\0");
    hash.update(
        serde_json::to_string(&sorted(selector))
            .expect("a JSON value re-encodes")
            .as_bytes(),
    );
    format!("{:x}", hash.finalize())
}

/// `value` with the keys of every object in name order.
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort();
            Value::Object(Map::from_iter(
                keys.into_iter()
                    .map(|key| (key.clone(), sorted(&object[key]))),
            ))
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// The name of the self index entry of one conversation of a Runtime, which core derives
/// from the Runtime Adapter's name and the selector its `self` answered. The prefix keeps
/// it apart from the key of a Provider that shares the name.
pub fn runtime_self_index_key(runtime: &str, selector: &Value) -> String {
    self_index_key(&format!("runtime/{runtime}"), selector)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::self_index_key;

    #[test]
    fn a_self_index_key_follows_the_provider_and_the_selector() {
        let selector = json!({"pane": "%3", "socket": "/tmp/feld"});
        assert_eq!(
            self_index_key("tmux", &selector),
            self_index_key("tmux", &json!({"socket": "/tmp/feld", "pane": "%3"})),
        );
        assert_ne!(
            self_index_key("tmux", &selector),
            self_index_key("herdr", &selector)
        );
        assert_ne!(
            self_index_key("tmux", &selector),
            self_index_key("tmux", &json!({"pane": "%4", "socket": "/tmp/feld"}))
        );
    }
}
