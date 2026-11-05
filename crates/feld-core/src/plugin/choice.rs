//! Which Execution Provider a declaration works with.

use super::{PROCESS_PROVIDER, PluginKind, none_installed};
use crate::model::ActorKind;

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
    /// An Agent needs a terminal, and only the process Provider is installed.
    NoTerminal,
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
            Self::NoTerminal => write!(
                formatter,
                "no Execution Provider that gives an Agent a terminal is installed; run `feld init --provider tmux`"
            ),
        }
    }
}

/// The Execution Provider a declaration of `kind` works with: the one it names, else the
/// prefix its `--<provider>-<key>` options carry, else for a Job the process Provider, else the
/// one whose `self` answers where the caller runs (`here`), else the only installed one that
/// gives a terminal. The process Provider runs a command without a terminal, so an Agent never
/// lands on it unnamed.
pub fn choose_provider(
    explicit: Option<&str>,
    option_flags: &[String],
    installed: &[String],
    here: Option<&str>,
    kind: ActorKind,
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
    if kind == ActorKind::Job && installed.iter().any(|name| name == PROCESS_PROVIDER) {
        return Ok(PROCESS_PROVIDER.to_owned());
    }
    if let Some(provider) = here {
        return Ok(provider.to_owned());
    }
    match terminal_providers(installed).as_slice() {
        [] => Err(ProviderChoiceError::NoTerminal),
        [only] => Ok((*only).clone()),
        _ => Err(ProviderChoiceError::Ambiguous),
    }
}

/// The installed Providers that give what they launch a terminal.
pub fn terminal_providers(installed: &[String]) -> Vec<&String> {
    installed
        .iter()
        .filter(|name| *name != PROCESS_PROVIDER)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ActorKind, PluginKind, ProviderChoiceError, choose_provider, none_installed};

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
                None,
                ActorKind::Agent
            )
            .unwrap(),
            "tmux"
        );
        assert_eq!(
            choose_provider(
                None,
                &names(&["docker-sandbox-image"]),
                &installed,
                Some("tmux"),
                ActorKind::Agent
            )
            .unwrap(),
            "docker-sandbox"
        );
        // Then the Provider whose `self` answers where the caller runs.
        assert_eq!(
            choose_provider(None, &[], &installed, Some("tmux"), ActorKind::Agent).unwrap(),
            "tmux"
        );
        // Then the only installed Provider.
        assert_eq!(
            choose_provider(None, &[], &names(&["tmux"]), None, ActorKind::Agent).unwrap(),
            "tmux"
        );
        // With none of the three, the declaration is refused.
        assert_eq!(
            choose_provider(None, &[], &installed, None, ActorKind::Agent),
            Err(ProviderChoiceError::Ambiguous)
        );
        assert_eq!(
            choose_provider(
                None,
                &[],
                &names(&["tmux", "herdr"]),
                None,
                ActorKind::Agent
            ),
            Err(ProviderChoiceError::Ambiguous)
        );
        assert!(
            choose_provider(
                None,
                &names(&["herdr-pane"]),
                &installed,
                None,
                ActorKind::Agent
            )
            .is_err()
        );
        // With nothing installed, the refusal names the `feld init` that places one.
        assert_eq!(
            choose_provider(None, &[], &[], None, ActorKind::Agent)
                .unwrap_err()
                .to_string(),
            "No Execution Providers are installed; run `feld init --provider tmux`"
        );
        assert_eq!(
            choose_provider(None, &names(&["tmux-socket"]), &[], None, ActorKind::Agent)
                .unwrap_err()
                .to_string(),
            none_installed(PluginKind::Provider)
        );
        // What `--provider` names wins over all three.
        assert_eq!(
            choose_provider(
                Some("docker"),
                &[],
                &installed,
                Some("tmux"),
                ActorKind::Agent
            )
            .unwrap(),
            "docker"
        );
    }

    #[test]
    fn a_job_takes_the_process_provider_and_an_agent_never_does() {
        let installed = names(&["tmux", "process"]);
        assert_eq!(
            choose_provider(None, &[], &installed, None, ActorKind::Job).unwrap(),
            "process"
        );
        // An option of another Provider, or a name, still wins for a Job.
        assert_eq!(
            choose_provider(
                None,
                &names(&["tmux-socket"]),
                &installed,
                None,
                ActorKind::Job
            )
            .unwrap(),
            "tmux"
        );
        assert_eq!(
            choose_provider(Some("tmux"), &[], &installed, None, ActorKind::Job).unwrap(),
            "tmux"
        );
        // A Job without the process Provider chooses as an Agent does.
        assert_eq!(
            choose_provider(None, &[], &names(&["tmux"]), None, ActorKind::Job).unwrap(),
            "tmux"
        );
        // An Agent leaves the process Provider out of the only installed one.
        assert_eq!(
            choose_provider(None, &[], &installed, None, ActorKind::Agent).unwrap(),
            "tmux"
        );
        assert_eq!(
            choose_provider(None, &[], &names(&["process"]), None, ActorKind::Agent),
            Err(ProviderChoiceError::NoTerminal)
        );
    }
}
