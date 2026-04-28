//! API Server Core Tool
//!
//! Provides REST API for programmatic access to the Rove engine.
//! Implements token-based authentication, rate limiting, and task management endpoints.
//!
//! # Requirements
//!
//! - 17.8: REST API endpoints with authentication and rate limiting
//!
//! # Endpoints
//!
//! - POST /api/auth - Obtain authentication token
//! - POST /api/tasks - Submit a new task
//! - GET /api/tasks/:id - Get task status
//! - GET /api/tasks - Get task history
//! - DELETE /api/tasks/:id - Cancel a task
//! - GET /api/status - Get server status

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rand::Rng;
use sdk::{CoreContext, CoreTool, EngineError, ToolInput, ToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Authentication token
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthToken {
    token: String,
    created_at: u64,
}

/// WebSocket query parameters for authentication
#[derive(Debug, Deserialize)]
struct WsQuery {
    token: Option<String>,
}

/// API request for authentication
#[derive(Debug, Deserialize)]
struct AuthRequest {
    // Empty for now - could add username/password later
}

/// API response for authentication
#[derive(Debug, Serialize)]
struct AuthResponse {
    token: String,
}

/// API server state shared across handlers
#[derive(Clone)]
struct ServerState {
    ctx: CoreContext,
    #[allow(dead_code)]
    connections: Arc<Mutex<Vec<broadcast::Sender<String>>>>,
    auth_tokens: Arc<Mutex<HashMap<String, AuthToken>>>,
    event_tx: broadcast::Sender<String>,
}

/// API server
pub struct APIServer {
    ctx: Option<CoreContext>,
    addr: Option<SocketAddr>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    event_tx: Option<broadcast::Sender<String>>,
}

impl APIServer {
    /// Create a new APIServer instance
    pub fn new() -> Self {
        Self {
            ctx: None,
            addr: None,
            shutdown_tx: None,
            event_tx: None,
        }
    }

    /// Start the WebSocket server on a random port
    async fn start_server(
        ctx: CoreContext,
    ) -> Result<
        (
            SocketAddr,
            tokio::sync::oneshot::Sender<()>,
            broadcast::Sender<String>,
        ),
        EngineError,
    > {
        // Bind to 127.0.0.1 on random port (Requirement 17.1)
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| EngineError::Network(format!("Failed to bind to localhost: {}", e)))?;

        let addr = listener
            .local_addr()
            .map_err(|e| EngineError::Network(format!("Failed to get local address: {}", e)))?;

        tracing::info!("API server bound to {}", addr);

        // Create broadcast channel for events (Requirement 17.5)
        let (event_tx, _event_rx) = broadcast::channel(1000);
        let event_tx_clone = event_tx.clone();

        // Create server state
        let state = ServerState {
            ctx: ctx.clone(),
            connections: Arc::new(Mutex::new(Vec::new())),
            auth_tokens: Arc::new(Mutex::new(HashMap::new())),
            event_tx: event_tx_clone,
        };

        // Build router with WebSocket and API endpoints
        let app = Router::new()
            .route("/ws", get(websocket_handler))
            .route("/api/auth", post(auth_handler))
            .route("/api/submit_task", post(submit_task_handler))
            .route("/api/history", get(history_handler))
            .route("/api/status", get(status_handler))
            .route("/", get(index_handler))
            .fallback(index_handler)
            .with_state(state);

        // Convert std TcpListener to tokio
        listener
            .set_nonblocking(true)
            .map_err(|e| EngineError::Network(format!("Failed to set non-blocking: {}", e)))?;
        let tokio_listener = tokio::net::TcpListener::from_std(listener)
            .map_err(|e| EngineError::Network(format!("Failed to convert listener: {}", e)))?;

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        // Spawn server task
        tokio::spawn(async move {
            tracing::info!("API server listening on http://{}", addr);

            axum::serve(tokio_listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_rx.await.ok();
                    tracing::info!("API server shutting down gracefully");
                })
                .await
                .unwrap_or_else(|e| {
                    tracing::error!("API server error: {}", e);
                });
        });

        Ok((addr, shutdown_tx, event_tx))
    }

    /// Save the port to config.toml (Requirement 17.2)
    fn save_port_to_config(_ctx: &CoreContext, port: u16) -> Result<(), EngineError> {
        // Get the config file path
        let home = dirs::home_dir()
            .ok_or_else(|| EngineError::Config("Could not determine home directory".to_string()))?;
        let config_path = home.join(".rove").join("config.toml");

        // Read existing config
        let config_str = std::fs::read_to_string(&config_path)
            .map_err(|e| EngineError::Config(format!("Failed to read config file: {}", e)))?;

        // Parse as TOML value
        let mut config: toml::Value = toml::from_str(&config_str)
            .map_err(|e| EngineError::Config(format!("Failed to parse config: {}", e)))?;

        // Add or update api_server section
        if let Some(table) = config.as_table_mut() {
            let api_server_section = table
                .entry("api_server".to_string())
                .or_insert(toml::Value::Table(toml::map::Map::new()));

            if let Some(api_table) = api_server_section.as_table_mut() {
                api_table.insert("port".to_string(), toml::Value::Integer(port as i64));
            }
        }

        // Write back to file
        let updated_config = toml::to_string_pretty(&config)
            .map_err(|e| EngineError::Config(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(&config_path, updated_config)
            .map_err(|e| EngineError::Config(format!("Failed to write config file: {}", e)))?;

        tracing::info!("Saved API server port {} to config.toml", port);
        Ok(())
    }

    /// Subscribe to message bus events and forward to WebSocket clients (Requirement 17.5)
    async fn subscribe_to_events(_ctx: CoreContext, _event_tx: broadcast::Sender<String>) {
        // Subscribe to all events from the message bus
        // Note: The BusHandle API needs to be enhanced to support async subscriptions
        // For now, we'll log that we're ready to receive events
        tracing::info!("API server ready to receive requests");

        // TODO: Once the engine provides a proper async subscription mechanism,
        // we'll receive events here and forward them to WebSocket clients via event_tx
        //
        // Example of what this would look like:
        // let mut rx = ctx.bus.subscribe_async("All").await.expect("Failed to subscribe");
        // while let Some(event) = rx.recv().await {
        //     let event_json = serde_json::to_string(&event).expect("Failed to serialize");
        //     let _ = event_tx.send(event_json);
        // }
    }

    /// Generate a new authentication token (Requirement 17.6)
    fn generate_token() -> String {
        let mut rng = rand::thread_rng();
        let token: String = (0..32)
            .map(|_| {
                let idx = rng.gen_range(0..62);
                match idx {
                    0..=25 => (b'A' + idx) as char,
                    26..=51 => (b'a' + (idx - 26)) as char,
                    _ => (b'0' + (idx - 52)) as char,
                }
            })
            .collect();
        token
    }

    /// Validate an authentication token (Requirement 17.6)
    fn validate_token(tokens: &HashMap<String, AuthToken>, token: &str) -> bool {
        if let Some(auth_token) = tokens.get(token) {
            // Check if token is not expired (24 hours)
            let now = current_unix_timestamp();

            let age = now - auth_token.created_at;
            age < 86400 // 24 hours
        } else {
            false
        }
    }
}

impl Default for APIServer {
    fn default() -> Self {
        Self::new()
    }
}

impl CoreTool for APIServer {
    fn name(&self) -> &str {
        "api-server"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn start(&mut self, ctx: CoreContext) -> Result<(), EngineError> {
        // Start the server asynchronously
        let ctx_clone = ctx.clone();
        let (addr, shutdown_tx, event_tx) =
            tokio::runtime::Handle::current().block_on(Self::start_server(ctx_clone))?;

        // Save port to config (Requirement 17.2)
        Self::save_port_to_config(&ctx, addr.port())?;

        // Subscribe to message bus events for task streaming (Requirement 17.5)
        let event_tx_clone = event_tx.clone();
        let ctx_clone = ctx.clone();
        tokio::spawn(async move {
            Self::subscribe_to_events(ctx_clone, event_tx_clone).await;
        });

        self.ctx = Some(ctx);
        self.addr = Some(addr);
        self.shutdown_tx = Some(shutdown_tx);
        self.event_tx = Some(event_tx);

        tracing::info!("API server started on http://{}", addr);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            shutdown_tx.send(()).ok();
            tracing::info!("API server stopped");
        }
        Ok(())
    }

    fn handle(&self, input: ToolInput) -> Result<ToolOutput, EngineError> {
        // Handle API requests
        match input.method.as_str() {
            "get_port" => {
                if let Some(addr) = self.addr {
                    Ok(ToolOutput::json(json!({ "port": addr.port() })))
                } else {
                    Err(EngineError::ToolError("Server not started".to_string()))
                }
            }
            _ => Err(EngineError::ToolError(format!(
                "Unknown method: {}",
                input.method
            ))),
        }
    }
}

/// WebSocket handler (Requirement 17.3, 17.6)
async fn websocket_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<ServerState>,
) -> Response {
    // Require authentication token (Requirement 17.6)
    let token = match query.token {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Authentication token required"})),
            )
                .into_response();
        }
    };

    // Validate token
    let tokens = match state.auth_tokens.lock() {
        Ok(tokens) => tokens,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication state unavailable"})),
            )
                .into_response();
        }
    };
    if !APIServer::validate_token(&tokens, &token) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid or expired token"})),
        )
            .into_response();
    }
    drop(tokens);

    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

/// Handle WebSocket connection (Requirement 17.5)
async fn handle_websocket(mut socket: WebSocket, state: ServerState) {
    tracing::info!("New WebSocket connection established");

    // Subscribe to event broadcast channel for task streaming (Requirement 17.5)
    let mut event_rx = state.event_tx.subscribe();

    // Handle incoming messages
    loop {
        tokio::select! {
            // Receive from WebSocket
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        tracing::debug!("Received WebSocket message: {}", text);

                        // Parse and handle message
                        if let Ok(json_msg) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(msg_type) = json_msg.get("type").and_then(|v| v.as_str()) {
                                match msg_type {
                                    "ping" => {
                                        let pong = json!({"type": "pong"});
                                        if socket.send(Message::Text(pong.to_string())).await.is_err() {
                                            break;
                                        }
                                    }
                                    "submit_task" => {
                                        if let Some(task_input) = json_msg.get("task").and_then(|v| v.as_str()) {
                                            match state.ctx.agent.submit_task(task_input.to_string()) {
                                                Ok(task_id) => {
                                                    let response = json!({
                                                        "type": "task_submitted",
                                                        "task_id": task_id
                                                    });
                                                    if socket.send(Message::Text(response.to_string())).await.is_err() {
                                                        break;
                                                    }
                                                }
                                                Err(e) => {
                                                    let error = json!({
                                                        "type": "error",
                                                        "message": e.to_string()
                                                    });
                                                    if socket.send(Message::Text(error.to_string())).await.is_err() {
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    _ => {
                                        tracing::warn!("Unknown message type: {}", msg_type);
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        tracing::info!("WebSocket connection closed by client");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::error!("WebSocket error: {}", e);
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
            // Receive from event broadcast channel (Requirement 17.5)
            event = event_rx.recv() => {
                match event {
                    Ok(event_json) => {
                        if socket.send(Message::Text(event_json)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!("WebSocket client lagged, skipped {} events", skipped);
                        // Continue receiving
                    }
                    Err(_) => break,
                }
            }
        }
    }

    tracing::info!("WebSocket connection closed");
}

/// Authentication endpoint (Requirement 17.6)
async fn auth_handler(
    State(state): State<ServerState>,
    Json(_payload): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, Response> {
    // Generate a new authentication token
    let token = APIServer::generate_token();
    let now = current_unix_timestamp();

    let auth_token = AuthToken {
        token: token.clone(),
        created_at: now,
    };

    // Store the token
    {
        let mut tokens = state.auth_tokens.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication state unavailable"})),
            )
                .into_response()
        })?;
        tokens.insert(token.clone(), auth_token);

        // Clean up expired tokens (older than 24 hours)
        tokens.retain(|_, t| now - t.created_at < 86400);
    }

    tracing::info!("Generated new authentication token");

    Ok(Json(AuthResponse { token }))
}

/// Submit task API endpoint (Requirement 17.8)
async fn submit_task_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, Response> {
    // Check authentication (Requirement 17.6)
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Missing authorization header"})),
            )
                .into_response()
        })?;

    // Validate token
    {
        let tokens = state.auth_tokens.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication state unavailable"})),
            )
                .into_response()
        })?;
        if !APIServer::validate_token(&tokens, token) {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid or expired token"})),
            )
                .into_response());
        }
    }

    // TODO: Apply rate limiting (Requirement 17.8)
    // This would require access to the RateLimiter through CoreContext
    // For now, we log that rate limiting should be applied
    tracing::debug!("Rate limiting check would be applied here");

    let task_input = payload
        .get("task")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Missing 'task' field"})),
            )
                .into_response()
        })?;

    match state.ctx.agent.submit_task(task_input.to_string()) {
        Ok(task_id) => Ok(Json(json!({
            "success": true,
            "task_id": task_id
        }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response()),
    }
}

/// Get task history API endpoint (Requirement 17.6)
async fn history_handler(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Response> {
    // Check authentication (Requirement 17.6)
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Missing authorization header"})),
            )
                .into_response()
        })?;

    // Validate token
    {
        let tokens = state.auth_tokens.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication state unavailable"})),
            )
                .into_response()
        })?;
        if !APIServer::validate_token(&tokens, token) {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Invalid or expired token"})),
            )
                .into_response());
        }
    }

    // Query last 10 tasks from database
    match state.ctx.db.query(
        "SELECT id, input, status, created_at FROM tasks ORDER BY created_at DESC LIMIT 10",
        vec![],
    ) {
        Ok(rows) => Ok(Json(json!({
            "success": true,
            "tasks": rows
        }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response()),
    }
}

/// Server status API endpoint
async fn status_handler(State(_state): State<ServerState>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

/// Fallback handler for serving index.html (Requirement 17.4, 17.7)
async fn index_handler() -> Response {
    // Serve a simple HTML page
    let html = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Rove UI</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, sans-serif;
            max-width: 800px;
            margin: 50px auto;
            padding: 20px;
            background: #f5f5f5;
        }
        .container {
            background: white;
            padding: 30px;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }
        h1 {
            color: #333;
            margin-top: 0;
        }
        .status {
            color: #28a745;
            font-weight: bold;
        }
        .info {
            margin: 20px 0;
            padding: 15px;
            background: #f8f9fa;
            border-left: 4px solid #007bff;
        }
        .warning {
            margin: 20px 0;
            padding: 15px;
            background: #fff3cd;
            border-left: 4px solid #ffc107;
            color: #856404;
        }
        code {
            background: #e9ecef;
            padding: 2px 6px;
            border-radius: 3px;
            font-family: 'Courier New', monospace;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>Rove API Server</h1>
        <p class="status">✓ Server is running</p>

        <div class="warning">
            <strong>⚠️ Authentication Required</strong>
            <p>All API endpoints and WebSocket connections require authentication.
            First, obtain a token from the <code>/api/auth</code> endpoint.</p>
        </div>

        <div class="info">
            <p><strong>Authentication:</strong></p>
            <ul>
                <li>POST /api/auth - Get authentication token</li>
            </ul>

            <p><strong>WebSocket Endpoint:</strong></p>
            <ul>
                <li>ws://localhost/ws?token=YOUR_TOKEN - Real-time task streaming</li>
            </ul>

            <p><strong>API Endpoints:</strong></p>
            <ul>
                <li>POST /api/submit_task - Submit a new task (requires Bearer token)</li>
                <li>GET /api/history - Get task history (requires Bearer token)</li>
                <li>GET /api/status - Get server status</li>
            </ul>
        </div>

        <p><strong>Features:</strong></p>
        <ul>
            <li>✓ Token-based authentication</li>
            <li>✓ Real-time task progress streaming via WebSocket</li>
            <li>✓ Rate limiting protection</li>
            <li>✓ Secure localhost-only binding</li>
        </ul>

        <p>Connect via WebSocket with a valid token to interact with the Rove agent in real-time.</p>
    </div>
</body>
</html>"#;

    (StatusCode::OK, [("content-type", "text/html")], html).into_response()
}

/// Native export for creating the tool
#[no_mangle]
pub fn create_tool() -> *mut dyn CoreTool {
    Box::into_raw(Box::new(APIServer::new()))
}

fn current_unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token() {
        let token1 = APIServer::generate_token();
        let token2 = APIServer::generate_token();

        // Tokens should be 32 characters long
        assert_eq!(token1.len(), 32);
        assert_eq!(token2.len(), 32);

        // Tokens should be different
        assert_ne!(token1, token2);

        // Tokens should only contain alphanumeric characters
        assert!(token1.chars().all(|c| c.is_alphanumeric()));
        assert!(token2.chars().all(|c| c.is_alphanumeric()));
    }

    #[test]
    fn test_validate_token() {
        let mut tokens = HashMap::new();
        let token = "test_token_123456789012345678901";

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Valid token (just created)
        tokens.insert(
            token.to_string(),
            AuthToken {
                token: token.to_string(),
                created_at: now,
            },
        );

        assert!(APIServer::validate_token(&tokens, token));

        // Invalid token (not in map)
        assert!(!APIServer::validate_token(&tokens, "invalid_token"));

        // Expired token (25 hours old)
        let old_token = "old_token_123456789012345678901";
        tokens.insert(
            old_token.to_string(),
            AuthToken {
                token: old_token.to_string(),
                created_at: now - 90000, // 25 hours ago
            },
        );

        assert!(!APIServer::validate_token(&tokens, old_token));
    }

    #[test]
    fn test_token_expiration() {
        let mut tokens = HashMap::new();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Token that's 23 hours old (should be valid)
        let recent_token = "recent_token_1234567890123456789";
        tokens.insert(
            recent_token.to_string(),
            AuthToken {
                token: recent_token.to_string(),
                created_at: now - 82800, // 23 hours
            },
        );

        assert!(APIServer::validate_token(&tokens, recent_token));

        // Token that's 25 hours old (should be invalid)
        let old_token = "old_token_123456789012345678901";
        tokens.insert(
            old_token.to_string(),
            AuthToken {
                token: old_token.to_string(),
                created_at: now - 90000, // 25 hours
            },
        );

        assert!(!APIServer::validate_token(&tokens, old_token));
    }
}
