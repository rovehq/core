//! WebSocket Event Stream
//!
//! Provides a WebSocket endpoint at /v1/events/ws that pushes real-time events
//! to the local WebUI. Clients subscribe to topics and receive typed events.
//!
//! Protocol (client → server):
//! ```json
//! { "type": "subscribe", "topic": "tasks" }
//! { "type": "subscribe", "topic": "daemon" }
//! { "type": "start_task", "input": "..." }
//! { "type": "subscribe_task", "task_id": "..." }
//! { "type": "ping" }
//! ```
//!
//! Protocol (server → client):
//! ```json
//! { "type": "connected", "version": "..." }
//! { "type": "task.created", "task_id": "..." }
//! { "type": "task.event", "task_id": "...", "event": {...} }
//! { "type": "task.completed", "task_id": "...", "result": "..." }
//! { "type": "auth.locked" }
//! { "type": "approval.required", "task_id": "...", "risk": "..." }
//! { "type": "pong" }
//! ```
//!
//! Subscriptions are topic-gated. Subscribing to "tasks" delivers events for
//! all tasks. Subscribing to "task:<id>" delivers events for a specific task.
//! Subscribing to "daemon" enables auth and daemon state events.
//! No event is sent as an immediate response to a subscribe — the stream is
//! push-only from the server side.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::HeaderMap,
    http::StatusCode,
    response::IntoResponse,
};
use futures::StreamExt;
use serde::Deserialize;
use std::collections::HashSet;
use tracing::{info, warn};

use super::{auth::AuthManager, AppState};
use crate::config::Config;
use crate::message_bus::{Event as BusEvent, EventType as BusEventType};
use crate::remote::RemoteManager;

// ── Client → Server messages ─────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    /// Submit a new task and stream its events.
    StartTask {
        input: String,
    },
    /// Subscribe to events for a specific task by ID.
    SubscribeTask {
        task_id: String,
    },
    /// Subscribe to a named topic: "tasks", "daemon", or "task:<id>".
    Subscribe {
        topic: String,
    },
    Ping,
}

// ── Auth query ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AuthQuery {
    pub token: Option<String>,
}

// ── Axum handlers ─────────────────────────────────────────────────────────────

/// Primary event-stream WebSocket handler (`GET /v1/events/ws?token=...`).
pub async fn task_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
) -> impl IntoResponse {
    let manager = AuthManager::new(state.db.clone());
    if let Some(token) = query.token.as_deref() {
        if let Err(error) = manager.validate_session(token, true).await {
            return (StatusCode::UNAUTHORIZED, error.to_string()).into_response();
        }
    } else {
        let config = match Config::load_or_create() {
            Ok(config) => config,
            Err(error) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
            }
        };
        if let Err(error) =
            RemoteManager::new(config).verify_signed_request(&headers, "event_stream", None)
        {
            return (StatusCode::UNAUTHORIZED, error.to_string()).into_response();
        }
    }
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Legacy telemetry handler (kept for backwards compat with `/ws/telemetry`).
pub async fn telemetry_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
) -> impl IntoResponse {
    task_ws_handler(ws, State(state), headers, Query(query)).await
}

// ── Socket handler ─────────────────────────────────────────────────────────────

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    info!("WebUI client connected via WebSocket");

    // Send a welcome message so the client knows the connection is live.
    let welcome =
        serde_json::json!({ "type": "connected", "version": crate::info::VERSION }).to_string();
    if socket.send(Message::Text(welcome)).await.is_err() {
        warn!("Failed to send welcome; client may have disconnected immediately");
        return;
    }

    // Subscribe to all bus events from the start. We filter by client
    // subscriptions when deciding what to forward.
    let mut bus_rx = state.message_bus.subscribe(BusEventType::All).await;

    // Topics the client has explicitly subscribed to.
    let mut subscribed: HashSet<String> = HashSet::new();

    loop {
        tokio::select! {
            // ── Incoming WebSocket message ─────────────────────────────────
            msg_result = socket.next() => {
                let msg = match msg_result {
                    Some(Ok(m)) => m,
                    _ => break, // socket closed or error
                };

                match msg {
                    Message::Text(text) => {
                        let client_msg: ClientMsg = match serde_json::from_str(&text) {
                            Ok(m) => m,
                            Err(e) => {
                                let err = serde_json::json!({
                                    "type": "error",
                                    "message": format!("Invalid message: {}", e)
                                })
                                .to_string();
                                let _ = socket.send(Message::Text(err)).await;
                                continue;
                            }
                        };

                        match client_msg {
                            // Client subscribes to a topic — register it silently.
                            // No immediate response; events are pushed as they occur.
                            ClientMsg::Subscribe { topic } => {
                                subscribed.insert(topic);
                            }
                            ClientMsg::SubscribeTask { task_id } => {
                                subscribed.insert(format!("task:{}", task_id));
                            }
                            ClientMsg::StartTask { input } => {
                                handle_start_task(&mut socket, &state, input).await;
                            }
                            ClientMsg::Ping => {
                                let pong = serde_json::json!({"type": "pong"}).to_string();
                                let _ = socket.send(Message::Text(pong)).await;
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

            // ── Outgoing event from the message bus ───────────────────────
            bus_event = bus_rx.recv() => {
                let Some(event) = bus_event else { break };

                let outbound = match &event {
                    BusEvent::TaskStarted { task_id, .. }
                        if wants_task(&subscribed, task_id) =>
                    {
                        serde_json::json!({
                            "type": "task.created",
                            "task_id": task_id,
                        })
                        .to_string()
                    }
                    BusEvent::TaskStream { task_id, event }
                        if wants_task(&subscribed, task_id) =>
                    {
                        serde_json::json!({
                            "type": "task.event",
                            "task_id": task_id,
                            "event": event,
                        })
                        .to_string()
                    }
                    BusEvent::TaskCompleted { task_id, result }
                        if wants_task(&subscribed, task_id) =>
                    {
                        serde_json::json!({
                            "type": "task.completed",
                            "task_id": task_id,
                            "result": result,
                        })
                        .to_string()
                    }
                    BusEvent::TaskFailed { task_id, error }
                        if wants_task(&subscribed, task_id) =>
                    {
                        serde_json::json!({
                            "type": "task.completed",
                            "task_id": task_id,
                            "result": error,
                        })
                        .to_string()
                    }
                    // Auth events go to "daemon" subscribers.
                    BusEvent::DaemonStopping if subscribed.contains("daemon") => {
                        serde_json::json!({"type": "auth.locked"}).to_string()
                    }
                    _ => continue, // not subscribed or not a forwarded event type
                };

                if socket.send(Message::Text(outbound)).await.is_err() {
                    break;
                }
            }
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Returns true if the client wants events for this task_id.
/// "tasks" subscription covers all tasks; "task:<id>" covers a specific one.
fn wants_task(subscribed: &HashSet<String>, task_id: &str) -> bool {
    subscribed.contains("tasks") || subscribed.contains(&format!("task:{}", task_id))
}

async fn handle_start_task(socket: &mut WebSocket, state: &AppState, input: String) {
    let task_id = match state.gateway.submit_webui(&input, None, None).await {
        Ok(id) => id,
        Err(e) => {
            let err = serde_json::json!({
                "type": "error",
                "message": format!("Failed to submit task: {}", e),
            })
            .to_string();
            let _ = socket.send(Message::Text(err)).await;
            return;
        }
    };

    // Acknowledge with task.created so the client can track it.
    let accepted = serde_json::json!({
        "type": "task.created",
        "task_id": task_id,
    })
    .to_string();
    let _ = socket.send(Message::Text(accepted)).await;
}
