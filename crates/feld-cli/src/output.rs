//! What every command prints with: `--json`, the error line, tables, and the lines of a
//! block.

use std::fmt;
use std::io::{self, Write};

use anyhow::{Context as _, Result};
use feld_core::time;
use serde::Serialize;

use super::Context;
use super::style::Style;

impl Context {
    /// `--json` prints the value as it is; a person reads what `human` prints of it.
    pub fn print<T: Serialize + ?Sized>(&self, value: &T, human: impl FnOnce(&T)) -> Result<()> {
        if self.json {
            print_json(value)
        } else {
            human(value);
            Ok(())
        }
    }

    /// A committed timestamp as a person reads it: the same instant in the daemon's local
    /// timezone, to the second. `--json` keeps the committed value.
    pub fn local_time(&self, value: &str) -> String {
        time::parse(value).map_or_else(
            |_| value.to_owned(),
            |instant| {
                instant
                    .with_timezone(&self.timezone())
                    .format("%Y-%m-%dT%H:%M:%S%:z")
                    .to_string()
            },
        )
    }

    /// [`Context::local_time`] for a timestamp a record may leave out, which reads as `-`.
    pub fn local_time_or_dash(&self, value: Option<&str>) -> String {
        value.map_or_else(|| "-".to_owned(), |value| self.local_time(value))
    }

    /// What was observed, and when: `kind (time)`.
    pub fn observed(&self, kind: &str, at: &str) -> String {
        format!("{kind} ({})", self.local_time(at))
    }
}

pub fn print_json(value: &(impl Serialize + ?Sized)) -> Result<()> {
    write_json(serde_json::to_vec_pretty(value)?)
}

/// One value as one line of JSON, written out at once so that the reader of a pipe has it
/// before the next poll: what `--follow --json` prints for each record or entry.
pub fn print_json_line(value: &(impl Serialize + ?Sized)) -> Result<()> {
    write_json(serde_json::to_vec(value)?)
}

fn write_json(mut output: Vec<u8>) -> Result<()> {
    output.push(b'\n');
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    end_if_stdout_closed(stdout.write_all(&output)).context("write JSON output")?;
    end_if_stdout_closed(stdout.flush()).context("flush JSON output")?;
    Ok(())
}

/// What `println!` writes. A reader that stops early (`feld ... | head`) has
/// read all it wants, so the command ends with 0 and says nothing more.
pub fn write_stdout(arguments: fmt::Arguments) {
    if let Err(error) = end_if_stdout_closed(io::stdout().write_fmt(arguments)) {
        panic!("failed printing to stdout: {error}");
    }
}

/// Ends the command with 0 when a write to stdout found the reader gone.
fn end_if_stdout_closed<T>(result: io::Result<T>) -> io::Result<T> {
    if result
        .as_ref()
        .is_err_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
    {
        std::process::exit(0);
    }
    result
}

/// The error a command ends with: its cause, and the step a person takes next when there
/// is one. The step follows the cause's first line, ahead of the lines that detail it.
pub fn print_error(json: bool, kind: &str, message: &str, next: Option<&str>) {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({
                "error": {
                    "kind": kind,
                    "message": message,
                    "next": next,
                }
            })
        );
    } else {
        let (cause, details) = message.split_once('\n').unwrap_or((message, ""));
        eprintln!("Error: {cause}");
        if let Some(next) = next {
            eprintln!("next: {next}");
        }
        if !details.is_empty() {
            eprintln!("{details}");
        }
    }
}

/// A line saying what a command did, marked as done when the output is marked.
pub fn print_done(style: &Style, headline: &str) {
    if style.is_marked() {
        println!("{} {headline}", style.ok(style.success()));
    } else {
        println!("{headline}");
    }
}

/// A name with its kind: `name (kind)`.
pub fn kinded(name: &str, kind: impl std::fmt::Display) -> String {
    format!("{name} ({kind})")
}

/// One line of an Actor, Session, or Message block.
pub fn change_line(label: &str, value: &str) {
    println!("  {label:<14}{value}");
}

/// One line of a `feld session get` block.
pub fn detail_line(label: &str, value: &str) {
    println!("  {label:<18}{value}");
}

/// A listing padded with spaces so that the columns line up.
pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let mut widths = Vec::new();
    widen_columns(&mut widths, headers.iter().copied());
    for row in rows {
        widen_columns(&mut widths, row.iter().map(String::as_str));
    }
    println!("{}", table_row(&widths, headers.iter().copied()));
    for row in rows {
        println!("{}", table_row(&widths, row.iter().map(String::as_str)));
    }
}

/// [`print_table`], or `empty` when there are no rows.
pub fn print_table_or(empty: &str, headers: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() {
        println!("{empty}");
    } else {
        print_table(headers, rows);
    }
}

/// Grows each column to hold `cells`, so that rows printed apart still line up.
pub fn widen_columns<'a>(widths: &mut Vec<usize>, cells: impl Iterator<Item = &'a str>) {
    for (index, cell) in cells.enumerate() {
        let width = cell.chars().count();
        match widths.get_mut(index) {
            Some(current) => *current = (*current).max(width),
            None => widths.push(width),
        }
    }
}

pub fn table_row<'a>(widths: &[usize], cells: impl Iterator<Item = &'a str>) -> String {
    let mut line = String::new();
    for (cell, width) in cells.zip(widths) {
        line.push_str(cell);
        line.extend(std::iter::repeat_n(' ', width + 2 - cell.chars().count()));
    }
    line.trim_end().to_owned()
}

/// The names of the installed Plugins of one kind on one line, or `(none)`.
pub fn listed(names: Vec<String>) -> String {
    if names.is_empty() {
        "(none)".to_owned()
    } else {
        names.join(", ")
    }
}
