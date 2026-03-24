use anyhow::{Context, Result};
use sdk::{AgentRunRecord, SpecRunStatus, WorkflowRunRecord};
use sqlx::{Row, SqlitePool};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct AgentRunRepository {
    pool: SqlitePool,
}

impl AgentRunRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn start_agent_run(
        &self,
        run_id: &str,
        agent_id: &str,
        task_id: Option<&str>,
        workflow_run_id: Option<&str>,
        input: &str,
    ) -> Result<AgentRunRecord> {
        let now = unix_now()?;
        sqlx::query(
            r#"INSERT INTO agent_runs
               (run_id, agent_id, task_id, workflow_run_id, status, input, created_at)
               VALUES (?, ?, ?, ?, 'running', ?, ?)"#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(task_id)
        .bind(workflow_run_id)
        .bind(input)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to start agent run")?;

        Ok(AgentRunRecord {
            run_id: run_id.to_string(),
            agent_id: agent_id.to_string(),
            task_id: task_id.map(ToOwned::to_owned),
            workflow_run_id: workflow_run_id.map(ToOwned::to_owned),
            status: SpecRunStatus::Running,
            input: input.to_string(),
            output: None,
            error: None,
            created_at: now,
            completed_at: None,
        })
    }

    pub async fn finish_agent_run(
        &self,
        run_id: &str,
        status: SpecRunStatus,
        task_id: Option<&str>,
        output: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        let now = unix_now()?;
        sqlx::query(
            r#"UPDATE agent_runs
               SET status = ?, task_id = COALESCE(?, task_id), output = ?, error = ?, completed_at = ?
               WHERE run_id = ?"#,
        )
        .bind(status.as_str())
        .bind(task_id)
        .bind(output)
        .bind(error)
        .bind(now)
        .bind(run_id)
        .execute(&self.pool)
        .await
        .context("Failed to finish agent run")?;
        Ok(())
    }

    pub async fn list_agent_runs(&self, limit: i64) -> Result<Vec<AgentRunRecord>> {
        let rows = sqlx::query(
            r#"SELECT run_id, agent_id, task_id, workflow_run_id, status, input, output, error, created_at, completed_at
               FROM agent_runs
               ORDER BY created_at DESC
               LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list agent runs")?;

        Ok(rows.into_iter().map(row_to_agent_run).collect())
    }

    pub async fn start_workflow_run(
        &self,
        run_id: &str,
        workflow_id: &str,
        input: &str,
    ) -> Result<WorkflowRunRecord> {
        let now = unix_now()?;
        sqlx::query(
            r#"INSERT INTO workflow_runs
               (run_id, workflow_id, status, input, created_at)
               VALUES (?, ?, 'running', ?, ?)"#,
        )
        .bind(run_id)
        .bind(workflow_id)
        .bind(input)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to start workflow run")?;

        Ok(WorkflowRunRecord {
            run_id: run_id.to_string(),
            workflow_id: workflow_id.to_string(),
            status: SpecRunStatus::Running,
            input: input.to_string(),
            output: None,
            error: None,
            created_at: now,
            completed_at: None,
        })
    }

    pub async fn finish_workflow_run(
        &self,
        run_id: &str,
        status: SpecRunStatus,
        output: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        let now = unix_now()?;
        sqlx::query(
            r#"UPDATE workflow_runs
               SET status = ?, output = ?, error = ?, completed_at = ?
               WHERE run_id = ?"#,
        )
        .bind(status.as_str())
        .bind(output)
        .bind(error)
        .bind(now)
        .bind(run_id)
        .execute(&self.pool)
        .await
        .context("Failed to finish workflow run")?;
        Ok(())
    }

    pub async fn list_workflow_runs(&self, limit: i64) -> Result<Vec<WorkflowRunRecord>> {
        let rows = sqlx::query(
            r#"SELECT run_id, workflow_id, status, input, output, error, created_at, completed_at
               FROM workflow_runs
               ORDER BY created_at DESC
               LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list workflow runs")?;

        Ok(rows.into_iter().map(row_to_workflow_run).collect())
    }
}

fn row_to_agent_run(row: sqlx::sqlite::SqliteRow) -> AgentRunRecord {
    AgentRunRecord {
        run_id: row.get("run_id"),
        agent_id: row.get("agent_id"),
        task_id: row.get("task_id"),
        workflow_run_id: row.get("workflow_run_id"),
        status: SpecRunStatus::parse(row.get::<String, _>("status").as_str()),
        input: row.get("input"),
        output: row.get("output"),
        error: row.get("error"),
        created_at: row.get("created_at"),
        completed_at: row.get("completed_at"),
    }
}

fn row_to_workflow_run(row: sqlx::sqlite::SqliteRow) -> WorkflowRunRecord {
    WorkflowRunRecord {
        run_id: row.get("run_id"),
        workflow_id: row.get("workflow_id"),
        status: SpecRunStatus::parse(row.get::<String, _>("status").as_str()),
        input: row.get("input"),
        output: row.get("output"),
        error: row.get("error"),
        created_at: row.get("created_at"),
        completed_at: row.get("completed_at"),
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
    use crate::storage::Database;
    use sdk::SpecRunStatus;
    use tempfile::TempDir;

    #[tokio::test]
    async fn records_agent_and_workflow_runs() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("runs.db")).await.unwrap();
        let repo = db.agent_runs();

        repo.start_agent_run("run-1", "assistant", Some("task-1"), None, "hello")
            .await
            .unwrap();
        repo.finish_agent_run(
            "run-1",
            SpecRunStatus::Completed,
            Some("task-1"),
            Some("done"),
            None,
        )
            .await
            .unwrap();

        repo.start_workflow_run("workflow-1", "release", "ship it")
            .await
            .unwrap();
        repo.finish_workflow_run("workflow-1", SpecRunStatus::Failed, None, Some("boom"))
            .await
            .unwrap();

        assert_eq!(repo.list_agent_runs(10).await.unwrap().len(), 1);
        assert_eq!(repo.list_workflow_runs(10).await.unwrap().len(), 1);
    }
}
