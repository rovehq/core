use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::agent::AgentConfig;
use super::approvals::ApprovalsConfig;
use super::brain::BrainsConfig;
use super::daemon::DaemonConfig;
use super::defaults::{default_data_dir, default_log_level, default_true};
use super::gateway::GatewayFileConfig;
use super::llm::LLMConfig;
use super::memory::MemoryConfig;
use super::security::SecurityConfig;
use super::secrets::SecretsConfig;
use super::policy::PolicyConfig;
use super::remote::RemoteConfig;
use super::telegram::TelegramConfig;
use super::tools::{PluginsConfig, ToolsConfig};
use super::transport::{McpConfig, WsClientConfig};
use super::webui::WebUIConfig;

/// Main configuration structure loaded from `~/.rove/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub core: CoreConfig,
    #[serde(default)]
    pub approvals: ApprovalsConfig,
    #[serde(default)]
    pub llm: LLMConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub brains: BrainsConfig,
    #[serde(default, alias = "steering")]
    pub policy: PolicyConfig,
    #[serde(default)]
    pub secrets: SecretsConfig,
    #[serde(default)]
    pub ws_client: WsClientConfig,
    #[serde(default)]
    pub remote: RemoteConfig,
    #[serde(default)]
    pub gateway: GatewayFileConfig,
    #[serde(default)]
    pub webui: WebUIConfig,
    #[serde(default)]
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub mcp: McpConfig,
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
