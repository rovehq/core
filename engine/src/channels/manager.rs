use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::agent::AgentCore;
use crate::channels::TelegramBot;
use crate::cli::database_path::database_path;
use crate::config::{metadata::SERVICE_NAME, Config};
use crate::crypto::CryptoModule;
use crate::fs_guard::FileSystemGuard;
use crate::gateway::Gateway;
use crate::runtime::{sdk_plugin_entry_from_installed_plugin, WasmRuntime};
use crate::secrets::SecretManager;
use crate::specs::{allowed_tools, SpecRepository};
use crate::storage::{Database, InstalledPlugin};
use crate::system::workflow_triggers;

#[derive(Debug, Clone, Serialize)]
pub struct ChannelStatus {
    pub name: String,
    pub enabled: bool,
    pub configured: bool,
    pub healthy: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TelegramChannelStatus {
    pub name: String,
    pub enabled: bool,
    pub configured: bool,
    pub token_configured: bool,
    pub can_receive: bool,
    pub allowed_ids: Vec<i64>,
    pub admin_ids: Vec<i64>,
    pub confirmation_chat_id: Option<i64>,
    pub api_base_url: Option<String>,
    pub default_agent_id: Option<String>,
    pub default_agent_name: Option<String>,
    pub doctor: Vec<String>,
    pub approval_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelegramChannelTestResult {
    pub ok: bool,
    pub message: String,
    pub bot_username: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TelegramSetupInput {
    pub token: Option<String>,
    pub allowed_ids: Vec<i64>,
    pub confirmation_chat_id: Option<i64>,
    pub api_base_url: Option<String>,
    pub default_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginChannelStatus {
    pub name: String,
    pub enabled: bool,
    pub configured: bool,
    pub healthy: bool,
    pub summary: String,
    pub tool: String,
    pub trust_tier: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginChannelDeliverInput {
    pub input: String,
    pub session_id: Option<String>,
    pub workspace: Option<String>,
    pub team_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginChannelDeliverResult {
    pub ok: bool,
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workflow_run_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workflow_ids: Vec<String>,
    pub preview: Option<String>,
    pub accepted_input: String,
    pub plugin_output: Value,
}

pub struct ChannelManager {
    config: Config,
}

impl ChannelManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn list(&self) -> Result<Vec<ChannelStatus>> {
        let telegram = self.telegram_status().await?;
        let mut channels = vec![ChannelStatus {
            name: "telegram".to_string(),
            enabled: telegram.enabled,
            configured: telegram.configured,
            healthy: telegram.can_receive,
            summary: if telegram.can_receive {
                format!(
                    "Enabled with default agent {}",
                    telegram
                        .default_agent_name
                        .as_deref()
                        .unwrap_or("unknown-agent")
                )
            } else if telegram.enabled {
                "Enabled but needs setup".to_string()
            } else {
                "Disabled".to_string()
            },
        }];

        channels.extend(
            self.plugin_statuses()
                .await?
                .into_iter()
                .map(|status| ChannelStatus {
                    name: status.name,
                    enabled: status.enabled,
                    configured: status.configured,
                    healthy: status.healthy,
                    summary: status.summary,
                }),
        );

        Ok(channels)
    }

    pub async fn telegram_status(&self) -> Result<TelegramChannelStatus> {
        let token_configured = self.secret_manager().has_secret("telegram_token").await;
        let default_agent = self.default_telegram_agent()?;
        let default_agent_id = default_agent.as_ref().map(|agent| agent.id.clone());
        let default_agent_name = default_agent.as_ref().map(|agent| agent.name.clone());
        let configured = token_configured && default_agent.is_some();

        let mut doctor = Vec::new();
        if !token_configured {
            doctor.push(
                "Telegram token is missing. Run `rove channel telegram setup --token <bot-token>`."
                    .to_string(),
            );
        }
        if default_agent.is_none() {
            doctor.push(
                "No default Telegram handler agent is bound. Run `rove channel telegram setup --agent <agent-id>`."
                    .to_string(),
            );
        }
        if self.config.telegram.allowed_ids.is_empty() {
            doctor.push(
                "No Telegram allow-list is configured. Add one or more `--allow-user <id>` values to restrict who can talk to the bot."
                    .to_string(),
            );
        }
        if !self.config.telegram.enabled {
            doctor.push(
                "Telegram polling is disabled. Run `rove channel telegram enable` after setup."
                    .to_string(),
            );
        } else if !configured {
            doctor.push(
                "Telegram is enabled but cannot receive tasks until both the token and default agent are configured."
                    .to_string(),
            );
        }

        Ok(TelegramChannelStatus {
            name: "telegram".to_string(),
            enabled: self.config.telegram.enabled,
            configured,
            token_configured,
            can_receive: self.config.telegram.enabled && configured,
            allowed_ids: self.config.telegram.allowed_ids.clone(),
            admin_ids: self.config.telegram.admin_ids.clone().unwrap_or_default(),
            confirmation_chat_id: self.config.telegram.confirmation_chat_id,
            api_base_url: self.config.telegram.api_base_url.clone(),
            default_agent_id,
            default_agent_name,
            doctor,
            approval_timeout_secs: 300,
        })
    }

    pub async fn telegram_setup(&self, input: TelegramSetupInput) -> Result<TelegramChannelStatus> {
        let mut config = self.config.clone();

        if let Some(token) = input
            .token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.secret_manager()
                .set_secret("telegram_token", token)
                .await?;
        }
        config.telegram.allowed_ids = input.allowed_ids;
        config.telegram.confirmation_chat_id = input.confirmation_chat_id;
        config.telegram.api_base_url = input
            .api_base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);

        config.save()?;

        if let Some(agent_id) = input
            .default_agent_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.bind_default_telegram_agent(agent_id)?;
        }

        ChannelManager::new(Config::load_or_create()?)
            .telegram_status()
            .await
    }

    pub async fn telegram_set_enabled(&self, enabled: bool) -> Result<TelegramChannelStatus> {
        let mut config = self.config.clone();
        config.telegram.enabled = enabled;
        config.save()?;
        ChannelManager::new(Config::load_or_create()?)
            .telegram_status()
            .await
    }

    pub async fn telegram_test(&self) -> Result<TelegramChannelTestResult> {
        let Some(token) = self.load_telegram_token().await else {
            return Ok(TelegramChannelTestResult {
                ok: false,
                message: "Telegram token is not configured.".to_string(),
                bot_username: None,
            });
        };

        let mut bot = TelegramBot::new(token, self.config.telegram.allowed_ids.clone());
        if let Some(base_url) = self.config.telegram.api_base_url.clone() {
            bot = bot.with_api_base_url(base_url);
        }

        match bot.get_me().await {
            Ok(me) => Ok(TelegramChannelTestResult {
                ok: true,
                message: format!(
                    "Telegram API reachable as @{}",
                    me.username.as_deref().unwrap_or("unknown-bot")
                ),
                bot_username: me.username,
            }),
            Err(error) => Ok(TelegramChannelTestResult {
                ok: false,
                message: error.to_string(),
                bot_username: None,
            }),
        }
    }

    pub async fn plugin_statuses(&self) -> Result<Vec<PluginChannelStatus>> {
        let plugins = self.list_channel_plugins().await?;
        Ok(plugins
            .into_iter()
            .map(|plugin| PluginChannelStatus {
                name: plugin.name.clone(),
                enabled: plugin.enabled,
                configured: plugin.binary_path.is_some(),
                healthy: plugin.enabled && plugin.binary_path.is_some(),
                summary: if plugin.enabled && plugin.binary_path.is_some() {
                    "Channel plugin ready for deliver dispatch".to_string()
                } else if plugin.enabled {
                    "Enabled but missing runtime artifact".to_string()
                } else {
                    "Installed but disabled".to_string()
                },
                tool: "deliver".to_string(),
                trust_tier: plugin.trust_tier,
            })
            .collect())
    }

    pub async fn deliver_plugin(
        &self,
        name: &str,
        input: PluginChannelDeliverInput,
        gateway: Arc<Gateway>,
    ) -> Result<PluginChannelDeliverResult> {
        let plugin = self
            .list_channel_plugins()
            .await?
            .into_iter()
            .find(|plugin| plugin.name == name)
            .with_context(|| format!("Channel plugin '{}' is not installed", name))?;
        if !plugin.enabled {
            return Err(anyhow!(
                "Channel plugin '{}' is installed but disabled",
                name
            ));
        }

        let mut runtime = self.build_channel_runtime(&plugin).await?;
        let payload = serde_json::json!({
            "input": input.input,
            "session_id": input.session_id,
            "workspace": input.workspace,
            "team_id": input.team_id,
        });
        let payload_bytes = serde_json::to_vec(&payload)?;
        let output = runtime
            .call_plugin(&plugin.name, "deliver", &payload_bytes)
            .await
            .with_context(|| format!("Channel plugin '{}' failed during deliver", plugin.name))?;
        let plugin_output: Value = serde_json::from_slice(&output)
            .with_context(|| format!("Channel plugin '{}' returned invalid JSON", plugin.name))?;

        let accepted_input = plugin_output
            .get("task_input")
            .and_then(|value| value.as_str())
            .or_else(|| plugin_output.get("input").and_then(|value| value.as_str()))
            .unwrap_or_else(|| payload["input"].as_str().unwrap_or_default())
            .trim()
            .to_string();
        if accepted_input.is_empty() {
            return Err(anyhow!(
                "Channel plugin '{}' did not provide any task input to submit",
                plugin.name
            ));
        }

        let trigger_targets = workflow_triggers::default_channel_targets(
            plugin_output
                .get("workflow_target")
                .and_then(|value| value.as_str()),
        );
        let db = Database::new(&database_path(&self.config)).await?;
        let repo = SpecRepository::new()?;
        let triggered = workflow_triggers::trigger_matching_workflows(
            &repo,
            &db,
            &self.config,
            &plugin.name,
            &trigger_targets,
            &accepted_input,
        )
        .await?;

        if !triggered.is_empty() {
            return Ok(PluginChannelDeliverResult {
                ok: true,
                task_id: None,
                workflow_run_ids: triggered.iter().map(|run| run.run_id.clone()).collect(),
                workflow_ids: triggered
                    .iter()
                    .map(|run| run.workflow_id.clone())
                    .collect(),
                preview: plugin_output
                    .get("preview")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned),
                accepted_input,
                plugin_output,
            });
        }

        let task_id = gateway
            .submit_channel(
                &plugin.name,
                &accepted_input,
                input.session_id.as_deref(),
                input.workspace.as_deref(),
                input.team_id.as_deref(),
            )
            .await?;

        Ok(PluginChannelDeliverResult {
            ok: true,
            task_id: Some(task_id),
            workflow_run_ids: Vec::new(),
            workflow_ids: Vec::new(),
            preview: plugin_output
                .get("preview")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned),
            accepted_input,
            plugin_output,
        })
    }

    pub fn start_enabled(&self, agent: Arc<RwLock<AgentCore>>) {
        if !self.config.telegram.enabled {
            return;
        }

        let config = self.config.clone();
        tokio::spawn(async move {
            let manager = ChannelManager::new(config.clone());
            let status = match manager.telegram_status().await {
                Ok(status) => status,
                Err(error) => {
                    tracing::warn!("Failed to inspect Telegram status: {}", error);
                    return;
                }
            };

            let Some(token) = manager.load_telegram_token().await else {
                tracing::warn!(
                    "Telegram is enabled but no telegram_token is configured. Run `rove channel telegram setup --token ...`."
                );
                return;
            };

            let mut bot = TelegramBot::new(token, config.telegram.allowed_ids.clone());
            match Database::new(&database_path(&config)).await {
                Ok(db) => {
                    bot = bot.with_database(Arc::new(db));
                }
                Err(error) => {
                    tracing::warn!(
                        "Telegram bot started without database support; workflow triggers and progress updates will be unavailable: {}",
                        error
                    );
                }
            }

            if let Some(agent_spec) = manager.default_telegram_agent().ok().flatten() {
                let execution_profile = sdk::TaskExecutionProfile {
                    agent_id: Some(agent_spec.id.clone()),
                    agent_name: Some(agent_spec.name.clone()),
                    worker_preset_id: None,
                    worker_preset_name: None,
                    purpose: Some(agent_spec.purpose.clone()),
                    instructions: agent_spec.instructions.clone(),
                    allowed_tools: allowed_tools(&agent_spec),
                    output_contract: agent_spec.output_contract.clone(),
                    max_iterations: None,
                };
                bot = bot
                    .with_agent(agent)
                    .with_execution_profile(execution_profile);
            } else {
                tracing::warn!(
                    "Telegram is enabled but no default handler agent is bound. {}",
                    status.doctor.join(" ")
                );
            }

            if let Some(chat_id) = config.telegram.confirmation_chat_id {
                bot = bot.with_confirmation_chat(chat_id);
            }
            if let Some(base_url) = config.telegram.api_base_url {
                bot = bot.with_api_base_url(base_url);
            }

            if let Err(error) = bot.start_polling().await {
                tracing::error!("Telegram polling stopped: {}", error);
            }
        });
    }

    fn default_telegram_agent(&self) -> Result<Option<sdk::AgentSpec>> {
        let repo = SpecRepository::new()?;
        Ok(repo.list_agents()?.into_iter().find(|agent| {
            agent.enabled
                && agent.channels.iter().any(|binding| {
                    binding.enabled
                        && binding.kind.eq_ignore_ascii_case("telegram")
                        && binding.target.as_deref() == Some("default")
                })
        }))
    }

    fn bind_default_telegram_agent(&self, agent_id: &str) -> Result<()> {
        let repo = SpecRepository::new()?;
        let _ = repo.load_agent(agent_id)?;
        let mut agents = repo.list_agents()?;

        for agent in &mut agents {
            agent.channels.retain(|binding| {
                !(binding.kind.eq_ignore_ascii_case("telegram")
                    && binding.target.as_deref() == Some("default"))
            });

            if agent.id == agent_id {
                agent.channels.push(sdk::ChannelBinding {
                    kind: "telegram".to_string(),
                    target: Some("default".to_string()),
                    enabled: true,
                    provenance: None,
                });
            }
            repo.save_agent(agent)?;
        }

        Ok(())
    }

    async fn load_telegram_token(&self) -> Option<String> {
        self.secret_manager()
            .lookup_secret("telegram_token")
            .await
            .map(|(token, _)| token)
    }

    fn secret_manager(&self) -> SecretManager {
        SecretManager::new(SERVICE_NAME)
    }

    async fn list_channel_plugins(&self) -> Result<Vec<InstalledPlugin>> {
        let db = Database::new(&database_path(&self.config)).await?;
        Ok(db
            .installed_plugins()
            .list_plugins()
            .await?
            .into_iter()
            .filter(|plugin| plugin.plugin_type == "Channel")
            .collect())
    }

    async fn build_channel_runtime(&self, plugin: &InstalledPlugin) -> Result<WasmRuntime> {
        let entry = sdk_plugin_entry_from_installed_plugin(plugin).ok_or_else(|| {
            anyhow!(
                "Installed plugin '{}' is not a WASM channel plugin",
                plugin.name
            )
        })?;
        let manifest = sdk::manifest::Manifest {
            version: "1.0.0".to_string(),
            team_public_key: String::new(),
            signature: String::new(),
            generated_at: String::new(),
            core_tools: Vec::new(),
            plugins: vec![entry],
        };
        let crypto = Arc::new(CryptoModule::new()?);
        let fs_guard = Arc::new(FileSystemGuard::new(self.config.core.workspace.clone())?);
        Ok(WasmRuntime::new(manifest, crypto, fs_guard))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::ChannelManager;
    use crate::cli::database_path::database_path;
    use crate::config::Config;
    use crate::storage::{Database, InstalledPlugin};

    fn sample_channel_plugin(name: &str, enabled: bool) -> InstalledPlugin {
        InstalledPlugin {
            id: name.to_string(),
            name: name.to_string(),
            version: "0.1.0".to_string(),
            plugin_type: "Channel".to_string(),
            trust_tier: 1,
            manifest: format!(
                r#"{{
                    "name": "{name}",
                    "version": "0.1.0",
                    "sdk_version": "0.1.0",
                    "plugin_type": "Channel",
                    "permissions": {{
                        "filesystem": [],
                        "network": ["api.example.com"],
                        "memory_read": false,
                        "memory_write": false,
                        "tools": []
                    }},
                    "trust_tier": "Reviewed",
                    "min_model": null,
                    "description": "channel plugin"
                }}"#
            ),
            binary_path: Some(format!("{name}.wasm")),
            binary_hash: "hash".to_string(),
            signature: "sig".to_string(),
            enabled,
            installed_at: 0,
            last_used: None,
            config: None,
            provenance_source: None,
            provenance_registry: None,
            catalog_trust_badge: None,
        }
    }

    #[tokio::test]
    async fn plugin_statuses_include_installed_channel_plugins() {
        let workspace = TempDir::new().expect("workspace");
        let data = TempDir::new().expect("data");
        let mut config = Config::default();
        config.core.workspace = workspace.path().to_path_buf();
        config.core.data_dir = data.path().to_path_buf();
        std::fs::create_dir_all(&config.core.workspace).expect("workspace dir");
        std::fs::create_dir_all(&config.core.data_dir).expect("data dir");

        let db = Database::new(&database_path(&config))
            .await
            .expect("database");
        db.installed_plugins()
            .upsert_plugin(&sample_channel_plugin("echo-channel", true))
            .await
            .expect("upsert");

        let statuses = ChannelManager::new(config)
            .plugin_statuses()
            .await
            .expect("statuses");

        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].name, "echo-channel");
        assert!(statuses[0].enabled);
        assert!(statuses[0].healthy);
    }
}
