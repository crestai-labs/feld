//! The Claude Code plugin this Adapter hands Claude Code.
//!
//! Its declarations and skill are the files beside this Plugin, copied as they are, and its
//! `bin/` are shims that run the four parts through this Plugin's executable. An owned
//! Session's shims name this Plugin's executable and carry the Session the helpers address,
//! which core hands the Runtime as a token alone. The shims a person's configuration root
//! holds find the executable in the Feld home of the environment they run in, so they are
//! the same whichever daemon wrote them.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use feld_core::home::FeldHome;
use feld_core::plugin::sdk::runtime;
use feld_core::process::quote;

/// What the Claude Code plugin takes from this Plugin's directory unchanged.
const DECLARATIONS: &[&str] = &[
    ".claude-plugin/plugin.json",
    "hooks/hooks.json",
    "monitors/monitors.json",
    "skills/feld/SKILL.md",
];

/// The four parts that run beside Claude Code.
pub const HELPERS: &[&str] = &["hook", "monitor", "otel-headers", "status-line"];

/// The helpers that address one Session directly, which an owned Runtime's environment
/// names by token alone.
const SESSION_HELPERS: &[&str] = &["hook", "monitor"];

/// Where the Feld home keeps this Plugin, relative to the home.
const INSTALLED: &str = "plugins/runtime/claude-code/plugin";

/// Writes the Claude Code plugin into `plugin`, and answers whether it was not there
/// before. Claude Code loads plugins when it starts, so a first write is what the CLI
/// turns into the `/reload-plugins` note.
pub fn write(home: &FeldHome, source: &Path, plugin: &Path, session: Option<&str>) -> Result<bool> {
    let first = !plugin.join(".claude-plugin/plugin.json").is_file();
    runtime::write_files(home, plugin, files(source, session)?)?;
    Ok(first)
}

/// The files [`write`] writes into a configuration root, relative to the plugin's
/// directory, with their contents.
pub fn root_files(source: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    files(source, None)
}

/// Every file of the Claude Code plugin, relative to its directory, with its contents.
fn files(source: &Path, session: Option<&str>) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut files = Vec::new();
    for declaration in DECLARATIONS {
        let contents = fs::read(source.join(declaration))
            .with_context(|| format!("read {}", source.join(declaration).display()))?;
        files.push((PathBuf::from(declaration), contents));
    }
    let executable = source.join("plugin");
    for name in HELPERS {
        let session_environment = match session.filter(|_| SESSION_HELPERS.contains(name)) {
            Some(session) => format!(
                "FELD_SESSION_ID={}\nexport FELD_SESSION_ID\n",
                quote(session)
            ),
            None => String::new(),
        };
        let script = match session {
            Some(_) => format!(
                "#!/bin/sh\n{session_environment}exec {} helper {name} \"$@\"\n",
                quote(&executable.to_string_lossy()),
            ),
            None => installed_shim(name),
        };
        files.push((Path::new("bin").join(name), script.into_bytes()));
    }
    Ok(files)
}

/// A shim that runs one part through the Plugin of the Feld home it finds in its
/// environment. Without that Plugin the part answers as if Feld were absent: no header,
/// the status line of the command it chains, or else an empty one, and a monitor that
/// waits silently, since Claude Code tells the model whenever a monitor ends.
fn installed_shim(name: &str) -> String {
    let absent = match name {
        "otel-headers" => "echo '{}'; exit 0; ",
        "status-line" => "[ \"$1\" = --chain ] && exec sh -c \"$2\"; echo; exit 0; ",
        "monitor" => "exec sleep 2147483647; ",
        _ => "exit 0; ",
    };
    runtime::installed_shim(INSTALLED, name, &format!("{{ {absent}}}"))
}
