use serde::{Deserialize, Serialize};

/// Gateway configuration loaded from config.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayFileConfig {
    /// Polling interval in milliseconds.
    pub poll_interval_ms: Option<u64>,
    /// Maximum tasks to fetch per poll.
    pub poll_limit: Option<i64>,
    /// Optional CLI password for local authentication.
    pub cli_password: Option<String>,
}
