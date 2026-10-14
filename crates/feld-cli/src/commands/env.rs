//! `feld env`.

use std::process::ExitCode;

use anyhow::{Context as _, Result};
use feld_core::home::{FELD_API_ENV, FELD_HOME_ENV, FeldHome, home_api};
use serde_json::Value;

use crate::{CliResult, Context};

/// `feld env` reads what the Feld home keeps: the daemon's API and the Collector
/// endpoint, which both stay the same across daemon restarts.
pub fn run(ctx: &Context) -> CliResult<ExitCode> {
    let environment = environment(&ctx.home)?;
    let exports: serde_json::Map<_, _> = environment
        .iter()
        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
        .collect();
    ctx.print(&exports, |_| {
        for (key, value) in &environment {
            println!(
                "export {key}={}",
                feld_core::process::quote_for_display(value)
            );
        }
    })?;
    Ok(ExitCode::SUCCESS)
}

/// What `feld env` exports for `home`, in the order it prints it.
pub fn environment(home: &FeldHome) -> Result<Vec<(String, String)>> {
    let root = std::path::absolute(home.root()).context("resolve the Feld home")?;
    let mut environment = vec![(FELD_HOME_ENV.to_owned(), root.display().to_string())];
    if let Ok(api) = home_api(home) {
        environment.push((FELD_API_ENV.to_owned(), api));
    }
    if let Some(collector) = feld_core::telemetry::persisted_endpoint(home) {
        environment.extend(feld_core::telemetry::shell_exporter_environment(&collector));
    }
    Ok(environment)
}

/// Whether the environment this command runs in holds everything `feld env` exports for
/// `home`, so that a Runtime started from it is connected already.
pub fn connected(home: &FeldHome) -> bool {
    environment(home).is_ok_and(|environment| {
        environment
            .iter()
            .all(|(key, value)| std::env::var(key).is_ok_and(|current| current == *value))
    })
}
