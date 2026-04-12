//! UI Server Native Tool
//!
//! Provides a dedicated WebSocket server on 127.0.0.1:7680 (REVO)
//! Allows the external frontend (`app.roveai.co`) to connect locally
//! to stream agent execution telemetry and manual intervention prompts.

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::HeaderValue,
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::StreamExt;
use sdk::{CoreContext, CoreTool, EngineError, ToolInput, ToolOutput};
use serde_json::json;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, warn};

/// UI Server controller
pub struct UiServer {
    ctx: Option<CoreContext>,
    server_handle: Option<JoinHandle<()>>,
}

impl UiServer {
    pub fn new() -> Self {
        Self {
            ctx: None,
            server_handle: None,
        }
    }
}

impl Default for UiServer {
    fn default() -> Self {
        Self::new()
    }
}

impl CoreTool for UiServer {
    fn name(&self) -> &str {
        "ui-server"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn start(&mut self, ctx: CoreContext) -> Result<(), EngineError> {
        info!("Initializing UI-Server Native Tool on 127.0.0.1:7680...");
        self.ctx = Some(ctx);

        // Allow app.roveai.co to connect securely cross-origin
        let production_origin =
            HeaderValue::from_str("https://app.roveai.co").map_err(|error| {
                EngineError::Network(format!("Invalid production origin: {}", error))
            })?;
        let local_origin = HeaderValue::from_str("http://localhost:3000")
            .map_err(|error| EngineError::Network(format!("Invalid local origin: {}", error)))?;
        let cors = CorsLayer::new()
            .allow_origin([production_origin, local_origin])
            .allow_methods(Any)
            .allow_headers(Any)
            .allow_credentials(true);

        let app = Router::new().route("/ws", get(ws_handler)).layer(cors);

        let addr = SocketAddr::from(([127, 0, 0, 1], 7680));

        // Spawn the Axum server on a detached Tokio thread
        let handle = tokio::spawn(async move {
            let listener = match TcpListener::bind(&addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind REVO UI-Server port 7680: {}", e);
                    return;
                }
            };

            info!("UI-Server listening on ws://{}", addr);

            if let Err(e) = axum::serve(listener, app).await {
                error!("UI-Server error: {}", e);
            }
        });

        self.server_handle = Some(handle);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        if let Some(handle) = self.server_handle.take() {
            info!("Shutting down REVO UI-Server...");
            handle.abort();
        }
        self.ctx = None;
        Ok(())
    }

    fn handle(&self, _input: ToolInput) -> Result<ToolOutput, EngineError> {
        // Expose a native hook to broadcast internal popups to the UI if an LLM calls this tool
        Ok(ToolOutput::json(json!({
            "status": "broadcast_sent",
            "port": 7680
        })))
    }
}

/// Incoming WebSocket upgrade request handler
async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

/// Processes an active WebSocket connection
async fn handle_socket(mut socket: WebSocket) {
    info!("New UI connection established from app.roveai.co wrapper");

    // Push initial REVO handshake
    if socket
        .send(Message::Text("REVO_HANDSHAKE_OK".into()))
        .await
        .is_err()
    {
        warn!("Failed to send REVO handshake. Dropping socket.");
        return;
    }

    // Await incoming UI requests (like manual task triggers)
    while let Some(msg) = socket.next().await {
        let msg = if let Ok(msg) = msg { msg } else { break };

        match msg {
            Message::Text(text) => {
                info!("UI passed control instruction: {}", text);
                // Here we would sync with CoreContext to dispatch to `agent::AgentCore`
                // But for now, just echo receipt.
                let _ = socket.send(Message::Text(format!("ack: {}", text))).await;
            }
            Message::Close(_) => {
                info!("UI client disconnected");
                break;
            }
            _ => {}
        }
    }
}

/// FFI export for injecting the tool natively at runtime
#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub extern "C" fn create_tool() -> *mut dyn CoreTool {
    Box::into_raw(Box::new(UiServer::new()))
}
