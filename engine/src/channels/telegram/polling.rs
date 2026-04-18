use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::Config;
use crate::specs::SpecRepository;
use crate::storage::{AgentEvent, Database, TaskStatus};
use crate::system::workflow_triggers;

use super::types::{CallbackQuery, Message};
use super::TelegramBot;

impl TelegramBot {
    pub async fn start_polling(&self) -> Result<()> {
        info!("Starting Telegram bot long-polling loop...");
        let mut offset = 0;
        let mut retry_delay = Duration::from_secs(2);

        loop {
            match self.get_updates(offset).await {
                Ok(updates) => {
                    retry_delay = Duration::from_secs(2);
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
                    error!(
                        "Failed to fetch Telegram updates: {}. Retrying in {}s",
                        error,
                        retry_delay.as_secs()
                    );
                    tokio::time::sleep(retry_delay).await;
                    retry_delay =
                        Duration::from_secs((retry_delay.as_secs().saturating_mul(2)).min(60));
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

            let workflow_targets = vec![
                "default".to_string(),
                format!("chat:{chat_id}"),
                format!("user:{user_id}"),
            ];
            let workflow_matches = match SpecRepository::new().and_then(|repo| {
                workflow_triggers::list_matching_workflows(&repo, "telegram", &workflow_targets)
            }) {
                Ok(matches) => matches,
                Err(error) => {
                    warn!("Failed to resolve Telegram workflow bindings: {}", error);
                    Vec::new()
                }
            };

            if !workflow_matches.is_empty() {
                let _ = self
                    .send_message(chat_id, "Triggering bound workflow...")
                    .await;
                let bot = self.clone();
                let text = text.to_string();
                tokio::spawn(async move {
                    let Some(db) = bot.db.clone() else {
                        let _ = bot
                            .send_message(
                                chat_id,
                                "Telegram workflow trigger failed: no database handle is attached.",
                            )
                            .await;
                        return;
                    };

                    let repo = match SpecRepository::new() {
                        Ok(repo) => repo,
                        Err(error) => {
                            let _ = bot
                                .send_message(
                                    chat_id,
                                    &format!("Telegram workflow trigger failed: {}", error),
                                )
                                .await;
                            return;
                        }
                    };
                    let config = match Config::load_or_create() {
                        Ok(config) => config,
                        Err(error) => {
                            let _ = bot
                                .send_message(
                                    chat_id,
                                    &format!("Telegram workflow trigger failed: {}", error),
                                )
                                .await;
                            return;
                        }
                    };

                    match workflow_triggers::trigger_matching_workflows(
                        &repo,
                        db.as_ref(),
                        &config,
                        "telegram",
                        &workflow_targets,
                        &text,
                    )
                    .await
                    {
                        Ok(results) => {
                            for result in results {
                                let reply = format!(
                                    "{}\n\n[workflow:{} run:{}]",
                                    format_telegram_reply(
                                        bot.secret_manager.scrub(&result.final_output)
                                    ),
                                    result.workflow_id,
                                    result.run_id
                                );
                                if let Err(error) = bot.send_message(chat_id, &reply).await {
                                    error!(
                                        "Failed to send workflow reply to {}: {}",
                                        chat_id, error
                                    );
                                }
                            }
                        }
                        Err(error) => {
                            let _ = bot
                                .send_message(
                                    chat_id,
                                    &bot.secret_manager
                                        .scrub(&format!("Workflow trigger failed: {}", error)),
                                )
                                .await;
                        }
                    }
                });
                return;
            }

            if let (Some(agent), Some(execution_profile)) =
                (self.agent.clone(), self.execution_profile.clone())
            {
                use crate::gateway::Task;
                let task = Task::build_from_telegram(text.as_str(), None)
                    .with_execution_profile(execution_profile);
                let task_id = task.id;
                let bot = self.clone();
                let confirmation_chat_id = self.confirmation_chat_id;
                let progress_message_id = match self
                    .send_message_tracked(
                        chat_id,
                        &format!(
                            "Processing your task...\nStatus: queued\nTask: {}",
                            short_task_id(task_id)
                        ),
                    )
                    .await
                {
                    Ok(message) => Some(message.message_id),
                    Err(error) => {
                        warn!("Failed to create Telegram progress message: {}", error);
                        None
                    }
                };

                tokio::spawn(async move {
                    if let (Some(db), Some(message_id)) = (bot.db.clone(), progress_message_id) {
                        tokio::spawn(run_progress_updates(
                            bot.clone(),
                            db,
                            chat_id,
                            message_id,
                            task_id,
                        ));
                    }

                    let mut agent_guard = agent.write().await;
                    match agent_guard.process_task(task).await {
                        Ok(result) => {
                            if let Some(message_id) = progress_message_id {
                                let target_label = confirmation_chat_id
                                    .filter(|target| *target != chat_id)
                                    .map(|target| format!("Final reply sent to {target}."))
                                    .unwrap_or_else(|| "Final reply ready.".to_string());
                                let _ = bot
                                    .edit_message_text(
                                        chat_id,
                                        message_id,
                                        &format!(
                                            "Task completed.\nStatus: done\nTask: {}\n{}",
                                            short_task_id(task_id),
                                            target_label
                                        ),
                                    )
                                    .await;
                            }

                            let reply =
                                format_telegram_reply(bot.secret_manager.scrub(&result.answer));
                            let target_chat = confirmation_chat_id.unwrap_or(chat_id);
                            if let Err(error) = bot.send_message(target_chat, &reply).await {
                                error!("Failed to send reply to {}: {}", target_chat, error);
                            }
                        }
                        Err(error) => {
                            if let Some(message_id) = progress_message_id {
                                let _ = bot
                                    .edit_message_text(
                                        chat_id,
                                        message_id,
                                        &format!(
                                            "Task failed.\nStatus: failed\nTask: {}",
                                            short_task_id(task_id)
                                        ),
                                    )
                                    .await;
                            }

                            let error_msg =
                                bot.secret_manager.scrub(&format!("Task failed: {}", error));
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

async fn run_progress_updates(
    bot: TelegramBot,
    db: Arc<Database>,
    chat_id: i64,
    message_id: i64,
    task_id: Uuid,
) {
    let task_id_str = task_id.to_string();
    let mut last_rendered = String::new();
    let mut consecutive_failures = 0_u8;

    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;

        let task = match db.tasks().get_task(&task_id).await {
            Ok(Some(task)) => task,
            Ok(None) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                if consecutive_failures >= 3 {
                    warn!(
                        "Stopping Telegram progress updates for task {}: task row never appeared",
                        task_id
                    );
                    return;
                }
                continue;
            }
            Err(error) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                warn!(
                    "Failed to fetch Telegram task status for {}: {}",
                    task_id, error
                );
                if consecutive_failures >= 3 {
                    return;
                }
                continue;
            }
        };

        consecutive_failures = 0;
        let events = match db.tasks().get_agent_events(&task_id_str).await {
            Ok(events) => events,
            Err(error) => {
                warn!(
                    "Failed to fetch Telegram agent events for {}: {}",
                    task_id, error
                );
                Vec::new()
            }
        };

        let progress = render_progress_message(task_id, &task.status, &events);
        if progress != last_rendered {
            if let Err(error) = bot.edit_message_text(chat_id, message_id, &progress).await {
                warn!(
                    "Failed to update Telegram progress message for {}: {}",
                    task_id, error
                );
                return;
            }
            last_rendered = progress;
        }

        if matches!(task.status, TaskStatus::Completed | TaskStatus::Failed) {
            return;
        }
    }
}

fn render_progress_message(task_id: Uuid, status: &TaskStatus, events: &[AgentEvent]) -> String {
    let mut lines = vec![
        "Processing your task...".to_string(),
        format!("Status: {}", telegram_status_label(status)),
        format!("Task: {}", short_task_id(task_id)),
    ];

    let recent = latest_progress_lines(events);
    if !recent.is_empty() {
        lines.push("Recent activity:".to_string());
        for line in recent {
            lines.push(format!("- {line}"));
        }
    }

    format_telegram_reply(lines.join("\n"))
}

fn latest_progress_lines(events: &[AgentEvent]) -> Vec<String> {
    let mut lines = events
        .iter()
        .rev()
        .filter_map(render_progress_event)
        .take(3)
        .collect::<Vec<_>>();
    lines.reverse();
    lines
}

fn render_progress_event(event: &AgentEvent) -> Option<String> {
    match event.event_type.as_str() {
        "thought" => Some(format!("Plan: {}", summarize_telegram_line(&event.payload))),
        "tool_call" => Some(format!("Tool: {}", summarize_telegram_line(&event.payload))),
        "observation" => Some(format!(
            "Observation: {}",
            summarize_telegram_line(&event.payload)
        )),
        "error" => Some(format!(
            "Error: {}",
            summarize_telegram_line(&event.payload)
        )),
        "dag_wave_started" => progress_dag_wave_line(&event.payload),
        "dag_step_started" => progress_dag_step_line("started", &event.payload),
        "dag_step_succeeded" => progress_dag_step_line("done", &event.payload),
        "dag_step_failed" => progress_dag_step_line("failed", &event.payload),
        "dag_step_blocked" => progress_dag_step_line("blocked", &event.payload),
        _ => None,
    }
}

fn progress_dag_wave_line(payload: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(payload).ok()?;
    let wave = json.get("wave")?.as_u64()?;
    let steps = json
        .get("steps")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    Some(format!("Wave {wave}: {}", summarize_telegram_line(&steps)))
}

fn progress_dag_step_line(status: &str, payload: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(payload).ok()?;
    let step_id = json.get("step_id")?.as_str()?;
    let role = json
        .get("role")
        .and_then(|value| value.as_str())
        .unwrap_or("step");
    let route = json
        .get("route")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let error = json.get("error").and_then(|value| value.as_str());

    let mut line = format!("Step {step_id} {status} ({role} · {route})");
    if let Some(error) = error {
        line.push_str(&format!(": {}", summarize_telegram_line(error)));
    }
    Some(line)
}

fn summarize_telegram_line(text: &str) -> String {
    let single_line = text.replace('\n', " ");
    if single_line.len() > 100 {
        format!("{}...", &single_line[..97])
    } else {
        single_line
    }
}

fn telegram_status_label(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "queued",
        TaskStatus::Running => "running",
        TaskStatus::Completed => "done",
        TaskStatus::Failed => "failed",
    }
}

fn short_task_id(task_id: Uuid) -> String {
    task_id.to_string().chars().take(8).collect()
}

#[cfg(test)]
mod progress_tests {
    use super::*;

    #[test]
    fn general_progress_summary_skips_irrelevant_events() {
        let events = vec![
            AgentEvent {
                id: "1".to_string(),
                task_id: "task".to_string(),
                parent_task_id: None,
                event_type: "thought".to_string(),
                payload: "inspect workflow bindings".to_string(),
                step_num: 1,
                domain: None,
                created_at: 0,
            },
            AgentEvent {
                id: "2".to_string(),
                task_id: "task".to_string(),
                parent_task_id: None,
                event_type: "debug".to_string(),
                payload: "ignored".to_string(),
                step_num: 2,
                domain: None,
                created_at: 0,
            },
            AgentEvent {
                id: "3".to_string(),
                task_id: "task".to_string(),
                parent_task_id: None,
                event_type: "tool_call".to_string(),
                payload: "run_command cargo check".to_string(),
                step_num: 3,
                domain: None,
                created_at: 0,
            },
        ];

        let lines = latest_progress_lines(&events);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("Plan:"));
        assert!(lines[1].starts_with("Tool:"));
    }

    #[test]
    fn progress_message_includes_status_and_recent_activity() {
        let task_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
        let events = vec![AgentEvent {
            id: "1".to_string(),
            task_id: "task".to_string(),
            parent_task_id: None,
            event_type: "observation".to_string(),
            payload: "found three workflow bindings".to_string(),
            step_num: 1,
            domain: None,
            created_at: 0,
        }];

        let message = render_progress_message(task_id, &TaskStatus::Running, &events);
        assert!(message.contains("Status: running"));
        assert!(message.contains("Task: 12345678"));
        assert!(message.contains("Observation: found three workflow bindings"));
    }
}
