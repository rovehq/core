/// Task persistence operations
///
/// This module provides functions for persisting tasks and task steps to the database.
/// All queries use parameterized queries for SQL injection prevention.
///
/// Requirements: 12.2, 12.4, 12.5, 12.7, 12.10
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::time::{SystemTime, UNIX_EPOCH};

/// Task status enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
        }
    }
}

/// Task step type enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepType {
    UserMessage,
    AssistantMessage,
    ToolCall,
    ToolResult,
    Research,
    Execute,
    Verify,
}

impl StepType {
    pub fn as_str(&self) -> &str {
        match self {
            StepType::UserMessage => "user_message",
            StepType::AssistantMessage => "assistant_message",
            StepType::ToolCall => "tool_call",
            StepType::ToolResult => "tool_result",
            StepType::Research => "research",
            StepType::Execute => "execute",
            StepType::Verify => "verify",
        }
    }
}

/// Task record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: uuid::Uuid,
    pub input: String,
    pub status: TaskStatus,
    pub provider_used: Option<String>,
    pub duration_ms: Option<i64>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

/// Task step record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStep {
    pub id: Option<i64>,
    pub task_id: uuid::Uuid,
    pub step_order: i64,
    pub step_type: StepType,
    pub content: String,
    pub created_at: i64,
}

/// Agent event record (immutable event sourcing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    pub id: String,
    pub task_id: String,
    pub parent_task_id: Option<String>,
    pub event_type: String,
    pub payload: String,
    pub step_num: i64,
    pub domain: Option<String>,
    pub created_at: i64,
}

/// Task repository for database operations
pub struct TaskRepository {
    pool: SqlitePool,
}

impl TaskRepository {
    /// Create a new task repository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new task
    ///
    /// Requirements: 12.4, 12.10
    pub async fn create_task(&self, id: &uuid::Uuid, input: &str) -> Result<Task> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let status = TaskStatus::Pending.as_str();
        let id_str = id.to_string();

        // Use parameterized query to prevent SQL injection
        sqlx::query(
            "INSERT OR IGNORE INTO tasks (id, input, status, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&id_str)
        .bind(input)
        .bind(status)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to create task")?;

        Ok(Task {
            id: *id,
            input: input.to_string(),
            status: TaskStatus::Pending,
            provider_used: None,
            duration_ms: None,
            created_at: now,
            completed_at: None,
        })
    }

    /// Update task status
    ///
    /// Requirements: 12.4, 12.10
    pub async fn update_task_status(&self, task_id: &uuid::Uuid, status: TaskStatus) -> Result<()> {
        let status_str = status.as_str();
        let task_id_str = task_id.to_string();

        sqlx::query("UPDATE tasks SET status = ? WHERE id = ?")
            .bind(status_str)
            .bind(&task_id_str)
            .execute(&self.pool)
            .await
            .context("Failed to update task status")?;

        Ok(())
    }

    /// Complete a task with results
    ///
    /// Requirements: 12.4, 12.10
    pub async fn complete_task(
        &self,
        task_id: &uuid::Uuid,
        provider_used: &str,
        duration_ms: i64,
    ) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let status = TaskStatus::Completed.as_str();
        let task_id_str = task_id.to_string();

        sqlx::query(
            "UPDATE tasks SET status = ?, provider_used = ?, duration_ms = ?, completed_at = ? WHERE id = ?"
        )
        .bind(status)
        .bind(provider_used)
        .bind(duration_ms)
        .bind(now)
        .bind(&task_id_str)
        .execute(&self.pool)
        .await
        .context("Failed to complete task")?;

        Ok(())
    }

    /// Mark a task as failed
    ///
    /// Requirements: 12.4, 12.10
    pub async fn fail_task(&self, task_id: &uuid::Uuid) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let status = TaskStatus::Failed.as_str();
        let task_id_str = task_id.to_string();

        sqlx::query("UPDATE tasks SET status = ?, completed_at = ? WHERE id = ?")
            .bind(status)
            .bind(now)
            .bind(&task_id_str)
            .execute(&self.pool)
            .await
            .context("Failed to mark task as failed")?;

        Ok(())
    }

    /// Get a task by ID
    ///
    /// Requirements: 12.4, 12.10
    pub async fn get_task(&self, task_id: &uuid::Uuid) -> Result<Option<Task>> {
        let task_id_str = task_id.to_string();
        let row = sqlx::query(
            "SELECT id, input, status, provider_used, duration_ms, created_at, completed_at FROM tasks WHERE id = ?"
        )
        .bind(&task_id_str)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch task")?;

        Ok(row.map(|r| {
            let id_str: String = r.get("id");
            Task {
                id: uuid::Uuid::parse_str(&id_str).unwrap_or_else(|_| uuid::Uuid::nil()),
                input: r.get("input"),
                status: match r.get::<String, _>("status").as_str() {
                    "pending" => TaskStatus::Pending,
                    "running" => TaskStatus::Running,
                    "completed" => TaskStatus::Completed,
                    "failed" => TaskStatus::Failed,
                    _ => TaskStatus::Failed,
                },
                provider_used: r.get("provider_used"),
                duration_ms: r.get("duration_ms"),
                created_at: r.get("created_at"),
                completed_at: r.get("completed_at"),
            }
        }))
    }

    /// Get recent tasks (last N tasks)
    ///
    /// Requirements: 12.4, 12.10
    pub async fn get_recent_tasks(&self, limit: i64) -> Result<Vec<Task>> {
        let rows = sqlx::query(
            "SELECT id, input, status, provider_used, duration_ms, created_at, completed_at FROM tasks ORDER BY created_at DESC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch recent tasks")?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let id_str: String = r.get("id");
                Task {
                    id: uuid::Uuid::parse_str(&id_str).unwrap_or_else(|_| uuid::Uuid::nil()),
                    input: r.get("input"),
                    status: match r.get::<String, _>("status").as_str() {
                        "pending" => TaskStatus::Pending,
                        "running" => TaskStatus::Running,
                        "completed" => TaskStatus::Completed,
                        "failed" => TaskStatus::Failed,
                        _ => TaskStatus::Failed,
                    },
                    provider_used: r.get("provider_used"),
                    duration_ms: r.get("duration_ms"),
                    created_at: r.get("created_at"),
                    completed_at: r.get("completed_at"),
                }
            })
            .collect())
    }

    /// Add a step to a task
    ///
    /// Requirements: 12.5, 12.10
    pub async fn add_task_step(
        &self,
        task_id: &uuid::Uuid,
        step_order: i64,
        step_type: StepType,
        content: &str,
    ) -> Result<TaskStep> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let step_type_str = step_type.as_str();
        let task_id_str = task_id.to_string();

        let result = sqlx::query(
            "INSERT INTO task_steps (task_id, step_order, step_type, content, created_at) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&task_id_str)
        .bind(step_order)
        .bind(step_type_str)
        .bind(content)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to add task step")?;

        Ok(TaskStep {
            id: Some(result.last_insert_rowid()),
            task_id: *task_id,
            step_order,
            step_type,
            content: content.to_string(),
            created_at: now,
        })
    }

    /// Insert an audit log record for an agent action
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_agent_action(
        &self,
        task_id: &str,
        action_type: &str,
        tool_name: &str,
        args_hash: &str,
        risk_tier: i32,
        approved_by: &str,
        result_summary: &str,
    ) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        sqlx::query(
            "INSERT INTO agent_actions (id, task_id, action_type, tool_name, args_hash, risk_tier, approved_by, result_summary, timestamp) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(id)
        .bind(task_id)
        .bind(action_type)
        .bind(tool_name)
        .bind(args_hash)
        .bind(risk_tier)
        .bind(approved_by)
        .bind(result_summary)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to insert agent action audit log")?;

        Ok(())
    }

    /// Insert an agent event (immutable append-only event sourcing)
    ///
    /// Event types:
    /// - "thought": LLM reasoning step
    /// - "tool_call": Tool invocation with args
    /// - "observation": Tool execution result
    /// - "answer": Final answer to user
    /// - "error": Error during execution
    ///
    /// Requirements: Phase 1 - Event Sourcing
    pub async fn insert_agent_event(
        &self,
        task_id: &uuid::Uuid,
        event_type: &str,
        payload: &str,
        step_num: i64,
        domain: Option<&str>,
    ) -> Result<String> {
        self.insert_agent_event_with_parent(task_id, None, event_type, payload, step_num, domain)
            .await
    }

    pub async fn insert_agent_event_with_parent(
        &self,
        task_id: &uuid::Uuid,
        parent_task_id: Option<&uuid::Uuid>,
        event_type: &str,
        payload: &str,
        step_num: i64,
        domain: Option<&str>,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let task_id_str = task_id.to_string();
        let parent_task_id_str = parent_task_id.map(uuid::Uuid::to_string);

        sqlx::query(
            "INSERT INTO agent_events (id, task_id, parent_task_id, event_type, payload, step_num, domain, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&id)
        .bind(&task_id_str)
        .bind(parent_task_id_str)
        .bind(event_type)
        .bind(payload)
        .bind(step_num)
        .bind(domain)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to insert agent event")?;

        Ok(id)
    }

    /// Get all events for a task (ordered by step_num for replay)
    ///
    /// Requirements: Phase 1 - Time-travel debugging
    pub async fn get_agent_events(&self, task_id: &str) -> Result<Vec<AgentEvent>> {
        let rows = sqlx::query(
            "SELECT id, task_id, parent_task_id, event_type, payload, step_num, domain, created_at FROM agent_events WHERE task_id = ? ORDER BY step_num ASC"
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch agent events")?;

        Ok(rows
            .into_iter()
            .map(|r| AgentEvent {
                id: r.get("id"),
                task_id: r.get("task_id"),
                parent_task_id: r.get("parent_task_id"),
                event_type: r.get("event_type"),
                payload: r.get("payload"),
                step_num: r.get("step_num"),
                domain: r.get("domain"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    pub async fn get_agent_events_by_parent(
        &self,
        parent_task_id: &str,
    ) -> Result<Vec<AgentEvent>> {
        let rows = sqlx::query(
            "SELECT id, task_id, parent_task_id, event_type, payload, step_num, domain, created_at FROM agent_events WHERE parent_task_id = ? ORDER BY created_at ASC, step_num ASC"
        )
        .bind(parent_task_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch child agent events")?;

        Ok(rows
            .into_iter()
            .map(|r| AgentEvent {
                id: r.get("id"),
                task_id: r.get("task_id"),
                parent_task_id: r.get("parent_task_id"),
                event_type: r.get("event_type"),
                payload: r.get("payload"),
                step_num: r.get("step_num"),
                domain: r.get("domain"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    /// Get the latest final answer recorded for a task.
    pub async fn get_latest_answer(&self, task_id: &str) -> Result<Option<String>> {
        let payload: Option<String> = sqlx::query_scalar(
            "SELECT payload FROM agent_events WHERE task_id = ? AND event_type = 'answer' ORDER BY step_num DESC, created_at DESC LIMIT 1"
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch latest task answer")?;

        Ok(payload.and_then(|payload| parse_answer_payload(&payload)))
    }

    /// Get all steps for a task
    ///
    /// Requirements: 12.5, 12.10
    pub async fn get_task_steps(&self, task_id: &uuid::Uuid) -> Result<Vec<TaskStep>> {
        let task_id_str = task_id.to_string();
        let rows = sqlx::query(
            "SELECT id, task_id, step_order, step_type, content, created_at FROM task_steps WHERE task_id = ? ORDER BY step_order ASC"
        )
        .bind(&task_id_str)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch task steps")?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let task_id_str: String = r.get("task_id");
                TaskStep {
                    id: Some(r.get("id")),
                    task_id: uuid::Uuid::parse_str(&task_id_str)
                        .unwrap_or_else(|_| uuid::Uuid::nil()),
                    step_order: r.get("step_order"),
                    step_type: match r.get::<String, _>("step_type").as_str() {
                        "user_message" => StepType::UserMessage,
                        "assistant_message" => StepType::AssistantMessage,
                        "tool_call" => StepType::ToolCall,
                        "tool_result" => StepType::ToolResult,
                        "research" => StepType::Research,
                        "execute" => StepType::Execute,
                        "verify" => StepType::Verify,
                        _ => StepType::UserMessage,
                    },
                    content: r.get("content"),
                    created_at: r.get("created_at"),
                }
            })
            .collect())
    }

    /// Delete old tasks (cleanup)
    ///
    /// Requirements: 12.4, 12.10
    pub async fn delete_old_tasks(&self, older_than_days: i64) -> Result<u64> {
        let cutoff = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64
            - (older_than_days * 24 * 60 * 60);

        let result = sqlx::query("DELETE FROM tasks WHERE created_at < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .context("Failed to delete old tasks")?;

        Ok(result.rows_affected())
    }
}

fn parse_answer_payload(payload: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(payload).ok()?;
    json.get("answer")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}
