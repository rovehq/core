use serde::{Deserialize, Serialize};

pub use crate::permission::PluginPermissions;
pub use crate::plugin::{CoreToolEntry, PluginEntry};

/// Main signed manifest structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub team_public_key: String,
    pub signature: String,
    pub generated_at: String,
    pub core_tools: Vec<CoreToolEntry>,
    pub plugins: Vec<PluginEntry>,
}

impl Manifest {
    pub fn get_core_tool(&self, name: &str) -> Option<&CoreToolEntry> {
        self.core_tools.iter().find(|tool| tool.name == name)
    }

    pub fn get_plugin(&self, name: &str) -> Option<&PluginEntry> {
        self.plugins.iter().find(|plugin| plugin.name == name)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}
