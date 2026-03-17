use serde::{Deserialize, Serialize};

use super::defaults::{
    default_consolidation_interval_mins, default_episodic_retention_days,
    default_max_session_tokens, default_min_importance_to_inject, default_min_to_consolidate,
    default_query_limit, default_true,
};

/// Memory system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum tokens for short-term session memory.
    #[serde(default = "default_max_session_tokens")]
    pub max_session_tokens: usize,
    /// Default number of days to keep episodic memories if active.
    #[serde(default = "default_episodic_retention_days")]
    pub episodic_retention_days: u32,
    /// Consolidation interval in minutes.
    #[serde(default = "default_consolidation_interval_mins")]
    pub consolidation_interval_mins: u64,
    /// Minimum memories required to trigger consolidation.
    #[serde(default = "default_min_to_consolidate")]
    pub min_to_consolidate: usize,
    /// Maximum results returned by query.
    #[serde(default = "default_query_limit")]
    pub query_limit: u32,
    /// Minimum importance threshold for injection.
    #[serde(default = "default_min_importance_to_inject")]
    pub min_importance_to_inject: f32,
    /// Enable automatic importance decay.
    #[serde(default = "default_true")]
    pub importance_decay_enabled: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_session_tokens: default_max_session_tokens(),
            episodic_retention_days: default_episodic_retention_days(),
            consolidation_interval_mins: default_consolidation_interval_mins(),
            min_to_consolidate: default_min_to_consolidate(),
            query_limit: default_query_limit(),
            min_importance_to_inject: default_min_importance_to_inject(),
            importance_decay_enabled: default_true(),
        }
    }
}
