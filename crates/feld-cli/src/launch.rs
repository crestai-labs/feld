//! The launchSpec a declaration carries, from the command line and the caller's place.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use feld_core::model::{LaunchSpec, LaunchSpecPatch, PatchValue};
use feld_core::plugin::{Plugins, ProviderChoiceError, Registry, choose_provider};
use serde_json::Value;

use super::args::LaunchArgs;
use super::provider_options::{ProviderOption, provider_options};
use super::{CliError, CliResult};

/// The Execution Provider a command works with, as [`choose_provider`] decides it from what
/// this environment can answer. Several installed Providers with nothing to tell them apart
/// end in the `--provider` that names each.
pub fn resolve_provider(
    explicit: Option<String>,
    options: &[ProviderOption],
    installed: &[String],
    registry: &Registry,
) -> CliResult<String> {
    provider_here(explicit, options, installed, registry).map_err(|error| {
        let refused = CliError::Rejected(error.to_string());
        match error {
            ProviderChoiceError::Ambiguous => {
                let flags: Vec<String> = installed
                    .iter()
                    .map(|provider| format!("`--provider {provider}`"))
                    .collect();
                refused.with_next(format!("add {}", flags.join(" or ")))
            }
            _ => refused,
        }
    })
}

/// [`choose_provider`] with the Provider whose `self` answers where the caller runs.
pub fn provider_here(
    explicit: Option<String>,
    options: &[ProviderOption],
    installed: &[String],
    registry: &Registry,
) -> Result<String, ProviderChoiceError> {
    let flags: Vec<String> = options.iter().map(|option| option.flag.clone()).collect();
    let here = installed.iter().find(|provider| {
        registry
            .provider(provider)
            .and_then(|provider| provider.current().ok())
            .is_some_and(|answer| answer.selector.is_some())
    });
    choose_provider(
        explicit.as_deref(),
        &flags,
        installed,
        here.map(String::as_str),
    )
}

/// `self`: where the caller sits on one Provider, when that Provider can say.
fn caller_selector(registry: &Registry, provider: &str) -> Option<Value> {
    registry.provider(provider)?.current().ok()?.selector
}

/// The launchSpec a declaration carries, with where the caller sits on its Provider.
pub fn resolve_launch(
    args: LaunchArgs,
    options: &[ProviderOption],
    installed: &[String],
    registry: &Registry,
) -> CliResult<(LaunchSpec, Option<Value>)> {
    let current_dir = std::env::current_dir().context("resolve current working directory")?;
    let node = args.node.unwrap_or_else(|| "local".to_owned());
    let workdir = check_workdir(
        &absolute_path(
            args.workdir.as_deref().unwrap_or(&current_dir),
            &current_dir,
        ),
        &node,
    )?;
    let runtime_settings = args
        .runtime_settings
        .map(|settings| {
            serde_json::from_str::<Value>(&settings).context("parse --runtime-settings")
        })
        .transpose()?;
    let provider = resolve_provider(args.provider, options, installed, registry)?;
    let caller = caller_selector(registry, &provider);
    Ok((
        LaunchSpec {
            runtime: args.runtime,
            runtime_settings,
            trust_workdir: args.trust_workdir.then_some(true),
            command: (!args.command.is_empty()).then_some(args.command),
            workdir: Some(workdir.display().to_string()),
            env: (!args.env.is_empty()).then(|| args.env.into_iter().collect()),
            provider_options: provider_options(&provider, options)?,
            provider: Some(provider),
            node: Some(node),
        },
        caller,
    ))
}

/// The workdir a start needs on this Node: an existing directory a Provider can enter.
/// Checking it before the declaration keeps a launchSpec that can never start out of the
/// store. Another Node has its own filesystem, so only `local` is checked and made canonical
/// here.
fn check_workdir(workdir: &Path, node: &str) -> CliResult<PathBuf> {
    if node != "local" {
        return Ok(workdir.to_owned());
    }
    let rejected = |reason: String| rejected!("workdir {} {reason}", workdir.display());
    let metadata = fs::metadata(workdir).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => rejected("does not exist".to_owned()),
        std::io::ErrorKind::PermissionDenied => rejected("cannot be reached".to_owned()),
        _ => rejected(format!("cannot be read: {error}")),
    })?;
    if !metadata.is_dir() {
        return Err(rejected("is not a directory".to_owned()));
    }
    fs::read_dir(workdir).map_err(|error| rejected(format!("cannot be entered: {error}")))?;
    fs::canonicalize(workdir).map_err(|error| rejected(format!("cannot be resolved: {error}")))
}

/// The launchSpec patch an update carries. `provider` is the Provider the command line resolves
/// to. `env` moves as a whole: the variables on the command line replace the object, and their
/// absence keeps it. `providerOptions` moves as a whole: the options on the command line replace
/// the object, `--provider` without options clears it for the new Provider's defaults, and neither
/// keeps it.
pub fn launch_patch(
    args: LaunchArgs,
    options: &[ProviderOption],
    installed: &[String],
    registry: &Registry,
) -> CliResult<(LaunchSpecPatch, Option<Value>)> {
    let current_dir = std::env::current_dir().context("resolve current working directory")?;
    let runtime_settings = args
        .runtime_settings
        .map(|settings| {
            serde_json::from_str::<Value>(&settings).context("parse --runtime-settings")
        })
        .transpose()?;
    let provider = match (args.provider, options.is_empty()) {
        (None, true) => None,
        (explicit, _) => Some(resolve_provider(explicit, options, installed, registry)?),
    };
    let caller = provider
        .as_deref()
        .and_then(|provider| caller_selector(registry, provider));
    let mut patch = LaunchSpecPatch {
        runtime: args.runtime.map_or(PatchValue::Missing, PatchValue::Value),
        runtime_settings: runtime_settings.map_or(PatchValue::Missing, PatchValue::Value),
        trust_workdir: match (args.trust_workdir, args.no_trust_workdir) {
            (true, _) => PatchValue::Value(true),
            (false, true) => PatchValue::Null,
            (false, false) => PatchValue::Missing,
        },
        command: if args.command.is_empty() {
            PatchValue::Missing
        } else {
            PatchValue::Value(args.command)
        },
        workdir: match &args.workdir {
            Some(path) => {
                let workdir = check_workdir(
                    &absolute_path(path, &current_dir),
                    args.node.as_deref().unwrap_or("local"),
                )?;
                PatchValue::Value(workdir.display().to_string())
            }
            None => PatchValue::Missing,
        },
        env: if args.env.is_empty() {
            PatchValue::Missing
        } else {
            PatchValue::Value(args.env.into_iter().collect())
        },
        provider_options: match provider.as_deref() {
            Some(provider) => match provider_options(provider, options)? {
                Some(options) => PatchValue::Value(options),
                None => PatchValue::Null,
            },
            None => PatchValue::Missing,
        },
        provider: provider.map_or(PatchValue::Missing, PatchValue::Value),
        node: args.node.map_or(PatchValue::Missing, PatchValue::Value),
    };
    if matches!(patch.runtime, PatchValue::Value(_)) {
        patch.command = PatchValue::Null;
    }
    if matches!(patch.command, PatchValue::Value(_)) {
        patch.runtime = PatchValue::Null;
        patch.runtime_settings = PatchValue::Null;
        patch.trust_workdir = PatchValue::Null;
    }
    Ok((patch, caller))
}

fn absolute_path(path: &Path, current_dir: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        current_dir.join(path)
    }
}

#[cfg(test)]
mod tests {
    use feld_core::home::FeldHome;

    use super::*;

    fn patch(provider: Option<&str>, options: &[(&str, &str)]) -> LaunchSpecPatch {
        let home = tempfile::tempdir().expect("tempdir");
        let registry = Registry::load(
            &FeldHome::resolve(Some(home.path().to_owned())).expect("home"),
            None,
        );
        let options: Vec<ProviderOption> = options
            .iter()
            .map(|(flag, value)| ProviderOption {
                flag: (*flag).to_owned(),
                value: (*value).to_owned(),
            })
            .collect();
        let args = LaunchArgs {
            provider: provider.map(str::to_owned),
            ..LaunchArgs::default()
        };
        let installed = ["herdr".to_owned(), "tmux".to_owned()];
        launch_patch(args, &options, &installed, &registry)
            .expect("patch")
            .0
    }

    #[test]
    fn options_without_provider_patch_the_provider_their_prefix_names() {
        let patch = patch(None, &[("tmux-session", "work")]);
        assert!(matches!(patch.provider, PatchValue::Value(provider) if provider == "tmux"));
        assert!(matches!(
            patch.provider_options,
            PatchValue::Value(options) if options == serde_json::json!({ "session": "work" })
        ));
    }

    #[test]
    fn provider_without_options_clears_the_options() {
        let patch = patch(Some("herdr"), &[]);
        assert!(matches!(patch.provider, PatchValue::Value(provider) if provider == "herdr"));
        assert!(matches!(patch.provider_options, PatchValue::Null));
    }

    #[test]
    fn neither_provider_nor_options_keeps_both() {
        let patch = patch(None, &[]);
        assert!(matches!(patch.provider, PatchValue::Missing));
        assert!(matches!(patch.provider_options, PatchValue::Missing));
    }
}
