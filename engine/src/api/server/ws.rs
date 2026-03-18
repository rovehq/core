//! WebSocket Task Streaming
//!
//! Provides a WebSocket endpoint that the local WebUI connects to.
//! Clients send a JSON `StartTask` message to kick off agent execution
//! and receive streamed progress / result events back.
//!
//! Protocol (client → server):
//! ```json
//! { "type": "start_task", "input": "Do something useful" }
//! { "type": "ping" }
//! ```
//!
//! Protocol (server → client):
//! ```json
//! { "type": "accepted",  "task_id": "..." }
//! { "type": "progress",  "message": "..." }
//! { "type": "result",    "answer": "...", "duration_ms": 1234, "iterations": 3 }
//! { "type": "error",     "message": "..." }
//! { "type": "pong" }
//! ```

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{error, info, warn};

use super::{completion, AppState};

// ── Client → Server messages ─────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    StartTask { input: String },
    Ping,
}

// ── Server → Client messages ─────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMsg<'a> {
    Accepted {
        task_id: &'a str,
    },
    Progress {
        message: &'a str,
    },
    Result {
        answer: String,
        provider: Option<String>,
        duration_ms: i64,
        iterations: usize,
    },
    Error {
        message: String,
    },
    Pong,
}

impl<'a> ServerMsg<'a> {
    fn to_text(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"type":"error","message":"serialization failure"}"#.into())
    }
}

// ── Axum handler ─────────────────────────────────────────────────────────────

/// Upgrade incoming HTTP to WebSocket and hand off to `handle_socket`.
pub async fn task_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Legacy telemetry handler (kept for backwards compat with `/ws/telemetry`).
pub async fn telemetry_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    info!("WebUI client connected via WebSocket");

    // Send a welcome ping so the client knows the connection is live
    let welcome =
        serde_json::json!({ "type": "connected", "version": crate::info::VERSION }).to_string();
    if socket.send(Message::Text(welcome)).await.is_err() {
        warn!("Failed to send welcome message; client may have disconnected");
        return;
    }

    while let Some(msg) = socket.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!("WebSocket read error: {}", e);
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                let client_msg: ClientMsg = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        let err = ServerMsg::Error {
                            message: format!("Invalid message: {}", e),
                        };
                        let _ = socket.send(Message::Text(err.to_text())).await;
                        continue;
                    }
                };

                match client_msg {
                    ClientMsg::StartTask { input } => {
                        handle_start_task(&mut socket, &state, input).await;
                    }
                    ClientMsg::Ping => {
                        let _ = socket.send(Message::Text(ServerMsg::Pong.to_text())).await;
                    }
                }
            }
            Message::Ping(bytes) => {
                let _ = socket.send(Message::Pong(bytes)).await;
            }
            Message::Close(_) => {
                info!("WebUI client disconnected from WebSocket");
                break;
            }
            _ => {}
        }
    }
}

async fn handle_start_task(socket: &mut WebSocket, state: &AppState, input: String) {
    // Submit task through gateway (durable inbox pattern)
    let task_id = match state.gateway.submit_webui(&input, None).await {
        Ok(id) => id,
        Err(e) => {
            let msg = ServerMsg::Error {
                message: format!("Failed to submit task: {}", e),
            };
            let _ = socket.send(Message::Text(msg.to_text())).await;
            return;
        }
    };

    // Immediately acknowledge with the task ID so the client can track it
    let accepted = ServerMsg::Accepted { task_id: &task_id };
    if socket
        .send(Message::Text(accepted.to_text()))
        .await
        .is_err()
    {
        return;
    }

    // Stream a progress note (pre-execution)
    let prog = ServerMsg::Progress {
        message: "Task accepted. Executing…",
    };
    let _ = socket.send(Message::Text(prog.to_text())).await;

    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;

        match completion::load_completion(state, &task_id).await {
            Ok(completion::CompletionState::Done(result)) => {
                let msg = ServerMsg::Result {
                    answer: result.answer,
                    provider: result.provider,
                    duration_ms: result.duration_ms.unwrap_or(0),
                    iterations: 0,
                };
                if let Err(error) = socket.send(Message::Text(msg.to_text())).await {
                    error!("Failed to stream task result over WebSocket: {}", error);
                }
                break;
            }
            Ok(completion::CompletionState::Failed(error)) => {
                let msg = ServerMsg::Error {
                    message: format!("Task failed: {}", error),
                };
                let _ = socket.send(Message::Text(msg.to_text())).await;
                break;
            }
            Ok(completion::CompletionState::Missing) => {
                let msg = ServerMsg::Error {
                    message: "Task not found".to_string(),
                };
                let _ = socket.send(Message::Text(msg.to_text())).await;
                break;
            }
            Ok(completion::CompletionState::Running) => {
                let prog = ServerMsg::Progress {
                    message: "Task is running...",
                };
                let _ = socket.send(Message::Text(prog.to_text())).await;
            }
            Err(error) => {
                let msg = ServerMsg::Error {
                    message: format!("Failed to get task status: {}", error),
                };
                let _ = socket.send(Message::Text(msg.to_text())).await;
                break;
            }
        }
    }
}
