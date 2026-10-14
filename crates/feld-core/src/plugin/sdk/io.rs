//! One operation per run: the request on stdin, the response on stdout, and the exit status
//! that says whether the Plugin answered.

use std::io::Read;
use std::process::ExitCode;

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Reads one operation's request from stdin and encodes what the operation answered. An
/// empty stdin is the request `{}`, which an operation that takes no field accepts.
pub fn answer<Request: DeserializeOwned, Response: Serialize>(
    operation: impl FnOnce(Request) -> Result<Response>,
) -> Result<String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("read the operation request")?;
    let request = match input.trim() {
        "" => serde_json::from_value(Value::Object(Default::default())),
        body => serde_json::from_str(body),
    }
    .context("decode the operation request")?;
    encode(&operation(request)?)
}

/// What a shipped Plugin's executable answers to `--version`: its name and the version of
/// the Feld it was built with, which `feld doctor` compares with its own.
pub fn version(name: &str) -> Result<String> {
    Ok(format!("feld-plugin-{name} {}", env!("CARGO_PKG_VERSION")))
}

/// Encodes the response of an operation that reads no request.
pub fn encode<Response: Serialize>(response: &Response) -> Result<String> {
    serde_json::to_string(response).context("encode the operation response")
}

/// Writes what an operation answered and exits 0, or writes why it could not answer to
/// stderr and exits non-zero.
pub fn exit(answer: Result<String>) -> ExitCode {
    match answer {
        Ok(body) => {
            println!("{body}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}
