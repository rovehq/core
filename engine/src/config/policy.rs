use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::defaults::{default_policy_dir, default_true};

/// Policy system configuration.
///
/// Historical config keys like `steering`, `skill_dir`, and `default_skills`
/// remain accepted through serde aliases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig {
    /// Default policies always active.
    #[serde(default, alias = "default_skills")]
    pub default_policies: Vec<String>,
    /// Allow auto-activation based on task content.
    #[serde(default = "default_true")]
    pub auto_detect: bool,
    /// Directory for policy files (supports `~` expansion).
    #[serde(default = "default_policy_dir", alias = "skill_dir")]
    pub policy_dir: PathBuf,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            default_policies: Vec::new(),
            auto_detect: default_true(),
            policy_dir: default_policy_dir(),
        }
    }
}

impl PolicyConfig {
    pub fn default_policies(&self) -> &[String] {
        &self.default_policies
    }

    pub fn default_policies_mut(&mut self) -> &mut Vec<String> {
        &mut self.default_policies
    }

    pub fn policy_dir(&self) -> &PathBuf {
        &self.policy_dir
    }

    pub fn policy_dir_mut(&mut self) -> &mut PathBuf {
        &mut self.policy_dir
    }
}
