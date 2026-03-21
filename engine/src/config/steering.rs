use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::defaults::{default_steering_dir, default_true};

/// Policy system configuration.
///
/// This struct keeps the historical `steering` field name for config
/// compatibility, but the public/control-plane meaning is `policy`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteeringConfig {
    /// Default policies always active.
    #[serde(default)]
    pub default_skills: Vec<String>,
    /// Allow auto-activation based on task content.
    #[serde(default = "default_true")]
    pub auto_detect: bool,
    /// Directory for policy files (supports `~` expansion).
    #[serde(default = "default_steering_dir")]
    pub skill_dir: PathBuf,
}

impl Default for SteeringConfig {
    fn default() -> Self {
        Self {
            default_skills: Vec::new(),
            auto_detect: default_true(),
            skill_dir: default_steering_dir(),
        }
    }
}

impl SteeringConfig {
    pub fn default_policies(&self) -> &[String] {
        &self.default_skills
    }

    pub fn default_policies_mut(&mut self) -> &mut Vec<String> {
        &mut self.default_skills
    }

    pub fn policy_dir(&self) -> &PathBuf {
        &self.skill_dir
    }

    pub fn policy_dir_mut(&mut self) -> &mut PathBuf {
        &mut self.skill_dir
    }
}
