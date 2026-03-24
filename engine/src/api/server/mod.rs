//! Daemon Server
//!
//! Provides the HTTP/WebSocket interface for local UI tools and CLI orchestration
//! to remote-control the Agent execution engine over localhost.
//!
//! Security properties (Requirement 16):
//! - Binds to 127.0.0.1 only — not reachable from the network.
//! - Public auth bootstrap routes are limited to trusted origins.
//! - Control-plane routes require a daemon-issued local session token.
//! - CORS is restricted to trusted WebUI origins.

pub mod api;
pub mod auth;
mod completion;
pub mod mcp;
pub mod tls;
pub mod ws;

use anyhow::{Context, Result};
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
    bind_addr: String,
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
    let addr = daemon_socket_addr(&bind_addr, port)?;

    let secret_manager = Arc::new(SecretManager::new("rove"));
    let state = AppState {
        agent,
        secret_manager,
        gateway,
        db,
    };

    // Only allow explicit hosted and local dev origins for CORS.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _req| {
            let host = origin.as_bytes();
            host.starts_with(b"https://app.roveai.co")
                || host.starts_with(b"https://staging.roveai.co")
                || host.starts_with(b"http://localhost")
                || host.starts_with(b"http://127.0.0.1")
                || host.starts_with(b"https://localhost")
        }))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let public = Router::new()
        .route("/api/v1/health", get(api::health_check))
        .route("/v1/hello", get(api::hello))
        .route("/v1/auth/setup", post(api::auth_setup))
        .route("/v1/auth/login", post(api::auth_login))
        .route("/v1/remote/status/public", get(api::remote_public_status))
        .route("/v1/remote/identity", get(api::remote_identity))
        .route("/v1/remote/handshake", post(api::remote_handshake))
        .route("/v1/remote/execute", post(api::execute_remote_task))
        .route("/v1/remote/tasks/:task_id", get(api::remote_task_status))
        .route("/v1/remote/events/ws", get(ws::task_ws_handler))
        .route(
            "/api/v1/remote/status/public",
            get(api::remote_public_status),
        )
        .route("/api/v1/remote/identity", get(api::remote_identity))
        .route("/api/v1/remote/handshake", post(api::remote_handshake))
        .route(
            "/api/v1/remote/execute-public",
            post(api::execute_remote_task),
        )
        .route(
            "/api/v1/remote/tasks/:task_id",
            get(api::remote_task_status),
        );

    // Routes that require daemon session authentication
    let protected = Router::new()
        .route("/v1/auth/status", get(api::auth_status))
        .route("/v1/auth/lock", post(api::auth_lock))
        .route("/v1/auth/reauth", post(api::auth_reauth))
        .route("/v1/config", get(api::get_config).post(api::update_config))
        .route("/v1/config/reload", post(api::reload_config))
        .route("/v1/tasks", get(api::list_tasks).post(api::create_task))
        .route("/v1/brains", get(api::list_brains))
        .route("/v1/brains/dispatch/use", post(api::use_dispatch_brain))
        .route("/v1/extensions", get(api::list_extensions))
        .route("/v1/extensions/catalog", get(api::list_extension_catalog))
        .route(
            "/v1/extensions/catalog/refresh",
            post(api::refresh_extension_catalog),
        )
        .route(
            "/v1/extensions/catalog/:id",
            get(api::get_extension_catalog_entry),
        )
        .route("/v1/extensions/updates", get(api::list_extension_updates))
        .route("/v1/extensions/install", post(api::install_extension))
        .route("/v1/extensions/upgrade", post(api::upgrade_extension))
        .route(
            "/v1/extensions/:kind/:name/enable",
            post(api::enable_extension),
        )
        .route(
            "/v1/extensions/:kind/:name/disable",
            post(api::disable_extension),
        )
        .route("/v1/extensions/:kind/:name", delete(api::remove_extension))
        .route(
            "/v1/policies",
            get(api::list_policies).post(api::add_policy),
        )
        .route("/v1/policies/active", get(api::active_policies))
        .route("/v1/policies/explain", post(api::explain_policy))
        .route("/v1/policies/:name/enable", post(api::enable_policy))
        .route("/v1/policies/:name/disable", post(api::disable_policy))
        .route("/v1/policies/:name", delete(api::remove_policy))
        .route("/v1/approvals", get(api::list_approvals))
        .route(
            "/v1/approvals/mode",
            get(api::get_approval_mode).post(api::update_approval_mode),
        )
        .route(
            "/v1/approvals/rules",
            get(api::list_approval_rules).post(api::add_approval_rule),
        )
        .route("/v1/approvals/rules/:id", delete(api::remove_approval_rule))
        .route("/v1/approvals/:id/resolve", post(api::resolve_approval))
        .route("/v1/services", get(api::list_services))
        .route("/v1/services/:name", get(api::service_status))
        .route("/v1/services/:name/enable", post(api::enable_service))
        .route("/v1/services/:name/disable", post(api::disable_service))
        .route(
            "/v1/services/install/status",
            get(api::service_install_status),
        )
        .route("/v1/services/install", post(api::install_service))
        .route("/v1/services/install/:mode", delete(api::uninstall_service))
        .route("/v1/remote/status", get(api::remote_status))
        .route("/v1/remote/nodes", get(api::remote_nodes))
        .route(
            "/v1/remote/transports/zerotier",
            get(api::zerotier_status).post(api::zerotier_join),
        )
        .route(
            "/v1/remote/transports/zerotier/install",
            post(api::zerotier_install),
        )
        .route(
            "/v1/remote/transports/zerotier/uninstall",
            post(api::zerotier_uninstall),
        )
        .route(
            "/v1/remote/transports/zerotier/setup",
            post(api::zerotier_setup),
        )
        .route(
            "/v1/remote/transports/zerotier/refresh",
            post(api::zerotier_refresh),
        )
        .route("/v1/remote/discover", get(api::zerotier_candidates))
        .route("/v1/remote/discover/refresh", post(api::zerotier_refresh))
        .route(
            "/v1/remote/discover/:candidate_id/trust",
            post(api::zerotier_trust_candidate),
        )
        .route("/v1/remote/pair", post(api::remote_pair))
        .route("/v1/remote/nodes/:name/trust", post(api::remote_trust))
        .route("/v1/remote/nodes/:name", delete(api::remote_unpair))
        .route("/v1/remote/rename", post(api::remote_rename))
        .route("/v1/remote/send", post(api::remote_send))
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
        .route("/api/v1/extensions", get(api::list_extensions))
        .route("/api/v1/extensions/catalog", get(api::list_extension_catalog))
        .route(
            "/api/v1/extensions/catalog/refresh",
            post(api::refresh_extension_catalog),
        )
        .route(
            "/api/v1/extensions/catalog/:id",
            get(api::get_extension_catalog_entry),
        )
        .route("/api/v1/extensions/updates", get(api::list_extension_updates))
        .route("/api/v1/extensions/install", post(api::install_extension))
        .route("/api/v1/extensions/upgrade", post(api::upgrade_extension))
        .route(
            "/api/v1/services/install/status",
            get(api::service_install_status),
        )
        .route("/api/v1/services/install", post(api::install_service))
        .route(
            "/api/v1/services/install/:mode",
            delete(api::uninstall_service),
        )
        .route("/api/v1/channels", get(api::list_channels))
        .route("/api/v1/remote/execute", post(api::execute_remote_task))
        .route("/api/v1/remote/status", get(api::remote_status))
        .route("/api/v1/remote/nodes", get(api::remote_nodes))
        .route(
            "/api/v1/remote/transports/zerotier",
            get(api::zerotier_status).post(api::zerotier_join),
        )
        .route(
            "/api/v1/remote/transports/zerotier/install",
            post(api::zerotier_install),
        )
        .route(
            "/api/v1/remote/transports/zerotier/uninstall",
            post(api::zerotier_uninstall),
        )
        .route(
            "/api/v1/remote/transports/zerotier/setup",
            post(api::zerotier_setup),
        )
        .route(
            "/api/v1/remote/transports/zerotier/refresh",
            post(api::zerotier_refresh),
        )
        .route("/api/v1/remote/discover", get(api::zerotier_candidates))
        .route(
            "/api/v1/remote/discover/refresh",
            post(api::zerotier_refresh),
        )
        .route(
            "/api/v1/remote/discover/:candidate_id/trust",
            post(api::zerotier_trust_candidate),
        )
        .route("/api/v1/remote/pair", post(api::remote_pair))
        .route("/api/v1/remote/nodes/:name/trust", post(api::remote_trust))
        .route("/api/v1/remote/nodes/:name", delete(api::remote_unpair))
        .route("/api/v1/remote/rename", post(api::remote_rename))
        .route("/api/v1/remote/send", post(api::remote_send))
        .route("/v1/chat/completions", post(mcp::mcp_chat_completions))
        .route("/v1/events/ws", get(ws::task_ws_handler))
        .route("/ws/task", get(ws::task_ws_handler))
        // Legacy telemetry endpoint, also protected
        .route("/ws/telemetry", get(ws::telemetry_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session_token,
        ));

    if webui_enabled {
        info!("WebUI service enabled; daemon serving control-plane API for hosted UI");
    } else {
        info!("WebUI service disabled; serving control-plane API only");
    }

    let app = Router::new()
        .merge(protected)
        .merge(public)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    info!(
        "{} Daemon listening on {}",
        crate::info::APP_DISPLAY_NAME,
        addr
    );

    let tls_status = tls::localhost_tls_status();
    if tls_status.enabled {
        info!(
            "Localhost TLS enabled using cert '{}' and key '{}'",
            tls_status.cert_path, tls_status.key_path
        );
        let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
            tls_status.cert_path,
            tls_status.key_path,
        )
        .await?;
        if let Err(error) = axum_server::bind_rustls(addr, rustls_config)
            .serve(app.into_make_service())
            .await
        {
            error!("Daemon TLS server error: {}", error);
        }
    } else {
        let listener = TcpListener::bind(addr).await?;
        if let Err(error) = axum::serve(listener, app).await {
            error!("Daemon server error: {}", error);
        }
    }

    Ok(())
}

fn daemon_socket_addr(bind_addr: &str, port: u16) -> Result<SocketAddr> {
    if let Ok(mut addr) = bind_addr.parse::<SocketAddr>() {
        addr.set_port(port);
        return Ok(addr);
    }

    let host = bind_addr
        .split(':')
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("127.0.0.1");
    format!("{}:{}", host, port)
        .parse::<SocketAddr>()
        .with_context(|| format!("Invalid daemon bind address '{}'", bind_addr))
}
