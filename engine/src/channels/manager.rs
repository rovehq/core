use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use tokio::sync::RwLock;

use crate::agent::AgentCore;
use crate::channels::TelegramBot;
use crate::config::{metadata::SERVICE_NAME, Config};
use crate::secrets::SecretManager;
use crate::specs::{allowed_tools, SpecRepository};

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
    pub confirmation_chat_id: Option<i64>,
    pub api_base_url: Option<String>,
    pub default_agent_id: Option<String>,
    pub default_agent_name: Option<String>,
    pub doctor: Vec<String>,
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

pub struct ChannelManager {
    config: Config,
}

impl ChannelManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn list(&self) -> Result<Vec<ChannelStatus>> {
        let telegram = self.telegram_status().await?;
        Ok(vec![ChannelStatus {
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
        }])
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
            confirmation_chat_id: self.config.telegram.confirmation_chat_id,
            api_base_url: self.config.telegram.api_base_url.clone(),
            default_agent_id,
            default_agent_name,
            doctor,
        })
    }

    pub async fn telegram_setup(&self, input: TelegramSetupInput) -> Result<TelegramChannelStatus> {
        let mut config = self.config.clone();

        if let Some(token) = input.token.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            self.secret_manager().set_secret("telegram_token", token).await?;
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

    pub fn start_enabled(
        &self,
        agent: Arc<RwLock<AgentCore>>,
    ) {
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

            if let Some(agent_spec) = manager.default_telegram_agent().ok().flatten() {
                let execution_profile = sdk::TaskExecutionProfile {
                    agent_id: Some(agent_spec.id.clone()),
                    agent_name: Some(agent_spec.name.clone()),
                    purpose: Some(agent_spec.purpose.clone()),
                    instructions: agent_spec.instructions.clone(),
                    allowed_tools: allowed_tools(&agent_spec),
                    output_contract: agent_spec.output_contract.clone(),
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
                });
            }
            repo.save_agent(agent)?;
        }

        Ok(())
    }

    async fn load_telegram_token(&self) -> Option<String> {
        self.secret_manager().lookup_secret("telegram_token").await.map(|(token, _)| token)
    }

    fn secret_manager(&self) -> SecretManager {
        SecretManager::new(SERVICE_NAME)
    }
}
