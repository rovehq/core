use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use super::AppState;

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
    pub task: String,
    pub risk_tier: u8,
}

#[derive(Serialize)]
pub struct ExecuteResponse {
    pub success: bool,
    pub message: String,
}

pub async fn execute_task(
    State(state): State<AppState>,
    Json(payload): Json<ExecuteRequest>,
) -> impl IntoResponse {
    let mut agent = state.agent.write().await;

    match agent
        .process_task(crate::gateway::Task::build_from_websocket(
            payload.task.clone(),
            None,
        ))
        .await
    {
        Ok(result) => {
            let res = ExecuteResponse {
                success: true,
                message: format!("Task finished with status: {:?}", result), // simplified for now
            };
            (StatusCode::OK, Json(res))
        }
        Err(e) => {
            let res = ExecuteResponse {
                success: false,
                message: format!("Error: {}", e),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(res))
        }
    }
}

pub async fn active_skills(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state.agent.read().await;
    // We assume AgentCore has a way to get the steering engine or active skills. For now, we mock.
    // In a full impl, we'd pull from `agent.steering.active_skills().await`
    (
        StatusCode::OK,
        Json(vec!["careful".to_string(), "local-only".to_string()]),
    ) // Placeholder
}
