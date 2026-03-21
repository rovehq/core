use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::{completion, AppState};
use crate::channels::manager::ChannelManager;
use crate::config::Config;
use crate::policy::PolicyManager;
use crate::remote::RemoteManager;
use crate::services::{ManagedService, ServiceManager};

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
    let input = payload
        .input
        .or(payload.task)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let Some(input) = input else {
        let res = ExecuteResponse {
            success: false,
            task_id: None,
            status: "invalid_request".to_string(),
            answer: None,
            provider: None,
            duration_ms: None,
            message: Some("Request must include a non-empty `input` or `task` field".to_string()),
        };
        return (StatusCode::BAD_REQUEST, Json(res));
    };

    let task_id = match state.gateway.submit_webui(&input, None).await {
        Ok(task_id) => task_id,
        Err(error) => {
            let res = ExecuteResponse {
                success: false,
                task_id: None,
                status: "failed".to_string(),
                answer: None,
                provider: None,
                duration_ms: None,
                message: Some(format!("Failed to submit task: {}", error)),
            };
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(res));
        }
    };

    match completion::wait_for_completion(&state, &task_id, Duration::from_secs(30)).await {
        Ok(completion::CompletionState::Done(result)) => {
            let res = ExecuteResponse {
                success: true,
                task_id: Some(result.task_id),
                status: "completed".to_string(),
                answer: Some(result.answer),
                provider: result.provider,
                duration_ms: result.duration_ms,
                message: None,
            };
            (StatusCode::OK, Json(res))
        }
        Ok(completion::CompletionState::Failed(error)) => {
            let res = ExecuteResponse {
                success: false,
                task_id: Some(task_id),
                status: "failed".to_string(),
                answer: None,
                provider: None,
                duration_ms: None,
                message: Some(error),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(res))
        }
        Ok(completion::CompletionState::Running) => {
            let res = ExecuteResponse {
                success: true,
                task_id: Some(task_id),
                status: "running".to_string(),
                answer: None,
                provider: None,
                duration_ms: None,
                message: Some("Task accepted and still running".to_string()),
            };
            (StatusCode::ACCEPTED, Json(res))
        }
        Ok(completion::CompletionState::Missing) => {
            let res = ExecuteResponse {
                success: false,
                task_id: Some(task_id),
                status: "missing".to_string(),
                answer: None,
                provider: None,
                duration_ms: None,
                message: Some("Task disappeared before completion".to_string()),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(res))
        }
        Err(error) => {
            let res = ExecuteResponse {
                success: false,
                task_id: Some(task_id),
                status: "failed".to_string(),
                answer: None,
                provider: None,
                duration_ms: None,
                message: Some(format!("Failed to fetch task result: {}", error)),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(res))
        }
    }
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
