use anyhow::Result;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};

use super::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use super::TelegramBot;

type ApprovalTx = oneshot::Sender<bool>;

struct PendingEntry {
    tx: ApprovalTx,
    timeout_handle: tokio::task::JoinHandle<()>,
}

struct PendingMap {
    entries: Mutex<HashMap<String, PendingEntry>>,
}

static PENDING_APPROVALS: OnceLock<PendingMap> = OnceLock::new();

fn pending() -> &'static PendingMap {
    PENDING_APPROVALS.get_or_init(|| PendingMap {
        entries: Mutex::new(HashMap::new()),
    })
}

/// Park an approval request that will auto-deny after `timeout_secs`.
pub async fn park_approval(key: String, timeout_secs: u64) -> oneshot::Receiver<bool> {
    let (tx, rx) = oneshot::channel();
    let key_clone = key.clone();

    let timeout_handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
        if let Some(entry) = pending().entries.lock().await.remove(&key_clone) {
            let _ = entry.tx.send(false); // auto-deny on timeout
        }
    });

    pending()
        .entries
        .lock()
        .await
        .insert(key, PendingEntry { tx, timeout_handle });
    rx
}

/// Resolve a pending approval. Returns true if the key was found.
pub async fn resolve_approval(key: &str, approved: bool) -> bool {
    if let Some(entry) = pending().entries.lock().await.remove(key) {
        entry.timeout_handle.abort();
        let _ = entry.tx.send(approved);
        true
    } else {
        false
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

    pub async fn request_tier2_approval(
        &self,
        chat_id: i64,
        user_id: i64,
        tool_name: &str,
        op_key: &str,
        task_id: Option<&str>,
        timeout_secs: u64,
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
                "⚠️ *Tier 2 Risk* — Agent wants to run:\n`{}`\n\nAuto-denies after {}s if no response.",
                tool_name, timeout_secs
            ),
            "parse_mode": "Markdown",
            "reply_markup": keyboard,
        });

        self.client.post(&url).json(&body).send().await?;

        // Audit log: approval requested
        if let Err(e) = self
            .log_telegram_audit(
                "approval_requested",
                user_id,
                Some(chat_id),
                task_id,
                Some(op_key),
                None,
                Some(tool_name),
            )
            .await
        {
            tracing::warn!("Failed to log telegram audit: {}", e);
        }

        Ok(park_approval(op_key.to_string(), timeout_secs).await)
    }

    pub async fn handle_approval_callback(
        &self,
        callback_user_id: i64,
        callback_chat_id: i64,
        approved: bool,
        op_key: &str,
        task_id: Option<&str>,
    ) -> &'static str {
        let resolved = resolve_approval(op_key, approved).await;
        if !resolved {
            return "This approval request has expired.";
        }

        // Audit log
        if let Err(e) = self
            .log_telegram_audit(
                if approved {
                    "approval_granted"
                } else {
                    "approval_denied"
                },
                callback_user_id,
                Some(callback_chat_id),
                task_id,
                Some(op_key),
                Some(approved),
                None,
            )
            .await
        {
            tracing::warn!("Failed to log telegram audit: {}", e);
        }

        if approved {
            "Approved. Agent will proceed."
        } else {
            "Denied. Agent will abort."
        }
    }
}
