use serde::{Deserialize, Serialize};

use super::defaults::{default_false, default_true, default_zerotier_service_url};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteConfig {
    #[serde(default)]
    pub transports: RemoteTransportsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteTransportsConfig {
    #[serde(default)]
    pub zerotier: ZeroTierConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZeroTierConfig {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub network_id: Option<String>,
    #[serde(default)]
    pub api_token_key: Option<String>,
    #[serde(default = "default_zerotier_service_url")]
    pub service_url: String,
    #[serde(default = "default_true")]
    pub managed_name_sync: bool,
}

impl Default for ZeroTierConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            network_id: None,
            api_token_key: None,
            service_url: default_zerotier_service_url(),
            managed_name_sync: true,
        }
    }
}
