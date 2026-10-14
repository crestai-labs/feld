use std::collections::HashSet;

use serde_json::{Map, Value};

use feld_core::model::message_preview;

/// The length of a token `fresh_token` issues: 64 lowercase hexadecimal digits.
const TOKEN_LEN: usize = 64;

/// The token values no response may carry. A token of the form `fresh_token` issues is
/// found by a lookup per position in a run of hexadecimal digits of either case, so the
/// time a text takes follows its length, whatever the number of tokens.
#[derive(Clone, Debug, Default)]
pub struct Secrets {
    tokens: HashSet<String>,
    /// The secrets of another form, such as a local token the Admin chose.
    others: Vec<String>,
}

impl Secrets {
    pub fn insert(&mut self, secret: &str) {
        if secret.is_empty() {
            return;
        }
        if secret.len() == TOKEN_LEN && secret.bytes().all(is_token_digit) {
            self.tokens.insert(secret.to_ascii_lowercase());
        } else if !self.others.iter().any(|other| other == secret) {
            self.others.push(secret.to_owned());
        }
    }

    fn redact_tokens(&self, text: &mut String) {
        if self.tokens.is_empty() || text.len() < TOKEN_LEN {
            return;
        }
        let bytes = text.as_bytes();
        let mut redacted = String::new();
        let mut copied = 0;
        let mut run = 0;
        let mut index = 0;
        while index < bytes.len() {
            if !is_token_digit(bytes[index]) {
                run = 0;
                index += 1;
                continue;
            }
            run += 1;
            index += 1;
            if run < TOKEN_LEN {
                continue;
            }
            let start = index - TOKEN_LEN;
            // Hexadecimal digits are ASCII, so the window lies on character boundaries.
            let window = &text[start..index];
            let found = if window.bytes().any(|byte| byte.is_ascii_uppercase()) {
                self.tokens.contains(&window.to_ascii_lowercase())
            } else {
                self.tokens.contains(window)
            };
            if found {
                redacted.push_str(&text[copied..start]);
                redacted.push_str("[REDACTED]");
                copied = index;
                run = 0;
            }
        }
        if copied > 0 {
            redacted.push_str(&text[copied..]);
            *text = redacted;
        }
    }
}

fn is_token_digit(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
}

impl<S: AsRef<str>> FromIterator<S> for Secrets {
    fn from_iter<I: IntoIterator<Item = S>>(secrets: I) -> Self {
        let mut collected = Self::default();
        for secret in secrets {
            collected.insert(secret.as_ref());
        }
        collected
    }
}

pub fn redact_json(value: &mut Value, secrets: &Secrets) {
    match value {
        Value::String(text) => redact_text(text, secrets),
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| redact_json(value, secrets)),
        Value::Object(values) => {
            redact_keys(values, secrets);
            values
                .values_mut()
                .for_each(|value| redact_json(value, secrets));
            // A Message's preview is cut from its body, so a token split by the cut
            // survives the replacement; rebuild it from the redacted body.
            if let (Some(Value::String(body)), Some(Value::String(_))) =
                (values.get("body"), values.get("preview"))
            {
                let preview = message_preview(body);
                values.insert("preview".to_owned(), Value::String(preview));
            }
        }
        _ => {}
    }
}

/// Rebuilds the object only when a key carries a secret, keeping the order of the keys.
fn redact_keys(values: &mut Map<String, Value>, secrets: &Secrets) {
    let redacted = |key: &str| {
        let mut key = key.to_owned();
        redact_text(&mut key, secrets);
        key
    };
    if values.keys().all(|key| redacted(key) == *key) {
        return;
    }
    *values = std::mem::take(values)
        .into_iter()
        .map(|(key, value)| (redacted(&key), value))
        .collect();
}

pub fn redact_text(text: &mut String, secrets: &Secrets) {
    secrets.redact_tokens(text);
    for secret in &secrets.others {
        if text.contains(secret.as_str()) {
            *text = text.replace(secret.as_str(), "[REDACTED]");
        }
    }
}
