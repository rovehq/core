use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub input: String,
    pub interval_secs: i64,
    pub enabled: bool,
    pub workspace: Option<String>,
    pub created_at: i64,
    pub last_run_at: Option<i64>,
    pub next_run_at: i64,
}

pub struct ScheduleRepository {
    pool: SqlitePool,
}

impl ScheduleRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        name: &str,
        input: &str,
        interval_secs: i64,
        workspace: Option<&str>,
        start_now: bool,
    ) -> Result<ScheduledTask> {
        let now = unix_now()?;
        let next_run_at = if start_now { now } else { now + interval_secs };
        let id = Uuid::new_v4().to_string();

        sqlx::query(
            r#"INSERT INTO scheduled_tasks
               (id, name, input, interval_secs, enabled, workspace, created_at, next_run_at)
               VALUES (?, ?, ?, ?, 1, ?, ?, ?)"#,
        )
        .bind(&id)
        .bind(name)
        .bind(input)
        .bind(interval_secs)
        .bind(workspace)
        .bind(now)
        .bind(next_run_at)
        .execute(&self.pool)
        .await
        .context("Failed to create scheduled task")?;

        Ok(ScheduledTask {
            id,
            name: name.to_string(),
            input: input.to_string(),
            interval_secs,
            enabled: true,
            workspace: workspace.map(ToOwned::to_owned),
            created_at: now,
            last_run_at: None,
            next_run_at,
        })
    }

    pub async fn list(&self) -> Result<Vec<ScheduledTask>> {
        let rows = sqlx::query(
            r#"SELECT id, name, input, interval_secs, enabled, workspace, created_at, last_run_at, next_run_at
               FROM scheduled_tasks
               ORDER BY name ASC"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to list scheduled tasks")?;

        Ok(rows.into_iter().map(row_to_scheduled_task).collect())
    }

    pub async fn remove(&self, name: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM scheduled_tasks WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await
            .context("Failed to remove scheduled task")?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn due(&self, limit: i64) -> Result<Vec<ScheduledTask>> {
        let now = unix_now()?;
        let rows = sqlx::query(
            r#"SELECT id, name, input, interval_secs, enabled, workspace, created_at, last_run_at, next_run_at
               FROM scheduled_tasks
               WHERE enabled = 1 AND next_run_at <= ?
               ORDER BY next_run_at ASC
               LIMIT ?"#,
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch due scheduled tasks")?;

        Ok(rows.into_iter().map(row_to_scheduled_task).collect())
    }

    pub async fn mark_dispatched(&self, id: &str, interval_secs: i64) -> Result<()> {
        let now = unix_now()?;
        let next_run_at = now + interval_secs;

        sqlx::query("UPDATE scheduled_tasks SET last_run_at = ?, next_run_at = ? WHERE id = ?")
            .bind(now)
            .bind(next_run_at)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to update scheduled task after dispatch")?;

        Ok(())
    }
}

fn row_to_scheduled_task(row: sqlx::sqlite::SqliteRow) -> ScheduledTask {
    ScheduledTask {
        id: row.get("id"),
        name: row.get("name"),
        input: row.get("input"),
        interval_secs: row.get("interval_secs"),
        enabled: row.get::<i64, _>("enabled") != 0,
        workspace: row.get("workspace"),
        created_at: row.get("created_at"),
        last_run_at: row.get("last_run_at"),
        next_run_at: row.get("next_run_at"),
    }
}

fn unix_now() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before UNIX_EPOCH")?
        .as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::ScheduleRepository;
    use crate::storage::Database;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_and_list_scheduled_task() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("schedule.db"))
            .await
            .unwrap();
        let repo: ScheduleRepository = db.schedules();

        repo.create(
            "daily-brief",
            "prepare the morning brief",
            3600,
            Some("/tmp/workspace"),
            false,
        )
        .await
        .unwrap();

        let tasks = repo.list().await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "daily-brief");
        assert_eq!(tasks[0].workspace.as_deref(), Some("/tmp/workspace"));
    }

    #[tokio::test]
    async fn test_due_scheduled_task_and_dispatch_mark() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("schedule.db"))
            .await
            .unwrap();
        let repo: ScheduleRepository = db.schedules();

        let task = repo
            .create("check-inbox", "summarize unread mail", 300, None, true)
            .await
            .unwrap();

        let due = repo.due(10).await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, task.id);

        repo.mark_dispatched(&task.id, task.interval_secs)
            .await
            .unwrap();

        let due_after = repo.due(10).await.unwrap();
        assert!(due_after.is_empty());
    }

    #[tokio::test]
    async fn test_remove_scheduled_task() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("schedule.db"))
            .await
            .unwrap();
        let repo: ScheduleRepository = db.schedules();

        repo.create("cleanup", "clean up inbox", 600, None, false)
            .await
            .unwrap();

        assert!(repo.remove("cleanup").await.unwrap());
        assert!(!repo.remove("cleanup").await.unwrap());
    }
}
