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

// ── Remote PTY terminal ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TerminalQuery {
    pub token: Option<String>,
    pub shell: Option<String>,
}

/// WebSocket handler for remote PTY terminal sessions (`GET /v1/remote/terminal`).
///
/// Verifies the signed remote request headers, then spawns a PTY using the
/// requested shell (default: sh). Bridges WS ↔ PTY bidirectionally.
///
/// Protocol (client → server):
/// ```json
/// { "type": "stdin", "data": "<base64>" }
/// { "type": "resize", "cols": 80, "rows": 24 }
/// ```
///
/// Protocol (server → client):
/// ```json
/// { "type": "stdout", "data": "<base64>" }
/// { "type": "exit", "code": 0 }
/// ```
pub async fn handle_remote_terminal(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    Query(query): Query<TerminalQuery>,
) -> impl IntoResponse {
    // Verify signed remote request.
    let config = match Config::load_or_create() {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let manager = RemoteManager::new(config);
    if let Err(error) = manager.verify_signed_request(&headers, "terminal", None) {
        return (StatusCode::UNAUTHORIZED, error.to_string()).into_response();
    }

    let shell = query
        .shell
        .clone()
        .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string()));

    ws.on_upgrade(move |socket| async move {
        if let Err(error) = run_terminal_session(socket, &shell).await {
            warn!(error = %error, "Remote terminal session error");
        }
    })
}

async fn run_terminal_session(mut socket: WebSocket, shell: &str) -> anyhow::Result<()> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let cmd = CommandBuilder::new(shell);
    let _child = pair.slave.spawn_command(cmd)?;

    let mut pty_reader = pair.master.try_clone_reader()?;
    let pty_writer = pair.master.take_writer()?;

    // Spawn a blocking thread to read PTY output and forward it to WS.
    let (pty_tx, mut pty_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match std::io::Read::read(&mut pty_reader, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if pty_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Spawn a blocking thread for PTY writes.
    let (stdin_tx, stdin_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut writer = pty_writer;
        for data in stdin_rx {
            if std::io::Write::write_all(&mut writer, &data).is_err() {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            // PTY output → WS
            data = pty_rx.recv() => {
                match data {
                    Some(bytes) => {
                        use base64::Engine;
                        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        let msg = serde_json::json!({ "type": "stdout", "data": encoded });
                        if socket.send(Message::Text(msg.to_string())).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        // PTY closed
                        let exit_msg = serde_json::json!({ "type": "exit", "code": 0 });
                        let _ = socket.send(Message::Text(exit_msg.to_string())).await;
                        break;
                    }
                }
            }
            // WS input → PTY
            msg = socket.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                            match val.get("type").and_then(|v| v.as_str()) {
                                Some("stdin") => {
                                    if let Some(data) = val.get("data").and_then(|v| v.as_str()) {
                                        use base64::Engine;
                                        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
                                            let _ = stdin_tx.send(bytes);
                                        }
                                    }
                                }
                                Some("resize") => {
                                    let cols = val.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
                                    let rows = val.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u16;
                                    let _ = pair.master.resize(PtySize {
                                        rows,
                                        cols,
                                        pixel_width: 0,
                                        pixel_height: 0,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
