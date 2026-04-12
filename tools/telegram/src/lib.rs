//! Telegram Bot Core Tool
//!
//! Provides Telegram bot integration for Rove via native cdylib plugin.
//!

use sdk::{CoreContext, CoreTool, EngineError, ToolInput, ToolOutput};
use serde_json::json;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use tracing::{info, warn};

/// Telegram bot native tool
pub struct TelegramBot {
    ctx: Option<CoreContext>,
    abort_handle: Option<tokio::task::AbortHandle>,
    bot: Option<Bot>,
}

impl TelegramBot {
    pub fn new() -> Self {
        Self {
            ctx: None,
            abort_handle: None,
            bot: None,
        }
    }
}

impl Default for TelegramBot {
    fn default() -> Self {
        Self::new()
    }
}

impl CoreTool for TelegramBot {
    fn name(&self) -> &str {
        "telegram"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn start(&mut self, ctx: CoreContext) -> Result<(), EngineError> {
        info!("Initializing Telegram Native Tool...");

        // Securely retrieve bot token from cache
        let token = match ctx.crypto.get_secret("TELEGRAM_BOT_TOKEN") {
            Ok(token) => token,
            Err(_) => {
                warn!("No TELEGRAM_BOT_TOKEN found. Telegram interface dormant.");
                return Ok(());
            }
        };

        let allowlist_str = ctx
            .config
            .get_string("telegram_allowed_users")
            .unwrap_or_default();
        let allowed_users: Vec<i64> = allowlist_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        self.ctx = Some(ctx.clone());

        // Spawn background polling task
        let bot = Bot::new(token);

        let handler = dptree::entry()
            .branch(Update::filter_message().endpoint(
                |bot: Bot, msg: Message, ctx: CoreContext, allowed_users: Arc<Vec<i64>>| async move {
                    let chat_id = msg.chat.id;
                    let user_id = msg.from().map(|u| u.id.0 as i64).unwrap_or(0);

                    if !allowed_users.contains(&user_id) && !allowed_users.is_empty() {
                        warn!("Unauthorized user {} attempted to use Telegram bot.", user_id);
                        return respond(());
                    }

                    if let Some(text) = msg.text() {
                        info!("Received message via tool: {}", text);

                        match text {
                            "/start" | "/help" => {
                                let _ = bot.send_message(chat_id, "Rove native Telegram tool online. Send any task.").await;
                            }
                            _ => {
                                let _ = bot.send_message(chat_id, "Processing task...").await;
                                match ctx.agent.submit_task(text.to_string()) {
                                    Ok(task_id) => {
                                        // A naive synchronous wait for the task to finish if we wanted,
                                        // or we just acknowledge it. E.g.
                                        let _ = bot.send_message(chat_id, format!("Task {} submitted.", task_id)).await;
                                    }
                                    Err(e) => {
                                        let _ = bot.send_message(chat_id, format!("Failed to submit: {:?}", e)).await;
                                    }
                                }
                            }
                        }
                    }
                    respond(())
                }
            ))
            .branch(Update::filter_callback_query().endpoint(
                |bot: Bot, q: CallbackQuery, ctx: CoreContext, allowed_users: Arc<Vec<i64>>| async move {
                    let user_id = q.from.id.0 as i64;
                    if !allowed_users.contains(&user_id) && !allowed_users.is_empty() {
                        let _ = bot.answer_callback_query(q.id).text("Unauthorized").await;
                        return respond(());
                    }

                    if let Some(data) = &q.data {
                        // Forward approval/denial payload to the agent bus
                        let _ = ctx.bus.publish("tier2_response", serde_json::json!({
                            "user_id": user_id,
                            "action": data
                        }));
                        let _ = bot.answer_callback_query(q.id).text(format!("Action recorded: {}", data)).await;
                    }
                    respond(())
                }
            ));

        let allowed_users_arc = Arc::new(allowed_users);
        let ctx_clone = ctx.clone();

        // Use tokio abort handle to cancel the loop when tool unloads
        let task = tokio::spawn(async move {
            Dispatcher::builder(bot, handler)
                .dependencies(dptree::deps![ctx_clone, allowed_users_arc])
                .build()
                .dispatch()
                .await;
        });

        self.abort_handle = Some(task.abort_handle());
        info!("Telegram bot started and bound to runtime.");
        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        info!("Telegram bot signaled to stop.");
        if let Some(handle) = self.abort_handle.take() {
            handle.abort();
        }
        self.ctx = None;
        Ok(())
    }

    fn handle(&self, input: ToolInput) -> Result<ToolOutput, EngineError> {
        let _ctx = self
            .ctx
            .as_ref()
            .ok_or_else(|| EngineError::ToolError("Telegram context not initialized".into()))?;

        let bot = self
            .bot
            .as_ref()
            .ok_or_else(|| EngineError::ToolError("Telegram bot absent".into()))?
            .clone();

        let method = input.method.as_str();

        match method {
            "tier2_prompt" => {
                let op = input
                    .param_str_opt("operation")
                    .unwrap_or_else(|| "Unknown".to_string());
                let chat_id = input.param_i64("chat_id").unwrap_or(0);

                info!(
                    "Telegram Native: Prompting Tier 2 in chat {}: {}",
                    chat_id, op
                );

                let keyboard = InlineKeyboardMarkup::default().append_row(vec![
                    InlineKeyboardButton::callback("Approve", format!("approve:{}", op)),
                    InlineKeyboardButton::callback("Cancel", format!("cancel:{}", op)),
                ]);

                let m = format!(
                    "⚠️ Tier 2 operation requires explicit approval:\n\n{}\n\nApprove or cancel?",
                    op
                );

                // Bridge sync Handle to async Bot execution
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.spawn(async move {
                        let _ = bot
                            .send_message(ChatId(chat_id), m)
                            .reply_markup(keyboard)
                            .await;
                    });
                }

                Ok(ToolOutput::json(json!({
                    "status": "prompt_queued",
                    "operation": op
                })))
            }
            _ => Ok(ToolOutput::json(json!({
                "status": "noop",
                "message": "Unsupported method"
            }))),
        }
    }
}

/// FFI export for injecting the tool natively at runtime
#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub extern "C" fn create_tool() -> *mut dyn CoreTool {
    Box::into_raw(Box::new(TelegramBot::new()))
}
