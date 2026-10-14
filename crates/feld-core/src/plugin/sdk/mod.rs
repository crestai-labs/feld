//! What the Plugin executables Feld ships share: the parts of `docs/spec/design/plugin.md`
//! that are the same whatever product a Plugin knows.
//!
//! A Plugin binary is the product-specific rest. The parts that run beside a Runtime share
//! [`crate::self_api`] besides.

pub mod env;
pub mod gauges;
pub mod io;
pub mod launch_env;
pub mod provider;
pub mod runtime;
pub mod transcript;
