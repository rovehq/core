use serde::{Deserialize, Serialize};

use super::defaults::{default_confirm_after, default_max_iterations};

/// Controls which LLM handles APEX multi-step planning.
///
/// - `auto`   — use complexity classifier: complex tasks use cloud, simpler ones stay local
/// - `local`  — always plan locally; no prompts sent to cloud providers (maximum privacy)
/// - `cloud`  — always plan with a cloud provider regardless of complexity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPolicy {
    #[default]
    Auto,
    Local,
    Cloud,
}

/// Agent execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Maximum number of iterations per task (0 = unlimited).
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    /// Number of iterations before asking the user to confirm continuation.
    #[serde(default = "default_confirm_after")]
    pub confirm_after: u32,
    /// Controls which LLM handles APEX planning. Defaults to `auto`.
    #[serde(default)]
    pub execution_policy: ExecutionPolicy,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: default_max_iterations(),
            confirm_after: default_confirm_after(),
            execution_policy: ExecutionPolicy::Auto,
        }
    }
}
