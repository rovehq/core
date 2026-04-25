//! Persistent subagent thread tracking.
//!
//! An `AgentThread` represents a stable conversation channel between a parent
//! agent and a named callable child agent.  Threads outlive individual tasks —
//! follow-up work dispatched to the same callable agent id reuses the same
//! thread, giving operators a single inspectable history per child relationship.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    Active,
    Idle,
    Completed,
}

impl ThreadStatus {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Completed => "completed",
        }
    }
}

impl std::str::FromStr for ThreadStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "active" => Ok(Self::Active),
            "idle" => Ok(Self::Idle),
            "completed" => Ok(Self::Completed),
            other => anyhow::bail!("unknown thread status: {other}"),
        }
    }
}

/// A persistent thread between a parent agent and a callable child agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentThread {
    /// Stable thread id (prefixed "thr_").
    pub id: String,
    /// Id of the agent that declared the callable_agent roster.
    pub parent_agent_id: String,
    /// Id matching `CallableAgentSpec.id` inside the parent spec.
    pub callable_agent_id: String,
    /// Human-readable name of the callable agent.
    pub callable_agent_name: String,
    pub status: ThreadStatus,
    pub task_count: i64,
    pub created_at: i64,
    pub last_active_at: i64,
}

/// A lifecycle event recorded on an `AgentThread`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentThreadEvent {
    pub id: String,
    pub thread_id: String,
    /// One of: "created", "dispatched", "completed", "idle", "error"
    pub event_type: String,
    pub task_id: Option<String>,
    pub payload: String,
    pub created_at: i64,
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ThreadRepository {
    pool: SqlitePool,
}

impl ThreadRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Find an existing thread or create a new one for this
    /// (parent_agent_id, callable_agent_id) pair.
    pub async fn get_or_create(
        &self,
        parent_agent_id: &str,
        callable_agent_id: &str,
        callable_agent_name: &str,
    ) -> Result<AgentThread> {
        let now = chrono::Utc::now().timestamp();

        // Try to find an existing active/idle thread for this pair.
        let existing: Option<AgentThread> = sqlx::query(
            "SELECT id, parent_agent_id, callable_agent_id, callable_agent_name,
                    status, task_count, created_at, last_active_at
             FROM agent_threads
             WHERE parent_agent_id = ? AND callable_agent_id = ?
               AND status != 'completed'
             ORDER BY last_active_at DESC
             LIMIT 1",
        )
        .bind(parent_agent_id)
        .bind(callable_agent_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to query agent_threads")?
        .map(|row| AgentThread {
            id: row.get("id"),
            parent_agent_id: row.get("parent_agent_id"),
            callable_agent_id: row.get("callable_agent_id"),
            callable_agent_name: row.get("callable_agent_name"),
            status: row
                .get::<String, _>("status")
                .parse()
                .unwrap_or(ThreadStatus::Idle),
            task_count: row.get("task_count"),
            created_at: row.get("created_at"),
            last_active_at: row.get("last_active_at"),
        });

        if let Some(thread) = existing {
            return Ok(thread);
        }

        // Create a new thread.
        let id = format!("thr_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO agent_threads
             (id, parent_agent_id, callable_agent_id, callable_agent_name,
              status, task_count, created_at, last_active_at)
             VALUES (?, ?, ?, ?, 'active', 0, ?, ?)",
        )
        .bind(&id)
        .bind(parent_agent_id)
        .bind(callable_agent_id)
        .bind(callable_agent_name)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to insert agent_thread")?;

        self.record_event(&id, "created", None, "Thread created")
            .await?;

        Ok(AgentThread {
            id,
            parent_agent_id: parent_agent_id.to_string(),
            callable_agent_id: callable_agent_id.to_string(),
            callable_agent_name: callable_agent_name.to_string(),
            status: ThreadStatus::Active,
            task_count: 0,
            created_at: now,
            last_active_at: now,
        })
    }

    /// Record a task dispatch on the thread (increments task_count, marks active).
    pub async fn record_dispatch(&self, thread_id: &str, task_id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE agent_threads
             SET status = 'active', task_count = task_count + 1, last_active_at = ?
             WHERE id = ?",
        )
        .bind(now)
        .bind(thread_id)
        .execute(&self.pool)
        .await
        .context("Failed to update thread on dispatch")?;

        self.record_event(thread_id, "dispatched", Some(task_id), "Task dispatched")
            .await
    }

    /// Mark the thread idle after a task completes.
    pub async fn record_completion(&self, thread_id: &str, task_id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE agent_threads
             SET status = 'idle', last_active_at = ?
             WHERE id = ?",
        )
        .bind(now)
        .bind(thread_id)
        .execute(&self.pool)
        .await
        .context("Failed to mark thread idle")?;

        self.record_event(thread_id, "completed", Some(task_id), "Task completed")
            .await
    }

    /// Record an error event on the thread (thread stays idle).
    pub async fn record_error(&self, thread_id: &str, task_id: &str, detail: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE agent_threads SET status = 'idle', last_active_at = ? WHERE id = ?")
            .bind(now)
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .context("Failed to update thread on error")?;

        self.record_event(thread_id, "error", Some(task_id), detail)
            .await
    }

    /// Get a single thread by id.
    pub async fn get(&self, thread_id: &str) -> Result<Option<AgentThread>> {
        sqlx::query(
            "SELECT id, parent_agent_id, callable_agent_id, callable_agent_name,
                    status, task_count, created_at, last_active_at
             FROM agent_threads WHERE id = ?",
        )
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch thread")
        .map(|opt| {
            opt.map(|row| AgentThread {
                id: row.get("id"),
                parent_agent_id: row.get("parent_agent_id"),
                callable_agent_id: row.get("callable_agent_id"),
                callable_agent_name: row.get("callable_agent_name"),
                status: row
                    .get::<String, _>("status")
                    .parse()
                    .unwrap_or(ThreadStatus::Idle),
                task_count: row.get("task_count"),
                created_at: row.get("created_at"),
                last_active_at: row.get("last_active_at"),
            })
        })
    }

    /// List all threads for a parent agent, newest first.
    pub async fn list_for_agent(
        &self,
        parent_agent_id: &str,
        limit: i64,
    ) -> Result<Vec<AgentThread>> {
        sqlx::query(
            "SELECT id, parent_agent_id, callable_agent_id, callable_agent_name,
                    status, task_count, created_at, last_active_at
             FROM agent_threads
             WHERE parent_agent_id = ?
             ORDER BY last_active_at DESC
             LIMIT ?",
        )
        .bind(parent_agent_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list threads for agent")
        .map(|rows| {
            rows.into_iter()
                .map(|row| AgentThread {
                    id: row.get("id"),
                    parent_agent_id: row.get("parent_agent_id"),
                    callable_agent_id: row.get("callable_agent_id"),
                    callable_agent_name: row.get("callable_agent_name"),
                    status: row
                        .get::<String, _>("status")
                        .parse()
                        .unwrap_or(ThreadStatus::Idle),
                    task_count: row.get("task_count"),
                    created_at: row.get("created_at"),
                    last_active_at: row.get("last_active_at"),
                })
                .collect()
        })
    }

    /// List all threads across all agents, newest first.
    pub async fn list_all(&self, limit: i64) -> Result<Vec<AgentThread>> {
        sqlx::query(
            "SELECT id, parent_agent_id, callable_agent_id, callable_agent_name,
                    status, task_count, created_at, last_active_at
             FROM agent_threads
             ORDER BY last_active_at DESC
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list all threads")
        .map(|rows| {
            rows.into_iter()
                .map(|row| AgentThread {
                    id: row.get("id"),
                    parent_agent_id: row.get("parent_agent_id"),
                    callable_agent_id: row.get("callable_agent_id"),
                    callable_agent_name: row.get("callable_agent_name"),
                    status: row
                        .get::<String, _>("status")
                        .parse()
                        .unwrap_or(ThreadStatus::Idle),
                    task_count: row.get("task_count"),
                    created_at: row.get("created_at"),
                    last_active_at: row.get("last_active_at"),
                })
                .collect()
        })
    }

    /// List events for a thread, oldest first.
    pub async fn events(&self, thread_id: &str) -> Result<Vec<AgentThreadEvent>> {
        sqlx::query(
            "SELECT id, thread_id, event_type, task_id, payload, created_at
             FROM agent_thread_events
             WHERE thread_id = ?
             ORDER BY created_at ASC",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch thread events")
        .map(|rows| {
            rows.into_iter()
                .map(|row| AgentThreadEvent {
                    id: row.get("id"),
                    thread_id: row.get("thread_id"),
                    event_type: row.get("event_type"),
                    task_id: row.get("task_id"),
                    payload: row.get("payload"),
                    created_at: row.get("created_at"),
                })
                .collect()
        })
    }

    async fn record_event(
        &self,
        thread_id: &str,
        event_type: &str,
        task_id: Option<&str>,
        payload: &str,
    ) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO agent_thread_events
             (id, thread_id, event_type, task_id, payload, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(thread_id)
        .bind(event_type)
        .bind(task_id)
        .bind(payload)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to insert thread event")?;
        Ok(())
    }
}
