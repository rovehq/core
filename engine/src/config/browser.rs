use serde::{Deserialize, Serialize};

use super::defaults::{default_false, default_true};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowserProfileMode {
    #[default]
    ManagedLocal,
    AttachExisting,
    RemoteCdp,
}

impl BrowserProfileMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ManagedLocal => "managed_local",
            Self::AttachExisting => "attach_existing",
            Self::RemoteCdp => "remote_cdp",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserApprovalConfig {
    #[serde(default = "default_true")]
    pub require_approval_for_managed_launch: bool,
    #[serde(default = "default_true")]
    pub require_approval_for_existing_session_attach: bool,
    #[serde(default = "default_true")]
    pub require_approval_for_remote_cdp: bool,
}

impl Default for BrowserApprovalConfig {
    fn default() -> Self {
        Self {
            require_approval_for_managed_launch: true,
            require_approval_for_existing_session_attach: true,
            require_approval_for_remote_cdp: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserProfileConfig {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub mode: BrowserProfileMode,
    #[serde(default)]
    pub browser: Option<String>,
    #[serde(default)]
    pub user_data_dir: Option<String>,
    #[serde(default)]
    pub startup_url: Option<String>,
    #[serde(default)]
    pub cdp_url: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl Default for BrowserProfileConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: true,
            backend: None,
            mode: BrowserProfileMode::ManagedLocal,
            browser: None,
            user_data_dir: None,
            startup_url: None,
            cdp_url: None,
            notes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserConfig {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub default_profile_id: Option<String>,
    #[serde(default)]
    pub approvals: BrowserApprovalConfig,
    #[serde(default)]
    pub profiles: Vec<BrowserProfileConfig>,
}
