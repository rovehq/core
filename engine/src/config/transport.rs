use serde::{Deserialize, Serialize};

use super::defaults::{default_ws_reconnect_delay, default_ws_url};
use crate::runtime::mcp::McpServerConfig;

/// WebSocket client configuration for connecting to an external UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsClientConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ws_url")]
    pub url: String,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default = "default_ws_reconnect_delay")]
    pub reconnect_delay_secs: u64,
}

impl Default for WsClientConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: default_ws_url(),
            auth_token: None,
            reconnect_delay_secs: default_ws_reconnect_delay(),
        }
    }
}

/// MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}
