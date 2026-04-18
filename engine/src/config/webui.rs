use serde::{Deserialize, Serialize};

use super::defaults::{
    default_false, default_privacy_mode, default_webui_absolute_timeout_secs,
    default_webui_allowed_origins, default_webui_bind_addr, default_webui_idle_timeout_secs,
    default_webui_reauth_window_secs,
};

/// WebUI server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebUIConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_webui_bind_addr")]
    pub bind_addr: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub password_hash: Option<String>,
    #[serde(default)]
    pub password_integrity: Option<String>,
    #[serde(default)]
    pub recovery_code_hash: Option<String>,
    #[serde(default)]
    pub passkey_user_uuid: Option<String>,
    #[serde(default = "default_webui_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_webui_absolute_timeout_secs")]
    pub absolute_timeout_secs: u64,
    #[serde(default = "default_webui_reauth_window_secs")]
    pub reauth_window_secs: u64,
    #[serde(default = "default_false")]
    pub session_persist_on_restart: bool,
    #[serde(default = "default_webui_allowed_origins")]
    pub allowed_origins: Vec<String>,
    #[serde(default = "default_privacy_mode")]
    pub privacy_mode: String,
}

impl Default for WebUIConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: default_webui_bind_addr(),
            token: None,
            password_hash: None,
            password_integrity: None,
            recovery_code_hash: None,
            passkey_user_uuid: None,
            idle_timeout_secs: default_webui_idle_timeout_secs(),
            absolute_timeout_secs: default_webui_absolute_timeout_secs(),
            reauth_window_secs: default_webui_reauth_window_secs(),
            session_persist_on_restart: false,
            allowed_origins: default_webui_allowed_origins(),
            privacy_mode: default_privacy_mode(),
        }
    }
}
