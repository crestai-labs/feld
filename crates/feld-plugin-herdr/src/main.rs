//! The herdr Execution Provider as a Plugin executable, following
//! `docs/spec/design/provider/herdr.md`.
//!
//! `feld-plugin-herdr <operation>` reads one JSON request from stdin and writes one JSON
//! response to stdout, which is the contract every Plugin keeps
//! (`docs/spec/design/plugin.md`). Exit 0 means this Provider answered, whatever the
//! answer says; a non-zero exit means it could not answer and stderr holds the reason.
//!
//! Everything herdr-specific lives in this crate: the three axes of a Placement, the IDs
//! and the terminal id that tell one resource from another that reused its ID, and the
//! herdr commands that resolve them. Core reaches it only through the operations below,
//! and the `summary` and `attachCommand` an operation answers travel back inside the
//! Placement, so that core can show a stored Placement without running an operation.
//!
//! Nothing here knows which Runtime runs in a pane. `attach` reports the process in the
//! pane's foreground and the Runtime Adapter's `match` decides what it is.

mod backend;
mod commands;
mod placement;
mod snapshot;
#[cfg(test)]
mod tests;

use std::process::ExitCode;

use feld_core::plugin::sdk::provider::serve;

use crate::commands::HerdrProvider;

fn main() -> ExitCode {
    serve::<HerdrProvider>()
}
