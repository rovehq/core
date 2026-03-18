use serde::{Deserialize, Serialize};

use super::defaults::default_false;

/// Telegram channel configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelegramConfig {
    #[serde(default = "default_false")]
    pub enabled: bool,
    #[serde(default)]
    pub allowed_ids: Vec<i64>,
    #[serde(default)]
    pub confirmation_chat_id: Option<i64>,
    #[serde(default)]
    pub api_base_url: Option<String>,
}
