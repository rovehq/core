//! Compatibility shim for the legacy `steering` module path.
//!
//! New code should use [`crate::policy`]. This module only re-exports the
//! policy implementation for one compatibility cycle.

pub use crate::policy::{bootstrap_builtins, PolicyEngine, PolicyRecord};

pub type SteeringEngine = crate::policy::PolicyEngine;
pub type Skill = crate::policy::PolicyRecord;

pub mod builtins {
    pub use crate::policy::builtins::*;
}

pub mod loader {
    pub use crate::policy::loader::*;

    pub type SteeringEngine = crate::policy::PolicyEngine;
    pub type Skill = crate::policy::PolicyRecord;
}

pub mod resolver {
    pub use crate::policy::resolver::*;
}

pub mod types {
    pub use crate::policy::types::*;
}
