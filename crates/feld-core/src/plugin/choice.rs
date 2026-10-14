//! Which Execution Provider a declaration works with.

use super::{PluginKind, none_installed};

/// Why [`choose_provider`] found no Execution Provider.
#[derive(Debug, PartialEq, Eq)]
pub enum ProviderChoiceError {
    NoneInstalled,
    /// A `--<provider>-<key>` option no installed Provider takes.
    UnknownOption {
        flag: String,
        installed: Vec<String>,
    },
    /// Several Providers are installed, and nothing tells which one.
    Ambiguous,
}

impl std::fmt::Display for ProviderChoiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoneInstalled => formatter.write_str(&none_installed(PluginKind::Provider)),
            Self::UnknownOption { flag, installed } => write!(
                formatter,
                "'--{flag}' is no Feld option and no installed Execution Provider takes it (installed: {})",
                installed.join(", ")
            ),
            Self::Ambiguous => {
                formatter.write_str("feld cannot tell which Execution Provider to use")
            }
        }
    }
}

/// The Execution Provider a declaration works with, in three steps: the one it names, else
/// the prefix its `--<provider>-<key>` options carry, else the one whose `self` answers where
/// the caller runs (`here`), else the only installed one.
pub fn choose_provider(
    explicit: Option<&str>,
    option_flags: &[String],
    installed: &[String],
    here: Option<&str>,
) -> std::result::Result<String, ProviderChoiceError> {
    if let Some(provider) = explicit {
        return Ok(provider.to_owned());
    }
    if installed.is_empty() {
        return Err(ProviderChoiceError::NoneInstalled);
    }
    if let Some(first) = option_flags.first() {
        let named = installed
            .iter()
            .filter(|provider| {
                let prefix = format!("{provider}-");
                option_flags.iter().all(|flag| flag.starts_with(&prefix))
            })
            .max_by_key(|provider| provider.len());
        return named
            .cloned()
            .ok_or_else(|| ProviderChoiceError::UnknownOption {
                flag: first.clone(),
                installed: installed.to_vec(),
            });
    }
    if let Some(provider) = here {
        return Ok(provider.to_owned());
    }
    match installed {
        [only] => Ok(only.clone()),
        _ => Err(ProviderChoiceError::Ambiguous),
    }
}

#[cfg(test)]
mod tests {
    use super::{PluginKind, ProviderChoiceError, choose_provider, none_installed};

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn provider_resolution_reads_the_option_prefix_then_the_caller_then_the_only_installed() {
        let installed = names(&["tmux", "docker", "docker-sandbox"]);
        // The prefix of the options, taking the longest Provider name that carries them all.
        assert_eq!(
            choose_provider(
                None,
                &names(&["tmux-socket", "tmux-unit"]),
                &installed,
                None
            )
            .unwrap(),
            "tmux"
        );
        assert_eq!(
            choose_provider(
                None,
                &names(&["docker-sandbox-image"]),
                &installed,
                Some("tmux")
            )
            .unwrap(),
            "docker-sandbox"
        );
        // Then the Provider whose `self` answers where the caller runs.
        assert_eq!(
            choose_provider(None, &[], &installed, Some("tmux")).unwrap(),
            "tmux"
        );
        // Then the only installed Provider.
        assert_eq!(
            choose_provider(None, &[], &names(&["tmux"]), None).unwrap(),
            "tmux"
        );
        // With none of the three, the declaration is refused.
        assert_eq!(
            choose_provider(None, &[], &installed, None),
            Err(ProviderChoiceError::Ambiguous)
        );
        assert_eq!(
            choose_provider(None, &[], &names(&["tmux", "herdr"]), None),
            Err(ProviderChoiceError::Ambiguous)
        );
        assert!(choose_provider(None, &names(&["herdr-pane"]), &installed, None).is_err());
        // With nothing installed, the refusal names the `feld init` that places one.
        assert_eq!(
            choose_provider(None, &[], &[], None)
                .unwrap_err()
                .to_string(),
            "No Execution Providers are installed; run `feld init --provider tmux`"
        );
        assert_eq!(
            choose_provider(None, &names(&["tmux-socket"]), &[], None)
                .unwrap_err()
                .to_string(),
            none_installed(PluginKind::Provider)
        );
        // What `--provider` names wins over all three.
        assert_eq!(
            choose_provider(Some("docker"), &[], &installed, Some("tmux")).unwrap(),
            "docker"
        );
    }
}
