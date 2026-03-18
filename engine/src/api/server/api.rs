use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::{completion, AppState};

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
