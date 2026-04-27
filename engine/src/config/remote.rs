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
    #[serde(default)]
    pub iroh: IrohConfig,
}

/// Configuration for iroh QUIC/UDP-hole-punch transport.
/// Iroh is the recommended zero-config cross-NAT transport — no OS-level VPN,
/// no kernel tap, no VPN icon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrohConfig {
    /// Whether the iroh transport is enabled (default: true).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Custom relay server URL. None = iroh public relay (free).
    #[serde(default)]
    pub relay_url: Option<String>,
    /// Relay mode: "auto" | "relay_only" | "disabled"
    #[serde(default = "default_relay_mode")]
    pub relay_mode: String,
}

impl Default for IrohConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            relay_url: None,
            relay_mode: default_relay_mode(),
        }
    }
}

fn default_relay_mode() -> String {
    "auto".to_string()
}

/// ZeroTier is opt-in. Use iroh for zero-config cross-NAT instead.
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
