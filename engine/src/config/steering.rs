//! Compatibility shim for the legacy `config::steering` module path.
//!
//! New code should use [`crate::config::policy`].

pub use super::policy::*;
