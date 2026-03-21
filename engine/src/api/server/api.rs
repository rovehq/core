use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

use super::{completion, AppState};
use crate::channels::manager::ChannelManager;
use crate::config::Config;
use crate::gateway::Task;
use crate::policy::PolicyManager;
use crate::remote::RemoteManager;
use crate::services::{ManagedService, ServiceManager};
use sdk::{RemoteExecutionPlan, RunContextId, RunIsolation, RunMode, TaskSource};

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

#[derive(Deserialize)]
pub struct ExecuteRequest {
    pub input: Option<String>,
    pub task: Option<String>,
    pub risk_tier: Option<u8>,
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

    let task_id = match state.gateway.submit_webui(&input, None).await {
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
    Json(payload): Json<RemoteExecuteRequest>,
) -> impl IntoResponse {
    let Some(input) = parse_task_input(payload.input.clone(), payload.task.clone()) else {
        return invalid_request("Request must include a non-empty `input` or `task` field");
    };

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
    completion_response(&task_id, completion::load_completion(&state, &task_id).await)
}

pub async fn active_skills(State(state): State<AppState>) -> impl IntoResponse {
    let agent = state.agent.read().await;
    let active = agent.active_steering_skills().await;
    (StatusCode::OK, Json(active))
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
        Ok(config) => {
            let channels = ChannelManager::new(config).list();
            (StatusCode::OK, Json(channels)).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
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

pub async fn remote_status() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match RemoteManager::new(config).status() {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
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

pub async fn remote_nodes() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match RemoteManager::new(config).nodes() {
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
