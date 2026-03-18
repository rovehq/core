use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Serialize;
use uuid::Uuid;

use super::AppState;
use crate::storage::PendingTaskStatus;

#[derive(Debug, Clone, Serialize)]
pub(super) struct TaskCompletion {
    pub task_id: String,
    pub answer: String,
    pub provider: Option<String>,
    pub duration_ms: Option<i64>,
}

pub(super) enum CompletionState {
    Running,
    Done(TaskCompletion),
    Failed(String),
    Missing,
}

pub(super) async fn load_completion(state: &AppState, task_id: &str) -> Result<CompletionState> {
    let pending = state.db.pending_tasks().get_task(task_id).await?;
    let Some(pending) = pending else {
        return Ok(CompletionState::Missing);
    };

    match pending.status {
        PendingTaskStatus::Pending | PendingTaskStatus::Running => Ok(CompletionState::Running),
        PendingTaskStatus::Failed => Ok(CompletionState::Failed(
            pending.error.unwrap_or_else(|| "Task failed".to_string()),
        )),
        PendingTaskStatus::Done => {
            let tasks = state.db.tasks();
            let answer = tasks
                .get_latest_answer(task_id)
                .await?
                .unwrap_or_else(|| "Task completed".to_string());

            let (provider, duration_ms) = match Uuid::parse_str(task_id) {
                Ok(task_uuid) => match tasks.get_task(&task_uuid).await? {
                    Some(task) => (task.provider_used, task.duration_ms),
                    None => (None, None),
                },
                Err(_) => (None, None),
            };

            Ok(CompletionState::Done(TaskCompletion {
                task_id: task_id.to_string(),
                answer,
                provider,
                duration_ms,
            }))
        }
    }
}

pub(super) async fn wait_for_completion(
    state: &AppState,
    task_id: &str,
    timeout: Duration,
) -> Result<CompletionState> {
    let deadline = Instant::now() + timeout;

    loop {
        let state_now = load_completion(state, task_id).await?;
        match state_now {
            CompletionState::Running if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            other => return Ok(other),
        }
    }
}
