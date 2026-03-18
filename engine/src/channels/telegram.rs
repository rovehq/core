//! Telegram channel integration.

mod api;
mod approvals;
mod polling;
#[cfg(test)]
mod tests;
mod types;

use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::agent::AgentCore;
use crate::config::metadata::SERVICE_NAME;
use crate::db::Database;
use crate::gateway::Gateway;
use crate::secrets::SecretManager;
use types::TelegramRateLimits;

pub type TaskHandler = Arc<Mutex<AgentCore>>;

#[derive(Clone)]
pub struct TelegramBot {
    token: String,
    allowed_users: Vec<i64>,
    client: Client,
    api_base_url: String,
    agent: Option<TaskHandler>,
    rate_limits: Arc<Mutex<TelegramRateLimits>>,
    confirmation_chat_id: Option<i64>,
    secret_manager: Arc<SecretManager>,
    gateway: Option<Arc<Gateway>>,
    db: Option<Arc<Database>>,
}

impl std::fmt::Debug for TelegramBot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramBot")
            .field("allowed_users", &self.allowed_users)
            .field("agent", &self.agent.as_ref().map(|_| "<AgentCore>"))
            .finish()
    }
}

impl TelegramBot {
    pub fn new(token: String, allowed_users: Vec<i64>) -> Self {
        Self {
            token,
            allowed_users,
            client: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            api_base_url: "https://api.telegram.org".to_string(),
            agent: None,
            rate_limits: Arc::new(Mutex::new(TelegramRateLimits::new())),
            confirmation_chat_id: None,
            secret_manager: Arc::new(SecretManager::new(SERVICE_NAME)),
            gateway: None,
            db: None,
        }
    }

    pub fn with_agent(mut self, agent: TaskHandler) -> Self {
        self.agent = Some(agent);
        self
    }

    pub fn with_gateway(mut self, gateway: Arc<Gateway>, db: Arc<Database>) -> Self {
        self.gateway = Some(gateway);
        self.db = Some(db);
        self
    }

    pub fn with_confirmation_chat(mut self, chat_id: i64) -> Self {
        self.confirmation_chat_id = Some(chat_id);
        self
    }

    pub fn with_api_base_url(mut self, api_base_url: impl Into<String>) -> Self {
        self.api_base_url = api_base_url.into().trim_end_matches('/').to_string();
        self
    }

    pub(super) fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.api_base_url, self.token, method)
    }
}
