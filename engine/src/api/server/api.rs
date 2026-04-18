use async_stream::stream;
use axum::{
    body::{Body, Bytes},
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

use super::{auth::AuthManager, completion, AppState};
use crate::channels::manager::{ChannelManager, PluginChannelDeliverInput, TelegramSetupInput};
use crate::cli::brain::dispatch_family;
use crate::cli::database_path::database_path;
use crate::cli::extensions;
use crate::config::Config;
use crate::gateway::Task;
pub use crate::message_bus::TaskStreamEvent;
use crate::message_bus::{Event as BusEvent, EventType as BusEventType};
use crate::policy::PolicyManager;
use crate::remote::RemoteManager;
use crate::security::approvals;
use crate::service_install::{ServiceInstallMode, ServiceInstaller};
use crate::services::{ManagedService, ServiceManager};
use crate::specs::{allowed_tools, SpecRepository};
use crate::system::{
    backup, browser as browser_surface, factory, health, logs, memory as memory_surface,
    metrics as prometheus_metrics, migrate, onboarding, starter_catalog, voice as voice_surface,
    worker_presets, workflow_runtime, workflow_triggers,
};
use crate::targeting::extract_task_target;
use crate::zerotier::ZeroTierManager;
use sdk::{
    AgentSpec, AuthState, DaemonCapabilities, DaemonHello, NodeLoadSnapshot, NodeSummary,
    PasskeyFinishRequest, PasskeyRegistrationStartRequest, PolicyScope, RemoteExecutionPlan,
    RunContextId, RunIsolation, RunMode, SpecRunStatus, TaskExecutionProfile, TaskSource,
    WorkflowSpec,
};

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

pub async fn health_check() -> impl IntoResponse {
    let res = HealthResponse {
        status: "ok".to_string(),
        version: crate::info::VERSION.to_string(),
    };
    (StatusCode::OK, Json(res))
}

pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    match prometheus_metrics::collect_metrics(&state.db).await {
        Ok(snapshot) => (
            StatusCode::OK,
            [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
            prometheus_metrics::render_prometheus(&snapshot),
        )
            .into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn request_auth_status(state: &AppState, headers: &HeaderMap) -> Option<sdk::AuthStatus> {
    let token = AuthManager::bearer_token(headers)?;
    AuthManager::new(state.db.clone())
        .status_for_token(&token)
        .await
        .ok()
}

#[derive(Debug, Deserialize)]
pub struct AuthSetupRequest {
    pub password: String,
    pub node_name: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthLoginRequest {
    pub password: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct EmptyJsonObject {}

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub prompt: Option<String>,
    pub input: Option<String>,
    pub node: Option<String>,
    pub agent_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TaskEventsResponse {
    pub task: crate::storage::Task,
    pub events: Vec<crate::storage::AgentEvent>,
    pub stream_events: Vec<TaskStreamEvent>,
}

#[derive(Debug, Deserialize)]
pub struct TaskListRequest {
    pub status: Option<String>,
    pub agent_id: Option<String>,
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TaskAgentFacet {
    pub agent_id: String,
    pub agent_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SpecRunRequest {
    pub prompt: Option<String>,
    pub input: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FactoryGenerateRequest {
    pub requirement: String,
    pub template_id: Option<String>,
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FromTaskGenerateRequest {
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DaemonConfigView {
    pub config_schema_version: u32,
    pub config_written_by: String,
    pub node_name: String,
    pub profile: String,
    pub developer_mode: bool,
    pub privacy_mode: String,
    pub idle_timeout_secs: u64,
    pub absolute_timeout_secs: u64,
    pub reauth_window_secs: u64,
    pub session_persist_on_restart: bool,
    pub approval_mode: String,
    pub approvals_rules_path: String,
    pub secret_backend: String,
    pub bind_addr: String,
    pub tls_enabled: bool,
    pub tls_cert_path: String,
    pub tls_key_path: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDaemonConfigRequest {
    pub node_name: Option<String>,
    pub profile: Option<String>,
    pub developer_mode: Option<bool>,
    pub privacy_mode: Option<String>,
    pub idle_timeout_secs: Option<u64>,
    pub absolute_timeout_secs: Option<u64>,
    pub reauth_window_secs: Option<u64>,
    pub session_persist_on_restart: Option<bool>,
    pub approval_mode: Option<String>,
    pub secret_backend: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateApprovalModeRequest {
    pub mode: String,
}

#[derive(Debug, Deserialize)]
pub struct MemoryGraphInspectQuery {
    pub entity: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddApprovalRuleRequest {
    pub id: String,
    pub action: String,
    pub tool: Option<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<String>,
    #[serde(default)]
    pub channels: Vec<String>,
    pub risk_tier: Option<u8>,
    pub effect: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InstallServiceRequest {
    pub mode: String,
    pub profile: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct ZeroTierJoinRequest {
    pub network_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ZeroTierSetupRequest {
    pub network_id: String,
    pub api_token_key: Option<String>,
    pub managed_name_sync: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ExtensionInstallRequest {
    pub kind: Option<String>,
    pub source: String,
    pub registry: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramChannelSetupRequest {
    pub token: Option<String>,
    #[serde(default)]
    pub allowed_ids: Vec<i64>,
    pub confirmation_chat_id: Option<i64>,
    pub api_base_url: Option<String>,
    pub default_agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PluginChannelDeliverRequest {
    pub input: String,
    pub session_id: Option<String>,
    pub workspace: Option<String>,
    pub team_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkflowTriggerResponse {
    pub triggered: Vec<workflow_triggers::TriggeredWorkflowRun>,
}

#[derive(Debug, Deserialize)]
pub struct BackupExportRequest {
    pub path: Option<String>,
    pub force: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct BackupRestoreRequest {
    pub path: String,
    pub force: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct MigrationRequest {
    pub path: Option<String>,
    pub dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct RemoteHandshakeRequest {
    pub challenge: String,
}

#[derive(Debug, Deserialize)]
pub struct DispatchBrainUseRequest {
    pub model: String,
}

pub async fn hello(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let auth_manager = AuthManager::new(state.db.clone());
    let config = match Config::load_or_create() {
        Ok(config) => config,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response();
        }
    };

    let remote_status = RemoteManager::new(config.clone()).status();
    let auth_state = match AuthManager::bearer_token(&headers) {
        Some(token) => match auth_manager.validate_session(&token, false).await {
            Ok(validated) => validated.status.state,
            Err(_) => auth_manager.auth_state().unwrap_or(AuthState::Locked),
        },
        None => auth_manager.auth_state().unwrap_or(AuthState::Locked),
    };

    let service_statuses = ServiceManager::new(config.clone()).list();
    let capabilities = DaemonCapabilities {
        brains: if config.brains.enabled {
            vec!["dispatch".to_string()]
        } else {
            Vec::new()
        },
        services: service_statuses
            .into_iter()
            .filter(|service| service.enabled)
            .map(|service| service.name)
            .collect(),
        extensions: match state.db.installed_plugins().list_plugins().await {
            Ok(plugins) => plugins
                .into_iter()
                .filter(|plugin| plugin.enabled)
                .map(|plugin| format!("{}:{}", plugin.plugin_type.to_lowercase(), plugin.name))
                .collect(),
            Err(_) => Vec::new(),
        },
    };

    let (node_id, node_name, role) = match remote_status {
        Ok(status) => (
            status.node.node_id,
            status.node.node_name,
            status.profile.execution_role,
        ),
        Err(_) => (
            "local-node".to_string(),
            "local".to_string(),
            sdk::NodeExecutionRole::Full,
        ),
    };

    (
        StatusCode::OK,
        Json(DaemonHello {
            version: crate::info::VERSION.to_string(),
            daemon_running: true,
            auth_state,
            node: NodeSummary {
                node_id,
                node_name,
                role,
            },
            capabilities,
        }),
    )
        .into_response()
}

pub async fn auth_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AuthSetupRequest>,
) -> impl IntoResponse {
    match AuthManager::new(state.db.clone())
        .setup(
            &payload.password,
            payload.node_name.as_deref(),
            payload.mode.as_deref(),
            &headers,
        )
        .await
    {
        Ok(session) => (StatusCode::CREATED, Json(session)).into_response(),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn auth_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AuthLoginRequest>,
) -> impl IntoResponse {
    match AuthManager::new(state.db.clone())
        .login(&payload.password, &headers)
        .await
    {
        Ok(session) => (StatusCode::OK, Json(session)).into_response(),
        Err(error) => json_error_response(StatusCode::UNAUTHORIZED, error),
    }
}

pub async fn auth_passkey_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    match AuthManager::new(state.db.clone())
        .passkey_status(&headers)
        .await
    {
        Ok(status) => (StatusCode::OK, Json(status)).into_response(),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn auth_passkey_login_start(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    match AuthManager::new(state.db.clone())
        .start_passkey_login(&headers)
        .await
    {
        Ok(challenge) => (StatusCode::OK, Json(challenge)).into_response(),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn auth_passkey_login_finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyFinishRequest>,
) -> impl IntoResponse {
    match AuthManager::new(state.db.clone())
        .finish_passkey_login(&payload, &headers)
        .await
    {
        Ok(session) => (StatusCode::OK, Json(session)).into_response(),
        Err(error) => json_error_response(StatusCode::UNAUTHORIZED, error),
    }
}

pub async fn auth_status(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Some(token) = AuthManager::bearer_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Missing bearer token" })),
        )
            .into_response();
    };

    match AuthManager::new(state.db.clone())
        .status_for_token(&token)
        .await
    {
        Ok(status) => (StatusCode::OK, Json(status)).into_response(),
        Err(error) => json_error_response(StatusCode::UNAUTHORIZED, error),
    }
}

pub async fn auth_lock(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let Some(token) = AuthManager::bearer_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Missing bearer token" })),
        )
            .into_response();
    };

    match AuthManager::new(state.db.clone()).lock(&token).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn auth_reauth(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AuthLoginRequest>,
) -> impl IntoResponse {
    let Some(token) = AuthManager::bearer_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Missing bearer token" })),
        )
            .into_response();
    };

    match AuthManager::new(state.db.clone())
        .reauth(&token, &payload.password, &headers)
        .await
    {
        Ok(status) => (StatusCode::OK, Json(status)).into_response(),
        Err(error) => json_error_response(StatusCode::UNAUTHORIZED, error),
    }
}

pub async fn list_passkeys(State(state): State<AppState>) -> impl IntoResponse {
    match AuthManager::new(state.db.clone()).list_passkeys().await {
        Ok(passkeys) => (StatusCode::OK, Json(passkeys)).into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn start_passkey_registration(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyRegistrationStartRequest>,
) -> impl IntoResponse {
    let Some(token) = AuthManager::bearer_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Missing bearer token" })),
        )
            .into_response();
    };

    match AuthManager::new(state.db.clone())
        .start_passkey_registration(&token, &payload, &headers)
        .await
    {
        Ok(challenge) => (StatusCode::OK, Json(challenge)).into_response(),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn finish_passkey_registration(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyFinishRequest>,
) -> impl IntoResponse {
    let Some(token) = AuthManager::bearer_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Missing bearer token" })),
        )
            .into_response();
    };

    match AuthManager::new(state.db.clone())
        .finish_passkey_registration(&token, &payload, &headers)
        .await
    {
        Ok(passkey) => (StatusCode::OK, Json(passkey)).into_response(),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn start_passkey_reauth(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(_payload): Json<EmptyJsonObject>,
) -> impl IntoResponse {
    let Some(token) = AuthManager::bearer_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Missing bearer token" })),
        )
            .into_response();
    };

    match AuthManager::new(state.db.clone())
        .start_passkey_reauth(&token, &headers)
        .await
    {
        Ok(challenge) => (StatusCode::OK, Json(challenge)).into_response(),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn finish_passkey_reauth(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyFinishRequest>,
) -> impl IntoResponse {
    let Some(token) = AuthManager::bearer_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Missing bearer token" })),
        )
            .into_response();
    };

    match AuthManager::new(state.db.clone())
        .finish_passkey_reauth(&token, &payload, &headers)
        .await
    {
        Ok(status) => (StatusCode::OK, Json(status)).into_response(),
        Err(error) => json_error_response(StatusCode::UNAUTHORIZED, error),
    }
}

pub async fn remove_passkey(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(token) = AuthManager::bearer_token(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Missing bearer token" })),
        )
            .into_response();
    };

    match AuthManager::new(state.db.clone())
        .delete_passkey(&token, &id)
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => json_error_response(StatusCode::NOT_FOUND, "Passkey not found"),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn list_tasks(
    State(state): State<AppState>,
    Query(query): Query<TaskListRequest>,
) -> impl IntoResponse {
    let status = match query.status.as_deref() {
        Some("pending") => Some(crate::storage::TaskStatus::Pending),
        Some("running") => Some(crate::storage::TaskStatus::Running),
        Some("completed") => Some(crate::storage::TaskStatus::Completed),
        Some("failed") => Some(crate::storage::TaskStatus::Failed),
        Some(other) => {
            return json_error_response(
                StatusCode::BAD_REQUEST,
                anyhow::anyhow!(
                    "Invalid task status '{}'. Use pending, running, completed, or failed.",
                    other
                ),
            );
        }
        None => None,
    };

    match state
        .db
        .tasks()
        .list_tasks(&crate::storage::TaskListQuery {
            status,
            agent_id: query.agent_id.filter(|value| !value.trim().is_empty()),
            date_from: query.date_from,
            date_to: query.date_to,
            limit: query.limit.unwrap_or(50).clamp(1, 500),
            offset: query.offset.unwrap_or(0).max(0),
        })
        .await
    {
        Ok(tasks) => (StatusCode::OK, Json(tasks)).into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_task_agents(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.tasks().list_task_agents().await {
        Ok(agents) => (
            StatusCode::OK,
            Json(
                agents
                    .into_iter()
                    .map(|(agent_id, agent_name)| TaskAgentFacet {
                        agent_id,
                        agent_name,
                    })
                    .collect::<Vec<_>>(),
            ),
        )
            .into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct AgentActionQueryRequest {
    pub action: Option<String>,
    pub source: Option<String>,
    pub severity: Option<String>,
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list_audit_log(
    State(state): State<AppState>,
    Query(query): Query<AgentActionQueryRequest>,
) -> impl IntoResponse {
    match state
        .db
        .tasks()
        .list_agent_actions(&crate::storage::AgentActionQuery {
            action_type: query.action.filter(|value| !value.trim().is_empty()),
            source: query.source.filter(|value| !value.trim().is_empty()),
            severity: query.severity.filter(|value| !value.trim().is_empty()),
            date_from: query.date_from,
            date_to: query.date_to,
            limit: query.limit.unwrap_or(100).clamp(1, 500),
            offset: query.offset.unwrap_or(0).max(0),
        })
        .await
    {
        Ok(entries) => (StatusCode::OK, Json(entries)).into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn get_task_events(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    let task_uuid = match Uuid::parse_str(&task_id) {
        Ok(task_uuid) => task_uuid,
        Err(_) => {
            return json_error_response(StatusCode::BAD_REQUEST, "Invalid task id");
        }
    };

    let task = match state.db.tasks().get_task(&task_uuid).await {
        Ok(Some(task)) => task,
        Ok(None) => return json_error_response(StatusCode::NOT_FOUND, "Task not found"),
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };

    match state.db.tasks().get_agent_events(&task_id).await {
        Ok(events) => (
            StatusCode::OK,
            Json(TaskEventsResponse {
                stream_events: normalize_task_stream_events(&task, &events),
                task,
                events,
            }),
        )
            .into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn stream_task_events(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    let task_uuid = match Uuid::parse_str(&task_id) {
        Ok(task_uuid) => task_uuid,
        Err(_) => {
            return json_error_response(StatusCode::BAD_REQUEST, "Invalid task id");
        }
    };

    let task = match state.db.tasks().get_task(&task_uuid).await {
        Ok(Some(task)) => task,
        Ok(None) => return json_error_response(StatusCode::NOT_FOUND, "Task not found"),
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };

    let events = match state.db.tasks().get_agent_events(&task_id).await {
        Ok(events) => events,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let initial = normalize_task_stream_events(&task, &events);
    let mut rx = state.message_bus.subscribe(BusEventType::All).await;
    let state_clone = state.clone();
    let task_id_clone = task_id.clone();
    let task_status = task.status.clone();

    let stream = stream! {
        for event in initial {
            let payload = serde_json::json!({
                "type": "task.event",
                "task_id": task_id_clone,
                "event": event,
            });
            yield Ok::<Bytes, Infallible>(Bytes::from(format!("data: {payload}\n\n")));
        }

        if matches!(task_status, crate::storage::TaskStatus::Completed | crate::storage::TaskStatus::Failed) {
            let final_payload = final_task_stream_payload(&state_clone, &task_id).await;
            yield Ok::<Bytes, Infallible>(Bytes::from(format!("data: {final_payload}\n\n")));
            return;
        }

        while let Some(event) = rx.recv().await {
            match event {
                BusEvent::TaskStream { task_id: live_task_id, event } if live_task_id == task_id => {
                    let payload = serde_json::json!({
                        "type": "task.event",
                        "task_id": live_task_id,
                        "event": event,
                    });
                    yield Ok::<Bytes, Infallible>(Bytes::from(format!("data: {payload}\n\n")));
                }
                BusEvent::TaskCompleted { task_id: live_task_id, .. } if live_task_id == task_id => {
                    let final_payload = final_task_stream_payload(&state_clone, &task_id).await;
                    yield Ok::<Bytes, Infallible>(Bytes::from(format!("data: {final_payload}\n\n")));
                    break;
                }
                BusEvent::TaskFailed { task_id: live_task_id, .. } if live_task_id == task_id => {
                    let final_payload = final_task_stream_payload(&state_clone, &task_id).await;
                    yield Ok::<Bytes, Infallible>(Bytes::from(format!("data: {final_payload}\n\n")));
                    break;
                }
                _ => {}
            }
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-store")
        .body(Body::from_stream(stream))
        .expect("task event stream response")
}

pub(super) fn normalize_task_stream_events(
    task: &crate::storage::Task,
    events: &[crate::storage::AgentEvent],
) -> Vec<TaskStreamEvent> {
    let mut normalized = vec![TaskStreamEvent {
        id: format!("{}:turn_start", task.id),
        task_id: task.id.to_string(),
        phase: "turn_start".to_string(),
        summary: "Task started".to_string(),
        detail: Some(task.input.clone()),
        raw_event_type: None,
        tool_name: None,
        status: Some(task.status.as_str().to_string()),
        step_num: 0,
        domain: None,
        created_at: task.created_at,
    }];

    for event in events {
        normalized.push(normalize_agent_event(event));
    }

    if matches!(
        task.status,
        crate::storage::TaskStatus::Completed | crate::storage::TaskStatus::Failed
    ) {
        normalized.push(TaskStreamEvent {
            id: format!("{}:turn_end", task.id),
            task_id: task.id.to_string(),
            phase: "turn_end".to_string(),
            summary: if task.status == crate::storage::TaskStatus::Completed {
                "Task completed".to_string()
            } else {
                "Task failed".to_string()
            },
            detail: None,
            raw_event_type: None,
            tool_name: None,
            status: Some(task.status.as_str().to_string()),
            step_num: events.iter().map(|event| event.step_num).max().unwrap_or(0) + 1,
            domain: None,
            created_at: task.completed_at.unwrap_or(task.created_at),
        });
    }

    normalized.sort_by_key(|event| (event.created_at, event.step_num));
    normalized
}

async fn final_task_stream_payload(state: &AppState, task_id: &str) -> serde_json::Value {
    match completion::load_completion(state, task_id).await {
        Ok(completion::CompletionState::Done(result)) => serde_json::json!({
            "type": "task.completed",
            "task_id": task_id,
            "result": result.answer,
        }),
        Ok(completion::CompletionState::Failed(error)) => serde_json::json!({
            "type": "task.completed",
            "task_id": task_id,
            "result": error,
        }),
        _ => serde_json::json!({
            "type": "task.completed",
            "task_id": task_id,
        }),
    }
}

fn normalize_agent_event(event: &crate::storage::AgentEvent) -> TaskStreamEvent {
    let payload = parse_event_payload(&event.payload);
    let phase = match event.event_type.as_str() {
        "tool_call" => "tool_use",
        "observation" => "tool_result",
        "answer" => "final_answer",
        "error" => "error",
        "thought" => "thought",
        _ => "activity",
    }
    .to_string();

    let tool_name = payload
        .as_ref()
        .and_then(|value| json_string(value, &["tool_name", "tool"]));
    let detail = event_detail(&event.event_type, payload.as_ref(), &event.payload);
    let summary = match event.event_type.as_str() {
        "tool_call" => tool_name
            .clone()
            .map(|tool_name| format!("Tool call: {}", tool_name))
            .unwrap_or_else(|| "Tool call".to_string()),
        "observation" => "Tool result".to_string(),
        "answer" => "Final answer".to_string(),
        "error" => "Execution error".to_string(),
        "thought" => "Reasoning".to_string(),
        other => other.replace('_', " "),
    };

    TaskStreamEvent {
        id: event.id.clone(),
        task_id: event.task_id.clone(),
        phase,
        summary,
        detail,
        raw_event_type: Some(event.event_type.clone()),
        tool_name,
        status: None,
        step_num: event.step_num,
        domain: event.domain.clone(),
        created_at: event.created_at,
    }
}

fn parse_event_payload(payload: &str) -> Option<serde_json::Value> {
    serde_json::from_str(payload).ok()
}

fn json_string(payload: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = payload.get(*key).and_then(|value| value.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn event_detail(
    event_type: &str,
    payload: Option<&serde_json::Value>,
    raw_payload: &str,
) -> Option<String> {
    let payload = payload?;
    match event_type {
        "thought" => json_string(payload, &["content"]),
        "tool_call" => json_string(payload, &["tool_args"]),
        "observation" => json_string(payload, &["observation"]),
        "answer" => json_string(payload, &["answer"]),
        "error" => json_string(payload, &["error"]),
        _ => Some(raw_payload.to_string()),
    }
}

pub async fn list_agents() -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match repo.list_agents() {
            Ok(items) => (StatusCode::OK, Json(items)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_agent_templates() -> impl IntoResponse {
    (StatusCode::OK, Json(factory::list_agent_templates())).into_response()
}

pub async fn list_worker_presets() -> impl IntoResponse {
    (StatusCode::OK, Json(worker_presets::list_worker_presets())).into_response()
}

pub async fn list_starters() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match starter_catalog::list(&config).await {
            Ok(entries) => (StatusCode::OK, Json(entries)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn preview_agent_factory(
    Json(payload): Json<FactoryGenerateRequest>,
) -> impl IntoResponse {
    let repo = SpecRepository::new().ok();
    match factory::preview_agent_result(
        repo.as_ref(),
        &payload.requirement,
        payload.template_id.as_deref(),
        payload.id.as_deref(),
        payload.name.as_deref(),
    ) {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn create_agent_factory(
    Json(payload): Json<FactoryGenerateRequest>,
) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match factory::create_agent(
            &repo,
            &payload.requirement,
            payload.template_id.as_deref(),
            payload.id.as_deref(),
            payload.name.as_deref(),
        ) {
            Ok(result) => (StatusCode::CREATED, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn create_agent_from_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(payload): Json<FromTaskGenerateRequest>,
) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match factory::agent_from_task(
            &repo,
            &state.db,
            &task_id,
            payload.id.as_deref(),
            payload.name.as_deref(),
        )
        .await
        {
            Ok(result) => (StatusCode::CREATED, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn get_agent_review(Path(id): Path<String>) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match factory::get_agent_review(&repo, &id) {
            Ok(review) => (StatusCode::OK, Json(review)).into_response(),
            Err(error) => json_error_response(StatusCode::NOT_FOUND, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn approve_agent_factory(Path(id): Path<String>) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match factory::approve_agent(&repo, &id) {
            Ok(spec) => (StatusCode::OK, Json(spec)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn get_agent(Path(id): Path<String>) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match repo.load_agent(&id) {
            Ok(item) => (StatusCode::OK, Json(item)).into_response(),
            Err(error) => json_error_response(StatusCode::NOT_FOUND, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn create_agent(Json(spec): Json<AgentSpec>) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match repo.save_agent(&spec) {
            Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn update_agent(
    Path(id): Path<String>,
    Json(mut spec): Json<AgentSpec>,
) -> impl IntoResponse {
    spec.id = id;
    match SpecRepository::new() {
        Ok(repo) => match repo.save_agent(&spec) {
            Ok(item) => (StatusCode::OK, Json(item)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn remove_agent(Path(id): Path<String>) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match repo.remove_agent(&id) {
            Ok(true) => StatusCode::NO_CONTENT.into_response(),
            Ok(false) => json_error_response(
                StatusCode::NOT_FOUND,
                anyhow::anyhow!("Agent '{}' was not found", id),
            ),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_agent_runs(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.agent_runs().list_agent_runs(50).await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn run_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<SpecRunRequest>,
) -> impl IntoResponse {
    let Some(input) = parse_task_input(payload.input, payload.prompt) else {
        return invalid_request("Request must include a non-empty `input` or `prompt` field");
    };

    let repo = match SpecRepository::new() {
        Ok(repo) => repo,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let spec = match repo.load_agent(&id) {
        Ok(spec) => spec,
        Err(error) => return json_error_response(StatusCode::NOT_FOUND, error),
    };
    if !spec.enabled {
        return json_error_response(
            StatusCode::BAD_REQUEST,
            anyhow::anyhow!("Agent '{}' is disabled", spec.id),
        );
    }

    let config = match Config::load_or_create() {
        Ok(config) => config,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };

    let run_id = Uuid::new_v4().to_string();
    if let Err(error) = state
        .db
        .agent_runs()
        .start_agent_run(&run_id, &spec.id, None, None, &input)
        .await
    {
        return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error);
    }

    let profile = TaskExecutionProfile {
        agent_id: Some(spec.id.clone()),
        agent_name: Some(spec.name.clone()),
        worker_preset_id: None,
        worker_preset_name: None,
        purpose: Some(spec.purpose.clone()),
        instructions: spec.instructions.clone(),
        allowed_tools: allowed_tools(&spec),
        output_contract: spec.output_contract.clone(),
        max_iterations: None,
    };

    match crate::cli::run::execute_local_task_request(
        input,
        &config,
        RunMode::Serial,
        RunIsolation::None,
        Some(profile),
    )
    .await
    {
        Ok(task_result) => {
            let _ = state
                .db
                .agent_runs()
                .finish_agent_run(
                    &run_id,
                    SpecRunStatus::Completed,
                    Some(&task_result.task_id),
                    Some(&task_result.answer),
                    None,
                )
                .await;
            (
                StatusCode::OK,
                Json(ExecuteResponse {
                    success: true,
                    task_id: Some(task_result.task_id),
                    status: "completed".to_string(),
                    answer: Some(task_result.answer),
                    provider: Some(task_result.provider_used),
                    duration_ms: Some(task_result.duration_ms),
                    message: Some(run_id),
                }),
            )
                .into_response()
        }
        Err(error) => {
            let _ = state
                .db
                .agent_runs()
                .finish_agent_run(
                    &run_id,
                    SpecRunStatus::Failed,
                    None,
                    None,
                    Some(&error.to_string()),
                )
                .await;
            json_error_response(StatusCode::BAD_REQUEST, error)
        }
    }
}

pub async fn list_workflows() -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match repo.list_workflows() {
            Ok(items) => (StatusCode::OK, Json(items)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_workflow_templates() -> impl IntoResponse {
    (StatusCode::OK, Json(factory::list_workflow_templates())).into_response()
}

pub async fn preview_workflow_factory(
    Json(payload): Json<FactoryGenerateRequest>,
) -> impl IntoResponse {
    let repo = SpecRepository::new().ok();
    match factory::preview_workflow_result(
        repo.as_ref(),
        &payload.requirement,
        payload.template_id.as_deref(),
        payload.id.as_deref(),
        payload.name.as_deref(),
    ) {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn create_workflow_factory(
    Json(payload): Json<FactoryGenerateRequest>,
) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match factory::create_workflow(
            &repo,
            &payload.requirement,
            payload.template_id.as_deref(),
            payload.id.as_deref(),
            payload.name.as_deref(),
        ) {
            Ok(result) => (StatusCode::CREATED, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn create_workflow_from_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(payload): Json<FromTaskGenerateRequest>,
) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match factory::workflow_from_task(
            &repo,
            &state.db,
            &task_id,
            payload.id.as_deref(),
            payload.name.as_deref(),
        )
        .await
        {
            Ok(result) => (StatusCode::CREATED, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn get_workflow_review(Path(id): Path<String>) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match factory::get_workflow_review(&repo, &id) {
            Ok(review) => (StatusCode::OK, Json(review)).into_response(),
            Err(error) => json_error_response(StatusCode::NOT_FOUND, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn approve_workflow_factory(Path(id): Path<String>) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match factory::approve_workflow(&repo, &id) {
            Ok(spec) => (StatusCode::OK, Json(spec)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn get_workflow(Path(id): Path<String>) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match repo.load_workflow(&id) {
            Ok(item) => (StatusCode::OK, Json(item)).into_response(),
            Err(error) => json_error_response(StatusCode::NOT_FOUND, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn create_workflow(Json(spec): Json<WorkflowSpec>) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match repo.save_workflow(&spec) {
            Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn update_workflow(
    Path(id): Path<String>,
    Json(mut spec): Json<WorkflowSpec>,
) -> impl IntoResponse {
    spec.id = id;
    match SpecRepository::new() {
        Ok(repo) => match repo.save_workflow(&spec) {
            Ok(item) => (StatusCode::OK, Json(item)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn remove_workflow(Path(id): Path<String>) -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match repo.remove_workflow(&id) {
            Ok(true) => StatusCode::NO_CONTENT.into_response(),
            Ok(false) => json_error_response(
                StatusCode::NOT_FOUND,
                anyhow::anyhow!("Workflow '{}' was not found", id),
            ),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_workflow_runs(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.agent_runs().list_workflow_runs(50).await {
        Ok(items) => (StatusCode::OK, Json(items)).into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn get_workflow_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    match state.db.agent_runs().get_workflow_run_detail(&run_id).await {
        Ok(Some(detail)) => (StatusCode::OK, Json(detail)).into_response(),
        Ok(None) => json_error_response(
            StatusCode::NOT_FOUND,
            anyhow::anyhow!("Workflow run '{}' was not found", run_id),
        ),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn run_workflow(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<SpecRunRequest>,
) -> impl IntoResponse {
    let Some(input) = parse_task_input(payload.input, payload.prompt) else {
        return invalid_request("Request must include a non-empty `input` or `prompt` field");
    };

    let repo = match SpecRepository::new() {
        Ok(repo) => repo,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let workflow = match repo.load_workflow(&id) {
        Ok(spec) => spec,
        Err(error) => return json_error_response(StatusCode::NOT_FOUND, error),
    };
    if !workflow.enabled {
        return json_error_response(
            StatusCode::BAD_REQUEST,
            anyhow::anyhow!("Workflow '{}' is disabled", workflow.id),
        );
    }

    let config = match Config::load_or_create() {
        Ok(config) => config,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };

    let result =
        match workflow_runtime::start_new_run(&repo, &state.db, &config, &workflow, &input).await {
            Ok(result) => result,
            Err(error) => return json_error_response(StatusCode::BAD_REQUEST, error),
        };
    (
        StatusCode::OK,
        Json(ExecuteResponse {
            success: true,
            task_id: None,
            status: "completed".to_string(),
            answer: Some(result.final_output),
            provider: None,
            duration_ms: None,
            message: Some(result.run.run_id),
        }),
    )
        .into_response()
}

pub async fn invoke_workflow_webhook(
    State(_state): State<AppState>,
    Path(webhook_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let config = match Config::load_or_create() {
        Ok(config) => config,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let repo = match SpecRepository::new() {
        Ok(repo) => repo,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };

    let webhook_exists = match workflow_triggers::webhook_binding_exists(&repo, &webhook_id) {
        Ok(exists) => exists,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    if !webhook_exists {
        return json_error_response(
            StatusCode::NOT_FOUND,
            anyhow::anyhow!("No workflow webhook '{}' is configured", webhook_id),
        );
    }

    let provided_secret = headers
        .get("x-rove-webhook-secret")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let db = match crate::storage::Database::new(&database_path(&config)).await {
        Ok(db) => db,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };

    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream");
    let payload = String::from_utf8_lossy(&body);
    let input = if payload.trim().is_empty() {
        format!(
            "Webhook '{}' triggered.\nContent-Type: {}",
            webhook_id, content_type
        )
    } else {
        format!(
            "Webhook '{}' triggered.\nContent-Type: {}\n\n{}",
            webhook_id, content_type, payload
        )
    };

    match workflow_triggers::trigger_matching_webhook_workflows(
        &repo,
        &db,
        &config,
        &webhook_id,
        provided_secret,
        &input,
    )
    .await
    {
        Ok(triggered) if !triggered.is_empty() => (
            StatusCode::ACCEPTED,
            Json(WorkflowTriggerResponse { triggered }),
        )
            .into_response(),
        Ok(_) => json_error_response(
            StatusCode::UNAUTHORIZED,
            anyhow::anyhow!("Webhook secret did not match any enabled workflow binding"),
        ),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn resume_workflow_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let repo = match SpecRepository::new() {
        Ok(repo) => repo,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let config = match Config::load_or_create() {
        Ok(config) => config,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };

    let result = match workflow_runtime::resume_run(&repo, &state.db, &config, &run_id).await {
        Ok(result) => result,
        Err(error) => return json_error_response(StatusCode::BAD_REQUEST, error),
    };
    (
        StatusCode::OK,
        Json(ExecuteResponse {
            success: true,
            task_id: result.run.last_task_id.clone(),
            status: result.run.status.as_str().to_string(),
            answer: Some(result.final_output),
            provider: None,
            duration_ms: None,
            message: Some(result.run.run_id),
        }),
    )
        .into_response()
}

pub async fn cancel_workflow_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    match state
        .db
        .agent_runs()
        .request_workflow_run_cancel(&run_id)
        .await
    {
        Ok(true) => (
            StatusCode::ACCEPTED,
            Json(ExecuteResponse {
                success: true,
                task_id: None,
                status: "cancel_requested".to_string(),
                answer: None,
                provider: None,
                duration_ms: None,
                message: Some(run_id),
            }),
        )
            .into_response(),
        Ok(false) => json_error_response(
            StatusCode::NOT_FOUND,
            anyhow::anyhow!(
                "Workflow run '{}' was not found or is already settled",
                run_id
            ),
        ),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn get_config() -> impl IntoResponse {
    match daemon_config_view() {
        Ok(view) => (StatusCode::OK, Json(view)).into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn update_config(Json(payload): Json<UpdateDaemonConfigRequest>) -> impl IntoResponse {
    match apply_config_update(payload) {
        Ok(view) => (StatusCode::OK, Json(view)).into_response(),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn list_extensions() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match extensions::inventory(&config).await {
            Ok(items) => (StatusCode::OK, Json(items)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_extension_catalog() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match extensions::catalog(&config, false).await {
            Ok(items) => (StatusCode::OK, Json(items)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn get_extension_catalog_entry(Path(id): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match extensions::catalog_entry(&config, &id, false).await {
            Ok(item) => (StatusCode::OK, Json(item)).into_response(),
            Err(error) => json_error_response(StatusCode::NOT_FOUND, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_extension_updates() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match extensions::updates(&config, false).await {
            Ok(items) => (StatusCode::OK, Json(items)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn refresh_extension_catalog() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match extensions::catalog(&config, true).await {
            Ok(items) => (StatusCode::OK, Json(items)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn install_extension(Json(payload): Json<ExtensionInstallRequest>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match extensions::install_extension_api(
            &config,
            payload.kind.as_deref(),
            &payload.source,
            payload.registry.as_deref(),
            payload.version.as_deref(),
        )
        .await
        {
            Ok(item) => (StatusCode::CREATED, Json(item)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn upgrade_extension(Json(payload): Json<ExtensionInstallRequest>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match extensions::upgrade_extension_api(
            &config,
            payload.kind.as_deref(),
            &payload.source,
            payload.registry.as_deref(),
            payload.version.as_deref(),
        )
        .await
        {
            Ok(item) => (StatusCode::OK, Json(item)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_brains() -> impl IntoResponse {
    match dispatch_family::status_view() {
        Ok(dispatch) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "dispatch": dispatch,
            })),
        )
            .into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn use_dispatch_brain(Json(payload): Json<DispatchBrainUseRequest>) -> impl IntoResponse {
    match dispatch_family::use_model(payload.model.trim()) {
        Ok(()) => match dispatch_family::status_view() {
            Ok(dispatch) => (StatusCode::OK, Json(dispatch)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn enable_extension(Path((kind, name)): Path<(String, String)>) -> impl IntoResponse {
    set_extension_enabled_inner(kind, name, true).await
}

pub async fn disable_extension(Path((kind, name)): Path<(String, String)>) -> impl IntoResponse {
    set_extension_enabled_inner(kind, name, false).await
}

pub async fn remove_extension(Path((kind, name)): Path<(String, String)>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match extensions::remove_extension_api(&config, &kind, &name).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

fn json_error_response(status: StatusCode, error: impl std::fmt::Display) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": error.to_string() })),
    )
        .into_response()
}

fn parse_profile(value: &str) -> anyhow::Result<crate::config::DaemonProfile> {
    match value.trim().to_ascii_lowercase().as_str() {
        "desktop" => Ok(crate::config::DaemonProfile::Desktop),
        "headless" => Ok(crate::config::DaemonProfile::Headless),
        "edge" => Ok(crate::config::DaemonProfile::Edge),
        other => Err(anyhow::anyhow!(
            "Invalid daemon profile '{}'. Use desktop, headless, or edge.",
            other
        )),
    }
}

fn parse_approval_mode(value: &str) -> anyhow::Result<crate::config::ApprovalMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "default" => Ok(crate::config::ApprovalMode::Default),
        "allowlist" => Ok(crate::config::ApprovalMode::Allowlist),
        "open" => Ok(crate::config::ApprovalMode::Open),
        "assisted" => Ok(crate::config::ApprovalMode::Assisted),
        other => Err(anyhow::anyhow!(
            "Invalid approval mode '{}'. Use default, allowlist, open, or assisted.",
            other
        )),
    }
}

fn parse_secret_backend(value: &str) -> anyhow::Result<crate::config::SecretBackend> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Ok(crate::config::SecretBackend::Auto),
        "vault" => Ok(crate::config::SecretBackend::Vault),
        "keychain" => Ok(crate::config::SecretBackend::Keychain),
        "env" => Ok(crate::config::SecretBackend::Env),
        other => Err(anyhow::anyhow!(
            "Invalid secret backend '{}'. Use auto, vault, keychain, or env.",
            other
        )),
    }
}

fn parse_migration_source(value: &str) -> anyhow::Result<migrate::MigrationSource> {
    match value.trim().to_ascii_lowercase().as_str() {
        "openclaw" => Ok(migrate::MigrationSource::OpenClaw),
        "zeroclaw" => Ok(migrate::MigrationSource::ZeroClaw),
        "moltis" => Ok(migrate::MigrationSource::Moltis),
        other => Err(anyhow::anyhow!(
            "Invalid migration source '{}'. Use openclaw, zeroclaw, or moltis.",
            other
        )),
    }
}

fn parse_rule_action(value: &str) -> anyhow::Result<approvals::ApprovalRuleAction> {
    match value.trim().to_ascii_lowercase().as_str() {
        "allow" => Ok(approvals::ApprovalRuleAction::Allow),
        "require_approval" | "require-approval" => {
            Ok(approvals::ApprovalRuleAction::RequireApproval)
        }
        other => Err(anyhow::anyhow!(
            "Invalid approval rule action '{}'. Use allow or require-approval.",
            other
        )),
    }
}

fn parse_service_install_mode(value: &str) -> anyhow::Result<ServiceInstallMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "login" => Ok(ServiceInstallMode::Login),
        "boot" => Ok(ServiceInstallMode::Boot),
        other => Err(anyhow::anyhow!(
            "Invalid service install mode '{}'. Use login or boot.",
            other
        )),
    }
}

async fn set_extension_enabled_inner(kind: String, name: String, enabled: bool) -> Response {
    match Config::load_or_create() {
        Ok(config) => {
            match extensions::set_extension_enabled_api(&config, &kind, &name, enabled).await {
                Ok(item) => (StatusCode::OK, Json(item)).into_response(),
                Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
            }
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

fn daemon_config_view() -> anyhow::Result<DaemonConfigView> {
    let config = Config::load_or_create()?;
    let remote = RemoteManager::new(config.clone());
    let tls_status = super::tls::localhost_tls_status();
    let node_name = remote.status()?.node.node_name;
    let approvals_rules_path = approvals::rules_path(&config)?;

    Ok(DaemonConfigView {
        config_schema_version: config.config_schema_version,
        config_written_by: config.config_written_by.clone(),
        node_name,
        profile: config.daemon.profile.as_str().to_string(),
        developer_mode: config.daemon.developer_mode,
        privacy_mode: config.webui.privacy_mode.clone(),
        idle_timeout_secs: config.webui.idle_timeout_secs,
        absolute_timeout_secs: config.webui.absolute_timeout_secs,
        reauth_window_secs: config.webui.reauth_window_secs,
        session_persist_on_restart: config.webui.session_persist_on_restart,
        approval_mode: config.approvals.mode.as_str().to_string(),
        approvals_rules_path: approvals_rules_path.display().to_string(),
        secret_backend: config.secrets.backend.as_str().to_string(),
        bind_addr: config.webui.bind_addr.clone(),
        tls_enabled: tls_status.enabled,
        tls_cert_path: tls_status.cert_path,
        tls_key_path: tls_status.key_path,
    })
}

fn apply_config_update(payload: UpdateDaemonConfigRequest) -> anyhow::Result<DaemonConfigView> {
    let mut config = Config::load_or_create()?;
    let remote = RemoteManager::new(config.clone());

    if let Some(node_name) = payload
        .node_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        remote.rename(node_name)?;
    }
    if let Some(profile) = payload.profile {
        config.daemon.profile = parse_profile(&profile)?;
        config.apply_profile_preset();
    }
    if let Some(developer_mode) = payload.developer_mode {
        config.daemon.developer_mode = developer_mode;
    }
    if let Some(privacy_mode) = payload.privacy_mode {
        config.webui.privacy_mode = privacy_mode;
    }
    if let Some(idle) = payload.idle_timeout_secs {
        config.webui.idle_timeout_secs = idle.max(60);
    }
    if let Some(absolute) = payload.absolute_timeout_secs {
        config.webui.absolute_timeout_secs = absolute.max(config.webui.idle_timeout_secs);
    }
    if let Some(reauth) = payload.reauth_window_secs {
        config.webui.reauth_window_secs = reauth.max(60);
    }
    if let Some(persist) = payload.session_persist_on_restart {
        config.webui.session_persist_on_restart = persist;
    }
    if let Some(mode) = payload.approval_mode {
        config.approvals.mode = parse_approval_mode(&mode)?;
    }
    if let Some(secret_backend) = payload.secret_backend {
        config.secrets.backend = parse_secret_backend(&secret_backend)?;
    }

    config.save()?;
    daemon_config_view()
}

pub async fn reload_config() -> impl IntoResponse {
    match daemon_config_view() {
        Ok(config) => (StatusCode::OK, Json(config)).into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn browser_status() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => (
            StatusCode::OK,
            Json(browser_surface::BrowserManager::new(config).status()),
        )
            .into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn update_browser(Json(payload): Json<sdk::BrowserSurfaceUpdate>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match browser_surface::BrowserManager::new(config).replace(payload) {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn voice_status() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match voice_surface::VoiceManager::new(config).status().await {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn install_voice_engine(
    Json(payload): Json<sdk::VoiceEngineInstallRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match voice_surface::VoiceManager::new(config)
            .install_engine(payload)
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn uninstall_voice_engine(
    Json(payload): Json<sdk::VoiceEngineSelectionRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match voice_surface::VoiceManager::new(config)
            .uninstall_engine(payload.engine)
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn activate_voice_input(
    Json(payload): Json<sdk::VoiceEngineSelectionRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match voice_surface::VoiceManager::new(config)
            .activate_input(payload.engine)
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn activate_voice_output(
    Json(payload): Json<sdk::VoiceEngineSelectionRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match voice_surface::VoiceManager::new(config)
            .activate_output(payload.engine)
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn test_voice_input(
    payload: Option<Json<sdk::VoiceInputTestRequest>>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match voice_surface::VoiceManager::new(config)
            .test_input(payload.map(|value| value.0).unwrap_or_default())
            .await
        {
            Ok(result) => (StatusCode::OK, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn test_voice_output(
    Json(payload): Json<sdk::VoiceOutputTestRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match voice_surface::VoiceManager::new(config)
            .test_output(payload)
            .await
        {
            Ok(result) => (StatusCode::OK, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn update_voice(Json(payload): Json<sdk::VoiceSurfaceUpdate>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match voice_surface::VoiceManager::new(config)
            .replace(payload)
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn memory_status() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config).status().await {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn update_memory(
    Json(payload): Json<memory_surface::MemorySurfaceUpdate>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .replace(payload)
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn query_memory(
    Json(payload): Json<memory_surface::MemoryQueryRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .query(payload)
            .await
        {
            Ok(response) => (StatusCode::OK, Json(response)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn inspect_memory_graph(
    Query(query): Query<MemoryGraphInspectQuery>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .inspect_graph(query.entity)
            .await
        {
            Ok(response) => (StatusCode::OK, Json(response)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn reindex_memory() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config).reindex().await {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn backfill_memory_embeddings(
    Json(payload): Json<memory_surface::MemoryBackfillRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = memory_surface::MemoryManager::new(config);
            let batch_size = payload.batch_size.unwrap_or(100).max(1);
            match manager.backfill_embeddings(batch_size).await {
                Ok(backfilled) => match manager.status().await {
                    Ok(status) => (
                        StatusCode::OK,
                        Json(memory_surface::MemoryBackfillResponse { backfilled, status }),
                    )
                        .into_response(),
                    Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
                },
                Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
            }
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn memory_adapters() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .adapter_status()
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn refresh_memory_adapters() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .refresh_adapters()
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn ingest_memory_note(
    Json(payload): Json<memory_surface::MemoryIngestRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .ingest_note(payload)
            .await
        {
            Ok(hit) => (StatusCode::OK, Json(hit)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

#[derive(Debug, Deserialize)]
pub struct EpisodicBrowseQuery {
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct MemoryDeleteQuery {
    pub expected_content_hash: Option<String>,
    pub actor: Option<String>,
    pub source_task_id: Option<String>,
}

pub async fn list_episodic_memories(Query(query): Query<EpisodicBrowseQuery>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let offset = query.offset.unwrap_or(0).max(0);
            let limit = query.limit.unwrap_or(50).clamp(1, 200);
            match memory_surface::MemoryManager::new(config)
                .list_episodic(offset, limit)
                .await
            {
                Ok(response) => (StatusCode::OK, Json(response)).into_response(),
                Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
            }
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_memory_facts() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .list_facts()
            .await
        {
            Ok(facts) => (StatusCode::OK, Json(facts)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn episodic_memory_history(Path(id): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .episodic_history(&id)
            .await
        {
            Ok(result) => (StatusCode::OK, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn fact_memory_history(Path(key): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .fact_history(&key)
            .await
        {
            Ok(result) => (StatusCode::OK, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn redact_episodic_memory(
    Path(id): Path<String>,
    Json(payload): Json<memory_surface::MemoryRedactRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .redact_episodic(&id, payload)
            .await
        {
            Ok(result) => (StatusCode::OK, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn redact_memory_fact(
    Path(key): Path<String>,
    Json(payload): Json<memory_surface::MemoryRedactRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .redact_fact(&key, payload)
            .await
        {
            Ok(result) => (StatusCode::OK, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn delete_episodic_memory(
    Path(id): Path<String>,
    Query(query): Query<MemoryDeleteQuery>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .delete_episodic(
                &id,
                memory_surface::MemoryDeleteRequest {
                    expected_content_hash: query.expected_content_hash,
                    actor: query.actor,
                    source_task_id: query.source_task_id,
                },
            )
            .await
        {
            Ok(result) => (StatusCode::OK, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn delete_memory_fact(
    Path(key): Path<String>,
    Query(query): Query<MemoryDeleteQuery>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match memory_surface::MemoryManager::new(config)
            .delete_fact(
                &key,
                memory_surface::MemoryDeleteRequest {
                    expected_content_hash: query.expected_content_hash,
                    actor: query.actor,
                    source_task_id: query.source_task_id,
                },
            )
            .await
        {
            Ok(result) => (StatusCode::OK, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn hook_status() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = crate::hooks::HookManager::discover(&config);
            (StatusCode::OK, Json(manager.status().await)).into_response()
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn inspect_hook(Path(name): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = crate::hooks::HookManager::discover(&config);
            match manager.inspect(&name).await {
                Some(hook) => (StatusCode::OK, Json(hook)).into_response(),
                None => json_error_response(StatusCode::NOT_FOUND, "Hook not found"),
            }
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn get_approval_mode() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => (
            StatusCode::OK,
            Json(serde_json::json!({ "mode": config.approvals.mode.as_str() })),
        )
            .into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn update_approval_mode(
    Json(payload): Json<UpdateApprovalModeRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(mut config) => match parse_approval_mode(&payload.mode) {
            Ok(mode) => {
                config.approvals.mode = mode;
                match config.save() {
                    Ok(()) => (
                        StatusCode::OK,
                        Json(serde_json::json!({ "mode": config.approvals.mode.as_str() })),
                    )
                        .into_response(),
                    Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
                }
            }
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_approval_rules() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match approvals::load_rules(&config) {
            Ok(file) => (StatusCode::OK, Json(file)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn add_approval_rule(Json(payload): Json<AddApprovalRuleRequest>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match parse_rule_action(&payload.action) {
            Ok(action) => {
                let rule = approvals::ApprovalRule {
                    id: payload.id,
                    action,
                    tool: payload.tool,
                    commands: payload.commands,
                    paths: payload.paths,
                    nodes: payload.nodes,
                    channels: payload.channels,
                    risk_tier: payload.risk_tier,
                    effect: payload.effect,
                };
                match approvals::add_rule(&config, rule) {
                    Ok(file) => (StatusCode::CREATED, Json(file)).into_response(),
                    Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
                }
            }
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn remove_approval_rule(Path(id): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match approvals::remove_rule(&config, &id) {
            Ok(true) => StatusCode::NO_CONTENT.into_response(),
            Ok(false) => json_error_response(
                StatusCode::NOT_FOUND,
                anyhow::anyhow!("Approval rule '{}' was not found", id),
            ),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn create_task(
    State(state): State<AppState>,
    Json(payload): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    let Some(raw_input) = parse_task_input(payload.input, payload.prompt) else {
        return invalid_request("Request must include a non-empty `input` or `prompt` field");
    };

    let (input, implicit_node) = extract_task_target(&raw_input);
    let target_node = payload.node.or(implicit_node);
    let execution_profile = if let Some(agent_id) = payload
        .agent_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let repo = match crate::system::specs::SpecRepository::new() {
            Ok(repo) => repo,
            Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        };
        match crate::cli::agents::execution_profile_for_agent(&repo, agent_id) {
            Ok(profile) => Some(profile),
            Err(error) => return json_error_response(StatusCode::BAD_REQUEST, error),
        }
    } else {
        None
    };

    if let Some(node) = target_node.filter(|value| !value.trim().is_empty()) {
        if execution_profile.is_some() {
            return json_error_response(
                StatusCode::BAD_REQUEST,
                anyhow::anyhow!("`agent_id` is not supported for remote node task submission yet"),
            );
        }
        match Config::load_or_create() {
            Ok(config) => match RemoteManager::new(config)
                .send_with_options(
                    &input,
                    crate::remote::RemoteSendOptions {
                        node: Some(node),
                        ..crate::remote::RemoteSendOptions::default()
                    },
                )
                .await
            {
                Ok(result) => {
                    return (
                        StatusCode::ACCEPTED,
                        Json(serde_json::json!({
                            "task_id": result.remote_task_id,
                            "status": result.status,
                        })),
                    )
                        .into_response();
                }
                Err(error) => {
                    return json_error_response(StatusCode::BAD_REQUEST, error);
                }
            },
            Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        }
    }

    match state
        .gateway
        .submit_webui(&input, None, execution_profile.as_ref())
        .await
    {
        Ok(task_id) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "task_id": task_id,
                "status": "pending",
            })),
        )
            .into_response(),
        Err(error) => internal_submission_error(error),
    }
}

#[derive(Deserialize)]
pub struct ExecuteRequest {
    pub input: Option<String>,
    pub task: Option<String>,
    pub risk_tier: Option<u8>,
    pub agent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct RemoteExecuteRequest {
    pub task_id: Option<String>,
    pub input: Option<String>,
    pub task: Option<String>,
    pub origin_node: Option<String>,
    pub coordinator_node: Option<String>,
    pub workspace: Option<String>,
    pub team_id: Option<String>,
    pub wait_seconds: Option<u64>,
    pub plan: Option<RemoteExecutionPlan>,
}

#[derive(Serialize)]
pub struct ExecuteResponse {
    pub success: bool,
    pub task_id: Option<String>,
    pub status: String,
    pub answer: Option<String>,
    pub provider: Option<String>,
    pub duration_ms: Option<i64>,
    pub message: Option<String>,
}

pub async fn execute_task(
    State(state): State<AppState>,
    Json(payload): Json<ExecuteRequest>,
) -> impl IntoResponse {
    let Some(input) = parse_task_input(payload.input, payload.task) else {
        return invalid_request("Request must include a non-empty `input` or `task` field");
    };

    let execution_profile = if let Some(agent_id) = payload
        .agent_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let repo = match crate::system::specs::SpecRepository::new() {
            Ok(repo) => repo,
            Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        };
        match crate::cli::agents::execution_profile_for_agent(&repo, agent_id) {
            Ok(profile) => Some(profile),
            Err(error) => return json_error_response(StatusCode::BAD_REQUEST, error),
        }
    } else {
        None
    };

    let task_id = match state
        .gateway
        .submit_webui(&input, None, execution_profile.as_ref())
        .await
    {
        Ok(task_id) => task_id,
        Err(error) => return internal_submission_error(error),
    };

    completion_response(
        &task_id,
        completion::wait_for_completion(&state, &task_id, Duration::from_secs(30)).await,
    )
}

pub async fn execute_remote_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RemoteExecuteRequest>,
) -> impl IntoResponse {
    let Some(input) = parse_task_input(payload.input.clone(), payload.task.clone()) else {
        return invalid_request("Request must include a non-empty `input` or `task` field");
    };

    match Config::load_or_create() {
        Ok(config) => {
            if payload.origin_node.is_some() {
                if payload.task_id.as_deref().is_none() {
                    return invalid_request("Remote execute requests must include a task_id");
                }
                if let Err(error) = RemoteManager::new(config).verify_signed_request(
                    &headers,
                    "execute",
                    payload.task_id.as_deref(),
                ) {
                    return json_error_response(StatusCode::UNAUTHORIZED, error);
                }
            }
        }
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }

    if let Some(plan) = payload.plan.clone() {
        return accept_remote_planned_task(state, payload, input, plan).await;
    }

    let task_id = match state
        .gateway
        .submit_remote(
            &input,
            payload.origin_node.as_deref(),
            None,
            payload.workspace.as_deref(),
            payload.team_id.as_deref(),
        )
        .await
    {
        Ok(task_id) => task_id,
        Err(error) => return internal_submission_error(error),
    };

    let wait = Duration::from_secs(payload.wait_seconds.unwrap_or(1).clamp(0, 300));
    completion_response(
        &task_id,
        completion::wait_for_completion(&state, &task_id, wait).await,
    )
}

async fn accept_remote_planned_task(
    state: AppState,
    payload: RemoteExecuteRequest,
    input: String,
    plan: RemoteExecutionPlan,
) -> axum::response::Response {
    let task_id = match payload.task_id.as_deref() {
        Some(raw) => match Uuid::parse_str(raw) {
            Ok(value) => value,
            Err(_) => return invalid_request("`task_id` must be a valid UUID when provided"),
        },
        None => Uuid::new_v4(),
    };
    let task_id_str = task_id.to_string();
    let source = TaskSource::Remote(payload.origin_node.clone().unwrap_or_default());
    let workspace_override = payload.workspace.clone().map(PathBuf::from);
    let domain = plan
        .domain_hint
        .clone()
        .unwrap_or_else(|| "general".to_string());

    if let Err(error) = state
        .db
        .pending_tasks()
        .create_task_with_dispatch(
            &task_id_str,
            &input,
            source.clone(),
            None,
            &domain,
            "simple",
            false,
            None,
            payload.workspace.as_deref(),
            payload.team_id.as_deref(),
        )
        .await
    {
        return internal_submission_error(error);
    }
    if let Err(error) = state.db.pending_tasks().mark_running(&task_id_str).await {
        return internal_submission_error(error);
    }

    let state_clone = state.clone();
    let task_id_for_spawn = task_id_str.clone();
    tokio::spawn(async move {
        let result = run_remote_planned_task(
            state_clone.clone(),
            task_id,
            input,
            source,
            workspace_override,
            plan,
        )
        .await;

        let pending_repo = state_clone.db.pending_tasks();
        match result {
            Ok(_) => {
                let _ = pending_repo.mark_done(&task_id_for_spawn).await;
            }
            Err(error) => {
                let _ = pending_repo
                    .mark_failed(&task_id_for_spawn, &error.to_string())
                    .await;
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(ExecuteResponse {
            success: true,
            task_id: Some(task_id_str),
            status: "running".to_string(),
            answer: None,
            provider: None,
            duration_ms: None,
            message: Some("Accepted direct remote execution plan".to_string()),
        }),
    )
        .into_response()
}

async fn run_remote_planned_task(
    state: AppState,
    task_id: Uuid,
    input: String,
    source: TaskSource,
    workspace_override: Option<PathBuf>,
    plan: RemoteExecutionPlan,
) -> anyhow::Result<()> {
    let task = Task {
        id: task_id,
        input,
        source,
        execution_profile: None,
        risk_tier_override: None,
        run_context_id: RunContextId(Uuid::new_v4().to_string()),
        run_mode: RunMode::Serial,
        run_isolation: RunIsolation::None,
        session_id: None,
        workspace: workspace_override.clone(),
        created_at: chrono::Utc::now().timestamp(),
    };

    if let Some(workspace) = workspace_override {
        let mut agent =
            crate::cli::bootstrap::build_task_agent(state.db.clone(), Some(workspace)).await?;
        agent.process_planned_task(task, plan).await?;
        return Ok(());
    }

    let mut agent = state.agent.write().await;
    agent.process_planned_task(task, plan).await?;
    Ok(())
}

pub async fn task_status(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    completion_response(
        &task_id,
        completion::load_completion(&state, &task_id).await,
    )
}

pub async fn remote_task_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            if let Err(error) = RemoteManager::new(config).verify_signed_request(
                &headers,
                "task_status",
                Some(&task_id),
            ) {
                return json_error_response(StatusCode::UNAUTHORIZED, error);
            }
        }
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }

    completion_response(
        &task_id,
        completion::load_completion(&state, &task_id).await,
    )
}

pub async fn list_services() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let services = ServiceManager::new(config).list();
            (StatusCode::OK, Json(services)).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn overview(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let config = match Config::load_or_create() {
        Ok(config) => config,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };

    let config_view = match daemon_config_view() {
        Ok(view) => view,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let tasks = match state.db.tasks().get_recent_tasks(20).await {
        Ok(items) => items,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let agent_runs = match state.db.agent_runs().list_agent_runs(20).await {
        Ok(items) => items,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let workflow_runs = match state.db.agent_runs().list_workflow_runs(20).await {
        Ok(items) => items,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let services = ServiceManager::new(config.clone()).list();
    let channels = match ChannelManager::new(config.clone()).list().await {
        Ok(items) => items,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let remote = RemoteManager::new(config.clone()).status().ok();
    let extensions = match extensions::inventory(&config).await {
        Ok(items) => items,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let extension_updates = match extensions::updates(&config, false).await {
        Ok(items) => items,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let pending_approvals = approvals::list_pending();
    let pending_approvals_count = pending_approvals.len();
    let extension_count = extensions.len();
    let queue = match state.db.pending_tasks().queue_stats().await {
        Ok(stats) => stats,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let local_load = match current_node_load(&state).await {
        Ok(load) => load,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let remote_nodes = RemoteManager::new(config.clone())
        .nodes()
        .unwrap_or_default();
    let remote_candidates = ZeroTierManager::new(config.clone())
        .list_candidates()
        .await
        .unwrap_or_default();
    let zerotier = ZeroTierManager::new(config.clone()).status().await.ok();
    let repo = match SpecRepository::new() {
        Ok(repo) => repo,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let agents = match repo.list_agents() {
        Ok(items) => items,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let workflows = match repo.list_workflows() {
        Ok(items) => items,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let recent_logs = match logs::recent_lines(120) {
        Ok(items) => items,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let auth_status = request_auth_status(&state, &headers).await;
    let health = match health::collect_snapshot_with_auth(&config, auth_status.as_ref()).await {
        Ok(snapshot) => snapshot,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let onboarding = match onboarding::collect(&config, &state.db, &health).await {
        Ok(checklist) => checklist,
        Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "config": config_view,
            "tasks": tasks,
            "agent_runs": agent_runs,
            "workflow_runs": workflow_runs,
            "approvals": pending_approvals,
            "services": services,
            "channels": channels,
            "remote": remote,
            "extensions": {
                "installed": extensions,
                "updates": extension_updates,
            },
            "counts": {
                "agents": agents.len(),
                "workflows": workflows.len(),
                "extensions": extension_count,
                "pending_approvals": pending_approvals_count,
            },
            "queue": queue,
            "local_load": local_load,
            "remote_nodes": remote_nodes,
            "remote_candidates": remote_candidates,
            "zerotier": zerotier,
            "health": health,
            "onboarding": onboarding,
            "recent_logs": recent_logs,
        })),
    )
        .into_response()
}

pub async fn recent_logs() -> impl IntoResponse {
    match logs::recent_lines(120) {
        Ok(lines) => (StatusCode::OK, Json(serde_json::json!({ "lines": lines }))).into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn stream_logs() -> impl IntoResponse {
    let stream = stream! {
        let mut sent_count = 0usize;

        loop {
            match logs::recent_lines(400) {
                Ok(lines) => {
                    if lines.len() < sent_count {
                        sent_count = 0;
                    }

                    for line in lines.iter().skip(sent_count) {
                        let payload = serde_json::json!({
                            "type": "line",
                            "line": line,
                        });
                        yield Ok::<Bytes, Infallible>(Bytes::from(format!("{payload}\n")));
                    }

                    sent_count = lines.len();
                }
                Err(error) => {
                    let payload = serde_json::json!({
                        "type": "error",
                        "error": error.to_string(),
                    });
                    yield Ok::<Bytes, Infallible>(Bytes::from(format!("{payload}\n")));
                    break;
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/x-ndjson")
        .header("cache-control", "no-store")
        .body(Body::from_stream(stream))
        .expect("log stream response")
}

pub async fn health_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let auth_status = request_auth_status(&state, &headers).await;
            match health::collect_snapshot_with_auth(&config, auth_status.as_ref()).await {
                Ok(snapshot) => (StatusCode::OK, Json(snapshot)).into_response(),
                Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
            }
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn export_backup(Json(payload): Json<BackupExportRequest>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = backup::BackupManager::new(config);
            let target = match payload.path {
                Some(path) => PathBuf::from(path),
                None => match manager.default_export_path() {
                    Ok(path) => path,
                    Err(error) => {
                        return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error)
                    }
                },
            };
            match manager.export(&target, payload.force.unwrap_or(false)) {
                Ok(manifest) => (
                    StatusCode::CREATED,
                    Json(serde_json::json!({
                        "path": target.display().to_string(),
                        "manifest": manifest,
                    })),
                )
                    .into_response(),
                Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
            }
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn restore_backup(Json(payload): Json<BackupRestoreRequest>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = backup::BackupManager::new(config);
            let source = PathBuf::from(payload.path);
            match manager.restore(&source, payload.force.unwrap_or(false)) {
                Ok(manifest) => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "path": source.display().to_string(),
                        "manifest": manifest,
                    })),
                )
                    .into_response(),
                Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
            }
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn inspect_migration(
    Path(source): Path<String>,
    Json(payload): Json<MigrationRequest>,
) -> impl IntoResponse {
    let source = match parse_migration_source(&source) {
        Ok(source) => source,
        Err(error) => return json_error_response(StatusCode::BAD_REQUEST, error),
    };
    let root_override = payload.path.as_ref().map(PathBuf::from);
    match migrate::inspect(source, root_override.as_deref()) {
        Ok(report) => (StatusCode::OK, Json(report)).into_response(),
        Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
    }
}

pub async fn import_migration(
    Path(source): Path<String>,
    Json(payload): Json<MigrationRequest>,
) -> impl IntoResponse {
    let source = match parse_migration_source(&source) {
        Ok(source) => source,
        Err(error) => return json_error_response(StatusCode::BAD_REQUEST, error),
    };
    let root_override = payload.path.as_ref().map(PathBuf::from);
    let dry_run = payload.dry_run.unwrap_or(false);
    match SpecRepository::new() {
        Ok(repo) => match migrate::import(&repo, source, root_override.as_deref(), dry_run) {
            Ok(result) => (StatusCode::CREATED, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn migration_status() -> impl IntoResponse {
    match SpecRepository::new() {
        Ok(repo) => match migrate::migrate_status(&repo) {
            Ok(report) => (StatusCode::OK, Json(report)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn service_status(Path(name): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let Some(service) = ManagedService::parse(&name) else {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": format!("Unknown service '{}'", name) })),
                )
                    .into_response();
            };
            let status = ServiceManager::new(config).describe(service);
            (StatusCode::OK, Json(status)).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn service_install_status() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ServiceInstaller::new(config).status() {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn install_service(Json(payload): Json<InstallServiceRequest>) -> impl IntoResponse {
    let mode = match parse_service_install_mode(&payload.mode) {
        Ok(mode) => mode,
        Err(error) => return json_error_response(StatusCode::BAD_REQUEST, error),
    };

    let profile = match payload.profile.as_deref() {
        Some(profile) => match parse_profile(profile) {
            Ok(profile) => Some(profile),
            Err(error) => return json_error_response(StatusCode::BAD_REQUEST, error),
        },
        None => None,
    };

    match Config::load_or_create() {
        Ok(config) => match ServiceInstaller::new(config).install(
            mode,
            profile,
            payload.port.unwrap_or(crate::info::DEFAULT_PORT),
        ) {
            Ok(status) => (StatusCode::CREATED, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn uninstall_service(Path(mode): Path<String>) -> impl IntoResponse {
    let mode = match parse_service_install_mode(&mode) {
        Ok(mode) => mode,
        Err(error) => return json_error_response(StatusCode::BAD_REQUEST, error),
    };

    match Config::load_or_create() {
        Ok(config) => match ServiceInstaller::new(config).uninstall(mode) {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn set_service_enabled_inner(name: String, enabled: bool) -> axum::response::Response {
    match Config::load_or_create() {
        Ok(config) => {
            let Some(service) = ManagedService::parse(&name) else {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": format!("Unknown service '{}'", name) })),
                )
                    .into_response();
            };
            let mut manager = ServiceManager::new(config);
            match manager.set_enabled(service, enabled) {
                Ok(status) => (StatusCode::OK, Json(status)).into_response(),
                Err(error) => (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": error.to_string() })),
                )
                    .into_response(),
            }
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn enable_service(Path(name): Path<String>) -> impl IntoResponse {
    set_service_enabled_inner(name, true).await
}

pub async fn disable_service(Path(name): Path<String>) -> impl IntoResponse {
    set_service_enabled_inner(name, false).await
}

pub async fn list_channels() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ChannelManager::new(config).list().await {
            Ok(channels) => (StatusCode::OK, Json(channels)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn telegram_channel_status() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ChannelManager::new(config).telegram_status().await {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn telegram_channel_setup(
    Json(payload): Json<TelegramChannelSetupRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ChannelManager::new(config)
            .telegram_setup(TelegramSetupInput {
                token: payload.token,
                allowed_ids: payload.allowed_ids,
                confirmation_chat_id: payload.confirmation_chat_id,
                api_base_url: payload.api_base_url,
                default_agent_id: payload.default_agent_id,
            })
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn telegram_channel_enable() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ChannelManager::new(config).telegram_set_enabled(true).await {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn telegram_channel_disable() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ChannelManager::new(config)
            .telegram_set_enabled(false)
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn plugin_channel_statuses() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ChannelManager::new(config).plugin_statuses().await {
            Ok(statuses) => (StatusCode::OK, Json(statuses)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn plugin_channel_deliver(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<PluginChannelDeliverRequest>,
) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ChannelManager::new(config)
            .deliver_plugin(
                &name,
                PluginChannelDeliverInput {
                    input: payload.input,
                    session_id: payload.session_id,
                    workspace: payload.workspace,
                    team_id: payload.team_id,
                },
                state.gateway.clone(),
            )
            .await
        {
            Ok(result) => (StatusCode::ACCEPTED, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn telegram_channel_test() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ChannelManager::new(config).telegram_test().await {
            Ok(result) => (StatusCode::OK, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_policies() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = PolicyManager::new(config, None);
            match manager.list().await {
                Ok(policies) => (StatusCode::OK, Json(policies)).into_response(),
                Err(error) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": error.to_string() })),
                )
                    .into_response(),
            }
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn active_policies() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = PolicyManager::new(config, None);
            match manager.active().await {
                Ok(policies) => (StatusCode::OK, Json(policies)).into_response(),
                Err(error) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": error.to_string() })),
                )
                    .into_response(),
            }
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct PolicyExplainRequest {
    pub task: String,
}

#[derive(Debug, Deserialize)]
pub struct PolicyAddRequest {
    pub name: String,
    pub scope: String,
}

#[derive(Debug, Deserialize)]
pub struct PolicyResolveRequest {
    pub approved: bool,
}

pub async fn explain_policy(Json(payload): Json<PolicyExplainRequest>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = PolicyManager::new(config, None);
            match manager.explain(&payload.task).await {
                Ok(report) => (StatusCode::OK, Json(report)).into_response(),
                Err(error) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": error.to_string() })),
                )
                    .into_response(),
            }
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn enable_policy(Path(name): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = PolicyManager::new(config, None);
            match manager.enable(&name).await {
                Ok(()) => StatusCode::NO_CONTENT.into_response(),
                Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
            }
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn disable_policy(Path(name): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = PolicyManager::new(config, None);
            match manager.disable(&name).await {
                Ok(()) => StatusCode::NO_CONTENT.into_response(),
                Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
            }
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn add_policy(Json(payload): Json<PolicyAddRequest>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = PolicyManager::new(config, None);
            let scope = match payload.scope.trim().to_ascii_lowercase().as_str() {
                "user" => PolicyScope::User,
                "workspace" => PolicyScope::Workspace,
                "project" => PolicyScope::Project,
                _ => {
                    return json_error_response(
                        StatusCode::BAD_REQUEST,
                        "scope must be user, workspace, or project",
                    );
                }
            };
            match manager.add(payload.name.trim(), scope).await {
                Ok(path) => (
                    StatusCode::CREATED,
                    Json(serde_json::json!({
                        "name": payload.name.trim(),
                        "scope": payload.scope.trim(),
                        "path": path,
                    })),
                )
                    .into_response(),
                Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
            }
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn remove_policy(Path(name): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => {
            let manager = PolicyManager::new(config, None);
            match manager.remove(&name).await {
                Ok(path) => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "name": name,
                        "path": path,
                    })),
                )
                    .into_response(),
                Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
            }
        }
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn list_approvals() -> impl IntoResponse {
    (StatusCode::OK, Json(approvals::list_pending())).into_response()
}

pub async fn resolve_approval(
    Path(id): Path<String>,
    Json(payload): Json<PolicyResolveRequest>,
) -> impl IntoResponse {
    if approvals::resolve(&id, payload.approved) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        json_error_response(StatusCode::NOT_FOUND, "Approval not found")
    }
}

pub async fn remote_status(State(state): State<AppState>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match RemoteManager::new(config.clone()).status() {
            Ok(mut status) => match current_node_load(&state).await {
                Ok(load) => {
                    status.load = Some(load);
                    if let Ok(transports) = ZeroTierManager::new(config).transport_records().await {
                        status.transports = transports;
                    }
                    (StatusCode::OK, Json(status)).into_response()
                }
                Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
            },
            Err(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response(),
        },
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn remote_public_status(State(state): State<AppState>) -> impl IntoResponse {
    remote_status(State(state)).await
}

pub async fn remote_identity() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match RemoteManager::new(config).identity_status() {
            Ok(identity) => (StatusCode::OK, Json(identity)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn remote_handshake(Json(payload): Json<RemoteHandshakeRequest>) -> impl IntoResponse {
    if payload.challenge.trim().is_empty() {
        return invalid_request("Remote handshake requests must include a non-empty challenge");
    }
    match Config::load_or_create() {
        Ok(config) => match RemoteManager::new(config).sign_handshake(&payload.challenge) {
            Ok(proof) => (StatusCode::OK, Json(proof)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn zerotier_status() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ZeroTierManager::new(config).status().await {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn zerotier_join(Json(payload): Json<ZeroTierJoinRequest>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ZeroTierManager::new(config)
            .join(payload.network_id.as_deref())
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn zerotier_install() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ZeroTierManager::new(config).install().await {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn zerotier_uninstall() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ZeroTierManager::new(config).uninstall().await {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn zerotier_setup(Json(payload): Json<ZeroTierSetupRequest>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ZeroTierManager::new(config)
            .setup(
                &payload.network_id,
                payload.api_token_key.as_deref(),
                payload.managed_name_sync.unwrap_or(true),
            )
            .await
        {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn zerotier_refresh() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ZeroTierManager::new(config).refresh().await {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn zerotier_candidates() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ZeroTierManager::new(config).list_candidates().await {
            Ok(candidates) => (StatusCode::OK, Json(candidates)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn zerotier_trust_candidate(Path(candidate_id): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match ZeroTierManager::new(config)
            .trust_candidate(&candidate_id)
            .await
        {
            Ok(candidate) => (StatusCode::OK, Json(candidate)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
        },
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn current_node_load(state: &AppState) -> anyhow::Result<NodeLoadSnapshot> {
    let queue = state.db.pending_tasks().queue_stats().await?;
    let outcomes = state.db.tasks().recent_outcome_stats(25).await?;
    Ok(NodeLoadSnapshot {
        pending_tasks: queue.pending,
        running_tasks: queue.running,
        recent_failures: outcomes.recent_failures,
        recent_successes: outcomes.recent_successes,
        cpu_load_percent: crate::platform::cpu_load_percent(),
        available_ram_mb: Some(crate::platform::available_ram() / (1024 * 1024)),
        recent_avg_duration_ms: outcomes.recent_avg_duration_ms,
    })
}

pub async fn remote_nodes() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match RemoteManager::new(config).nodes_with_status().await {
            Ok(nodes) => (StatusCode::OK, Json(nodes)).into_response(),
            Err(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response(),
        },
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct RemotePairRequest {
    pub target: String,
    pub url: Option<String>,
    pub token: Option<String>,
    #[serde(default)]
    pub executor_only: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

pub async fn remote_pair(Json(payload): Json<RemotePairRequest>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match RemoteManager::new(config)
            .pair(
                &payload.target,
                payload.url.as_deref(),
                payload.token.as_deref(),
                payload.executor_only,
                &payload.tags,
                &payload.capabilities,
            )
            .await
        {
            Ok(peer) => (StatusCode::CREATED, Json(peer)).into_response(),
            Err(error) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response(),
        },
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn remote_unpair(Path(name): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match RemoteManager::new(config).unpair(&name).await {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response(),
        },
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct RemoteRenameRequest {
    pub name: String,
}

pub async fn remote_rename(Json(payload): Json<RemoteRenameRequest>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match RemoteManager::new(config).rename(&payload.name) {
            Ok(identity) => (StatusCode::OK, Json(identity)).into_response(),
            Err(error) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response(),
        },
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn remote_trust(Path(name): Path<String>) -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match RemoteManager::new(config).trust(&name) {
            Ok(peer) => (StatusCode::OK, Json(peer)).into_response(),
            Err(error) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response(),
        },
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct RemoteSendRequest {
    pub node: Option<String>,
    pub input: Option<String>,
    pub task: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub allow_executor_only: bool,
    #[serde(default)]
    pub prefer_executor_only: bool,
}

pub async fn remote_send(Json(payload): Json<RemoteSendRequest>) -> impl IntoResponse {
    let Some(task) = parse_task_input(payload.input, payload.task) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Missing task input. Provide `input` or `task`."
            })),
        )
            .into_response();
    };

    match Config::load_or_create() {
        Ok(config) => match RemoteManager::new(config)
            .send_with_options(
                &task,
                crate::remote::RemoteSendOptions {
                    node: payload.node,
                    required_tags: payload.tags,
                    required_capabilities: payload.capabilities,
                    allow_executor_only: payload.allow_executor_only,
                    prefer_executor_only: payload.prefer_executor_only,
                    execution_plan: None,
                },
            )
            .await
        {
            Ok(result) => (StatusCode::OK, Json(result)).into_response(),
            Err(error) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
                .into_response(),
        },
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

fn parse_task_input(input: Option<String>, task: Option<String>) -> Option<String> {
    input
        .or(task)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn invalid_request(message: &str) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ExecuteResponse {
            success: false,
            task_id: None,
            status: "invalid_request".to_string(),
            answer: None,
            provider: None,
            duration_ms: None,
            message: Some(message.to_string()),
        }),
    )
        .into_response()
}

fn internal_submission_error(error: anyhow::Error) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ExecuteResponse {
            success: false,
            task_id: None,
            status: "failed".to_string(),
            answer: None,
            provider: None,
            duration_ms: None,
            message: Some(format!("Failed to submit task: {}", error)),
        }),
    )
        .into_response()
}

fn completion_response(
    task_id: &str,
    completion: anyhow::Result<completion::CompletionState>,
) -> axum::response::Response {
    match completion {
        Ok(completion::CompletionState::Done(result)) => (
            StatusCode::OK,
            Json(ExecuteResponse {
                success: true,
                task_id: Some(result.task_id),
                status: "completed".to_string(),
                answer: Some(result.answer),
                provider: result.provider,
                duration_ms: result.duration_ms,
                message: None,
            }),
        )
            .into_response(),
        Ok(completion::CompletionState::Failed(error)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ExecuteResponse {
                success: false,
                task_id: Some(task_id.to_string()),
                status: "failed".to_string(),
                answer: None,
                provider: None,
                duration_ms: None,
                message: Some(error),
            }),
        )
            .into_response(),
        Ok(completion::CompletionState::Running) => (
            StatusCode::ACCEPTED,
            Json(ExecuteResponse {
                success: true,
                task_id: Some(task_id.to_string()),
                status: "running".to_string(),
                answer: None,
                provider: None,
                duration_ms: None,
                message: Some("Task accepted and still running".to_string()),
            }),
        )
            .into_response(),
        Ok(completion::CompletionState::Missing) => (
            StatusCode::NOT_FOUND,
            Json(ExecuteResponse {
                success: false,
                task_id: Some(task_id.to_string()),
                status: "missing".to_string(),
                answer: None,
                provider: None,
                duration_ms: None,
                message: Some("Task not found".to_string()),
            }),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ExecuteResponse {
                success: false,
                task_id: Some(task_id.to_string()),
                status: "failed".to_string(),
                answer: None,
                provider: None,
                duration_ms: None,
                message: Some(format!("Failed to fetch task result: {}", error)),
            }),
        )
            .into_response(),
    }
}
