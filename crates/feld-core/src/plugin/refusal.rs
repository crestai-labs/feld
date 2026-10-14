//! What core answers when a Plugin it needs is not installed.

use super::{DEFAULTS, PluginKind};
use crate::model::{InstallRefusal, InstalledPlugins};

/// The `feld init` that places the default Plugins of the given kinds.
pub fn default_init(kinds: &[PluginKind]) -> String {
    let mut command = "feld init".to_owned();
    for kind in kinds {
        for (_, name) in DEFAULTS.iter().filter(|(default, _)| default == kind) {
            command.push_str(&format!(" --{} {name}", kind.as_str()));
        }
    }
    command
}

/// The answer for a Plugin name that is not installed.
pub fn not_installed(kind: PluginKind, name: &str) -> String {
    format!("{} '{name}' is not installed", kind.as_str())
}

/// The refusal when no Plugin of one kind is installed, naming the `feld init` that
/// places the kind's default.
pub fn none_installed(kind: PluginKind) -> String {
    format!(
        "No {} are installed; run `{}`",
        plural(kind),
        default_init(&[kind])
    )
}

fn plural(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Runtime => "Runtime Adapters",
        PluginKind::Provider => "Execution Providers",
    }
}

impl std::fmt::Display for InstallRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoneInstalled(kind) => formatter.write_str(&none_installed(*kind)),
            Self::NotInstalled { kind, name } => write!(
                formatter,
                "{}; `feld daemon status` lists the installed {}",
                not_installed(*kind, name),
                plural(*kind)
            ),
            Self::OwnedSetting { runtime, key } => write!(
                formatter,
                "runtimeSettings key '{key}' is owned by the '{runtime}' Runtime Adapter"
            ),
        }
    }
}

/// The refusal of a Runtime Adapter name that is not installed, naming the `feld init`
/// that places one when none is.
pub fn runtime_not_installed(installed: &dyn InstalledPlugins, runtime: &str) -> String {
    InstallRefusal::missing(installed, PluginKind::Runtime, runtime).to_string()
}

/// The refusal of an Execution Provider name that is not installed, naming the `feld init`
/// that places one when none is.
pub fn provider_not_installed(installed: &dyn InstalledPlugins, provider: &str) -> String {
    InstallRefusal::missing(installed, PluginKind::Provider, provider).to_string()
}
