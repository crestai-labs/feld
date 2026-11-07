//! The env file a launchSpec names: `KEY=VALUE` lines the daemon reads at each launch.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::plugin::sdk::provider::valid_env_name;

/// Reads the variables of the env file at `path`.
pub fn read(path: &Path) -> Result<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read env file {}", path.display()))?;
    parse(&text).with_context(|| format!("parse env file {}", path.display()))
}

/// The variables of an env file. Each line is blank, a `#` comment, or `KEY=VALUE` with an
/// optional `export ` before it. A value in single or double quotes is taken as written between
/// them; an unquoted value is trimmed. A later line wins over an earlier one of the same name.
pub fn parse(text: &str) -> Result<BTreeMap<String, String>> {
    let mut variables = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else {
            bail!("line {} is not KEY=VALUE", index + 1);
        };
        let name = name.trim();
        if !valid_env_name(name) {
            bail!(
                "line {}: env name '{name}' must be letters, digits, and '_', not starting with a digit",
                index + 1
            );
        }
        let value = value.trim();
        let value = ['"', '\'']
            .into_iter()
            .find_map(|quote| {
                value
                    .strip_prefix(quote)
                    .and_then(|rest| rest.strip_suffix(quote))
            })
            .unwrap_or(value);
        variables.insert(name.to_owned(), value.to_owned());
    }
    Ok(variables)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn reads_assignments_comments_exports_and_quotes() {
        let variables = parse(
            "# secrets\n\nGH_TOKEN=abc\nexport SLACK = 'x y'\nQUOTED=\"a=b\"\nGH_TOKEN=def\n",
        )
        .expect("parse");
        assert_eq!(variables["GH_TOKEN"], "def");
        assert_eq!(variables["SLACK"], "x y");
        assert_eq!(variables["QUOTED"], "a=b");
        assert_eq!(variables.len(), 3);
    }

    #[test]
    fn refuses_a_line_without_an_assignment_or_with_a_bad_name() {
        assert!(parse("TOKEN").is_err());
        assert!(parse("1TOKEN=x").is_err());
    }
}
