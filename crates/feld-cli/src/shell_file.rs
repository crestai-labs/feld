//! The shell settings files that read `feld env`: the one `feld init` adds the line to,
//! and the ones `feld init` and `feld uninstall` look for it in.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// The line that connects a shell to the daemon of its Feld home.
pub const FELD_ENV_LINE: &str = "eval \"$(feld env)\"";

/// The line above [`FELD_ENV_LINE`] that says Feld added it.
pub const FELD_ENV_COMMENT: &str = "# Added by `feld init`: connects this shell to the Feld daemon";

/// The shell settings files under `HOME` that may read `feld env`.
pub const SETTINGS_FILES: [&str; 5] = [
    ".zprofile",
    ".zshrc",
    ".bash_profile",
    ".bashrc",
    ".profile",
];

/// Whether `path` mentions `feld env` anywhere, as `feld uninstall` looks for the line to
/// name.
pub fn mentions_feld_env(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|text| text.contains("feld env"))
}

/// Whether `path` has a line that runs `eval` on `feld env`, as [`FELD_ENV_LINE`] does:
/// `eval` is the command of the line or of one of its `;`, `&&`, `||` parts, after a `then`,
/// `do` or `else` if any, and `feld env` comes before a comment. A comment, an alias that
/// only holds the line, or another command that names `feld env` does not count.
pub fn reads_feld_env(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|text| text.lines().any(runs_feld_env))
}

fn runs_feld_env(line: &str) -> bool {
    let code = line
        .trim_start()
        .split_once('#')
        .filter(|(before, _)| before.is_empty() || before.ends_with(char::is_whitespace))
        .map_or(line, |(before, _)| before);
    code.split([';', '&', '|']).any(|part| {
        let mut words = part
            .split_whitespace()
            .skip_while(|word| matches!(*word, "then" | "do" | "else"));
        words.next() == Some("eval") && part.contains("feld env")
    })
}

/// The shell of `$SHELL`, as `feld init` adds the `feld env` line for it.
pub struct Shell {
    /// The file an interactive shell reads each time it starts: `.zshrc` under `ZDOTDIR`
    /// or `HOME` for zsh, `~/.bashrc` for bash.
    pub interactive: PathBuf,
    /// The other settings files this shell reads, beside `interactive`.
    others: Vec<PathBuf>,
}

impl Shell {
    /// The file of this shell that reads `feld env` already, `interactive` first.
    pub fn reading_feld_env(&self) -> Option<&Path> {
        std::iter::once(&self.interactive)
            .chain(&self.others)
            .map(PathBuf::as_path)
            .find(|path| reads_feld_env(path))
    }
}

/// The shell of `$SHELL` when it is zsh or bash; any other shell is the error, by name.
pub fn shell() -> std::result::Result<Shell, String> {
    let shell = std::env::var_os("SHELL").unwrap_or_default();
    let name = Path::new(&shell)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let directory = |variable: &str| {
        std::env::var_os(variable)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let unsupported = || {
        if name.is_empty() {
            "the shell".to_owned()
        } else {
            name.clone()
        }
    };
    let home = directory("HOME").ok_or_else(unsupported)?;
    let (interactive, others) = match name.as_str() {
        "zsh" => {
            let root = directory("ZDOTDIR").unwrap_or_else(|| home.clone());
            (root.join(".zshrc"), vec![root.join(".zprofile")])
        }
        "bash" => (
            home.join(".bashrc"),
            vec![home.join(".bash_profile"), home.join(".profile")],
        ),
        _ => return Err(unsupported()),
    };
    Ok(Shell {
        interactive,
        others,
    })
}

/// Adds [`FELD_ENV_COMMENT`] and [`FELD_ENV_LINE`] to the end of `path`, making it if it
/// is not there, with a blank line before them when the file has lines of its own.
pub fn add_feld_env(path: &Path) -> Result<()> {
    let current = fs::read(path).unwrap_or_default();
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory).with_context(|| format!("create {}", directory.display()))?;
    }
    let separator = match current.last() {
        None => "",
        Some(b'\n') => "\n",
        Some(_) => "\n\n",
    };
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| write!(file, "{separator}{FELD_ENV_COMMENT}\n{FELD_ENV_LINE}\n"))
        .with_context(|| format!("add `{FELD_ENV_LINE}` to {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::runs_feld_env;

    #[test]
    fn only_an_eval_that_runs_counts() {
        for runs in [
            "eval \"$(feld env)\"",
            "  eval \"$(feld env)\"  # Feld",
            "command -v feld >/dev/null && eval \"$(feld env)\"",
            "if command -v feld >/dev/null; then eval \"$(feld env)\"; fi",
        ] {
            assert!(runs_feld_env(runs), "{runs}");
        }
        for named in [
            "# eval \"$(feld env)\"",
            "alias feldconnect='eval \"$(feld env)\"'",
            "echo feld env",
            "export PATH=\"$HOME/bin:$PATH\" # eval \"$(feld env)\"",
            "evaluate feld env",
        ] {
            assert!(!runs_feld_env(named), "{named}");
        }
    }
}
