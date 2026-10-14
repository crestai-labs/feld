//! What is installed: the Plugin directories under `$FELD_HOME/plugins/`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use crate::home::{FeldHome, absent_as_none};

use super::invoke::{CALL_DEADLINE, PluginEnvironment, PluginProcess, has_executable};
use super::manifest::{Manifest, PluginKind};
use super::remote::{PluginProvider, PluginRuntime};
use super::{ExecutionProvider, PluginSummary, RuntimeAdapter};

/// The Runtime Adapters and Execution Providers the Session controller calls by name.
/// [`Registry`] answers with the installed ones.
pub trait Plugins: Send + Sync {
    fn runtime(&self, name: &str) -> Option<Arc<dyn RuntimeAdapter>>;

    fn provider(&self, name: &str) -> Option<Arc<dyn ExecutionProvider>>;
}

/// The Runtime Adapters and Execution Providers this daemon or CLI can call.
#[derive(Clone)]
pub struct Registry {
    runtimes: BTreeMap<String, Arc<dyn RuntimeAdapter>>,
    providers: BTreeMap<String, Arc<dyn ExecutionProvider>>,
    /// The directories left out of installed, in the words the daemon logs.
    problems: Vec<String>,
}

impl Registry {
    /// Reads `$FELD_HOME/plugins/`, installing each Plugin directory under its kind and
    /// name.
    pub fn load(home: &FeldHome, api: Option<&str>) -> Self {
        let environment = PluginEnvironment {
            home: home.root().to_path_buf(),
            api: api.map(str::to_owned),
        };
        let mut registry = Self {
            runtimes: BTreeMap::new(),
            providers: BTreeMap::new(),
            problems: Vec::new(),
        };
        let root = home.plugins_dir();
        for kind in [PluginKind::Runtime, PluginKind::Provider] {
            let directory = root.join(kind.as_str());
            let entries = match absent_as_none(std::fs::read_dir(&directory)) {
                Ok(Some(entries)) => entries,
                Ok(None) => continue,
                Err(error) => {
                    registry
                        .problems
                        .push(format!("read {}: {error}", directory.display()));
                    continue;
                }
            };
            let mut found = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>();
            found.sort();
            for path in found {
                registry.install(&path, kind, &environment);
            }
        }
        registry
    }

    fn install(&mut self, directory: &Path, kind: PluginKind, environment: &PluginEnvironment) {
        let manifest = match Manifest::load(directory, kind) {
            Ok(manifest) => manifest,
            Err(error) => {
                self.problems.push(format!(
                    "Plugin {} is not installed: {error:#}",
                    directory.display()
                ));
                return;
            }
        };
        if !has_executable(directory) {
            self.problems.push(format!(
                "Plugin {} is not installed: it has no executable `plugin`",
                directory.display()
            ));
            return;
        }
        let process = PluginProcess {
            name: manifest.name.clone(),
            directory: directory.to_path_buf(),
            environment: environment.clone(),
            deadline: CALL_DEADLINE,
        };
        match kind {
            PluginKind::Runtime => {
                self.runtimes.insert(
                    manifest.name.clone(),
                    Arc::new(PluginRuntime::new(manifest, process)),
                );
            }
            PluginKind::Provider => {
                self.providers.insert(
                    manifest.name.clone(),
                    Arc::new(PluginProvider::new(manifest, process)),
                );
            }
        }
    }

    pub fn runtimes(&self) -> Vec<PluginSummary> {
        summaries(self.runtimes.values().map(|runtime| runtime.manifest()))
    }

    pub fn providers(&self) -> Vec<PluginSummary> {
        summaries(self.providers.values().map(|provider| provider.manifest()))
    }

    /// The only installed Provider, which `--provider` falls back to when no Provider
    /// answers `self`.
    pub fn sole_provider(&self) -> Option<&str> {
        match self.providers.len() {
            1 => self.providers.keys().next().map(String::as_str),
            _ => None,
        }
    }

    pub fn problems(&self) -> &[String] {
        &self.problems
    }
}

impl Plugins for Registry {
    fn runtime(&self, name: &str) -> Option<Arc<dyn RuntimeAdapter>> {
        self.runtimes.get(name).cloned()
    }

    fn provider(&self, name: &str) -> Option<Arc<dyn ExecutionProvider>> {
        self.providers.get(name).cloned()
    }
}

impl crate::model::InstalledPlugins for Registry {
    fn has_runtime(&self, name: &str) -> bool {
        self.runtimes.contains_key(name)
    }

    fn has_provider(&self, name: &str) -> bool {
        self.providers.contains_key(name)
    }

    fn has_any(&self, kind: super::PluginKind) -> bool {
        match kind {
            super::PluginKind::Runtime => !self.runtimes.is_empty(),
            super::PluginKind::Provider => !self.providers.is_empty(),
        }
    }

    fn runtime_owned_settings(&self, name: &str) -> Vec<String> {
        self.runtimes
            .get(name)
            .map(|runtime| runtime.manifest().owned_settings.clone())
            .unwrap_or_default()
    }
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Registry")
            .field("runtimes", &self.runtimes.keys())
            .field("providers", &self.providers.keys())
            .field("problems", &self.problems)
            .finish()
    }
}

fn summaries<'a>(manifests: impl Iterator<Item = &'a Manifest>) -> Vec<PluginSummary> {
    manifests
        .map(|manifest| PluginSummary {
            name: manifest.name.clone(),
            label: manifest.label.clone(),
        })
        .collect()
}
