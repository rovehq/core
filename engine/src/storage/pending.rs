/// Pending tasks repository for durable inbox pattern
///
/// This module provides functions for persisting tasks to the pending_tasks table
/// before execution starts. This ensures tasks survive engine crashes.
///
/// The durable inbox pattern:
/// 1. Task INSERTED into pending_tasks (status='pending') BEFORE spawn
/// 2. Gateway polls pending_tasks WHERE status='pending'
/// 3. Mark status='running' atomically BEFORE tokio::spawn
/// 4. On completion: status='done' or status='failed'
/// 5. On startup: recover crashed tasks (running → pending)
///
/// Requirements: Phase 3 — Gateway + Durable Inbox
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::time::{SystemTime, UNIX_EPOCH};
use sdk::TaskSource;

/// Task status for durable inbox
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PendingTaskStatus {
    Pending,
    Running,
    Done,
    Failed,
}

impl PendingTaskStatus {
    pub fn as_str(&self) -> &str {
        match self {
            PendingTaskStatus::Pending => "pending",
            PendingTaskStatus::Running => "running",
            PendingTaskStatus::Done => "done",
            PendingTaskStatus::Failed => "failed",
        }
    }

    pub fn parse_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "pending" => PendingTaskStatus::Pending,
            "running" => PendingTaskStatus::Running,
            "done" | "completed" => PendingTaskStatus::Done, // Support both for backwards compat
            "failed" => PendingTaskStatus::Failed,
            _ => PendingTaskStatus::Failed,
        }
    }
}

/// Pending task record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTask {
    pub id: String,
    pub input: String,
    pub source: TaskSource,
    pub status: PendingTaskStatus,
    pub domain: String,
    pub complexity: String,
    pub sensitive: bool,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub done_at: Option<i64>,
    pub error: Option<String>,
    pub session_id: Option<String>,
    pub workspace: Option<String>,
    pub team_id: Option<String>,
}

/// Pending task repository for database operations
pub struct PendingTaskRepository {
    pool: SqlitePool,
}

impl PendingTaskRepository {
    /// Create a new pending task repository
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Create a new pending task (durable inbox entry point)
    ///
    /// This INSERTs the task BEFORE execution starts, ensuring it survives crashes.
    pub async fn create_task(
        &self,
        id: &str,
        input: &str,
        source: TaskSource,
        session_id: Option<&str>,
        workspace: Option<&str>,
        team_id: Option<&str>,
    ) -> Result<PendingTask> {
        self.create_task_with_dispatch(
            id,
            input,
            source,
            "general",
            "simple",
            false,
            session_id,
            workspace,
            team_id,
        )
        .await
    }

    /// Create a new pending task with dispatch classification
    ///
    /// This is the full version that includes domain/complexity/sensitive fields
    /// set by the dispatch brain at gateway entry.
    pub async fn create_task_with_dispatch(
        &self,
        id: &str,
        input: &str,
        source: TaskSource,
        domain: &str,
        complexity: &str,
        sensitive: bool,
        session_id: Option<&str>,
        workspace: Option<&str>,
        team_id: Option<&str>,
    ) -> Result<PendingTask> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let status = PendingTaskStatus::Pending.as_str();
        let source_str = source.as_str();
        let sensitive_int = if sensitive { 1 } else { 0 };

        sqlx::query(
            r#"INSERT INTO pending_tasks 
               (id, input, source, status, domain, complexity, sensitive, created_at, session_id, workspace, team_id)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(id)
        .bind(input)
        .bind(source_str)
        .bind(status)
        .bind(domain)
        .bind(complexity)
        .bind(sensitive_int)
        .bind(now)
        .bind(session_id)
        .bind(workspace)
        .bind(team_id)
        .execute(&self.pool)
        .await
        .context("Failed to create pending task")?;

        Ok(PendingTask {
            id: id.to_string(),
            input: input.to_string(),
            source,
            status: PendingTaskStatus::Pending,
            domain: domain.to_string(),
            complexity: complexity.to_string(),
            sensitive,
            created_at: now,
            started_at: None,
            done_at: None,
            error: None,
            session_id: session_id.map(String::from),
            workspace: workspace.map(String::from),
            team_id: team_id.map(String::from),
        })
    }

    /// Get all pending tasks for gateway polling
    ///
    /// Returns tasks ordered by created_at (oldest first) with optional limit.
    pub async fn get_pending_tasks(&self, limit: i64) -> Result<Vec<PendingTask>> {
        let rows = sqlx::query(
            r#"SELECT id, input, source, status, domain, complexity, sensitive, created_at, started_at, done_at, error, session_id, workspace, team_id
               FROM pending_tasks
               WHERE status = 'pending'
               ORDER BY created_at ASC
               LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch pending tasks")?;

        Ok(rows
            .into_iter()
            .map(|r| PendingTask {
                id: r.get("id"),
                input: r.get("input"),
                source: TaskSource::parse_str(&r.get::<String, _>("source")),
                status: PendingTaskStatus::parse_str(&r.get::<String, _>("status")),
                domain: r.get("domain"),
                complexity: r.get("complexity"),
                sensitive: r.get::<i32, _>("sensitive") != 0,
                created_at: r.get("created_at"),
                started_at: r.get("started_at"),
                done_at: r.get("done_at"),
                error: r.get("error"),
                session_id: r.get("session_id"),
                workspace: r.get("workspace"),
                team_id: r.get("team_id"),
            })
            .collect())
    }

    /// Mark task as running (atomic transition before spawn)
    ///
    /// This should be called IMMEDIATELY before tokio::spawn to prevent
    /// two workers from picking up the same task.
    pub async fn mark_running(&self, task_id: &str) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let status = PendingTaskStatus::Running.as_str();

        sqlx::query(
            "UPDATE pending_tasks SET status = ?, started_at = ? WHERE id = ? AND status = 'pending'"
        )
        .bind(status)
        .bind(now)
        .bind(task_id)
        .execute(&self.pool)
        .await
        .context("Failed to mark task as running")?;

        Ok(())
    }

    /// Mark task as done (successful completion)
    pub async fn mark_done(&self, task_id: &str) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let status = PendingTaskStatus::Done.as_str();

        sqlx::query("UPDATE pending_tasks SET status = ?, done_at = ? WHERE id = ?")
            .bind(status)
            .bind(now)
            .bind(task_id)
            .execute(&self.pool)
            .await
            .context("Failed to mark task as done")?;

        Ok(())
    }

    /// Mark task as failed (with error message)
    pub async fn mark_failed(&self, task_id: &str, error: &str) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

        let status = PendingTaskStatus::Failed.as_str();

        sqlx::query("UPDATE pending_tasks SET status = ?, done_at = ?, error = ? WHERE id = ?")
            .bind(status)
            .bind(now)
            .bind(error)
            .bind(task_id)
            .execute(&self.pool)
            .await
            .context("Failed to mark task as failed")?;

        Ok(())
    }

    /// Get a pending task by ID
    pub async fn get_task(&self, task_id: &str) -> Result<Option<PendingTask>> {
        let row = sqlx::query(
            r#"SELECT id, input, source, status, domain, complexity, sensitive, created_at, started_at, done_at, error, session_id, workspace, team_id
               FROM pending_tasks
               WHERE id = ?"#,
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch pending task")?;

        Ok(row.map(|r| PendingTask {
            id: r.get("id"),
            input: r.get("input"),
            source: TaskSource::parse_str(&r.get::<String, _>("source")),
            status: PendingTaskStatus::parse_str(&r.get::<String, _>("status")),
            domain: r.get("domain"),
            complexity: r.get("complexity"),
            sensitive: r.get::<i32, _>("sensitive") != 0,
            created_at: r.get("created_at"),
            started_at: r.get("started_at"),
            done_at: r.get("done_at"),
            error: r.get("error"),
            session_id: r.get("session_id"),
            workspace: r.get("workspace"),
            team_id: r.get("team_id"),
        }))
    }

    /// Recovery: Reset all running tasks to pending (crash recovery)
    ///
    /// Call this on engine startup to recover tasks that were running
    /// when the engine crashed.
    pub async fn recover_crashed_tasks(&self) -> Result<usize> {
        let result = sqlx::query(
            "UPDATE pending_tasks SET status = 'pending', started_at = NULL, error = NULL WHERE status = 'running'"
        )
        .execute(&self.pool)
        .await
        .context("Failed to recover crashed tasks")?;

        Ok(result.rows_affected() as usize)
    }

    /// Delete old completed/failed tasks (cleanup)
    pub async fn delete_old_tasks(&self, older_than_days: i64) -> Result<u64> {
        let cutoff = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64
            - (older_than_days * 24 * 60 * 60);

        let result = sqlx::query(
            "DELETE FROM pending_tasks WHERE status IN ('done', 'failed') AND created_at < ?",
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await
        .context("Failed to delete old pending tasks")?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use tempfile::TempDir;

    async fn test_db() -> (Database, PendingTaskRepository, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // Create database and run migrations
        let db = Database::new(&db_path).await.unwrap();

        let repo = PendingTaskRepository::new(db.pool().clone());
        (db, repo, temp_dir)
    }

    #[tokio::test]
    async fn test_create_pending_task() {
        let (_db, repo, _temp) = test_db().await;

        let task = repo
            .create_task(
                "task-1",
                "list files",
                TaskSource::Cli,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(task.id, "task-1");
        assert_eq!(task.input, "list files");
        assert_eq!(task.source, TaskSource::Cli);
        assert_eq!(task.status, PendingTaskStatus::Pending);
        assert!(task.started_at.is_none());
        assert!(task.done_at.is_none());
    }

    #[tokio::test]
    async fn test_get_pending_tasks() {
        let (_db, repo, _temp) = test_db().await;

        // Create multiple pending tasks
        repo.create_task("task-1", "first", TaskSource::Cli, None, None, None)
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        repo.create_task(
            "task-2",
            "second",
            TaskSource::Telegram(String::new()),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        repo.create_task(
            "task-3",
            "third",
            TaskSource::WebUI,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let pending = repo.get_pending_tasks(10).await.unwrap();

        assert_eq!(pending.len(), 3);
        assert_eq!(pending[0].id, "task-1"); // Oldest first
        assert_eq!(pending[1].id, "task-2");
        assert_eq!(pending[2].id, "task-3");
    }

    #[tokio::test]
    async fn test_mark_running() {
        let (_db, repo, _temp) = test_db().await;

        repo.create_task("task-1", "test", TaskSource::Cli, None, None, None)
            .await
            .unwrap();

        repo.mark_running("task-1").await.unwrap();

        let task = repo.get_task("task-1").await.unwrap().unwrap();
        assert_eq!(task.status, PendingTaskStatus::Running);
        assert!(task.started_at.is_some());
    }

    #[tokio::test]
    async fn test_mark_done() {
        let (_db, repo, _temp) = test_db().await;

        repo.create_task("task-1", "test", TaskSource::Cli, None, None, None)
            .await
            .unwrap();
        repo.mark_running("task-1").await.unwrap();

        repo.mark_done("task-1").await.unwrap();

        let task = repo.get_task("task-1").await.unwrap().unwrap();
        assert_eq!(task.status, PendingTaskStatus::Done);
        assert!(task.done_at.is_some());
    }

    #[tokio::test]
    async fn test_mark_failed() {
        let (_db, repo, _temp) = test_db().await;

        repo.create_task("task-1", "test", TaskSource::Cli, None, None, None)
            .await
            .unwrap();
        repo.mark_running("task-1").await.unwrap();

        repo.mark_failed("task-1", "something went wrong")
            .await
            .unwrap();

        let task = repo.get_task("task-1").await.unwrap().unwrap();
        assert_eq!(task.status, PendingTaskStatus::Failed);
        assert_eq!(task.error, Some("something went wrong".to_string()));
    }

    #[tokio::test]
    async fn test_recover_crashed_tasks() {
        let (_db, repo, _temp) = test_db().await;

        // Create tasks and mark some as running
        repo.create_task("task-1", "first", TaskSource::Cli, None, None, None)
            .await
            .unwrap();
        repo.create_task("task-2", "second", TaskSource::Cli, None, None, None)
            .await
            .unwrap();
        repo.create_task("task-3", "third", TaskSource::Cli, None, None, None)
            .await
            .unwrap();

        repo.mark_running("task-1").await.unwrap();
        repo.mark_running("task-2").await.unwrap();
        // task-3 stays pending

        // Simulate crash recovery
        let recovered = repo.recover_crashed_tasks().await.unwrap();

        assert_eq!(recovered, 2); // task-1 and task-2 recovered

        let pending = repo.get_pending_tasks(10).await.unwrap();
        assert_eq!(pending.len(), 3); // All three should be pending now
    }

    #[tokio::test]
    async fn test_delete_old_tasks() {
        let (_db, repo, _temp) = test_db().await;

        // Create a task with a timestamp 2 days in the past
        let task_id = "task-old-1";
        let two_days_ago = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - (2 * 24 * 60 * 60);

        sqlx::query(
            "INSERT INTO pending_tasks (id, input, source, status, created_at) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(task_id)
        .bind("old task")
        .bind("cli")
        .bind("done")
        .bind(two_days_ago)
        .execute(&repo.pool)
        .await
        .unwrap();

        // Delete tasks older than 1 day (should delete our 2-day-old task)
        let deleted = repo.delete_old_tasks(1).await.unwrap();

        assert!(deleted >= 1, "Should have deleted at least 1 task");

        let pending = repo.get_pending_tasks(10).await.unwrap();
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn test_pending_task_with_metadata() {
        let (_db, repo, _temp) = test_db().await;

        let task = repo
            .create_task(
                "task-1",
                "test",
                TaskSource::Remote(String::new()),
                Some("session-123"),
                Some("/workspace"),
                Some("team-456"),
            )
            .await
            .unwrap();

        assert_eq!(task.session_id, Some("session-123".to_string()));
        assert_eq!(task.workspace, Some("/workspace".to_string()));
        assert_eq!(task.team_id, Some("team-456".to_string()));
    }
}
