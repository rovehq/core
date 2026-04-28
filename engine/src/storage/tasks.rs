/// Task persistence operations
///
/// This module provides functions for persisting tasks and task steps to the database.
/// All queries use parameterized queries for SQL injection prevention.
///
/// Requirements: 12.2, 12.4, 12.5, 12.7, 12.10
use anyhow::{Context, Result};
use sdk::{TaskExecutionProfile, TaskSource};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::time::{SystemTime, UNIX_EPOCH};

/// Task status enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

    pub fn parse_str(value: &str) -> Self {
        match value {
            "pending" => TaskStatus::Pending,
            "running" => TaskStatus::Running,
            "completed" => TaskStatus::Completed,
            "failed" => TaskStatus::Failed,
            _ => TaskStatus::Failed,
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
    pub source: String,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub thread_id: Option<String>,
    pub worker_preset_id: Option<String>,
    pub worker_preset_name: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentActionRecord {
    pub id: String,
    pub task_id: String,
    pub action_type: String,
    pub tool_name: String,
    pub args_hash: String,
    pub risk_tier: i32,
    pub severity: String,
    pub approved_by: String,
    pub result_summary: String,
    pub source: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApexHistorySummary {
    pub task_id: String,
    pub status: TaskStatus,
    pub duration_ms: Option<i64>,
    pub dag_waves: i64,
    pub dag_step_successes: i64,
    pub dag_step_failures: i64,
    pub last_event_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadHistoryEntry {
    pub task_id: String,
    pub input: String,
    pub answer: Option<String>,
    pub status: TaskStatus,
    pub created_at: i64,
}

/// Task repository for database operations
pub struct TaskRepository {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskOutcomeStats {
    pub recent_failures: u64,
    pub recent_successes: u64,
    pub recent_avg_duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskListQuery {
    pub status: Option<TaskStatus>,
    pub agent_id: Option<String>,
    pub thread_id: Option<String>,
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentActionQuery {
    pub action_type: Option<String>,
    pub source: Option<String>,
    pub severity: Option<String>,
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub limit: i64,
    pub offset: i64,
}

impl TaskRepository {
    /// Create a new task repository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Create a new task
    ///
    /// Requirements: 12.4, 12.10
    pub async fn create_task(&self, id: &uuid::Uuid, input: &str) -> Result<Task> {
        self.create_task_with_metadata(id, input, None, None).await
    }

    pub async fn create_task_with_metadata(
        &self,
        id: &uuid::Uuid,
        input: &str,
        source: Option<&TaskSource>,
        execution_profile: Option<&TaskExecutionProfile>,
    ) -> Result<Task> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let status = TaskStatus::Pending.as_str();
        let id_str = id.to_string();
        let source = source
            .map(TaskSource::as_str)
            .unwrap_or_else(|| "cli".to_string());
        let (agent_id, agent_name, thread_id, worker_preset_id, worker_preset_name) =
            execution_profile.map_or((None, None, None, None, None), |profile| {
                (
                    profile.agent_id.clone(),
                    profile.agent_name.clone(),
                    profile.thread_id.clone(),
                    profile.worker_preset_id.clone(),
                    profile.worker_preset_name.clone(),
                )
            });

        // Use parameterized query to prevent SQL injection
        sqlx::query(
            "INSERT OR IGNORE INTO tasks (id, input, source, agent_id, agent_name, thread_id, worker_preset_id, worker_preset_name, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id_str)
        .bind(input)
        .bind(&source)
        .bind(&agent_id)
        .bind(&agent_name)
        .bind(&thread_id)
        .bind(&worker_preset_id)
        .bind(&worker_preset_name)
        .bind(status)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to create task")?;

        Ok(Task {
            id: *id,
            input: input.to_string(),
            source,
            agent_id,
            agent_name,
            thread_id,
            worker_preset_id,
            worker_preset_name,
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
            "SELECT id, input, source, agent_id, agent_name, thread_id, worker_preset_id, worker_preset_name, status, provider_used, duration_ms, created_at, completed_at FROM tasks WHERE id = ?"
        )
        .bind(&task_id_str)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch task")?;

        Ok(row.map(map_task_row))
    }

    /// Get recent tasks (last N tasks)
    ///
    /// Requirements: 12.4, 12.10
    pub async fn get_recent_tasks(&self, limit: i64) -> Result<Vec<Task>> {
        self.list_tasks(&TaskListQuery {
            limit,
            ..TaskListQuery::default()
        })
        .await
    }

    pub async fn get_recent_thread_history(
        &self,
        thread_id: &str,
        exclude_task_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ThreadHistoryEntry>> {
        let rows = sqlx::query(
            r#"SELECT t.id AS task_id, t.input, t.status, t.created_at,
                      (
                        SELECT payload
                        FROM agent_events ev
                        WHERE ev.task_id = t.id
                          AND ev.event_type = 'answer'
                        ORDER BY ev.step_num DESC
                        LIMIT 1
                      ) AS answer_payload
               FROM tasks t
               WHERE t.thread_id = ?1
                 AND (?2 IS NULL OR t.id != ?2)
               ORDER BY t.created_at DESC
               LIMIT ?3"#,
        )
        .bind(thread_id)
        .bind(exclude_task_id)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
        .context("Failed to load recent thread history")?;

        let mut entries = rows
            .into_iter()
            .map(|row| ThreadHistoryEntry {
                task_id: row.get("task_id"),
                input: row.get("input"),
                answer: row
                    .get::<Option<String>, _>("answer_payload")
                    .and_then(|payload| parse_answer_payload(&payload)),
                status: TaskStatus::parse_str(row.get::<String, _>("status").as_str()),
                created_at: row.get("created_at"),
            })
            .collect::<Vec<_>>();
        entries.reverse();
        Ok(entries)
    }

    pub async fn list_tasks(&self, query: &TaskListQuery) -> Result<Vec<Task>> {
        let rows = sqlx::query(
            r#"SELECT id, input, source, agent_id, agent_name, thread_id, worker_preset_id, worker_preset_name,
                      status, provider_used, duration_ms, created_at, completed_at
               FROM tasks
               WHERE (?1 IS NULL OR status = ?1)
                 AND (?2 IS NULL OR agent_id = ?2)
                 AND (?3 IS NULL OR thread_id = ?3)
                 AND (?4 IS NULL OR created_at >= ?4)
                 AND (?5 IS NULL OR created_at <= ?5)
               ORDER BY created_at DESC
               LIMIT ?6 OFFSET ?7"#,
        )
        .bind(query.status.as_ref().map(TaskStatus::as_str))
        .bind(query.agent_id.as_deref())
        .bind(query.thread_id.as_deref())
        .bind(query.date_from)
        .bind(query.date_to)
        .bind(query.limit.max(1))
        .bind(query.offset.max(0))
        .fetch_all(&self.pool)
        .await
        .context("Failed to list tasks")?;

        Ok(rows.into_iter().map(map_task_row).collect())
    }

    pub async fn list_task_agents(&self) -> Result<Vec<(String, Option<String>)>> {
        let rows = sqlx::query(
            r#"SELECT DISTINCT agent_id, agent_name
               FROM tasks
               WHERE agent_id IS NOT NULL AND TRIM(agent_id) != ''
               ORDER BY agent_name ASC, agent_id ASC"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list task agents")?;

        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("agent_id"),
                    row.get::<Option<String>, _>("agent_name"),
                )
            })
            .collect())
    }

    pub async fn recent_outcome_stats(&self, limit: i64) -> Result<TaskOutcomeStats> {
        let rows = sqlx::query(
            "SELECT status, duration_ms FROM tasks WHERE status IN ('completed', 'failed') ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch recent task outcome stats")?;

        let mut recent_failures = 0_u64;
        let mut recent_successes = 0_u64;
        let mut duration_total = 0_i64;
        let mut duration_count = 0_i64;

        for row in rows {
            match row.get::<String, _>("status").as_str() {
                "failed" => recent_failures += 1,
                "completed" => {
                    recent_successes += 1;
                    if let Some(duration_ms) = row.get::<Option<i64>, _>("duration_ms") {
                        duration_total += duration_ms.max(0);
                        duration_count += 1;
                    }
                }
                _ => {}
            }
        }

        Ok(TaskOutcomeStats {
            recent_failures,
            recent_successes,
            recent_avg_duration_ms: (duration_count > 0).then_some(duration_total / duration_count),
        })
    }

    pub async fn list_agent_actions(
        &self,
        query: &AgentActionQuery,
    ) -> Result<Vec<AgentActionRecord>> {
        let limit = if query.limit <= 0 { 100 } else { query.limit };
        let offset = query.offset.max(0);
        let rows = sqlx::query(
            r#"SELECT aa.id,
                      aa.task_id,
                      aa.action_type,
                      aa.tool_name,
                      aa.args_hash,
                      aa.risk_tier,
                      aa.approved_by,
                      aa.result_summary,
                      aa.timestamp,
                      t.source
               FROM agent_actions aa
               LEFT JOIN tasks t ON t.id = aa.task_id
               WHERE (?1 IS NULL OR aa.action_type = ?1)
                 AND (?2 IS NULL OR t.source = ?2)
                 AND (
                        ?3 IS NULL
                        OR (?3 = 'low' AND aa.risk_tier <= 0)
                        OR (?3 = 'medium' AND aa.risk_tier = 1)
                        OR (?3 = 'high' AND aa.risk_tier >= 2)
                     )
                 AND (?4 IS NULL OR aa.timestamp >= ?4)
                 AND (?5 IS NULL OR aa.timestamp <= ?5)
               ORDER BY aa.timestamp DESC
               LIMIT ?6 OFFSET ?7"#,
        )
        .bind(query.action_type.as_deref())
        .bind(query.source.as_deref())
        .bind(query.severity.as_deref())
        .bind(query.date_from)
        .bind(query.date_to)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list agent action audit log")?;

        Ok(rows
            .into_iter()
            .map(|row| AgentActionRecord {
                id: row.get("id"),
                task_id: row.get("task_id"),
                action_type: row.get("action_type"),
                tool_name: row.get("tool_name"),
                args_hash: row.get("args_hash"),
                risk_tier: row.get("risk_tier"),
                severity: severity_label(row.get("risk_tier")),
                approved_by: row.get("approved_by"),
                result_summary: row.get("result_summary"),
                source: row.get("source"),
                timestamp: row.get("timestamp"),
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

    pub async fn get_recent_apex_history(
        &self,
        domain: &str,
        limit: i64,
    ) -> Result<Vec<ApexHistorySummary>> {
        let rows = sqlx::query(
            r#"SELECT
                   ae.task_id AS task_id,
                   t.status AS status,
                   t.duration_ms AS duration_ms,
                   SUM(CASE WHEN ae.event_type = 'dag_wave_started' THEN 1 ELSE 0 END) AS dag_waves,
                   SUM(CASE WHEN ae.event_type = 'dag_step_succeeded' THEN 1 ELSE 0 END) AS dag_step_successes,
                   SUM(CASE WHEN ae.event_type = 'dag_step_failed' THEN 1 ELSE 0 END) AS dag_step_failures,
                   MAX(ae.created_at) AS last_event_at
               FROM agent_events ae
               JOIN tasks t ON t.id = ae.task_id
               WHERE ae.domain = ?
               GROUP BY ae.task_id, t.status, t.duration_ms
               ORDER BY last_event_at DESC
               LIMIT ?"#,
        )
        .bind(domain)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch recent DAG history")?;

        Ok(rows
            .into_iter()
            .map(|r| ApexHistorySummary {
                task_id: r.get("task_id"),
                status: match r.get::<String, _>("status").as_str() {
                    "pending" => TaskStatus::Pending,
                    "running" => TaskStatus::Running,
                    "completed" => TaskStatus::Completed,
                    "failed" => TaskStatus::Failed,
                    _ => TaskStatus::Failed,
                },
                duration_ms: r.get("duration_ms"),
                dag_waves: r.get("dag_waves"),
                dag_step_successes: r.get("dag_step_successes"),
                dag_step_failures: r.get("dag_step_failures"),
                last_event_at: r.get("last_event_at"),
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

fn map_task_row(r: sqlx::sqlite::SqliteRow) -> Task {
    let id_str: String = r.get("id");
    Task {
        id: uuid::Uuid::parse_str(&id_str).unwrap_or_else(|_| uuid::Uuid::nil()),
        input: r.get("input"),
        source: r.get("source"),
        agent_id: r.get("agent_id"),
        agent_name: r.get("agent_name"),
        thread_id: r.get("thread_id"),
        worker_preset_id: r.get("worker_preset_id"),
        worker_preset_name: r.get("worker_preset_name"),
        status: TaskStatus::parse_str(r.get::<String, _>("status").as_str()),
        provider_used: r.get("provider_used"),
        duration_ms: r.get("duration_ms"),
        created_at: r.get("created_at"),
        completed_at: r.get("completed_at"),
    }
}

fn parse_answer_payload(payload: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(payload).ok()?;
    json.get("answer")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

fn severity_label(risk_tier: i32) -> String {
    if risk_tier >= 2 {
        "high".to_string()
    } else if risk_tier == 1 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use tempfile::TempDir;

    fn test_profile(agent_id: &str, agent_name: &str) -> TaskExecutionProfile {
        TaskExecutionProfile {
            agent_id: Some(agent_id.to_string()),
            agent_name: Some(agent_name.to_string()),
            thread_id: None,
            worker_preset_id: Some("worker.research".to_string()),
            worker_preset_name: Some("Research Worker".to_string()),
            purpose: Some("test".to_string()),
            instructions: "Be precise".to_string(),
            allowed_tools: vec!["read_file".to_string()],
            output_contract: None,
            outcome_contract: None,
            max_iterations: Some(4),
            callable_agents: vec![],
        }
    }

    #[tokio::test]
    async fn create_task_with_metadata_persists_agent_fields() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("tasks.db"))
            .await
            .unwrap();
        let repo = db.tasks();
        let task_id = uuid::Uuid::new_v4();
        let profile = test_profile("agent.release", "Release Agent");

        let created = repo
            .create_task_with_metadata(
                &task_id,
                "ship release",
                Some(&TaskSource::WebUI),
                Some(&profile),
            )
            .await
            .unwrap();

        assert_eq!(created.source, "webui");
        assert_eq!(created.agent_id.as_deref(), Some("agent.release"));
        assert_eq!(created.agent_name.as_deref(), Some("Release Agent"));
        assert_eq!(created.worker_preset_id.as_deref(), Some("worker.research"));
        assert_eq!(
            created.worker_preset_name.as_deref(),
            Some("Research Worker")
        );

        let persisted = repo.get_task(&task_id).await.unwrap().unwrap();
        assert_eq!(persisted.source, "webui");
        assert_eq!(persisted.agent_id, created.agent_id);
        assert_eq!(persisted.agent_name, created.agent_name);
        assert_eq!(persisted.worker_preset_id, created.worker_preset_id);
        assert_eq!(persisted.worker_preset_name, created.worker_preset_name);
    }

    #[tokio::test]
    async fn list_agent_actions_filters_by_source_and_severity() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("audit.db"))
            .await
            .unwrap();
        let repo = db.tasks();
        let task_id = uuid::Uuid::new_v4();
        repo.create_task_with_metadata(&task_id, "audit me", Some(&TaskSource::WebUI), None)
            .await
            .unwrap();
        repo.insert_agent_action(
            &task_id.to_string(),
            "tool_execution",
            "write_file",
            "hash",
            2,
            "registry",
            "wrote file",
        )
        .await
        .unwrap();

        let records = repo
            .list_agent_actions(&AgentActionQuery {
                source: Some("webui".to_string()),
                severity: Some("high".to_string()),
                limit: 20,
                ..AgentActionQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool_name, "write_file");
        assert_eq!(records[0].severity, "high");
        assert_eq!(records[0].source.as_deref(), Some("webui"));
    }

    #[tokio::test]
    async fn list_tasks_supports_status_and_agent_filters() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("task-list.db"))
            .await
            .unwrap();
        let repo = db.tasks();

        let alpha_id = uuid::Uuid::new_v4();
        repo.create_task_with_metadata(
            &alpha_id,
            "investigate incident",
            Some(&TaskSource::Cli),
            Some(&test_profile("agent.alpha", "Alpha")),
        )
        .await
        .unwrap();
        repo.update_task_status(&alpha_id, TaskStatus::Running)
            .await
            .unwrap();

        let beta_id = uuid::Uuid::new_v4();
        repo.create_task_with_metadata(
            &beta_id,
            "fix regression",
            Some(&TaskSource::Remote("node-b".to_string())),
            Some(&test_profile("agent.beta", "Beta")),
        )
        .await
        .unwrap();
        repo.complete_task(&beta_id, "localbrain", 2400)
            .await
            .unwrap();

        let running_for_alpha = repo
            .list_tasks(&TaskListQuery {
                status: Some(TaskStatus::Running),
                agent_id: Some("agent.alpha".to_string()),
                limit: 20,
                offset: 0,
                ..TaskListQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(running_for_alpha.len(), 1);
        assert_eq!(running_for_alpha[0].id, alpha_id);
        assert_eq!(running_for_alpha[0].status, TaskStatus::Running);
        assert_eq!(
            running_for_alpha[0].agent_id.as_deref(),
            Some("agent.alpha")
        );

        let completed_for_beta = repo
            .list_tasks(&TaskListQuery {
                status: Some(TaskStatus::Completed),
                agent_id: Some("agent.beta".to_string()),
                limit: 20,
                offset: 0,
                ..TaskListQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(completed_for_beta.len(), 1);
        assert_eq!(completed_for_beta[0].id, beta_id);
        assert_eq!(completed_for_beta[0].source, "remote:node-b");
        assert_eq!(
            completed_for_beta[0].provider_used.as_deref(),
            Some("localbrain")
        );
    }

    #[tokio::test]
    async fn list_tasks_supports_date_windows() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("task-dates.db"))
            .await
            .unwrap();
        let repo = db.tasks();

        let old_id = uuid::Uuid::new_v4();
        repo.create_task(&old_id, "older task").await.unwrap();
        sqlx::query("UPDATE tasks SET created_at = ? WHERE id = ?")
            .bind(100_i64)
            .bind(old_id.to_string())
            .execute(db.pool())
            .await
            .unwrap();

        let new_id = uuid::Uuid::new_v4();
        repo.create_task(&new_id, "newer task").await.unwrap();
        sqlx::query("UPDATE tasks SET created_at = ? WHERE id = ?")
            .bind(200_i64)
            .bind(new_id.to_string())
            .execute(db.pool())
            .await
            .unwrap();

        let filtered = repo
            .list_tasks(&TaskListQuery {
                date_from: Some(200),
                date_to: Some(200),
                limit: 20,
                offset: 0,
                ..TaskListQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, new_id);
        assert_eq!(filtered[0].created_at, 200);
    }

    #[tokio::test]
    async fn list_tasks_supports_thread_filter() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("task-thread-filter.db"))
            .await
            .unwrap();
        let repo = db.tasks();

        let alpha_id = uuid::Uuid::new_v4();
        repo.create_task_with_metadata(
            &alpha_id,
            "research release",
            Some(&TaskSource::Cli),
            Some(&test_profile("agent.alpha", "Alpha")),
        )
        .await
        .unwrap();

        let beta_profile = TaskExecutionProfile {
            thread_id: Some("thread:beta".to_string()),
            ..test_profile("agent.beta", "Beta")
        };
        let beta_id = uuid::Uuid::new_v4();
        repo.create_task_with_metadata(
            &beta_id,
            "ship release",
            Some(&TaskSource::Cli),
            Some(&beta_profile),
        )
        .await
        .unwrap();

        let filtered = repo
            .list_tasks(&TaskListQuery {
                thread_id: Some("thread:beta".to_string()),
                limit: 20,
                offset: 0,
                ..TaskListQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, beta_id);
        assert_eq!(filtered[0].thread_id.as_deref(), Some("thread:beta"));
    }
}
