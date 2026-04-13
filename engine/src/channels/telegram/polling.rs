use anyhow::Result;
use std::time::Duration;
use tracing::{error, info, warn};

use super::types::{CallbackQuery, Message};
use super::TelegramBot;

impl TelegramBot {
    pub async fn start_polling(&self) -> Result<()> {
        info!("Starting Telegram bot long-polling loop...");
        let mut offset = 0;

        loop {
            match self.get_updates(offset).await {
                Ok(updates) => {
                    for update in updates {
                        offset = update.update_id + 1;
                        if let Some(msg) = update.message {
                            self.handle_message(&msg).await;
                        }
                        if let Some(cbq) = update.callback_query {
                            self.handle_callback_query(cbq).await;
                        }
                    }
                }
                Err(error) => {
                    error!("Failed to fetch Telegram updates: {}", error);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn handle_message(&self, msg: &Message) {
        let chat_id = msg.chat.id;

        let user_id = match msg.from.as_ref() {
            Some(user) => user.id,
            None => {
                warn!("Message with no user info - ignoring");
                return;
            }
        };

        if !self.allowed_users.contains(&user_id) && !self.allowed_users.is_empty() {
            info!("telegram: ignoring unlisted user_id={}", user_id);
            let _ = self
                .log_telegram_audit(
                    "unauthorized_message",
                    user_id,
                    Some(chat_id),
                    None,
                    None,
                    None,
                    None,
                )
                .await;
            return;
        }

        if let Some(text) = &msg.text {
            info!("Received from {}: {}", user_id, text);

            if text.starts_with('/') {
                self.handle_command(chat_id, text, user_id).await;
                return;
            }

            let mut limits = self.rate_limits.lock().await;
            if !limits.check_general() {
                let _ = self
                    .send_message(chat_id, "Rate limit exceeded (60/hour). Please wait.")
                    .await;
                return;
            }
            if !limits.check_tier2() {
                let _ = self
                    .send_message(
                        chat_id,
                        "Tier 2 rate limit exceeded (10/10min). Please wait.",
                    )
                    .await;
                return;
            }
            drop(limits);

            let _ = self
                .log_telegram_audit(
                    "task_submitted",
                    user_id,
                    Some(chat_id),
                    None,
                    None,
                    None,
                    Some(&text[..text.len().min(100)]),
                )
                .await;

            if let (Some(agent), Some(execution_profile)) =
                (self.agent.clone(), self.execution_profile.clone())
            {
                let _ = self.send_message(chat_id, "Processing your task...").await;

                use crate::gateway::Task;
                let task = Task::build_from_telegram(text.as_str(), None)
                    .with_execution_profile(execution_profile);
                let bot = self.clone();
                let confirmation_chat_id = self.confirmation_chat_id;

                tokio::spawn(async move {
                    let mut agent_guard = agent.write().await;
                    match agent_guard.process_task(task).await {
                        Ok(result) => {
                            let reply = format_telegram_reply(bot.secret_manager.scrub(&result.answer));
                            let target_chat = confirmation_chat_id.unwrap_or(chat_id);
                            if let Err(error) = bot.send_message(target_chat, &reply).await {
                                error!("Failed to send reply to {}: {}", target_chat, error);
                            }
                        }
                        Err(error) => {
                            let error_msg = bot.secret_manager.scrub(&format!("Task failed: {}", error));
                            let _ = bot.send_message(chat_id, &error_msg).await;
                        }
                    }
                });
            } else if let Err(error) = self.send_message(
                chat_id,
                "Telegram is online, but no default Telegram handler agent is configured. Bind one from `rove channel telegram setup --agent <id>` or the WebUI CHANNELS page.",
            ).await {
                error!("Failed to send reply to {}: {}", chat_id, error);
            }
        }
    }

    async fn handle_callback_query(&self, cbq: CallbackQuery) {
        let user_id = cbq.from.id;
        let chat_id = cbq.message.as_ref().map(|m| m.chat.id).unwrap_or(0);

        if !self.allowed_users.contains(&user_id) && !self.allowed_users.is_empty() {
            info!("telegram callback: ignoring unlisted user_id={}", user_id);
            return;
        }

        let data = match cbq.data.as_deref() {
            Some(data) => data,
            None => return,
        };

        let (approved, op_key) = if let Some(key) = data.strip_prefix("approve:") {
            (true, key)
        } else if let Some(key) = data.strip_prefix("deny:") {
            (false, key)
        } else {
            return;
        };

        let is_admin = self.is_admin(user_id);
        let ack_msg = self
            .handle_approval_callback(user_id, chat_id, approved, op_key, None)
            .await;
        let _ = self.answer_callback_query(&cbq.id, ack_msg).await;

        if let Some(msg) = &cbq.message {
            let status_text = if approved {
                if is_admin {
                    format!("Operation approved by admin {}.", user_id)
                } else {
                    "Operation approved by user.".to_string().to_string()
                }
            } else {
                "Operation denied by user.".to_string()
            };
            let _ = self.send_message(msg.chat.id, &status_text).await;
        }
    }

    async fn handle_command(&self, chat_id: i64, cmd: &str, user_id: i64) {
        let is_admin = self.is_admin(user_id);
        let reply = match cmd.split_whitespace().next().unwrap_or("") {
            "/start" => "Rove is ready. Send me a task and I'll process it.".to_string(),
            "/status" => {
                let role = if is_admin { "admin" } else { "user" };
                format!("Rove is running. Your role: {}", role)
            }
            "/help" => {
                let mut help = "Available commands:\n/start  - Initialize bot\n/status - Check bot status\n/help   - Show this help\n\nSend any text to run it as a task.".to_string();
                if is_admin {
                    help.push_str("\n\nAdmin: you can approve/deny operations for any user.");
                }
                help
            }
            _ => format!("Unknown command: {}", cmd),
        };

        if let Err(error) = self.send_message(chat_id, &reply).await {
            error!("Failed to send command reply: {}", error);
        }
    }
}

fn format_telegram_reply(text: String) -> String {
    if text.len() > 4000 {
        format!("{}...\n\n(truncated)", &text[..4000])
    } else {
        text
    }
}
