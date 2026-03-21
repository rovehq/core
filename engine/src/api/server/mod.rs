//! Daemon Server
//!
//! Provides the HTTP/WebSocket interface for local UI tools and CLI orchestration
//! to remote-control the Agent execution engine over localhost.
//!
//! Security properties (Requirement 16):
//! - Binds to 127.0.0.1 only — not reachable from the network.
//! - All routes are protected by a bearer token stored in the OS keychain.
//! - CORS is restricted to localhost origins.

pub mod api;
pub mod auth;
mod completion;
pub mod mcp;
pub mod ws;

use anyhow::Result;
use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{error, info};

use crate::agent::AgentCore;
use crate::db::Database;
use crate::gateway::Gateway;
use crate::secrets::SecretManager;

/// Application state shared across all HTTP requests
#[derive(Clone)]
pub struct AppState {
    pub agent: Arc<RwLock<AgentCore>>,
    pub secret_manager: Arc<SecretManager>,
    pub gateway: Arc<Gateway>,
    pub db: Arc<Database>,
}

/// Start the background HTTP daemon on the specified port.
///
/// Binds to 127.0.0.1 only. All API routes require a valid bearer token
/// except for the `/api/v1/health` endpoint which is public.
pub async fn start_daemon(
    agent: Arc<RwLock<AgentCore>>,
    port: u16,
    db: Arc<Database>,
    gateway: Arc<Gateway>,
    webui_enabled: bool,
) -> Result<()> {
    let port = if port == 0 {
        std::env::var("ROVE_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(crate::info::DEFAULT_PORT)
    } else {
        port
    };

    let secret_manager = Arc::new(SecretManager::new("rove"));
    let state = AppState {
        agent,
        secret_manager,
        gateway,
        db,
    };

    // Only allow localhost origins for CORS
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _req| {
            let host = origin.as_bytes();
            host.starts_with(b"http://localhost")
                || host.starts_with(b"http://127.0.0.1")
                || host.starts_with(b"https://localhost")
        }))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    // Routes that require bearer token authentication
    let protected = Router::new()
        .route("/api/run", post(api::execute_task))
        .route("/api/v1/execute", post(api::execute_task))
        .route("/api/v1/tasks/:task_id", get(api::task_status))
        .route("/api/v1/policy/active", get(api::active_policies))
        .route("/api/v1/steering/active", get(api::active_policies))
        .route("/api/v1/policies", get(api::list_policies))
        .route("/api/v1/policies/active", get(api::active_policies))
        .route("/api/v1/policies/explain", post(api::explain_policy))
        .route("/api/v1/services", get(api::list_services))
        .route("/api/v1/services/:name", get(api::service_status))
        .route("/api/v1/services/:name/enable", post(api::enable_service))
        .route("/api/v1/services/:name/disable", post(api::disable_service))
        .route("/api/v1/channels", get(api::list_channels))
        .route("/api/v1/remote/execute", post(api::execute_remote_task))
        .route("/api/v1/remote/status", get(api::remote_status))
        .route("/api/v1/remote/nodes", get(api::remote_nodes))
        .route("/api/v1/remote/pair", post(api::remote_pair))
        .route("/api/v1/remote/nodes/:name/trust", post(api::remote_trust))
        .route("/api/v1/remote/nodes/:name", delete(api::remote_unpair))
        .route("/api/v1/remote/rename", post(api::remote_rename))
        .route("/api/v1/remote/send", post(api::remote_send))
        .route("/v1/chat/completions", post(mcp::mcp_chat_completions))
        .route("/ws/task", get(ws::task_ws_handler))
        // Legacy telemetry endpoint, also protected
        .route("/ws/telemetry", get(ws::telemetry_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer_token,
        ));

    // Health check is always public (for process monitoring)
    // Serve WebUI static files from Next.js build
    let public = if webui_enabled {
        let webui_dir = std::env::current_exe()
            .ok()
            .and_then(|p| {
                p.parent()
                    .map(|p| p.join("../../webui/dist").canonicalize().ok())
            })
            .flatten()
            .or_else(|| {
                std::path::PathBuf::from("core/webui/dist")
                    .canonicalize()
                    .ok()
            })
            .unwrap_or_else(|| std::path::PathBuf::from("core/webui"));

        info!("Serving WebUI from: {:?}", webui_dir);

        Router::new()
            .route("/api/v1/health", get(api::health_check))
            .nest_service(
                "/",
                ServeDir::new(&webui_dir).append_index_html_on_directories(true),
            )
    } else {
        info!("WebUI service disabled; serving control-plane API only");
        Router::new().route("/api/v1/health", get(api::health_check))
    };

    let app = Router::new()
        .merge(protected)
        .merge(public)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!(
        "{} Daemon listening on {} (localhost only)",
        crate::info::APP_DISPLAY_NAME,
        addr
    );

    let listener = TcpListener::bind(addr).await?;
    if let Err(e) = axum::serve(listener, app).await {
        error!("Daemon server error: {}", e);
    }

    Ok(())
}
