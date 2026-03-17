//! Model Context Protocol (MCP) Endpoints
//!
//! Provides OpenAI/Anthropic compatible endpoints allowing standard UI clients
//! (like Cursor or Continue.dev) to attach to Rove as a sub-agent.

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use super::AppState;
use crate::gateway::Task;

#[derive(Deserialize)]
pub struct ChatCompletionRequest {
    pub messages: Vec<McpMessage>,
    pub model: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct McpMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<McpChoice>,
}

#[derive(Serialize)]
pub struct McpChoice {
    pub message: McpResponseMessage,
    pub finish_reason: String,
    pub index: usize,
}

#[derive(Serialize)]
pub struct McpResponseMessage {
    pub role: String,
    pub content: String,
}

/// A standard chat completion endpoint masking the Rove Agent Loop
pub async fn mcp_chat_completions(
    State(state): State<AppState>,
    Json(payload): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    let mut agent = state.agent.write().await;

    // In a typical MCP flow, we only process the last user message as the task.
    // The history is technically already tracked by `WorkingMemory`, but complex
    // MCP clients might pass history we need to sync. For Phase 4, we extract the last message.
    let last_user_msg = payload
        .messages
        .iter()
        .rfind(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_else(|| "Hello".to_string());

    let task = Task::build_from_websocket(last_user_msg, None);

    match agent.process_task(task).await {
        Ok(result) => {
            let res = ChatCompletionResponse {
                id: format!("rove-{}", result.task_id),
                object: "chat.completion".to_string(),
                created: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                    .as_secs(),
                model: payload.model.unwrap_or_else(|| "rove-agent".to_string()),
                choices: vec![McpChoice {
                    message: McpResponseMessage {
                        role: "assistant".to_string(),
                        content: result.answer,
                    },
                    finish_reason: "stop".to_string(),
                    index: 0,
                }],
            };
            (StatusCode::OK, Json(res))
        }
        Err(e) => {
            // Simplified error structure
            let res = ChatCompletionResponse {
                id: "rove-error".to_string(),
                object: "chat.completion".to_string(),
                created: 0,
                model: "rove-agent".to_string(),
                choices: vec![McpChoice {
                    message: McpResponseMessage {
                        role: "assistant".to_string(),
                        content: format!("Rove Agent Error: {}", e),
                    },
                    finish_reason: "error".to_string(),
                    index: 0,
                }],
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(res))
        }
    }
}
