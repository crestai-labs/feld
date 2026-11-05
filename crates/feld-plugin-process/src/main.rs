//! The process Execution Provider as a Plugin executable, following
//! `docs/spec/design/provider/process.md`.
//!
//! `feld-plugin-process <operation>` keeps the contract every Plugin keeps
//! (`docs/spec/design/plugin.md`). A launch starts this executable again as the
//! [`supervisor`], which leaves the Plugin's session, runs the command as an OS process of
//! its own, waits for it, and records how it ended in the Placement's state directory. No
//! terminal multiplexer stands between Feld and a Job, so a Job shares no server's working
//! directory, environment, or life with any other Session.

mod backend;
mod supervisor;
#[cfg(test)]
mod tests;

use std::process::ExitCode;

use feld_core::plugin::sdk::provider::serve;

use crate::backend::ProcessProvider;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    if arguments.next().as_deref() == Some(supervisor::OPERATION) {
        return supervisor::run(arguments);
    }
    serve::<ProcessProvider>()
}
