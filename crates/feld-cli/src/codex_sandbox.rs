//! What the CLI tells about Codex's sandbox and the spool (`docs/spec/design/spool.md`): why
//! a `feld` inside the sandbox cannot write its request, and a sandbox whose network reaches
//! the socket of the pane it runs in.

use std::path::{Path, PathBuf};

use feld_core::model::LaunchSpec;
use feld_core::plugin::sdk::runtime::ConfigRoot;
use feld_core::spool::covered_by;
use serde_json::Value;

const CONFIG_ROOT: ConfigRoot = ConfigRoot {
    variable: "CODEX_HOME",
    home_dir: ".codex",
};

/// The Execution Providers whose panes carry the socket of their server in the environment.
pub const SOCKET_PROVIDERS: [&str; 2] = ["tmux", "herdr"];

/// What a sandbox open to the network does on a Provider of [`SOCKET_PROVIDERS`].
pub const NETWORK_RISK: &str = "an Agent inside the sandbox reaches the socket of the pane and runs commands in panes outside the sandbox";

/// How to close the sandbox to the network in `place`, keeping `feld` reaching Feld.
pub fn network_fix(place: &str) -> String {
    format!(
        "set sandbox_workspace_write.network_access = false in {place}; `feld` inside the sandbox reaches Feld through the spool"
    )
}

/// The `config.toml` of Codex's configuration root.
pub fn config_path() -> Option<PathBuf> {
    CONFIG_ROOT.path().map(|root| root.join("config.toml"))
}

/// What a Codex tool process finds in its environment about the sandbox it runs in.
pub struct Sandbox {
    /// `CODEX_PERMISSION_PROFILE`, which is `:read-only` in a read-only sandbox.
    pub permission_profile: Option<String>,
    /// Whether `CODEX_SANDBOX` is there, which Codex sets in every sandbox it starts.
    pub sandboxed: bool,
    /// `CODEX_THREAD_ID`, the thread to resume.
    pub thread: Option<String>,
    /// The `config.toml` of the configuration root, and whether its writable roots cover the
    /// spool; None when it could not be read.
    pub config: Option<(PathBuf, bool)>,
}

impl Sandbox {
    pub fn of_environment(spool: &Path) -> Self {
        let variable = |name| std::env::var(name).ok().filter(|value| !value.is_empty());
        let config = config_path();
        Self {
            permission_profile: variable("CODEX_PERMISSION_PROFILE"),
            sandboxed: variable("CODEX_SANDBOX").is_some(),
            thread: variable("CODEX_THREAD_ID"),
            config: config.and_then(|path| {
                let covered = config_covers(&path, spool)?;
                Some((path, covered))
            }),
        }
    }

    /// Why the sandbox denies the directory of the spool and how to undo that, by what the
    /// sandbox tells; None for a sandbox that tells nothing. Only the `shared` directory of
    /// the `attached` Sessions comes from `config.toml`; Feld's launch hands an `owned`
    /// Session its own.
    pub fn unwritable_hint(&self, spool: &Path, shared: bool) -> Option<String> {
        let read_only = format!(
            "start a new thread with `/new`, or quit Codex and run `codex resume {} --sandbox workspace-write`",
            self.thread.as_deref().unwrap_or("<thread id>")
        );
        if self.permission_profile.as_deref() == Some(":read-only") {
            return Some(format!(
                "this Codex thread runs in a read-only sandbox, which an app-server restart also leaves it in; {read_only}"
            ));
        }
        if !self.sandboxed || !shared {
            return None;
        }
        let (config, covered) = self.config.as_ref()?;
        Some(if *covered {
            format!(
                "{} lists it among writable_roots, yet the sandbox denies it: if Codex started with -c sandbox_workspace_write.writable_roots, that list replaces the one of config.toml, so add {} to it; if the thread runs read-only, {read_only}",
                config.display(),
                spool.display()
            )
        } else {
            format!(
                "it is not in the writable_roots of {}; run `feld init --runtime codex` outside the sandbox, then start Codex again",
                config.display()
            )
        })
    }
}

/// Whether the writable roots of a `config.toml` cover the spool; None when the file cannot
/// be read as one.
fn config_covers(path: &Path, spool: &Path) -> Option<bool> {
    let document = read_config(path)?;
    let roots = document
        .get("sandbox_workspace_write")
        .and_then(|table| table.get("writable_roots"));
    let Some(roots) = roots else {
        return Some(false);
    };
    Some(
        roots
            .as_array()?
            .iter()
            .filter_map(|root| root.as_str())
            .any(|root| covered_by(spool, Path::new(root))),
    )
}

/// A `config.toml`, empty when there is none; None when it cannot be read as one.
fn read_config(path: &Path) -> Option<toml_edit::DocumentMut> {
    match std::fs::read_to_string(path) {
        Ok(text) => text.parse().ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Some(toml_edit::DocumentMut::new())
        }
        Err(_) => None,
    }
}

/// What a `config.toml` sets for the sandbox: `sandbox_mode` and
/// `sandbox_workspace_write.network_access`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ConfigSandbox {
    pub mode: Option<String>,
    pub network_access: Option<bool>,
}

/// What the `config.toml` at `path` sets for the sandbox; None when it cannot be read as one.
pub fn config_sandbox(path: &Path) -> Option<ConfigSandbox> {
    let document = read_config(path)?;
    Some(ConfigSandbox {
        mode: document
            .get("sandbox_mode")
            .and_then(|mode| mode.as_str())
            .map(str::to_owned),
        network_access: document
            .get("sandbox_workspace_write")
            .and_then(|table| table.get("network_access"))
            .and_then(|value| value.as_bool()),
    })
}

/// The note of a Codex declaration on a Provider of [`SOCKET_PROVIDERS`] whose
/// `workspace-write` sandbox is open to the network, by its `runtimeSettings` over `config`.
pub fn network_note(launch: &LaunchSpec, config: &ConfigSandbox) -> Option<String> {
    if launch.runtime.as_deref() != Some("codex") {
        return None;
    }
    let provider = launch.provider.as_deref()?;
    if !SOCKET_PROVIDERS.contains(&provider) {
        return None;
    }
    let settings = launch.runtime_settings.as_ref().and_then(Value::as_object);
    let setting = |key: &str| settings.and_then(|settings| settings.get(key));
    let mode = setting("sandbox_mode")
        .and_then(Value::as_str)
        .or(config.mode.as_deref());
    if mode.is_some_and(|mode| mode != "workspace-write") {
        return None;
    }
    let network_access = setting("sandbox_workspace_write.network_access")
        .or_else(|| setting("sandbox_workspace_write")?.get("network_access"))
        .and_then(Value::as_bool)
        .or(config.network_access);
    (network_access == Some(true)).then(|| {
        format!(
            "the Codex sandbox is open to the network on {provider}: {NETWORK_RISK}; {}",
            network_fix("runtimeSettings or Codex config.toml")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SPOOL: &str = "/Users/alice/.feld/spool/shared";

    fn sandbox(profile: Option<&str>, sandboxed: bool, covered: Option<bool>) -> Sandbox {
        Sandbox {
            permission_profile: profile.map(str::to_owned),
            sandboxed,
            thread: Some("019a0000-0000-7000-8000-000000000000".to_owned()),
            config: covered
                .map(|covered| (PathBuf::from("/Users/alice/.codex/config.toml"), covered)),
        }
    }

    #[test]
    fn each_cause_tells_its_own_way_out() {
        let spool = Path::new(SPOOL);
        let read_only = sandbox(Some(":read-only"), true, Some(true))
            .unwritable_hint(spool, true)
            .unwrap();
        assert!(read_only.contains("read-only sandbox"), "{read_only}");
        assert!(
            read_only.contains(
                "`codex resume 019a0000-0000-7000-8000-000000000000 --sandbox workspace-write`"
            ),
            "{read_only}"
        );

        let unprepared = sandbox(None, true, Some(false))
            .unwritable_hint(spool, true)
            .unwrap();
        assert!(
            unprepared.contains("not in the writable_roots of /Users/alice/.codex/config.toml"),
            "{unprepared}"
        );
        assert!(
            unprepared.contains("`feld init --runtime codex`"),
            "{unprepared}"
        );

        let replaced = sandbox(None, true, Some(true))
            .unwritable_hint(spool, true)
            .unwrap();
        assert!(
            replaced.contains("-c sandbox_workspace_write"),
            "{replaced}"
        );
        assert!(replaced.contains("`/new`"), "{replaced}");

        assert!(
            sandbox(None, false, Some(false))
                .unwritable_hint(spool, true)
                .is_none()
        );
        assert!(
            sandbox(None, true, None)
                .unwritable_hint(spool, true)
                .is_none()
        );
        // An `owned` Session's own directory comes from the launch, not from config.toml.
        assert!(
            sandbox(None, true, Some(false))
                .unwritable_hint(spool, false)
                .is_none()
        );
    }

    #[test]
    fn a_config_that_lists_the_spool_or_its_home_covers_it() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("config.toml");
        let spool = Path::new(SPOOL);
        assert_eq!(config_covers(&path, spool), Some(false));
        for (text, covered) in [
            ("model = \"o3\"\n", Some(false)),
            (
                "[sandbox_workspace_write]\nwritable_roots = [\"/work\"]\n",
                Some(false),
            ),
            (
                "[sandbox_workspace_write]\nwritable_roots = [\"/Users/alice/.feld\"]\n",
                Some(true),
            ),
            (
                "sandbox_workspace_write.writable_roots = [\"/Users/alice/.feld/spool\"]\n",
                Some(true),
            ),
            ("[sandbox_workspace_write\n", None),
        ] {
            std::fs::write(&path, text).unwrap();
            assert_eq!(config_covers(&path, spool), covered, "{text}");
        }
    }

    #[test]
    fn a_config_tells_its_sandbox_mode_and_network() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("config.toml");
        assert_eq!(config_sandbox(&path), Some(ConfigSandbox::default()));
        for (text, expected) in [
            (
                "sandbox_mode = \"workspace-write\"\n[sandbox_workspace_write]\nnetwork_access = true\n",
                Some(ConfigSandbox {
                    mode: Some("workspace-write".to_owned()),
                    network_access: Some(true),
                }),
            ),
            (
                "sandbox_workspace_write.network_access = false\n",
                Some(ConfigSandbox {
                    mode: None,
                    network_access: Some(false),
                }),
            ),
            ("[sandbox_workspace_write\n", None),
        ] {
            std::fs::write(&path, text).unwrap();
            assert_eq!(config_sandbox(&path), expected, "{text}");
        }
    }

    #[test]
    fn a_workspace_write_sandbox_open_to_the_network_on_a_pane_is_noted() {
        let launch = |provider: &str, settings: Value| LaunchSpec {
            runtime: Some("codex".to_owned()),
            runtime_settings: Some(settings),
            provider: Some(provider.to_owned()),
            ..LaunchSpec::default()
        };
        let closed = ConfigSandbox::default();
        let open = ConfigSandbox {
            mode: None,
            network_access: Some(true),
        };
        let noted =
            |launch: &LaunchSpec, config: &ConfigSandbox| network_note(launch, config).is_some();
        let settings = json!({
            "sandbox_mode": "workspace-write",
            "sandbox_workspace_write": {"network_access": true},
        });
        assert!(noted(&launch("tmux", settings.clone()), &closed));
        assert!(noted(&launch("herdr", settings.clone()), &closed));
        assert!(!noted(&launch("docker", settings), &closed));
        assert!(noted(
            &launch(
                "tmux",
                json!({"sandbox_workspace_write.network_access": true})
            ),
            &closed
        ));
        assert!(noted(&launch("tmux", json!({})), &open));
        assert!(!noted(
            &launch(
                "tmux",
                json!({"sandbox_workspace_write.network_access": false})
            ),
            &open
        ));
        assert!(!noted(
            &launch("tmux", json!({"sandbox_mode": "read-only"})),
            &open
        ));
        assert!(!noted(
            &launch("tmux", json!({})),
            &ConfigSandbox {
                mode: Some("danger-full-access".to_owned()),
                network_access: Some(true),
            }
        ));
        let mut other = launch("tmux", json!({}));
        other.runtime = Some("claude-code".to_owned());
        assert!(!noted(&other, &open));
    }
}
