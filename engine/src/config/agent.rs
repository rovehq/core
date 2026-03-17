use serde::{Deserialize, Serialize};

use super::defaults::{default_confirm_after, default_max_iterations};

/// Agent execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Maximum number of iterations per task (0 = unlimited).
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    /// Number of iterations before asking the user to confirm continuation.
    #[serde(default = "default_confirm_after")]
    pub confirm_after: u32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: default_max_iterations(),
            confirm_after: default_confirm_after(),
        }
    }
}
