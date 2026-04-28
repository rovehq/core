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
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, RwLock};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

use crate::agent::AgentCore;
use crate::cli::bootstrap::build_tools;
use crate::cli::database_path::{database_path, expand_data_dir};
use crate::config::Config;
use crate::db::Database;
use crate::gateway::Gateway;
use crate::message_bus::MessageBus;
use crate::secrets::SecretManager;
use crate::system::{runtime_state, workflow_triggers};

/// Application state shared across all HTTP requests
#[derive(Clone)]
pub struct AppState {
    pub agent: Arc<RwLock<AgentCore>>,
    pub secret_manager: Arc<SecretManager>,
    pub gateway: Arc<Gateway>,
    pub db: Arc<Database>,
    pub message_bus: Arc<MessageBus>,
}

/// Start the background HTTP daemon on the specified port.
///
/// Binds to 127.0.0.1 only. All API routes require a valid bearer token
/// except for the `/api/v1/health` endpoint which is public.
pub async fn start_daemon(
    agent: Arc<RwLock<AgentCore>>,
    port: u16,
    bind_addr: String,
    config: Config,
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
    let message_bus = Arc::new(MessageBus::new());
    let state = AppState {
        agent,
        secret_manager,
        gateway,
        db,
        message_bus: Arc::clone(&message_bus),
    };
    state
        .agent
        .write()
        .await
        .set_message_bus(Arc::clone(&message_bus));
    spawn_extension_hot_reload_watcher(
        Arc::clone(&state.agent),
        Arc::clone(&state.db),
        config.clone(),
    );
    spawn_workflow_file_watch_watcher(config.clone());
    spawn_workflow_cron_scheduler(Arc::clone(&state.db), config.clone());
    crate::system::auto_update::spawn_auto_update_scheduler();

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
        .route("/metrics", get(api::metrics))
        .route("/v1/hello", get(api::hello))
        .route("/v1/auth/setup", post(api::auth_setup))
        .route("/v1/auth/login", post(api::auth_login))
        .route("/v1/auth/passkeys/status", get(api::auth_passkey_status))
        .route(
            "/v1/auth/passkeys/login/start",
            post(api::auth_passkey_login_start),
        )
        .route(
            "/v1/auth/passkeys/login/finish",
            post(api::auth_passkey_login_finish),
        )
        .route("/v1/remote/status/public", get(api::remote_public_status))
        .route("/v1/remote/identity", get(api::remote_identity))
        .route("/v1/remote/handshake", post(api::remote_handshake))
        .route("/v1/remote/execute", post(api::execute_remote_task))
        .route("/v1/remote/tasks/:task_id", get(api::remote_task_status))
        .route("/v1/remote/events/ws", get(ws::task_ws_handler))
        .route("/v1/remote/presence", post(api::handle_remote_presence))
        .route("/v1/remote/terminal", get(ws::handle_remote_terminal))
        .route(
            "/v1/workflows/webhooks/:webhook_id",
            post(api::invoke_workflow_webhook),
        )
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

    let protected_websockets = Router::new()
        .route("/v1/events/ws", get(ws::task_ws_handler))
        .route("/ws/task", get(ws::task_ws_handler))
        .route("/ws/telemetry", get(ws::telemetry_handler));

    // Routes that require daemon session authentication
    let protected = Router::new()
        .route("/v1/auth/status", get(api::auth_status))
        .route("/v1/auth/lock", post(api::auth_lock))
        .route("/v1/auth/reauth", post(api::auth_reauth))
        .route("/v1/auth/passkeys", get(api::list_passkeys))
        .route(
            "/v1/auth/passkeys/register/start",
            post(api::start_passkey_registration),
        )
        .route(
            "/v1/auth/passkeys/register/finish",
            post(api::finish_passkey_registration),
        )
        .route(
            "/v1/auth/passkeys/reauth/start",
            post(api::start_passkey_reauth),
        )
        .route(
            "/v1/auth/passkeys/reauth/finish",
            post(api::finish_passkey_reauth),
        )
        .route("/v1/auth/passkeys/:id", delete(api::remove_passkey))
        .route("/v1/config", get(api::get_config).post(api::update_config))
        .route(
            "/v1/browser",
            get(api::browser_status).put(api::update_browser),
        )
        .route(
            "/v1/memory",
            get(api::memory_status).put(api::update_memory),
        )
        .route("/v1/memory/status", get(api::memory_status))
        .route("/v1/memory/query", post(api::query_memory))
        .route("/v1/memory/graph", get(api::inspect_memory_graph))
        .route("/v1/memory/reindex", post(api::reindex_memory))
        .route("/v1/memory/backfill", post(api::backfill_memory_embeddings))
        .route("/v1/memory/adapters", get(api::memory_adapters))
        .route(
            "/v1/memory/adapters/refresh",
            post(api::refresh_memory_adapters),
        )
        .route("/v1/memory/ingest", post(api::ingest_memory_note))
        .route("/v1/memory/episodic", get(api::list_episodic_memories))
        .route(
            "/v1/memory/episodic/:id/history",
            get(api::episodic_memory_history),
        )
        .route(
            "/v1/memory/episodic/:id/redact",
            post(api::redact_episodic_memory),
        )
        .route(
            "/v1/memory/episodic/:id",
            delete(api::delete_episodic_memory),
        )
        .route("/v1/memory/facts", get(api::list_memory_facts))
        .route(
            "/v1/memory/facts/:key/history",
            get(api::fact_memory_history),
        )
        .route(
            "/v1/memory/facts/:key/redact",
            post(api::redact_memory_fact),
        )
        .route("/v1/memory/facts/:key", delete(api::delete_memory_fact))
        .route("/v1/hooks", get(api::hook_status))
        .route("/v1/hooks/:name", get(api::inspect_hook))
        .route("/v1/voice", get(api::voice_status).put(api::update_voice))
        .route("/v1/voice/install", post(api::install_voice_engine))
        .route("/v1/voice/uninstall", post(api::uninstall_voice_engine))
        .route("/v1/voice/activate-input", post(api::activate_voice_input))
        .route(
            "/v1/voice/activate-output",
            post(api::activate_voice_output),
        )
        .route("/v1/voice/test-input", post(api::test_voice_input))
        .route("/v1/voice/test-output", post(api::test_voice_output))
        .route("/v1/config/reload", post(api::reload_config))
        .route("/v1/overview", get(api::overview))
        .route("/v1/update/available", get(api::update_available))
        .route("/v1/health/snapshot", get(api::health_snapshot))
        .route("/v1/audit", get(api::list_audit_log))
        .route("/v1/logs/recent", get(api::recent_logs))
        .route("/v1/logs/stream", get(api::stream_logs))
        .route("/v1/backups/export", post(api::export_backup))
        .route("/v1/backups/restore", post(api::restore_backup))
        .route("/v1/migrate/:source/inspect", post(api::inspect_migration))
        .route("/v1/migrate/:source/import", post(api::import_migration))
        .route("/v1/migrate/status", get(api::migration_status))
        .route("/v1/tasks", get(api::list_tasks).post(api::create_task))
        .route("/v1/tasks/agents", get(api::list_task_agents))
        .route("/v1/tasks/:task_id/events", get(api::get_task_events))
        .route("/v1/tasks/:task_id/stream", get(api::stream_task_events))
        .route("/v1/agents", get(api::list_agents).post(api::create_agent))
        .route(
            "/v1/managed-agents/agents",
            get(api::list_managed_agents).post(api::create_managed_agent),
        )
        .route(
            "/v1/managed-agents/agents/:id",
            get(api::get_managed_agent),
        )
        .route(
            "/v1/managed-agents/environments",
            get(api::list_managed_agent_environments),
        )
        .route(
            "/v1/managed-agents/environments/:id",
            get(api::get_managed_agent_environment),
        )
        .route(
            "/v1/managed-agents/sessions",
            get(api::list_managed_agent_sessions).post(api::create_managed_agent_session),
        )
        .route(
            "/v1/managed-agents/sessions/:id",
            get(api::get_managed_agent_session),
        )
        .route(
            "/v1/managed-agents/sessions/:id/wake",
            post(api::wake_managed_agent_session),
        )
        .route(
            "/v1/managed-agents/sessions/:id/messages",
            post(api::send_managed_agent_session_message),
        )
        .route(
            "/v1/managed-agents/sessions/:id/events",
            get(api::list_managed_agent_session_events),
        )
        .route("/v1/agents/templates", get(api::list_agent_templates))
        .route(
            "/v1/agents/factory/preview",
            post(api::preview_agent_factory),
        )
        .route("/v1/agents/factory/create", post(api::create_agent_factory))
        .route(
            "/v1/agents/from-task/:task_id",
            post(api::create_agent_from_task),
        )
        .route("/v1/agents/:id/review", get(api::get_agent_review))
        .route("/v1/agents/:id/approve", post(api::approve_agent_factory))
        .route("/v1/agents/runs", get(api::list_agent_runs))
        .route(
            "/v1/agents/:id",
            get(api::get_agent)
                .put(api::update_agent)
                .delete(api::remove_agent),
        )
        .route("/v1/agents/:id/run", post(api::run_agent))
        .route("/v1/agents/:id/threads", get(api::list_agent_threads))
        .route("/v1/threads", get(api::list_threads))
        .route("/v1/threads/:thread_id", get(api::get_thread))
        .route(
            "/v1/workflows",
            get(api::list_workflows).post(api::create_workflow),
        )
        .route("/v1/starters", get(api::list_starters))
        .route("/v1/workers/presets", get(api::list_worker_presets))
        .route("/v1/workflows/templates", get(api::list_workflow_templates))
        .route(
            "/v1/workflows/factory/preview",
            post(api::preview_workflow_factory),
        )
        .route(
            "/v1/workflows/factory/create",
            post(api::create_workflow_factory),
        )
        .route(
            "/v1/workflows/from-task/:task_id",
            post(api::create_workflow_from_task),
        )
        .route("/v1/workflows/:id/review", get(api::get_workflow_review))
        .route(
            "/v1/workflows/:id/approve",
            post(api::approve_workflow_factory),
        )
        .route("/v1/workflows/runs", get(api::list_workflow_runs))
        .route("/v1/workflows/runs/:run_id", get(api::get_workflow_run))
        .route(
            "/v1/workflows/runs/:run_id/resume",
            post(api::resume_workflow_run),
        )
        .route(
            "/v1/workflows/runs/:run_id/cancel",
            post(api::cancel_workflow_run),
        )
        .route(
            "/v1/workflows/:id",
            get(api::get_workflow)
                .put(api::update_workflow)
                .delete(api::remove_workflow),
        )
        .route("/v1/workflows/:id/run", post(api::run_workflow))
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
        .route("/v1/channels", get(api::list_channels))
        .route("/v1/channels/plugins", get(api::plugin_channel_statuses))
        .route(
            "/v1/channels/plugins/:name/deliver",
            post(api::plugin_channel_deliver),
        )
        .route("/v1/channels/telegram", get(api::telegram_channel_status))
        .route(
            "/v1/channels/telegram/setup",
            post(api::telegram_channel_setup),
        )
        .route(
            "/v1/channels/telegram/enable",
            post(api::telegram_channel_enable),
        )
        .route(
            "/v1/channels/telegram/disable",
            post(api::telegram_channel_disable),
        )
        .route(
            "/v1/channels/telegram/test",
            post(api::telegram_channel_test),
        )
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
        .route("/api/v1/overview", get(api::overview))
        .route("/api/v1/update/available", get(api::update_available))
        .route(
            "/api/v1/browser",
            get(api::browser_status).put(api::update_browser),
        )
        .route(
            "/api/v1/memory",
            get(api::memory_status).put(api::update_memory),
        )
        .route("/api/v1/memory/status", get(api::memory_status))
        .route("/api/v1/memory/query", post(api::query_memory))
        .route("/api/v1/memory/graph", get(api::inspect_memory_graph))
        .route("/api/v1/memory/reindex", post(api::reindex_memory))
        .route("/api/v1/memory/ingest", post(api::ingest_memory_note))
        .route(
            "/api/v1/voice",
            get(api::voice_status).put(api::update_voice),
        )
        .route("/api/v1/voice/install", post(api::install_voice_engine))
        .route("/api/v1/voice/uninstall", post(api::uninstall_voice_engine))
        .route(
            "/api/v1/voice/activate-input",
            post(api::activate_voice_input),
        )
        .route(
            "/api/v1/voice/activate-output",
            post(api::activate_voice_output),
        )
        .route("/api/v1/voice/test-input", post(api::test_voice_input))
        .route("/api/v1/voice/test-output", post(api::test_voice_output))
        .route("/api/v1/health/snapshot", get(api::health_snapshot))
        .route("/api/v1/audit", get(api::list_audit_log))
        .route("/api/v1/logs/recent", get(api::recent_logs))
        .route("/api/v1/logs/stream", get(api::stream_logs))
        .route("/api/v1/backups/export", post(api::export_backup))
        .route("/api/v1/backups/restore", post(api::restore_backup))
        .route(
            "/api/v1/migrate/:source/inspect",
            post(api::inspect_migration),
        )
        .route(
            "/api/v1/migrate/:source/import",
            post(api::import_migration),
        )
        .route("/api/v1/migrate/status", get(api::migration_status))
        .route(
            "/api/v1/agents",
            get(api::list_agents).post(api::create_agent),
        )
        .route("/api/v1/agents/templates", get(api::list_agent_templates))
        .route(
            "/api/v1/agents/factory/preview",
            post(api::preview_agent_factory),
        )
        .route(
            "/api/v1/agents/factory/create",
            post(api::create_agent_factory),
        )
        .route(
            "/api/v1/agents/from-task/:task_id",
            post(api::create_agent_from_task),
        )
        .route("/api/v1/agents/:id/review", get(api::get_agent_review))
        .route(
            "/api/v1/agents/:id/approve",
            post(api::approve_agent_factory),
        )
        .route("/api/v1/agents/runs", get(api::list_agent_runs))
        .route(
            "/api/v1/agents/:id",
            get(api::get_agent)
                .put(api::update_agent)
                .delete(api::remove_agent),
        )
        .route("/api/v1/agents/:id/run", post(api::run_agent))
        .route(
            "/api/v1/agents/:id/threads",
            get(api::list_agent_threads),
        )
        .route(
            "/api/v1/managed-agents",
            get(api::list_managed_agents).post(api::create_managed_agent),
        )
        .route("/api/v1/managed-agents/:id", get(api::get_managed_agent))
        .route(
            "/api/v1/managed-agents/environments",
            get(api::list_managed_agent_environments),
        )
        .route(
            "/api/v1/managed-agents/environments/:id",
            get(api::get_managed_agent_environment),
        )
        .route(
            "/api/v1/managed-agents/sessions",
            get(api::list_managed_agent_sessions).post(api::create_managed_agent_session),
        )
        .route(
            "/api/v1/managed-agents/sessions/:id",
            get(api::get_managed_agent_session),
        )
        .route(
            "/api/v1/managed-agents/sessions/:id/wake",
            post(api::wake_managed_agent_session),
        )
        .route(
            "/api/v1/managed-agents/sessions/:id/message",
            post(api::send_managed_agent_session_message),
        )
        .route(
            "/api/v1/managed-agents/sessions/:id/events",
            get(api::list_managed_agent_session_events),
        )
        .route(
            "/api/v1/agent-threads",
            get(api::list_agent_threads_all),
        )
        .route(
            "/api/v1/agent-threads/:id",
            get(api::get_agent_thread),
        )
        .route(
            "/api/v1/agent-threads/:id/events",
            get(api::list_agent_thread_events),
        )
        .route(
            "/api/v1/workflows",
            get(api::list_workflows).post(api::create_workflow),
        )
        .route(
            "/api/v1/workflows/templates",
            get(api::list_workflow_templates),
        )
        .route(
            "/api/v1/workflows/factory/preview",
            post(api::preview_workflow_factory),
        )
        .route(
            "/api/v1/workflows/factory/create",
            post(api::create_workflow_factory),
        )
        .route(
            "/api/v1/workflows/from-task/:task_id",
            post(api::create_workflow_from_task),
        )
        .route(
            "/api/v1/workflows/:id/review",
            get(api::get_workflow_review),
        )
        .route(
            "/api/v1/workflows/:id/approve",
            post(api::approve_workflow_factory),
        )
        .route("/api/v1/workflows/runs", get(api::list_workflow_runs))
        .route("/api/v1/workflows/runs/:run_id", get(api::get_workflow_run))
        .route(
            "/api/v1/workflows/runs/:run_id/resume",
            post(api::resume_workflow_run),
        )
        .route(
            "/api/v1/workflows/runs/:run_id/cancel",
            post(api::cancel_workflow_run),
        )
        .route(
            "/api/v1/workflows/:id",
            get(api::get_workflow)
                .put(api::update_workflow)
                .delete(api::remove_workflow),
        )
        .route("/api/v1/workflows/:id/run", post(api::run_workflow))
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
        .route(
            "/api/v1/extensions/catalog",
            get(api::list_extension_catalog),
        )
        .route(
            "/api/v1/extensions/catalog/refresh",
            post(api::refresh_extension_catalog),
        )
        .route(
            "/api/v1/extensions/catalog/:id",
            get(api::get_extension_catalog_entry),
        )
        .route(
            "/api/v1/extensions/updates",
            get(api::list_extension_updates),
        )
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
        .route(
            "/api/v1/channels/plugins",
            get(api::plugin_channel_statuses),
        )
        .route(
            "/api/v1/channels/plugins/:name/deliver",
            post(api::plugin_channel_deliver),
        )
        .route(
            "/api/v1/channels/telegram",
            get(api::telegram_channel_status),
        )
        .route(
            "/api/v1/channels/telegram/setup",
            post(api::telegram_channel_setup),
        )
        .route(
            "/api/v1/channels/telegram/enable",
            post(api::telegram_channel_enable),
        )
        .route(
            "/api/v1/channels/telegram/disable",
            post(api::telegram_channel_disable),
        )
        .route(
            "/api/v1/channels/telegram/test",
            post(api::telegram_channel_test),
        )
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
        .route(
            "/api/v1/knowledge",
            get(api::list_knowledge),
        )
        .route(
            "/api/v1/knowledge/stats",
            get(api::knowledge_stats),
        )
        .route(
            "/api/v1/knowledge/search",
            get(api::search_knowledge),
        )
        .route(
            "/api/v1/knowledge/ingest/upload",
            post(api::ingest_knowledge_upload),
        )
        .route(
            "/api/v1/knowledge/ingest/file",
            post(api::ingest_knowledge_file),
        )
        .route(
            "/api/v1/knowledge/ingest/folder",
            post(api::ingest_knowledge_folder),
        )
        .route(
            "/api/v1/knowledge/ingest/url",
            post(api::ingest_knowledge_url),
        )
        .route(
            "/api/v1/knowledge/ingest/sitemap",
            post(api::ingest_knowledge_sitemap),
        )
        .route(
            "/api/v1/knowledge/:id",
            get(api::get_knowledge).delete(api::remove_knowledge),
        )
        .route(
            "/api/v1/knowledge/jobs",
            get(api::list_knowledge_jobs),
        )
        .route(
            "/api/v1/knowledge/jobs/:id",
            get(api::get_knowledge_job),
        )
        .route("/v1/chat/completions", post(mcp::mcp_chat_completions))
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
        .merge(protected_websockets)
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
        let state_record = runtime_state::build_record(&bind_addr, addr, port, true, webui_enabled);
        runtime_state::write(&config, &state_record)?;
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
        if let Err(error) = runtime_state::clear(&config) {
            error!("Failed to clear daemon runtime state: {}", error);
        }
    } else {
        let listener = TcpListener::bind(addr).await?;
        let state_record =
            runtime_state::build_record(&bind_addr, addr, port, false, webui_enabled);
        runtime_state::write(&config, &state_record)?;
        if let Err(error) = axum::serve(listener, app).await {
            error!("Daemon server error: {}", error);
        }
        if let Err(error) = runtime_state::clear(&config) {
            error!("Failed to clear daemon runtime state: {}", error);
        }
    }

    Ok(())
}

fn spawn_extension_hot_reload_watcher(
    agent: Arc<RwLock<AgentCore>>,
    db: Arc<Database>,
    config: Config,
) {
    let db_path = database_path(&config);
    let data_dir = expand_data_dir(&config.core.data_dir);
    let plugin_dir = data_dir.join("plugins");

    tokio::spawn(async move {
        let (event_tx, mut event_rx) = mpsc::channel(100);

        let watcher_res = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    if is_extension_reload_event(&event, &db_path, &plugin_dir) {
                        let _ = event_tx.blocking_send(());
                    }
                }
            },
            NotifyConfig::default(),
        );

        let mut watcher = match watcher_res {
            Ok(watcher) => watcher,
            Err(error) => {
                warn!(
                    "Failed to initialize extension hot-reload watcher: {}",
                    error
                );
                return;
            }
        };

        if let Err(error) = watcher.watch(&data_dir, RecursiveMode::Recursive) {
            warn!(
                "Failed to watch '{}' for extension hot-reload: {}",
                data_dir.display(),
                error
            );
            return;
        }

        info!(
            "Watching '{}' for installed plugin changes",
            data_dir.display()
        );

        loop {
            tokio::select! {
                Some(_) = event_rx.recv() => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    while event_rx.try_recv().is_ok() {}

                    match build_tools(db.as_ref(), &config).await {
                        Ok(tools) => {
                            agent.write().await.set_tools(tools);
                            info!("Hot-reloaded daemon tool registry after extension change");
                        }
                        Err(error) => {
                            warn!("Failed to hot-reload daemon tool registry: {}", error);
                        }
                    }
                }
                else => break,
            }
        }
    });
}

fn spawn_workflow_file_watch_watcher(config: Config) {
    let db_path = database_path(&config);
    let workspace_root = PathBuf::from(&config.core.workspace);

    tokio::spawn(async move {
        let db = match crate::storage::Database::new(&db_path).await {
            Ok(db) => db,
            Err(error) => {
                warn!(
                    "Failed to initialize workflow file-watch database '{}': {}",
                    db_path.display(),
                    error
                );
                return;
            }
        };

        let (event_tx, mut event_rx) = mpsc::channel(100);
        let watcher_res = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                let _ = event_tx.blocking_send(res);
            },
            NotifyConfig::default(),
        );

        let mut watcher = match watcher_res {
            Ok(watcher) => watcher,
            Err(error) => {
                warn!(
                    "Failed to initialize workflow file-watch watcher: {}",
                    error
                );
                return;
            }
        };
        let mut watched_paths: HashMap<PathBuf, RecursiveMode> = HashMap::new();

        loop {
            if let Err(error) =
                sync_workflow_file_watch_paths(&mut watcher, &mut watched_paths, &workspace_root)
            {
                warn!("Failed to refresh workflow file-watch paths: {}", error);
            }

            tokio::select! {
                Some(res) = event_rx.recv() => match res {
                    Ok(event) => {
                        let Some(event_kind) = workflow_file_watch_event_kind(&event.kind) else {
                            continue;
                        };
                        let Some(changed_path) = event.paths.first().cloned() else {
                            continue;
                        };
                        let repo = match crate::specs::SpecRepository::new() {
                            Ok(repo) => repo,
                            Err(error) => {
                                warn!("Failed to load workflow specs for file-watch event: {}", error);
                                continue;
                            }
                        };
                        let input = format!(
                            "File watch trigger.\nEvent: {}\nPath: {}",
                            event_kind,
                            changed_path.display()
                        );
                        match workflow_triggers::trigger_matching_file_watch_workflows(
                            &repo,
                            &db,
                            &config,
                            &workspace_root,
                            &changed_path,
                            event_kind,
                            &input,
                        )
                        .await
                        {
                            Ok(triggered) if !triggered.is_empty() => {
                                info!(
                                    "Triggered {} workflow(s) from file-watch event {}:{}",
                                    triggered.len(),
                                    event_kind,
                                    changed_path.display()
                                );
                            }
                            Ok(_) => {}
                            Err(error) => {
                                warn!(
                                    "Workflow file-watch trigger failed for {}:{}: {}",
                                    event_kind,
                                    changed_path.display(),
                                    error
                                );
                            }
                        }
                    }
                    Err(error) => {
                        warn!("Workflow file-watch event error: {}", error);
                    }
                },
                _ = tokio::time::sleep(Duration::from_secs(15)) => {}
            }
        }
    });
}

fn sync_workflow_file_watch_paths(
    watcher: &mut RecommendedWatcher,
    watched_paths: &mut HashMap<PathBuf, RecursiveMode>,
    workspace_root: &Path,
) -> Result<()> {
    let repo = crate::specs::SpecRepository::new()?;
    let registrations = workflow_triggers::collect_file_watch_registrations(&repo, workspace_root)?;
    let mut desired = HashMap::new();

    for registration in registrations {
        if !registration.path.exists() {
            continue;
        }
        let mode = if registration.recursive && registration.path.is_dir() {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        match desired.get(&registration.path) {
            Some(RecursiveMode::Recursive) => {}
            _ => {
                desired.insert(registration.path.clone(), mode);
            }
        }
    }

    let stale_paths: Vec<PathBuf> = watched_paths
        .keys()
        .filter(|path| !desired.contains_key(*path))
        .cloned()
        .collect();
    for path in stale_paths {
        if let Err(error) = watcher.unwatch(&path) {
            warn!(
                "Failed to remove workflow file-watch for '{}': {}",
                path.display(),
                error
            );
        }
        watched_paths.remove(&path);
    }

    for (path, mode) in desired {
        let needs_watch = match watched_paths.get(&path) {
            Some(existing) => existing != &mode,
            None => true,
        };
        if !needs_watch {
            continue;
        }

        if watched_paths.contains_key(&path) {
            let _ = watcher.unwatch(&path);
        }
        let watch_mode = if matches!(mode, RecursiveMode::Recursive) {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        watcher.watch(&path, watch_mode)?;
        watched_paths.insert(path, mode);
    }

    Ok(())
}

fn workflow_file_watch_event_kind(kind: &notify::EventKind) -> Option<&'static str> {
    match kind {
        notify::EventKind::Create(_) => Some("create"),
        notify::EventKind::Modify(_) => Some("modify"),
        notify::EventKind::Remove(_) => Some("remove"),
        _ => None,
    }
}

fn is_extension_reload_event(event: &Event, db_path: &Path, plugin_dir: &Path) -> bool {
    event
        .paths
        .iter()
        .any(|path| path.starts_with(plugin_dir) || matches_database_file(path, db_path))
}

fn matches_database_file(path: &Path, db_path: &Path) -> bool {
    if path == db_path {
        return true;
    }

    let Some(parent) = path.parent() else {
        return false;
    };
    if Some(parent) != db_path.parent() {
        return false;
    }

    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(db_name) = db_path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };

    file_name == db_name
        || file_name == format!("{db_name}-wal")
        || file_name == format!("{db_name}-shm")
}

fn spawn_workflow_cron_scheduler(db: Arc<Database>, config: Config) {
    let db_path = database_path(&config);

    tokio::spawn(async move {
        // Wait until next minute boundary before starting the tick loop
        // so cron expressions fire predictably at wall-clock minute boundaries.
        let now = chrono::Utc::now();
        use chrono::Timelike;
        let secs_until_next_minute = 60 - now.second() as u64;
        tokio::time::sleep(Duration::from_secs(secs_until_next_minute)).await;

        let _ = db; // drop captured Arc; open fresh connection each tick to avoid lifetime issues
        let mut interval = tokio::time::interval(Duration::from_secs(60));

        loop {
            interval.tick().await;
            let tick_time = chrono::Utc::now();
            let one_minute_ago = tick_time - chrono::Duration::seconds(60);

            let repo = match crate::specs::SpecRepository::new() {
                Ok(r) => r,
                Err(e) => {
                    warn!("cron scheduler: failed to load specs: {}", e);
                    continue;
                }
            };
            let fresh_db = match crate::storage::Database::new(&db_path).await {
                Ok(db) => db,
                Err(e) => {
                    warn!("cron scheduler: failed to open db: {}", e);
                    continue;
                }
            };

            let workflows = match repo.list_workflows() {
                Ok(w) => w,
                Err(e) => {
                    warn!("cron scheduler: failed to list workflows: {}", e);
                    continue;
                }
            };

            for workflow in workflows {
                if !workflow.enabled || workflow.schedules.is_empty() {
                    continue;
                }
                for expr in &workflow.schedules {
                    let schedule = match expr.parse::<cron::Schedule>() {
                        Ok(s) => s,
                        Err(e) => {
                            warn!(
                                workflow_id = %workflow.id,
                                expr = %expr,
                                "cron scheduler: invalid cron expression: {}",
                                e
                            );
                            continue;
                        }
                    };
                    // Fire if any scheduled time falls in the [one_minute_ago, tick_time) window.
                    let fired = schedule
                        .after(&one_minute_ago)
                        .next()
                        .is_some_and(|next| next <= tick_time);
                    if !fired {
                        continue;
                    }
                    let input = format!("Scheduled run: {}", workflow.name);
                    match crate::system::workflow_runtime::start_new_run(
                        &repo,
                        &fresh_db,
                        &config,
                        &workflow,
                        &input,
                    )
                    .await
                    {
                        Ok(result) => {
                            info!(
                                workflow_id = %workflow.id,
                                run_id = %result.run.run_id,
                                expr = %expr,
                                "cron trigger fired"
                            );
                        }
                        Err(e) => {
                            warn!(
                                workflow_id = %workflow.id,
                                expr = %expr,
                                "cron trigger failed to start workflow run: {}",
                                e
                            );
                        }
                    }
                    // Only fire each workflow once per tick even if multiple
                    // expressions would match the same minute.
                    break;
                }
            }
        }
    });
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use notify::{
        event::{CreateKind, ModifyKind},
        Event, EventKind,
    };

    use super::{is_extension_reload_event, matches_database_file};

    #[test]
    fn database_file_match_includes_wal_and_shm() {
        let db_path = PathBuf::from("/tmp/rove/rove.db");
        assert!(matches_database_file(
            &PathBuf::from("/tmp/rove/rove.db"),
            &db_path
        ));
        assert!(matches_database_file(
            &PathBuf::from("/tmp/rove/rove.db-wal"),
            &db_path
        ));
        assert!(matches_database_file(
            &PathBuf::from("/tmp/rove/rove.db-shm"),
            &db_path
        ));
        assert!(!matches_database_file(
            &PathBuf::from("/tmp/other/rove.db"),
            &db_path
        ));
    }

    #[test]
    fn extension_reload_event_filters_plugin_and_database_paths() {
        let db_path = PathBuf::from("/tmp/rove/rove.db");
        let plugin_dir = PathBuf::from("/tmp/rove/plugins");
        let event = Event {
            kind: EventKind::Create(CreateKind::File),
            paths: vec![PathBuf::from("/tmp/rove/plugins/demo/runtime.json")],
            attrs: Default::default(),
        };
        assert!(is_extension_reload_event(&event, &db_path, &plugin_dir));

        let unrelated = Event {
            kind: EventKind::Create(CreateKind::File),
            paths: vec![PathBuf::from("/tmp/rove/logs/rove.log")],
            attrs: Default::default(),
        };
        assert!(!is_extension_reload_event(
            &unrelated,
            &db_path,
            &plugin_dir
        ));
    }

    #[test]
    fn extension_reload_event_accepts_plugin_modify_events() {
        let db_path = PathBuf::from("/tmp/rove/rove.db");
        let plugin_dir = PathBuf::from("/tmp/rove/plugins");
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Content)),
            paths: vec![PathBuf::from("/tmp/rove/plugins/demo/demo.wasm")],
            attrs: Default::default(),
        };
        assert!(is_extension_reload_event(&event, &db_path, &plugin_dir));
    }
}
