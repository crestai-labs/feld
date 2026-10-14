//! JSON written as the TOML `codex -c <key>=<value>` reads.
//!
//! `runtimeSettings` is Codex's `config.toml` expressed in JSON, and the settings this
//! Adapter owns are built as JSON too, so both reach Codex through one encoding: every
//! value becomes the TOML inline form of itself, on one line, as one `-c` argument.

use anyhow::{Result, bail};
use serde_json::{Map, Value};

/// One `-c <key>=<value>` argument, with the value written as TOML.
pub fn override_argument(key: &str, value: &Value) -> Result<String> {
    check_key(key)?;
    Ok(format!("{key}={}", encode(key, value)?))
}

/// A dotted path of TOML bare keys, which is what `-c` takes and what a `config.toml`
/// entry is named by. Anything else would reach Codex as a different setting than it
/// spells, so it is refused rather than quoted.
fn check_key(key: &str) -> Result<()> {
    if !key.is_empty() && key.split('.').all(is_bare_key) {
        return Ok(());
    }
    bail!("'{key}' is no Codex setting: a setting is a dotted path of [A-Za-z0-9_-] names")
}

fn is_bare_key(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

fn encode(key: &str, value: &Value) -> Result<String> {
    Ok(match value {
        Value::Null => bail!("Codex setting '{key}' is null, which TOML has no value for"),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => string(value),
        Value::Array(values) => {
            let items = values
                .iter()
                .map(|item| encode(key, item))
                .collect::<Result<Vec<_>>>()?;
            format!("[{}]", items.join(","))
        }
        Value::Object(entries) => table(key, entries)?,
    })
}

fn table(key: &str, entries: &Map<String, Value>) -> Result<String> {
    let items = entries
        .iter()
        .map(|(name, value)| {
            let name = if is_bare_key(name) {
                name.clone()
            } else {
                string(name)
            };
            Ok(format!("{name}={}", encode(key, value)?))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(format!("{{{}}}", items.join(",")))
}

/// A TOML basic string: the quoted form, with the escapes TOML names and `\uXXXX` for the
/// remaining control characters.
fn string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\u{8}' => encoded.push_str("\\b"),
            '\t' => encoded.push_str("\\t"),
            '\n' => encoded.push_str("\\n"),
            '\u{c}' => encoded.push_str("\\f"),
            '\r' => encoded.push_str("\\r"),
            control if control.is_control() => {
                encoded.push_str(&format!("\\u{:04X}", control as u32))
            }
            other => encoded.push(other),
        }
    }
    encoded.push('"');
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn encoded(key: &str, value: Value) -> String {
        override_argument(key, &value).unwrap()
    }

    #[test]
    fn every_json_value_reaches_codex_as_its_toml_form() {
        assert_eq!(encoded("model", json!("gpt-5.6")), r#"model="gpt-5.6""#);
        assert_eq!(encoded("check", json!(false)), "check=false");
        assert_eq!(encoded("n", json!(12)), "n=12");
        assert_eq!(
            encoded("sandbox_permissions", json!(["disk-full-read-access"])),
            r#"sandbox_permissions=["disk-full-read-access"]"#
        );
        assert_eq!(
            encoded(
                "otel.exporter",
                json!({"otlp-http": {"endpoint": "http://127.0.0.1:1/v1/logs", "protocol": "binary"}})
            ),
            r#"otel.exporter={otlp-http={endpoint="http://127.0.0.1:1/v1/logs",protocol="binary"}}"#
        );
        // A path with a quote or a backslash in it stays one TOML string.
        assert_eq!(
            encoded("command", json!("/tmp/a\"b\\c\nd")),
            r#"command="/tmp/a\"b\\c\nd""#
        );
        assert_eq!(encoded("t", json!({"a b": 1})), r#"t={"a b"=1}"#);
    }

    #[test]
    fn a_setting_toml_cannot_name_or_hold_is_refused() {
        for key in ["", "a..b", "a b", "a=b", "a.\"b\""] {
            assert!(
                override_argument(key, &json!(1)).is_err(),
                "'{key}' is no setting"
            );
        }
        assert!(override_argument("model", &Value::Null).is_err());
        assert!(override_argument("t", &json!({"a": null})).is_err());
    }
}
