use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::defaults::{default_data_dir, default_log_level, default_true};
use super::agent::AgentConfig;
use super::brain::BrainsConfig;
use super::gateway::GatewayFileConfig;
use super::llm::LLMConfig;
use super::memory::MemoryConfig;
use super::security::SecurityConfig;
use super::steering::SteeringConfig;
use super::tools::{PluginsConfig, ToolsConfig};
use super::transport::{McpConfig, WsClientConfig};
use super::webui::WebUIConfig;

/// Main configuration structure loaded from `~/.rove/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub core: CoreConfig,
    pub llm: LLMConfig,
    pub tools: ToolsConfig,
    pub plugins: PluginsConfig,
    pub security: SecurityConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub brains: BrainsConfig,
    #[serde(default)]
    pub steering: SteeringConfig,
    #[serde(default)]
    pub ws_client: WsClientConfig,
    #[serde(default)]
    pub gateway: GatewayFileConfig,
    #[serde(default)]
    pub webui: WebUIConfig,
    #[serde(default)]
    pub mcp: McpConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            core: CoreConfig::default(),
            llm: LLMConfig::default(),
            tools: ToolsConfig::default(),
            plugins: PluginsConfig::default(),
            security: SecurityConfig::default(),
            agent: AgentConfig::default(),
            memory: MemoryConfig::default(),
            brains: BrainsConfig::default(),
            steering: SteeringConfig::default(),
            ws_client: WsClientConfig::default(),
            gateway: GatewayFileConfig::default(),
            webui: WebUIConfig::default(),
            mcp: McpConfig::default(),
        }
    }
}

/// Core engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    /// Workspace directory path (supports `~` expansion).
    pub workspace: PathBuf,
    /// Log level (`error`, `warn`, `info`, `debug`, `trace`).
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Enable auto-sync.
    #[serde(default = "default_true")]
    pub auto_sync: bool,
    /// Data directory path (supports `~` expansion).
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            workspace: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("projects"),
            log_level: default_log_level(),
            auto_sync: default_true(),
            data_dir: default_data_dir(),
        }
    }
}
