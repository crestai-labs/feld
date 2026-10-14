//! The Plugins Feld ships, and how `feld init` writes one into `$FELD_HOME/plugins/`.
//!
//! A shipped Plugin is packaged in two parts (see the module documentation of
//! [`feld_core::plugin`]):
//! the static files of its directory are embedded here with `include_str!`, and its
//! executable is the `feld-plugin-<name>` binary that sits beside the running `feld`.
//! Installing one writes the first out and copies the second to the directory's `plugin`,
//! which `feld init` does for the Plugins it chooses and the daemon does, when it starts,
//! for the ones already installed.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use feld_core::home::FeldHome;

use feld_core::plugin::DEFAULTS;
use feld_core::plugin::manifest::{EXECUTABLE_FILE, MANIFEST_FILE, Manifest, PluginKind};

/// One file of a shipped Plugin directory, as it is written into the Plugin directory.
pub struct BundledFile {
    pub path: &'static str,
    pub contents: &'static str,
    /// Whether the file is run rather than read, which the shims beside Claude Code are.
    pub runnable: bool,
}

/// One Plugin Feld ships.
pub struct BundledPlugin {
    pub name: &'static str,
    pub kind: PluginKind,
    /// The executable of the product the Plugin connects, which `feld init` looks for on
    /// `PATH`.
    pub command: &'static str,
    /// The arguments `command` prints its version to, which `feld doctor` shows.
    pub version_args: &'static [&'static str],
    /// The oldest version of the product the Plugin supports, the table of supported
    /// versions in `docs/spec/design/plugin.md`.
    pub oldest: &'static str,
    pub files: &'static [BundledFile],
}

impl BundledPlugin {
    /// The display name, which the Plugin's own manifest carries.
    pub fn label(&self) -> String {
        let manifest: serde_json::Value =
            serde_json::from_str(self.manifest()).expect("a shipped Plugin manifest is valid JSON");
        manifest["label"]
            .as_str()
            .expect("a shipped Plugin manifest has a label")
            .to_owned()
    }

    /// The step that puts the product on `PATH`, which `feld doctor` and a refused start
    /// both tell. A Runtime also wants its login, which Feld does not check.
    pub fn install_next(&self) -> String {
        let install = format!(
            "install {} so that `{}` is on PATH",
            self.label(),
            self.command
        );
        match self.kind {
            PluginKind::Provider => install,
            PluginKind::Runtime => format!("{install}, and log in with it"),
        }
    }

    /// What to tell a person whose product, as `version_args` answers, is older than
    /// [`oldest`](Self::oldest). An answer without a word that begins with a digit, such as
    /// `tmux next-3.7` or `tmux openbsd-7.5`, passes.
    pub fn unsupported(&self, answer: &str) -> Option<String> {
        let version = answer
            .split_whitespace()
            .find(|word| word.starts_with(|character: char| character.is_ascii_digit()))?;
        older(version, self.oldest).then(|| {
            let (label, oldest) = (self.label(), self.oldest);
            let kind = match self.kind {
                PluginKind::Provider => "Provider",
                PluginKind::Runtime => "Runtime Adapter",
            };
            format!(
                "{label} {version} is not supported; the {label} {kind} needs {label} {oldest} or later"
            )
        })
    }

    fn manifest(&self) -> &'static str {
        self.files
            .iter()
            .find(|file| file.path == MANIFEST_FILE)
            .expect("a shipped Plugin has a manifest")
            .contents
    }

    /// Whether the product's executable is on `PATH`, read as a shell reads it: a relative
    /// directory from the working directory, and an empty one as the working directory.
    pub fn is_on_path(&self) -> bool {
        std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|directory| {
                let directory = if directory.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    directory
                };
                is_executable(&directory.join(self.command))
            })
        })
    }

    pub fn directory(&self, home: &FeldHome) -> PathBuf {
        home.plugins_dir().join(self.kind.as_str()).join(self.name)
    }

    /// The executable of this Plugin, the binary of the workspace crate
    /// `crates/feld-plugin-<name>`, which `cargo build --workspace` and `cargo install` of
    /// that crate both put beside `feld`.
    pub fn executable(&self, beside: &Path) -> PathBuf {
        beside.join(format!("feld-plugin-{}", self.name))
    }

    /// Writes the Plugin directory with this version's files. Each file is written beside
    /// its place and renamed over it, so that a Session running one of them meanwhile, as
    /// a hook or monitor does, runs either the earlier file or this one.
    pub fn install(&self, home: &FeldHome, beside: &Path) -> Result<PathBuf> {
        let executable = self.executable(beside);
        let directory = self.directory(home);
        home.create_owner_dir(&directory)?;
        for file in self.files {
            let path = directory.join(file.path);
            home.create_owner_dir(path.parent().expect("a Plugin file sits in a directory"))?;
            let temporary = temporary_beside(&path);
            home.write_owner_file(&temporary, file.contents.as_bytes())?;
            if file.runnable {
                make_runnable(&temporary)?;
            }
            replace(&temporary, &path)?;
        }
        let target = directory.join(EXECUTABLE_FILE);
        let temporary = temporary_beside(&target);
        fs::copy(&executable, &temporary)
            .with_context(|| format!("copy {} to {}", executable.display(), temporary.display()))?;
        make_runnable(&temporary)?;
        replace(&temporary, &target)?;
        for extra in self.extras(&directory) {
            let removed = if extra.is_dir() {
                fs::remove_dir_all(&extra)
            } else {
                fs::remove_file(&extra)
            };
            removed.with_context(|| format!("remove {}", extra.display()))?;
        }
        Ok(directory)
    }

    /// What the directory holds beyond this Plugin's files, such as a file an earlier
    /// version shipped. A directory none of this Plugin's files sit in is listed whole.
    fn extras(&self, directory: &Path) -> Vec<PathBuf> {
        let mut extras = Vec::new();
        let mut pending = vec![PathBuf::new()];
        while let Some(relative) = pending.pop() {
            let Ok(entries) = fs::read_dir(directory.join(&relative)) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let path = relative.join(entry.file_name());
                if entry.path().is_dir() {
                    if self
                        .files
                        .iter()
                        .any(|file| Path::new(file.path).starts_with(&path))
                    {
                        pending.push(path);
                    } else {
                        extras.push(entry.path());
                    }
                } else if path != Path::new(EXECUTABLE_FILE)
                    && !self.files.iter().any(|file| Path::new(file.path) == path)
                {
                    extras.push(entry.path());
                }
            }
        }
        extras
    }

    /// Whether the installed directory is this Plugin: its manifest names this Plugin, so
    /// that a directory someone else placed under the same name is left alone.
    fn is_installed(&self, home: &FeldHome) -> bool {
        Manifest::load(&self.directory(home), self.kind).is_ok()
    }

    /// Whether the installed directory already holds this version's files.
    pub fn is_current(&self, home: &FeldHome, beside: &Path) -> bool {
        let directory = self.directory(home);
        let same =
            |path: &Path, contents: &[u8]| fs::read(path).is_ok_and(|found| found == contents);
        self.extras(&directory).is_empty()
            && self
                .files
                .iter()
                .all(|file| same(&directory.join(file.path), file.contents.as_bytes()))
            && fs::read(self.executable(beside))
                .is_ok_and(|executable| same(&directory.join(EXECUTABLE_FILE), &executable))
    }
}

/// Replaces the installed shipped Plugins with this version's, which the daemon does when
/// it starts: the daemon and a Plugin talk in the shape of one version. Only the Plugins
/// `feld init` installed are replaced. Returns what the daemon logs.
pub fn update_installed(home: &FeldHome, beside: &Path) -> Vec<String> {
    let mut log = Vec::new();
    for plugin in BUNDLED {
        if !plugin.is_installed(home) || plugin.is_current(home, beside) {
            continue;
        }
        let executable = plugin.executable(beside);
        if !executable.is_file() {
            log.push(format!(
                "{} is left as it was: {} is not beside feld",
                plugin.directory(home).display(),
                executable.display()
            ));
            continue;
        }
        match plugin.install(home, beside) {
            Ok(directory) => log.push(format!(
                "updated {} to this version of feld",
                directory.display()
            )),
            Err(error) => log.push(format!(
                "{} is left as it was: {error:#}",
                plugin.directory(home).display()
            )),
        }
    }
    log
}

fn temporary_beside(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .expect("a Plugin file has a name")
        .to_string_lossy();
    path.with_file_name(format!(".{name}.tmp-{}", std::process::id()))
}

fn replace(temporary: &Path, path: &Path) -> Result<()> {
    fs::rename(temporary, path).with_context(|| format!("replace {}", path.display()))
}

/// Where `feld init` looks for the shipped Plugin executables.
pub fn executables_directory() -> Result<PathBuf> {
    let feld = std::env::current_exe().context("resolve the running feld executable")?;
    let directory = feld
        .parent()
        .context("the running feld executable has no directory")?;
    Ok(directory.to_path_buf())
}

/// The shipped Plugins of one kind, in the order `feld init` shows them.
pub fn of_kind(kind: PluginKind) -> impl Iterator<Item = &'static BundledPlugin> {
    BUNDLED.iter().filter(move |plugin| plugin.kind == kind)
}

/// The default shipped Plugins of one kind, which `feld init` takes for a kind with
/// nothing of it on `PATH`.
pub fn default_of_kind(kind: PluginKind) -> impl Iterator<Item = &'static BundledPlugin> {
    of_kind(kind).filter(move |plugin| DEFAULTS.contains(&(kind, plugin.name)))
}

/// The shipped Plugins of one kind whose product's executable is on `PATH`.
/// The step that puts the executables of the shipped Plugins `names` beside `feld`, which
/// `feld init` and `feld doctor` both tell.
pub fn reinstall_next(names: &[&str]) -> String {
    format!(
        "reinstall Feld so that `feld` and `feld-plugin-{}` are in the same directory",
        names.join("`, `feld-plugin-")
    )
}

/// Whether `version` is older than `oldest`, both read as numbers between dots, each
/// without what follows its digits (`3.2a` as `3.2`), and a missing number as 0.
fn older(version: &str, oldest: &str) -> bool {
    let numbers = |version: &str| -> Vec<u32> {
        version
            .split('.')
            .map_while(|part| {
                let digits = part
                    .find(|character: char| !character.is_ascii_digit())
                    .map_or(part, |end| &part[..end]);
                digits.parse().ok()
            })
            .collect()
    };
    let (version, oldest) = (numbers(version), numbers(oldest));
    let width = version.len().max(oldest.len());
    let at = |numbers: &[u32], index: usize| numbers.get(index).copied().unwrap_or(0);
    (0..width)
        .map(|index| at(&version, index).cmp(&at(&oldest, index)))
        .find(|order| order.is_ne())
        .is_some_and(std::cmp::Ordering::is_lt)
}

pub fn on_path_of_kind(kind: PluginKind) -> impl Iterator<Item = &'static BundledPlugin> {
    of_kind(kind).filter(|plugin| plugin.is_on_path())
}

pub fn find(kind: PluginKind, name: &str) -> Option<&'static BundledPlugin> {
    of_kind(kind).find(|plugin| plugin.name == name)
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn make_runnable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("make {} runnable", path.display()))?;
    }
    let _ = path;
    Ok(())
}

macro_rules! file {
    ($directory:literal, $path:literal, $runnable:literal) => {
        BundledFile {
            path: $path,
            contents: include_str!(concat!("../../../plugins/", $directory, "/", $path)),
            runnable: $runnable,
        }
    };
}

pub static BUNDLED: &[BundledPlugin] = &[
    BundledPlugin {
        name: "claude-code",
        kind: PluginKind::Runtime,
        command: "claude",
        version_args: &["--version"],
        oldest: "2.1.181",
        files: &[
            file!("runtime/claude-code", "plugin.json", false),
            file!("runtime/claude-code", ".claude-plugin/plugin.json", false),
            file!("runtime/claude-code", "hooks/hooks.json", false),
            file!("runtime/claude-code", "monitors/monitors.json", false),
            file!("runtime/claude-code", "skills/feld/SKILL.md", false),
            file!("runtime/claude-code", "bin/hook", true),
            file!("runtime/claude-code", "bin/monitor", true),
            file!("runtime/claude-code", "bin/otel-headers", true),
            file!("runtime/claude-code", "bin/status-line", true),
        ],
    },
    BundledPlugin {
        name: "codex",
        kind: PluginKind::Runtime,
        command: "codex",
        version_args: &["--version"],
        oldest: "0.156.0",
        files: &[
            file!("runtime/codex", "plugin.json", false),
            file!("runtime/codex", "skills/feld/SKILL.md", false),
            file!("runtime/codex", "bin/hook", true),
            file!("runtime/codex", "bin/queue-helper", true),
        ],
    },
    BundledPlugin {
        name: "tmux",
        kind: PluginKind::Provider,
        command: "tmux",
        version_args: &["-V"],
        oldest: "3.2",
        files: &[file!("provider/tmux", "plugin.json", false)],
    },
    BundledPlugin {
        name: "herdr",
        kind: PluginKind::Provider,
        command: "herdr",
        version_args: &["--version"],
        oldest: "0.9.0",
        files: &[file!("provider/herdr", "plugin.json", false)],
    },
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use super::{BUNDLED, BundledPlugin, find, update_installed};
    use feld_core::home::FeldHome;
    use feld_core::plugin::{DEFAULTS, PluginKind};

    fn repository(plugin: &BundledPlugin) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins")
            .join(plugin.kind.as_str())
            .join(plugin.name)
    }

    fn walk(directory: &Path, prefix: &str, into: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(directory).expect("read a shipped Plugin directory") {
            let entry = entry.expect("read a shipped Plugin entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{prefix}{name}");
            if entry.path().is_dir() {
                walk(&entry.path(), &format!("{path}/"), into);
            } else {
                into.insert(path);
            }
        }
    }

    #[test]
    fn a_product_older_than_the_oldest_is_unsupported() {
        let tmux = find(PluginKind::Provider, "tmux").unwrap();
        assert_eq!(
            tmux.unsupported("tmux 3.1c").as_deref(),
            Some("tmux 3.1c is not supported; the tmux Provider needs tmux 3.2 or later")
        );
        for answer in [
            "tmux 3.2",
            "tmux 3.2a",
            "tmux 3.10",
            "tmux next-3.7",
            "tmux openbsd-7.5",
        ] {
            assert_eq!(tmux.unsupported(answer), None, "{answer}");
        }
        let claude = find(PluginKind::Runtime, "claude-code").unwrap();
        assert_eq!(
            claude.unsupported("2.1.150 (Claude Code)").as_deref(),
            Some(
                "Claude Code 2.1.150 is not supported; the Claude Code Runtime Adapter needs Claude Code 2.1.181 or later"
            )
        );
        assert_eq!(claude.unsupported("2.1.181 (Claude Code)"), None);
        assert_eq!(claude.unsupported("2.2.0 (Claude Code)"), None);
        let codex = find(PluginKind::Runtime, "codex").unwrap();
        assert!(codex.unsupported("codex-cli 0.155.1").is_some());
        assert_eq!(codex.unsupported("codex-cli 0.156.0"), None);
        let herdr = find(PluginKind::Provider, "herdr").unwrap();
        assert!(herdr.unsupported("herdr 0.8.2").is_some());
        assert_eq!(herdr.unsupported("herdr 0.9.0"), None);
    }

    /// The oldest version of each product is the one the table of supported versions names
    /// and the one the minimum configuration of the CI checks with.
    #[test]
    fn the_oldest_versions_are_those_of_the_table_and_the_ci() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let table = std::fs::read_to_string(root.join("docs/spec/design/plugin.md")).unwrap();
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
        for plugin in BUNDLED {
            let row = format!("| {} | {} |", plugin.label(), plugin.oldest);
            assert!(table.contains(&row), "the table has no row {row}");
        }
        // The minimum tmux is the one of Ubuntu 22.04, which the workflow names in a comment.
        for name in ["herdr", "claude-code", "codex"] {
            let plugin = BUNDLED.iter().find(|plugin| plugin.name == name).unwrap();
            let input = ci
                .split_once(&format!("\n      {name}:\n"))
                .and_then(|(_, rest)| {
                    rest.lines()
                        .find_map(|line| line.trim().strip_prefix("default: "))
                })
                .unwrap_or_else(|| panic!("ci.yml has no input {name}"));
            assert_eq!(input, plugin.oldest, "{name}");
        }
        let tmux = BUNDLED.iter().find(|plugin| plugin.name == "tmux").unwrap();
        assert!(ci.contains(&format!("tmux of Ubuntu 22.04 ({}a)", tmux.oldest)));
    }

    #[test]
    fn every_default_plugin_is_shipped() {
        for (kind, name) in DEFAULTS {
            assert!(find(*kind, name).is_some(), "{name} is not shipped");
        }
    }

    /// The embedded files are the Plugin directories of the repository, so that a file
    /// added there reaches `$FELD_HOME/plugins/` too.
    #[test]
    fn every_shipped_plugin_file_is_embedded_as_it_is_in_the_repository() {
        for plugin in BUNDLED {
            let directory = repository(plugin);
            let mut found = BTreeSet::new();
            walk(&directory, "", &mut found);
            let embedded: BTreeSet<String> = plugin
                .files
                .iter()
                .map(|file| file.path.to_owned())
                .collect();
            assert_eq!(
                embedded,
                found,
                "the embedded files of the {} Plugin are not {}",
                plugin.name,
                directory.display()
            );
            for file in plugin.files {
                let path = directory.join(file.path);
                assert_eq!(
                    file.contents,
                    std::fs::read_to_string(&path).expect("read a shipped Plugin file"),
                    "{} is embedded as something else",
                    path.display()
                );
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
                    assert_eq!(
                        file.runnable,
                        mode & 0o111 != 0,
                        "{} is installed with the wrong permissions",
                        path.display()
                    );
                }
            }
            assert!(
                !plugin.label().is_empty(),
                "the {} Plugin has no label",
                plugin.name
            );
        }
    }

    /// The daemon brings the Plugins `feld init` installed to its own version, and leaves
    /// the rest as they are.
    #[test]
    fn the_installed_shipped_plugins_are_brought_to_this_version() {
        let temporary = tempfile::tempdir().unwrap();
        let home = FeldHome::resolve(Some(temporary.path().join("home"))).unwrap();
        let beside = temporary.path().join("bin");
        std::fs::create_dir_all(&beside).unwrap();
        for plugin in BUNDLED {
            std::fs::write(plugin.executable(&beside), "earlier").unwrap();
        }
        let claude = find(PluginKind::Runtime, "claude-code").unwrap();
        let tmux = find(PluginKind::Provider, "tmux").unwrap();
        claude.install(&home, &beside).unwrap();
        tmux.install(&home, &beside).unwrap();
        let hook = claude.directory(&home).join("bin/hook");
        std::fs::write(&hook, "earlier").unwrap();
        let stale = claude.directory(&home).join("bin/from-an-earlier-version");
        std::fs::write(&stale, "earlier").unwrap();
        // A directory under a shipped name whose manifest is someone else's.
        let herdr = find(PluginKind::Provider, "herdr").unwrap();
        std::fs::create_dir_all(herdr.directory(&home)).unwrap();
        std::fs::write(herdr.directory(&home).join("plugin.json"), "{}").unwrap();

        for plugin in BUNDLED {
            std::fs::write(plugin.executable(&beside), "this version").unwrap();
        }
        let hook_before = std::fs::metadata(&hook).unwrap();
        let log = update_installed(&home, &beside);

        assert_eq!(log.len(), 2, "{log:?}");
        for plugin in [claude, tmux] {
            assert_eq!(
                std::fs::read_to_string(plugin.directory(&home).join("plugin")).unwrap(),
                "this version"
            );
        }
        assert_eq!(
            std::fs::read_to_string(&hook).unwrap(),
            claude
                .files
                .iter()
                .find(|file| file.path == "bin/hook")
                .unwrap()
                .contents
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // Renamed over, so that a process holding the earlier file keeps it.
            assert_ne!(std::fs::metadata(&hook).unwrap().ino(), hook_before.ino());
        }
        let _ = hook_before;
        assert!(!stale.exists());
        assert!(
            !find(PluginKind::Runtime, "codex")
                .unwrap()
                .directory(&home)
                .exists()
        );
        assert_eq!(
            std::fs::read_to_string(herdr.directory(&home).join("plugin.json")).unwrap(),
            "{}"
        );
        assert!(!herdr.directory(&home).join("plugin").exists());
        assert!(update_installed(&home, &beside).is_empty());
    }
}
