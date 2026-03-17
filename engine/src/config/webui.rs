use serde::{Deserialize, Serialize};

use super::defaults::default_webui_bind_addr;

/// WebUI server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebUIConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_webui_bind_addr")]
    pub bind_addr: String,
    #[serde(default)]
    pub token: Option<String>,
}

impl Default for WebUIConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: default_webui_bind_addr(),
            token: None,
        }
    }
}
