use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::ActorKind;
use crate::plugin::sdk::provider::valid_env_name;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_settings: Option<Value>,
    /// Whether the Admin trusts the settings the workdir itself carries for the Runtime, so
    /// that the Runtime Adapter lets the Runtime read them without asking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_workdir: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
    /// The environment the Session starts in, beneath what Feld sets itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// The Execution Provider's own choices, as the Provider defines them. Core stores the
    /// object the Provider's `resolve` returned and never reads inside it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<Value>,
}

/// The two kinds of Plugin: a Runtime Adapter and an Execution Provider.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Runtime,
    Provider,
}

impl PluginKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Provider => "provider",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "runtime" => Some(Self::Runtime),
            "provider" => Some(Self::Provider),
            _ => None,
        }
    }
}

/// What the daemon and the CLI have installed, so that a launchSpec can be checked against
/// the Plugin names that exist. [`crate::plugin::Registry`] answers it.
pub trait InstalledPlugins {
    fn has_runtime(&self, name: &str) -> bool;
    fn has_provider(&self, name: &str) -> bool;
    /// Whether any Plugin of the kind is installed.
    fn has_any(&self, kind: PluginKind) -> bool;
    /// The `runtimeSettings` keys one Runtime Adapter builds itself.
    fn runtime_owned_settings(&self, name: &str) -> Vec<String>;
}

/// Why a launchSpec does not fit what is installed. [`crate::plugin`] words it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallRefusal {
    /// No Plugin of the kind is installed at all.
    NoneInstalled(PluginKind),
    /// The named Plugin is missing while others of its kind are installed.
    NotInstalled { kind: PluginKind, name: String },
    /// `runtimeSettings` carries a key the Runtime Adapter builds itself.
    OwnedSetting { runtime: String, key: String },
}

impl InstallRefusal {
    /// The refusal of a `kind` Plugin named `name` that is not installed.
    pub fn missing(installed: &dyn InstalledPlugins, kind: PluginKind, name: &str) -> Self {
        if installed.has_any(kind) {
            Self::NotInstalled {
                kind,
                name: name.to_owned(),
            }
        } else {
            Self::NoneInstalled(kind)
        }
    }
}

impl LaunchSpec {
    /// Whether the launchSpec declares `trustWorkdir`, which only the Admin gives.
    pub fn trusts_workdir(&self) -> bool {
        self.trust_workdir == Some(true)
    }

    /// The shape of a launchSpec, on its own. The `runtime` and `provider` names are
    /// checked against what is installed by [`LaunchSpec::validate_installed`].
    pub fn validate(&self) -> Result<(), String> {
        match (&self.runtime, &self.command) {
            (Some(_), None) => {}
            (None, Some(command)) if !command.is_empty() && !command[0].is_empty() => {}
            (None, Some(_)) => return Err("command must contain a non-empty argv".to_owned()),
            (Some(_), Some(_)) => {
                return Err("launchSpec runtime and command are mutually exclusive".to_owned());
            }
            (None, None) => {
                return Err("launchSpec requires either runtime or command".to_owned());
            }
        }
        if self.runtime.is_none() && self.runtime_settings.is_some() {
            return Err("runtimeSettings requires runtime".to_owned());
        }
        if self.runtime.is_none() && self.trust_workdir.is_some() {
            return Err("trustWorkdir requires runtime".to_owned());
        }
        if let Some(settings) = &self.runtime_settings {
            settings
                .as_object()
                .ok_or_else(|| "runtimeSettings must be a JSON object".to_owned())?;
        }
        let workdir = self
            .workdir
            .as_deref()
            .ok_or_else(|| "launchSpec requires workdir".to_owned())?;
        if !Path::new(workdir).is_absolute() {
            return Err("launchSpec workdir must be absolute".to_owned());
        }
        if let Some(name) = self
            .env
            .iter()
            .flat_map(BTreeMap::keys)
            .find(|name| !valid_env_name(name))
        {
            return Err(format!(
                "env name '{name}' must be letters, digits, and '_', not starting with a digit"
            ));
        }
        if self.provider.as_deref().is_none_or(str::is_empty) {
            return Err("launchSpec requires provider".to_owned());
        }
        if self.node.as_deref().is_none_or(str::is_empty) {
            return Err("launchSpec requires node".to_owned());
        }
        if self
            .provider_options
            .as_ref()
            .is_some_and(|options| !options.is_object())
        {
            return Err("providerOptions must be a JSON object".to_owned());
        }
        Ok(())
    }

    /// Whether the Plugin names this launchSpec chooses are installed.
    pub fn validate_installed(
        &self,
        installed: &dyn InstalledPlugins,
    ) -> Result<(), InstallRefusal> {
        if let Some(runtime) = &self.runtime
            && !installed.has_runtime(runtime)
        {
            return Err(InstallRefusal::missing(
                installed,
                PluginKind::Runtime,
                runtime,
            ));
        }
        if let Some(provider) = &self.provider
            && !installed.has_provider(provider)
        {
            return Err(InstallRefusal::missing(
                installed,
                PluginKind::Provider,
                provider,
            ));
        }
        // The keys an Adapter builds itself are refused where the declaration is made, so
        // that a Session is never started with settings that would cut it off from its
        // hooks or its Collector.
        if let (Some(runtime), Some(settings)) = (
            &self.runtime,
            self.runtime_settings.as_ref().and_then(Value::as_object),
        ) {
            for owned in installed.runtime_owned_settings(runtime) {
                if settings.contains_key(&owned) {
                    return Err(InstallRefusal::OwnedSetting {
                        runtime: runtime.clone(),
                        key: owned,
                    });
                }
            }
        }
        Ok(())
    }

    /// Whether Feld can start an Actor of `kind` from this launchSpec: a Job runs a
    /// `command`, and an Agent runs a `runtime`.
    pub fn supports(&self, kind: ActorKind) -> bool {
        match kind {
            ActorKind::Job => self.command.is_some(),
            ActorKind::Agent => self.runtime.is_some(),
        }
    }

    /// Refuses the launchSpec an Actor of `kind` could not start from, so that the
    /// declaration is refused before it is written.
    pub fn validate_kind(&self, kind: ActorKind) -> Result<(), String> {
        if self.supports(kind) {
            return Ok(());
        }
        Err(match kind {
            ActorKind::Agent => "an agent Actor starts a runtime; a command runs as a job",
            ActorKind::Job => "a job Actor starts a command; a runtime runs as an agent",
        }
        .to_owned())
    }

    pub fn apply_patch(&self, patch: &LaunchSpecPatch) -> Self {
        Self {
            runtime: patch.runtime.apply(self.runtime.clone()),
            runtime_settings: patch.runtime_settings.apply(self.runtime_settings.clone()),
            trust_workdir: patch.trust_workdir.apply(self.trust_workdir),
            command: patch.command.apply(self.command.clone()),
            workdir: patch.workdir.apply(self.workdir.clone()),
            env: patch.env.apply(self.env.clone()),
            provider: patch.provider.apply(self.provider.clone()),
            node: patch.node.apply(self.node.clone()),
            provider_options: patch.provider_options.apply(self.provider_options.clone()),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub enum PatchValue<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

impl<T: Clone> PatchValue<T> {
    fn apply(&self, current: Option<T>) -> Option<T> {
        match self {
            Self::Missing => current,
            Self::Null => None,
            Self::Value(value) => Some(value.clone()),
        }
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for PatchValue<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Self::Value(value),
            None => Self::Null,
        })
    }
}

impl<T: Serialize> Serialize for PatchValue<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Missing | Self::Null => serializer.serialize_none(),
            Self::Value(value) => value.serialize(serializer),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchSpecPatch {
    #[serde(default, skip_serializing_if = "PatchValue::is_missing")]
    pub runtime: PatchValue<String>,
    #[serde(default, skip_serializing_if = "PatchValue::is_missing")]
    pub runtime_settings: PatchValue<Value>,
    #[serde(default, skip_serializing_if = "PatchValue::is_missing")]
    pub trust_workdir: PatchValue<bool>,
    #[serde(default, skip_serializing_if = "PatchValue::is_missing")]
    pub command: PatchValue<Vec<String>>,
    #[serde(default, skip_serializing_if = "PatchValue::is_missing")]
    pub workdir: PatchValue<String>,
    /// `env` moves as a whole, as `providerOptions` does.
    #[serde(default, skip_serializing_if = "PatchValue::is_missing")]
    pub env: PatchValue<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "PatchValue::is_missing")]
    pub provider: PatchValue<String>,
    #[serde(default, skip_serializing_if = "PatchValue::is_missing")]
    pub node: PatchValue<String>,
    /// `providerOptions` moves as a whole: a request replaces the object, omits it to keep
    /// it, or sends null to drop it.
    #[serde(default, skip_serializing_if = "PatchValue::is_missing")]
    pub provider_options: PatchValue<Value>,
}

impl LaunchSpecPatch {
    /// Whether the patch turns `trustWorkdir` on, which only the Admin does.
    pub fn trusts_workdir(&self) -> bool {
        matches!(self.trust_workdir, PatchValue::Value(true))
    }

    pub fn is_empty(&self) -> bool {
        self.runtime.is_missing()
            && self.runtime_settings.is_missing()
            && self.trust_workdir.is_missing()
            && self.command.is_missing()
            && self.workdir.is_missing()
            && self.env.is_missing()
            && self.provider.is_missing()
            && self.node.is_missing()
            && self.provider_options.is_missing()
    }
}
