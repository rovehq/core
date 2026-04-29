use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::agent::AgentConfig;
use super::approvals::ApprovalsConfig;
use super::brain::BrainsConfig;
use super::browser::BrowserConfig;
use super::daemon::DaemonConfig;
use super::defaults::{default_data_dir, default_log_level, default_true};
use super::extensions::ExtensionsConfig;
use super::gateway::GatewayFileConfig;
use super::llm::LLMConfig;
use super::memory::MemoryConfig;
use super::metadata::{CONFIG_SCHEMA_VERSION, VERSION};
use super::policy::PolicyConfig;
use super::profile::{LoadoutsConfig, ProfilesConfig};
use super::remote::RemoteConfig;
use super::search::SearchConfig;
use super::secrets::SecretsConfig;
use super::security::SecurityConfig;
use super::telegram::TelegramConfig;
use super::tools::{PluginsConfig, ToolsConfig};
use super::transport::{McpConfig, WsClientConfig};
use super::update::UpdateConfig;
use super::voice::VoiceConfig;
use super::wasm::WasmConfig;
use super::webui::WebUIConfig;

/// Main configuration structure loaded from `~/.rove/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default = "default_config_schema_version")]
    pub config_schema_version: u32,
    #[serde(default = "default_config_written_by")]
    pub config_written_by: String,
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub core: CoreConfig,
    #[serde(default)]
    pub profiles: ProfilesConfig,
    #[serde(default)]
    pub loadouts: LoadoutsConfig,
    #[serde(default)]
    pub approvals: ApprovalsConfig,
    #[serde(default)]
    pub browser: BrowserConfig,
    #[serde(default)]
    pub search: SearchConfig,
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
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub wasm: WasmConfig,
    #[serde(default)]
    pub update: UpdateConfig,
    #[serde(default)]
    pub extensions: ExtensionsConfig,
}

fn default_config_schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}

fn default_config_written_by() -> String {
    VERSION.to_string()
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
