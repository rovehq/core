use serde::{Deserialize, Serialize};

use super::defaults::default_true;

/// Core tool enablement configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    #[serde(default, rename = "tg-controller")]
    pub tg_controller: bool,
    #[serde(default, rename = "ui-server")]
    pub ui_server: bool,
    #[serde(default, rename = "api-server")]
    pub api_server: bool,
}

/// Plugin enablement configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(default = "default_true", rename = "fs-editor")]
    pub fs_editor: bool,
    #[serde(default = "default_true")]
    pub terminal: bool,
    #[serde(default)]
    pub screenshot: bool,
    #[serde(default = "default_true")]
    pub git: bool,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            fs_editor: default_true(),
            terminal: default_true(),
            screenshot: false,
            git: default_true(),
        }
    }
}
