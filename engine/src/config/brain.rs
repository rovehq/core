use serde::{Deserialize, Serialize};

use super::defaults::{default_fallback_provider, default_ram_limit, default_true};

/// Brains configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainsConfig {
    /// Enable brains feature.
    #[serde(default)]
    pub enabled: bool,
    /// RAM limit in MB.
    #[serde(default = "default_ram_limit")]
    pub ram_limit_mb: u64,
    /// Fallback provider when brains are unavailable.
    #[serde(default = "default_fallback_provider")]
    pub fallback: String,
    /// Auto-unload unused brains.
    #[serde(default = "default_true")]
    pub auto_unload: bool,
    /// ID of an installed brain plugin to use as the active inference backend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_backend: Option<String>,
}

impl Default for BrainsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ram_limit_mb: default_ram_limit(),
            fallback: default_fallback_provider(),
            auto_unload: default_true(),
            plugin_backend: None,
        }
    }
}
