use serde::{Deserialize, Serialize};

use super::defaults::{default_daemon_profile, default_false};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DaemonProfile {
    #[default]
    Desktop,
    Headless,
}

impl DaemonProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Headless => "headless",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_daemon_profile")]
    pub profile: DaemonProfile,
    #[serde(default = "default_false")]
    pub developer_mode: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            profile: default_daemon_profile(),
            developer_mode: default_false(),
        }
    }
}
