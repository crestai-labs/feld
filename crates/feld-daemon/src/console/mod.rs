//! The Console: what a Session's log reads as.
//!
//! A Runtime Session's entries come from its Runtime Adapter, already projected onto the
//! common transcript entry shape `plugin.md` defines (`cursor`, `at`, `kind`, `text`, `subAgent`,
//! `subAgentEnded`, `raw`). Core knows nothing of any Runtime's own line shape: it pages
//! the projected entries, tells apart the states the entries can be in, derives the Sub
//! Agent Sessions from `subAgent` and `subAgentEnded`, and hides tokens. A `program`
//! Session has no Adapter, so its Console is the stdout and stderr the Provider captured.

mod entry;
mod output;
mod redact;
mod sub_agent;
mod transcript;
mod turn;

pub use entry::*;
pub use output::*;
pub use redact::*;
pub use sub_agent::*;
pub use transcript::*;
pub use turn::*;

#[cfg(test)]
mod tests;
