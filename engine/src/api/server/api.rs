use axum::{
    extract::{Json, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

use super::{auth::AuthManager, completion, AppState};
use crate::channels::manager::{ChannelManager, TelegramSetupInput};
use crate::cli::brain::dispatch_family;
use crate::cli::extensions;
use crate::config::Config;
use crate::gateway::Task;
use crate::policy::PolicyManager;
use crate::remote::RemoteManager;
use crate::security::approvals;
use crate::service_install::{ServiceInstallMode, ServiceInstaller};
use crate::services::{ManagedService, ServiceManager};
use crate::system::{backup, factory, health, logs, migrate};
use crate::specs::{allowed_tools, SpecRepository};
use crate::targeting::extract_task_target;
use crate::zerotier::ZeroTierManager;
use sdk::{
    AgentSpec, AuthState, DaemonCapabilities, DaemonHello, NodeLoadSnapshot, NodeSummary,
    PolicyScope, RemoteExecutionPlan, RunContextId, RunIsolation, RunMode, SpecRunStatus,
    TaskExecutionProfile, TaskSource, WorkflowSpec,
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

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub prompt: Option<String>,
    pub input: Option<String>,
    pub node: Option<String>,
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

pub async fn list_tasks(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.tasks().get_recent_tasks(50).await {
        Ok(tasks) => (StatusCode::OK, Json(tasks)).into_response(),
        Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
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

pub async fn preview_agent_factory(
    Json(payload): Json<FactoryGenerateRequest>,
) -> impl IntoResponse {
    match factory::preview_agent(
        &payload.requirement,
        payload.template_id.as_deref(),
        payload.id.as_deref(),
        payload.name.as_deref(),
    ) {
        Ok(spec) => (StatusCode::OK, Json(spec)).into_response(),
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
            Ok(spec) => (StatusCode::CREATED, Json(spec)).into_response(),
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
            Ok(spec) => (StatusCode::CREATED, Json(spec)).into_response(),
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
        purpose: Some(spec.purpose.clone()),
        instructions: spec.instructions.clone(),
        allowed_tools: allowed_tools(&spec),
        output_contract: spec.output_contract.clone(),
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
                .finish_agent_run(&run_id, SpecRunStatus::Failed, None, None, Some(&error.to_string()))
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
    (
        StatusCode::OK,
        Json(factory::list_workflow_templates()),
    )
        .into_response()
}

pub async fn preview_workflow_factory(
    Json(payload): Json<FactoryGenerateRequest>,
) -> impl IntoResponse {
    match factory::preview_workflow(
        &payload.requirement,
        payload.template_id.as_deref(),
        payload.id.as_deref(),
        payload.name.as_deref(),
    ) {
        Ok(spec) => (StatusCode::OK, Json(spec)).into_response(),
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
            Ok(spec) => (StatusCode::CREATED, Json(spec)).into_response(),
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
            Ok(spec) => (StatusCode::CREATED, Json(spec)).into_response(),
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

    let run_id = Uuid::new_v4().to_string();
    if let Err(error) = state
        .db
        .agent_runs()
        .start_workflow_run(&run_id, &workflow.id, &input)
        .await
    {
        return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error);
    }

    let mut last_output = input.clone();
    for step in &workflow.steps {
        let rendered = step
            .prompt
            .replace("{{input}}", &input)
            .replace("{{last_output}}", &last_output);
        let profile = match step.agent_id.as_deref() {
            Some(agent_id) => match repo.load_agent(agent_id) {
                Ok(spec) => Some(TaskExecutionProfile {
                    agent_id: Some(spec.id.clone()),
                    agent_name: Some(spec.name.clone()),
                    purpose: Some(spec.purpose.clone()),
                    instructions: spec.instructions.clone(),
                    allowed_tools: allowed_tools(&spec),
                    output_contract: spec.output_contract.clone(),
                }),
                Err(error) => {
                    let _ = state
                        .db
                        .agent_runs()
                        .finish_workflow_run(
                            &run_id,
                            SpecRunStatus::Failed,
                            None,
                            Some(&error.to_string()),
                        )
                        .await;
                    return json_error_response(StatusCode::BAD_REQUEST, error);
                }
            },
            None => None,
        };

        match crate::cli::run::execute_local_task_request(
            rendered,
            &config,
            RunMode::Serial,
            RunIsolation::None,
            profile,
        )
        .await
        {
            Ok(task_result) => {
                last_output = task_result.answer;
            }
            Err(error) => {
                let _ = state
                    .db
                    .agent_runs()
                    .finish_workflow_run(
                        &run_id,
                        SpecRunStatus::Failed,
                        None,
                        Some(&error.to_string()),
                    )
                    .await;
                return json_error_response(StatusCode::BAD_REQUEST, error);
            }
        }
    }

    let _ = state
        .db
        .agent_runs()
        .finish_workflow_run(&run_id, SpecRunStatus::Completed, Some(&last_output), None)
        .await;
    (
        StatusCode::OK,
        Json(ExecuteResponse {
            success: true,
            task_id: None,
            status: "completed".to_string(),
            answer: Some(last_output),
            provider: None,
            duration_ms: None,
            message: Some(run_id),
        }),
    )
        .into_response()
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
        other => Err(anyhow::anyhow!(
            "Invalid daemon profile '{}'. Use desktop or headless.",
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

    if let Some(node) = target_node.filter(|value| !value.trim().is_empty()) {
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

    match state.gateway.submit_webui(&input, None).await {
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

pub async fn overview(State(state): State<AppState>) -> impl IntoResponse {
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
    let health = match health::collect_snapshot(&config).await {
        Ok(snapshot) => snapshot,
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
            "health": health,
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

pub async fn health_snapshot() -> impl IntoResponse {
    match Config::load_or_create() {
        Ok(config) => match health::collect_snapshot(&config).await {
            Ok(snapshot) => (StatusCode::OK, Json(snapshot)).into_response(),
            Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
        },
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
                    Err(error) => return json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
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
    match SpecRepository::new() {
        Ok(repo) => match migrate::import(&repo, source, root_override.as_deref()) {
            Ok(result) => (StatusCode::CREATED, Json(result)).into_response(),
            Err(error) => json_error_response(StatusCode::BAD_REQUEST, error),
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
        Ok(config) => {
            match ChannelManager::new(config).list().await {
                Ok(channels) => (StatusCode::OK, Json(channels)).into_response(),
                Err(error) => json_error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
            }
        }
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
        Ok(config) => match ChannelManager::new(config).telegram_set_enabled(false).await {
            Ok(status) => (StatusCode::OK, Json(status)).into_response(),
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
        recent_avg_duration_ms: outcomes.recent_avg_duration_ms,
    })
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
