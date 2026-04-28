use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct TelegramRateLimits {
    recent_ops: Vec<std::time::Instant>,
    tier2_ops: Vec<std::time::Instant>,
}

impl Default for TelegramRateLimits {
    fn default() -> Self {
        Self::new()
    }
}

impl TelegramRateLimits {
    pub fn new() -> Self {
        Self {
            recent_ops: Vec::new(),
            tier2_ops: Vec::new(),
        }
    }

    pub fn check_general(&mut self) -> bool {
        let now = std::time::Instant::now();
        let one_hour = Duration::from_secs(3600);
        self.recent_ops
            .retain(|time| now.duration_since(*time) < one_hour);
        if self.recent_ops.len() >= 60 {
            return false;
        }
        self.recent_ops.push(now);
        true
    }

    pub fn check_tier2(&mut self) -> bool {
        let now = std::time::Instant::now();
        let ten_min = Duration::from_secs(600);
        self.tier2_ops
            .retain(|time| now.duration_since(*time) < ten_min);
        if self.tier2_ops.len() >= 10 {
            return false;
        }
        self.tier2_ops.push(now);
        true
    }
}

#[derive(Serialize)]
pub struct InlineKeyboardButton {
    pub text: String,
    pub callback_data: String,
}

#[derive(Serialize)]
pub struct InlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

#[derive(Deserialize, Debug, Clone)]
pub(super) struct Update {
    pub(super) update_id: i64,
    pub(super) message: Option<Message>,
    pub(super) callback_query: Option<CallbackQuery>,
}

#[derive(Deserialize, Debug, Clone)]
pub(super) struct Message {
    pub(super) message_id: i64,
    pub(super) chat: Chat,
    pub(super) text: Option<String>,
    pub(super) from: Option<User>,
}

#[derive(Deserialize, Debug, Clone)]
pub(super) struct CallbackQuery {
    pub(super) id: String,
    pub(super) data: Option<String>,
    pub(super) from: User,
    pub(super) message: Option<Message>,
}

#[derive(Deserialize, Debug, Clone)]
pub(super) struct Chat {
    pub(super) id: i64,
}

#[derive(Deserialize, Debug, Clone)]
pub(super) struct User {
    pub(super) id: i64,
}

#[derive(Deserialize, Debug)]
pub(super) struct TelegramApiResponse<T> {
    pub(super) ok: bool,
    pub(super) result: Option<T>,
    pub(super) description: Option<String>,
}

pub(super) type GetUpdatesResponse = TelegramApiResponse<Vec<Update>>;
pub(super) type GetMeResponse = TelegramApiResponse<BotUser>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BotUser {
    pub id: i64,
    pub username: Option<String>,
}
