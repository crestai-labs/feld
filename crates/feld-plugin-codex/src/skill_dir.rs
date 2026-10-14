//! The `feld` skill this Adapter puts in the configuration root of a Codex a person
//! starts themselves.
//!
//! The skill itself is the file beside this Plugin, copied as it is, and its `bin/` are
//! shims that run the two parts beside Codex through the Plugin of the Feld home they find
//! in their environment, so they are the same whichever daemon wrote them. Codex reads the
//! skill from `$CODEX_HOME/skills/`, and the hook declaration of `$CODEX_HOME/hooks.json`
//! names the same shims.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use feld_core::home::FeldHome;
use feld_core::plugin::sdk::runtime;

/// What the skill takes from this Plugin's directory unchanged.
const SKILL: &str = "skills/feld/SKILL.md";

/// Where the Feld home keeps this Plugin, relative to the home.
const INSTALLED: &str = "plugins/runtime/codex/plugin";

/// The two parts that run beside Codex.
pub const HELPERS: &[&str] = &["hook", "queue-helper"];

/// Writes the skill into `skill`. Without the Plugin in `FELD_HOME` (or else `~/.feld`)
/// the shims do nothing.
pub fn write(home: &FeldHome, source: &Path, skill: &Path) -> Result<()> {
    runtime::write_files(home, skill, files(source)?)
}

/// Every file of the skill, relative to its directory, with its contents.
pub fn files(source: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    let contents = fs::read(source.join(SKILL))
        .with_context(|| format!("read {}", source.join(SKILL).display()))?;
    let mut files = vec![(PathBuf::from("SKILL.md"), contents)];
    for name in HELPERS {
        let script = runtime::installed_shim(INSTALLED, name, "exit 0");
        files.push((Path::new("bin").join(name), script.into_bytes()));
    }
    Ok(files)
}

/// Where one part beside Codex is reached in a written skill.
pub fn helper(skill: &Path, name: &str) -> PathBuf {
    skill.join("bin").join(name)
}
