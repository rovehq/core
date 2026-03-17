use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::defaults::{default_steering_dir, default_true};

/// Steering system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteeringConfig {
    /// Default steering files always active.
    #[serde(default)]
    pub default_skills: Vec<String>,
    /// Allow auto-activation based on task content.
    #[serde(default = "default_true")]
    pub auto_detect: bool,
    /// Directory for steering files (supports `~` expansion).
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
