//! The `--<provider>-<key> <value>` options, which each Execution Provider defines for itself.

use std::collections::HashSet;
use std::ffi::OsString;

use anyhow::{Result, anyhow};
use clap::CommandFactory;
use serde_json::Value;

use super::args::Cli;

/// A `--<provider>-<key> <value>` option, as the command line gave it.
#[derive(Clone, Debug)]
pub struct ProviderOption {
    pub flag: String,
    pub value: String,
}

/// The commands that declare a Placement, the only ones that read `--<provider>-<key>`.
const PLACEMENT_COMMANDS: [[&str; 2]; 3] = [
    ["actor", "create"],
    ["actor", "update"],
    ["actor", "attach"],
];

/// Take the `--<provider>-<key> <value>` options out of the command line before clap reads
/// it. Feld's own long options stay; in a command that declares a Placement, a hyphenated
/// long option Feld does not define belongs to an Execution Provider, and the prefix says
/// which one. Elsewhere every argument stays, so clap refuses an unknown option as usual.
pub fn split_provider_options(
    arguments: Vec<OsString>,
) -> Result<(Vec<OsString>, Vec<ProviderOption>)> {
    let root = Cli::command();
    let path = subcommand_path(&root, &arguments);
    if !PLACEMENT_COMMANDS.iter().any(|command| path == command) {
        return Ok((arguments, Vec::new()));
    }
    let known = known_long_options(&root);
    let mut kept = Vec::with_capacity(arguments.len());
    let mut options = Vec::new();
    let mut arguments = arguments.into_iter().peekable();
    while let Some(argument) = arguments.next() {
        let Some(text) = argument.to_str() else {
            kept.push(argument);
            continue;
        };
        // Everything past `--` is the program's own argv.
        if text == "--" {
            kept.push(argument);
            kept.extend(arguments);
            break;
        }
        let Some(long) = text.strip_prefix("--") else {
            kept.push(argument);
            continue;
        };
        let (flag, inline) = match long.split_once('=') {
            Some((flag, value)) => (flag, Some(value.to_owned())),
            None => (long, None),
        };
        if known.contains(flag) || !is_provider_flag(flag) {
            kept.push(argument);
            continue;
        }
        // As clap reads a value, an argument that starts with `-` is the next option, so
        // `--tmux-socket --json` lacks a value; such a value is given inline instead.
        let value = match inline {
            Some(value) => value,
            None => arguments
                .next_if(|value| value.to_str().is_some_and(is_value))
                .and_then(|value| value.into_string().ok())
                .ok_or_else(|| {
                    anyhow!("a value is required for '--{flag} <VALUE>' but none was supplied")
                })?,
        };
        options.push(ProviderOption {
            flag: flag.to_owned(),
            value,
        });
    }
    Ok((kept, options))
}

/// The subcommand names the command line walks down to, as clap would resolve them.
fn subcommand_path(root: &clap::Command, arguments: &[OsString]) -> Vec<String> {
    let mut path = Vec::new();
    // Global options are defined on the root, so a lookup also searches the commands above.
    let mut commands = vec![root];
    let mut arguments = arguments.iter().skip(1);
    while let Some(argument) = arguments.next() {
        let Some(text) = argument.to_str() else {
            break;
        };
        if text == "--" {
            break;
        }
        let option = if let Some(long) = text.strip_prefix("--") {
            (!long.contains('=')).then(|| {
                commands.iter().find_map(|command| {
                    command
                        .get_arguments()
                        .find(|argument| argument.get_long() == Some(long))
                })
            })
        } else if let Some(short) = text.strip_prefix('-') {
            let mut characters = short.chars();
            match (characters.next(), characters.next()) {
                (Some(short), None) => Some(commands.iter().find_map(|command| {
                    command
                        .get_arguments()
                        .find(|argument| argument.get_short() == Some(short))
                })),
                _ => None,
            }
        } else {
            let current = commands.last().expect("the root is always there");
            let Some(subcommand) = current.find_subcommand(text) else {
                break;
            };
            path.push(subcommand.get_name().to_owned());
            commands.push(subcommand);
            continue;
        };
        if option
            .flatten()
            .is_some_and(|argument| argument.get_action().takes_values())
        {
            arguments.next();
        }
    }
    path
}

/// The long options Feld defines itself, across every command.
fn known_long_options(root: &clap::Command) -> HashSet<String> {
    fn collect(command: &clap::Command, into: &mut HashSet<String>) {
        for argument in command.get_arguments() {
            if let Some(long) = argument.get_long() {
                into.insert(long.to_owned());
            }
            into.extend(
                argument
                    .get_all_aliases()
                    .into_iter()
                    .flatten()
                    .map(str::to_owned),
            );
        }
        for subcommand in command.get_subcommands() {
            collect(subcommand, into);
        }
    }
    let mut known = HashSet::from(["help".to_owned(), "version".to_owned()]);
    collect(root, &mut known);
    known
}

fn is_value(argument: &str) -> bool {
    argument == "-" || !argument.starts_with('-')
}

fn is_provider_flag(flag: &str) -> bool {
    flag.contains('-')
        && flag.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

/// The `providerOptions` one Provider's `--<provider>-<key>` options make.
pub fn provider_options(provider: &str, options: &[ProviderOption]) -> Result<Option<Value>> {
    if options.is_empty() {
        return Ok(None);
    }
    let prefix = format!("{provider}-");
    let mut resolved = serde_json::Map::new();
    for option in options {
        let key = option.flag.strip_prefix(&prefix).ok_or_else(|| {
            anyhow!(
                "'--{}' is not an option of Execution Provider '{provider}'",
                option.flag
            )
        })?;
        resolved.insert(key.to_owned(), Value::String(option.value.clone()));
    }
    Ok(Some(Value::Object(resolved)))
}
