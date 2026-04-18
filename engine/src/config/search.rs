use serde::{Deserialize, Serialize};

use super::defaults::{
    default_search_provider, default_search_timeout_secs, default_searxng_base_url,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchProviderKind {
    Disabled,
    Searxng,
}

impl Default for SearchProviderKind {
    fn default() -> Self {
        default_search_provider()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearxngSearchConfig {
    #[serde(default = "default_searxng_base_url")]
    pub base_url: String,
    #[serde(default = "default_search_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for SearxngSearchConfig {
    fn default() -> Self {
        Self {
            base_url: default_searxng_base_url(),
            timeout_secs: default_search_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchConfig {
    #[serde(default)]
    pub provider: SearchProviderKind,
    #[serde(default)]
    pub searxng: SearxngSearchConfig,
}
