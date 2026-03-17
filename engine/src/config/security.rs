use serde::{Deserialize, Serialize};

use super::defaults::{default_max_risk_tier, default_tier1_delay, default_true};

/// Security configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Maximum risk tier allowed (0, 1, or 2).
    #[serde(default = "default_max_risk_tier")]
    pub max_risk_tier: u8,
    /// Require confirmation for Tier 1 operations.
    #[serde(default = "default_true")]
    pub confirm_tier1: bool,
    /// Countdown delay for Tier 1 operations in seconds.
    #[serde(default = "default_tier1_delay")]
    pub confirm_tier1_delay: u64,
    /// Require explicit confirmation for Tier 2 operations.
    #[serde(default = "default_true")]
    pub require_explicit_tier2: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            max_risk_tier: default_max_risk_tier(),
            confirm_tier1: default_true(),
            confirm_tier1_delay: default_tier1_delay(),
            require_explicit_tier2: default_true(),
        }
    }
}
