//! Telegram channel integration.

pub mod api;
pub mod approvals;
pub mod polling;
pub mod types;

use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

use crate::agent::AgentCore;
use crate::config::metadata::SERVICE_NAME;
use crate::secrets::SecretManager;
use crate::storage::Database;
use types::TelegramRateLimits;

pub type TaskHandler = Arc<RwLock<AgentCore>>;

#[derive(Clone)]
pub struct TelegramBot {
    pub token: String,
    pub allowed_users: Vec<i64>,
    pub admin_users: Vec<i64>,
    client: Client,
    api_base_url: String,
    pub agent: Option<TaskHandler>,
    execution_profile: Option<sdk::TaskExecutionProfile>,
    rate_limits: Arc<Mutex<TelegramRateLimits>>,
    pub confirmation_chat_id: Option<i64>,
    secret_manager: Arc<SecretManager>,
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
            admin_users: Vec::new(),
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            api_base_url: "https://api.telegram.org".to_string(),
            agent: None,
            execution_profile: None,
            rate_limits: Arc::new(Mutex::new(TelegramRateLimits::new())),
            confirmation_chat_id: None,
            secret_manager: Arc::new(SecretManager::new(SERVICE_NAME)),
            db: None,
        }
    }

    pub fn with_database(mut self, db: Arc<Database>) -> Self {
        self.db = Some(db);
        self
    }

    pub fn with_admin_users(mut self, admin_ids: Vec<i64>) -> Self {
        self.admin_users = admin_ids;
        self
    }

    pub fn with_agent(mut self, agent: TaskHandler) -> Self {
        self.agent = Some(agent);
        self
    }

    pub fn with_execution_profile(mut self, execution_profile: sdk::TaskExecutionProfile) -> Self {
        self.execution_profile = Some(execution_profile);
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

    pub(super) fn is_admin(&self, user_id: i64) -> bool {
        self.admin_users.contains(&user_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn log_telegram_audit(
        &self,
        event_type: &str,
        telegram_user: i64,
        chat_id: Option<i64>,
        task_id: Option<&str>,
        approval_key: Option<&str>,
        approved: Option<bool>,
        operation: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(ref db) = self.db {
            db.telegram_audit()
                .log(
                    event_type,
                    telegram_user,
                    chat_id,
                    task_id,
                    approval_key,
                    approved,
                    operation,
                )
                .await?;
        }
        Ok(())
    }
}
