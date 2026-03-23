use serde::{Deserialize, Serialize};

use super::defaults::default_daemon_profile;

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
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            profile: default_daemon_profile(),
        }
    }
}
