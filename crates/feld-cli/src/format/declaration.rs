//! A declaration file against its Workspace: the changes `feld diff` shows and `feld apply`
//! made.

use feld_core::model::FieldChange;
use serde_json::Value;

use crate::commands::apply::{Action, Report};
use crate::output::kinded;

pub fn print_changes(report: &Report<'_>) {
    if report.changes.is_empty() {
        println!("{}", no_changes(report.name, report.workspace));
    } else {
        println!(
            "Declaration file {} · Workspace {}",
            report.name, report.workspace
        );
        for change in &report.changes {
            println!(
                "{} {}",
                change.action.mark(),
                kinded(&change.actor, change.kind)
            );
            // A new Actor lists only what it declares.
            let declared = |field: &&FieldChange| {
                change.action != Action::Create
                    || !matches!(&field.after, Some(Value::Null))
                        && field
                            .after
                            .as_ref()
                            .and_then(Value::as_array)
                            .is_none_or(|items| !items.is_empty())
            };
            for field in change.fields.iter().filter(declared) {
                println!(
                    "    {}: {}",
                    field.field,
                    field_change(change.action, field)
                );
            }
        }
    }
    print_prunable(report);
}

pub fn print_applied(report: &Report<'_>) {
    if report.changes.is_empty() {
        println!("{}", no_changes(report.name, report.workspace));
    } else {
        println!(
            "Declaration file {} applied to Workspace {}.",
            report.name, report.workspace
        );
        for change in &report.changes {
            let done = match change.action {
                Action::Create => "created",
                Action::Replace => "replaced",
                Action::Delete => "deleted",
            };
            println!(
                "  {} {:<30} {done}",
                change.action.mark(),
                kinded(&change.actor, change.kind)
            );
        }
    }
    print_prunable(report);
}

/// The Actors of the file it no longer declares, which stay without `--prune`.
fn print_prunable(report: &Report<'_>) {
    if report.prunable.is_empty() {
        return;
    }
    println!(
        "Declared by {} before and not in the file now; --prune deletes them:",
        report.name
    );
    for item in &report.prunable {
        println!("  {}", kinded(&item.actor, item.kind));
    }
}

fn no_changes(name: &str, workspace: &str) -> String {
    format!("No changes: Workspace {workspace} matches declaration file {name}.")
}

/// One field as it changes. An `env` value can be a secret, so only `--json` shows it.
fn field_change(action: Action, field: &FieldChange) -> String {
    if field.field.starts_with("env.") {
        return match (&field.before, &field.after) {
            (None, _) => "set",
            (_, None) => "removed",
            _ => "changed",
        }
        .to_owned();
    }
    match action {
        Action::Create => value(field.after.as_ref()),
        _ => format!(
            "{} -> {}",
            value(field.before.as_ref()),
            value(field.after.as_ref())
        ),
    }
}

fn value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "-".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(value) => value.to_string(),
    }
}
