use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::defaults::{default_approval_mode, default_false};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    #[default]
    Default,
    Allowlist,
    Open,
    Assisted,
}

impl ApprovalMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Allowlist => "allowlist",
            Self::Open => "open",
            Self::Assisted => "assisted",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalsConfig {
    #[serde(default = "default_approval_mode")]
    pub mode: ApprovalMode,
    #[serde(default)]
    pub rules_path: Option<PathBuf>,
    #[serde(default = "default_false")]
    pub allow_remote_admin_approvals: bool,
}

impl Default for ApprovalsConfig {
    fn default() -> Self {
        Self {
            mode: default_approval_mode(),
            rules_path: None,
            allow_remote_admin_approvals: false,
        }
    }
}
