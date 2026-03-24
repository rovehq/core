use anyhow::Result;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};

use super::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use super::TelegramBot;

type ApprovalTx = oneshot::Sender<bool>;
type PendingMap = Mutex<HashMap<String, ApprovalTx>>;

static PENDING_APPROVALS: OnceLock<PendingMap> = OnceLock::new();

fn pending() -> &'static PendingMap {
    PENDING_APPROVALS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn park_approval(key: String) -> oneshot::Receiver<bool> {
    let (tx, rx) = oneshot::channel();
    pending().lock().await.insert(key, tx);
    rx
}

pub(super) async fn resolve_approval(key: &str, approved: bool) {
    if let Some(tx) = pending().lock().await.remove(key) {
        let _ = tx.send(approved);
    }
}

impl TelegramBot {
    #[allow(dead_code)]
    pub async fn send_tier1_countdown(
        &self,
        chat_id: i64,
        operation: &str,
        delay_secs: u64,
    ) -> Result<bool> {
        let msg = format!(
            "Tier 1 operation: {}\nExecuting in {} seconds... Send /cancel to abort.",
            operation, delay_secs
        );
        self.send_message(chat_id, &msg).await?;
        tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        Ok(true)
    }

    #[allow(dead_code)]
    pub async fn request_tier2_approval(
        &self,
        chat_id: i64,
        tool_name: &str,
        op_key: &str,
    ) -> Result<oneshot::Receiver<bool>> {
        let url = self.api_url("sendMessage");

        let keyboard = InlineKeyboardMarkup {
            inline_keyboard: vec![vec![
                InlineKeyboardButton {
                    text: "Approve".to_string(),
                    callback_data: format!("approve:{}", op_key),
                },
                InlineKeyboardButton {
                    text: "Deny".to_string(),
                    callback_data: format!("deny:{}", op_key),
                },
            ]],
        };

        let body = serde_json::json!({
            "chat_id": chat_id,
            "text": format!(
                "⚠️ *Tier 2 Risk* — Agent wants to run:\n`{}`\n\nThis is potentially destructive / irreversible. Approve?",
                tool_name
            ),
            "parse_mode": "Markdown",
            "reply_markup": keyboard,
        });

        self.client.post(&url).json(&body).send().await?;
        Ok(park_approval(op_key.to_string()).await)
    }
}
