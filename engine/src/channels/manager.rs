use std::sync::Arc;

use serde::Serialize;

use crate::channels::TelegramBot;
use crate::config::Config;
use crate::gateway::Gateway;
use crate::secrets::SecretManager;
use crate::storage::Database;

#[derive(Debug, Clone, Serialize)]
pub struct ChannelStatus {
    pub name: String,
    pub enabled: bool,
    pub configured: bool,
}

pub struct ChannelManager {
    config: Config,
}

impl ChannelManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn list(&self) -> Vec<ChannelStatus> {
        vec![ChannelStatus {
            name: "telegram".to_string(),
            enabled: self.config.telegram.enabled,
            configured: !self.config.telegram.allowed_ids.is_empty()
                || self.config.telegram.confirmation_chat_id.is_some(),
        }]
    }

    pub fn start_enabled(&self, gateway: Arc<Gateway>, database: Arc<Database>) {
        if !self.config.telegram.enabled {
            return;
        }

        let config = self.config.clone();
        tokio::spawn(async move {
            let secret_manager = SecretManager::new(crate::config::metadata::SERVICE_NAME);
            if !secret_manager.has_secret("telegram_token").await {
                tracing::warn!(
                    "Telegram is enabled but no telegram_token is configured. Run `rove secrets set telegram`."
                );
                return;
            }

            let token = match secret_manager.get_secret("telegram_token").await {
                Ok(token) => token,
                Err(error) => {
                    tracing::warn!("Failed to load telegram token: {}", error);
                    return;
                }
            };

            let mut bot = TelegramBot::new(token, config.telegram.allowed_ids.clone())
                .with_gateway(gateway, database);
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
}
