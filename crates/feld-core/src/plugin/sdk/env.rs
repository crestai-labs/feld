//! What core puts in the environment of every Plugin operation.

use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::home::{FELD_HOME_ENV, FELD_PLUGIN_DIR_ENV, FeldHome};

/// The Feld home this Plugin works in: what core put in the environment, otherwise the
/// one the parts beside a Runtime find beside a Runtime a person started themselves.
pub fn feld_home() -> Result<FeldHome> {
    let explicit = std::env::var_os(FELD_HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    FeldHome::resolve(explicit)
}

/// The directory this Plugin lives in, which holds the files the Plugin's static parts
/// are made of. Core puts it in the environment of every operation.
pub fn plugin_directory() -> Result<PathBuf> {
    match std::env::var_os(FELD_PLUGIN_DIR_ENV).filter(|value| !value.is_empty()) {
        Some(value) => Ok(PathBuf::from(value)),
        None => bail!("FELD_PLUGIN_DIR is not set"),
    }
}
