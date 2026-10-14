//! `plugin.json`: what a Plugin directory declares about itself.

use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::home::read_json;
use crate::id::valid_key;
pub use crate::model::PluginKind;

/// The file core reads in a Plugin directory.
pub const MANIFEST_FILE: &str = "plugin.json";
/// The file core runs for every operation.
pub const EXECUTABLE_FILE: &str = "plugin";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub name: String,
    pub kind: PluginKind,
    pub label: String,
    /// Runtime Adapter: the inputs `feld session stop` delivers to the Runtime, in order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_input: Option<Vec<String>>,
    /// Runtime Adapter: the `runtimeSettings` keys the Adapter builds itself, which a
    /// declaration is refused for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owned_settings: Vec<String>,
    /// Everything else the Plugin declares for itself, such as a Runtime's `metrics` map.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Manifest {
    /// Reads and checks the manifest of one Plugin directory. `name` must be the directory
    /// name, `kind` its parent's, and `name` must follow the Actor id grammar.
    pub fn load(directory: &Path, kind: PluginKind) -> Result<Self> {
        let path = directory.join(MANIFEST_FILE);
        let manifest: Self = read_json(&path, &path.display().to_string())?;
        let directory_name = directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if manifest.name != directory_name {
            bail!(
                "manifest name '{}' is not the directory name '{directory_name}'",
                manifest.name
            );
        }
        if manifest.kind != kind {
            bail!(
                "manifest kind '{}' is not the parent directory '{}'",
                manifest.kind.as_str(),
                kind.as_str()
            );
        }
        if !valid_key(&manifest.name) {
            bail!(
                "Plugin names must match ^[a-z0-9][a-z0-9-]*$, and '{}' does not",
                manifest.name
            );
        }
        if manifest.label.is_empty() {
            bail!("manifest label must not be empty");
        }
        Ok(manifest)
    }
}
