use anyhow::Result;
use std::time::Duration;
use tracing::{error, info, warn};

use super::approvals::resolve_approval;
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
            return;
        }

        if let Some(text) = &msg.text {
            info!("Received command from {}: {}", user_id, text);

            if text.starts_with('/') {
                self.handle_command(chat_id, text).await;
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

            if let (Some(gateway), Some(db)) = (self.gateway.clone(), self.db.clone()) {
                let _ = self.send_message(chat_id, "Processing your task...").await;

                let bot = self.clone();
                let confirmation_chat_id = self.confirmation_chat_id;
                let chat_id_for_task = chat_id;
                let text_clone = text.clone();

                tokio::spawn(async move {
                    let task_id = match gateway
                        .submit_telegram(&text_clone, Some(&chat_id_for_task.to_string()))
                        .await
                    {
                        Ok(id) => id,
                        Err(error) => {
                            let _ = bot
                                .send_message(
                                    chat_id_for_task,
                                    &format!("Failed to submit task: {}", error),
                                )
                                .await;
                            return;
                        }
                    };

                    let repo = db.pending_tasks();
                    let tasks = db.tasks();
                    loop {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        if let Ok(Some(task)) = repo.get_task(&task_id).await {
                            match task.status {
                                crate::storage::PendingTaskStatus::Done => {
                                    let answer = tasks
                                        .get_latest_answer(&task_id)
                                        .await
                                        .ok()
                                        .flatten()
                                        .unwrap_or_else(|| "Task completed".to_string());
                                    let reply =
                                        format_telegram_reply(bot.secret_manager.scrub(&answer));
                                    let target_chat =
                                        confirmation_chat_id.unwrap_or(chat_id_for_task);
                                    let _ = bot.send_message(target_chat, &reply).await;
                                    break;
                                }
                                crate::storage::PendingTaskStatus::Failed => {
                                    let error_msg =
                                        format!("Task failed: {}", task.error.unwrap_or_default());
                                    let _ = bot.send_message(chat_id_for_task, &error_msg).await;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                });
            } else if let Some(agent) = self.agent.clone() {
                let _ = self.send_message(chat_id, "Processing your task...").await;

                use crate::gateway::Task;
                let task = Task::build_from_telegram(text.as_str(), None);
                let bot = self.clone();
                let confirmation_chat_id = self.confirmation_chat_id;

                tokio::spawn(async move {
                    let mut agent_guard = agent.lock().await;
                    match agent_guard.process_task(task).await {
                        Ok(result) => {
                            let reply =
                                format_telegram_reply(bot.secret_manager.scrub(&result.answer));

                            let target_chat = confirmation_chat_id.unwrap_or(chat_id);
                            if let Err(error) = bot.send_message(target_chat, &reply).await {
                                error!("Failed to send reply to {}: {}", target_chat, error);
                            }
                        }
                        Err(error) => {
                            let error_msg =
                                bot.secret_manager.scrub(&format!("Task failed: {}", error));
                            let _ = bot.send_message(chat_id, &error_msg).await;
                        }
                    }
                });
            } else if let Err(error) = self
                .send_message(chat_id, &format!("Task accepted: {}", text))
                .await
            {
                error!("Failed to send reply to {}: {}", chat_id, error);
            }
        }
    }

    async fn handle_callback_query(&self, cbq: CallbackQuery) {
        let user_id = cbq.from.id;
        if !self.allowed_users.contains(&user_id) && !self.allowed_users.is_empty() {
            info!("telegram: ignoring unlisted user_id={}", user_id);
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

        resolve_approval(op_key, approved).await;

        let ack = if approved {
            "Approved. Agent will proceed."
        } else {
            "Denied. Agent will abort."
        };
        let _ = self.answer_callback_query(&cbq.id, ack).await;

        if let Some(msg) = &cbq.message {
            let status_text = if approved {
                "Operation approved by user."
            } else {
                "Operation denied by user."
            };
            let _ = self.send_message(msg.chat.id, status_text).await;
        }
    }

    async fn handle_command(&self, chat_id: i64, cmd: &str) {
        let reply = match cmd.split_whitespace().next().unwrap_or("") {
            "/start" => "Rove is ready. Send me a task and I'll process it.".to_string(),
            "/status" => "Rove is running.".to_string(),
            "/help" => "Available commands:\n/start  - Initialize bot\n/status - Check bot status\n/help   - Show this help\n\nSend any text to run it as a task.".to_string(),
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
