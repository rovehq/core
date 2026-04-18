use anyhow::{Context, Result};
use sdk::{
    AgentRunRecord, SpecRunStatus, WorkflowRunDetail, WorkflowRunRecord, WorkflowRunStepRecord,
};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct AgentRunRepository {
    pool: SqlitePool,
}

pub struct WorkflowStepStart<'a> {
    pub run_id: &'a str,
    pub step_index: i64,
    pub step_id: &'a str,
    pub step_name: &'a str,
    pub agent_id: Option<&'a str>,
    pub worker_preset: Option<&'a str>,
    pub prompt: &'a str,
}

pub struct WorkflowStepFinish<'a> {
    pub run_id: &'a str,
    pub step_index: i64,
    pub status: SpecRunStatus,
    pub task_id: Option<&'a str>,
    pub output: Option<&'a str>,
    pub error: Option<&'a str>,
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
        steps_total: i64,
    ) -> Result<WorkflowRunRecord> {
        let now = unix_now()?;
        sqlx::query(
            r#"INSERT INTO workflow_runs
               (run_id, workflow_id, status, input, output, error, steps_total, steps_completed, retry_count, cancel_requested, created_at)
               VALUES (?, ?, 'running', ?, NULL, NULL, ?, 0, 0, 0, ?)"#,
        )
        .bind(run_id)
        .bind(workflow_id)
        .bind(input)
        .bind(steps_total)
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
            steps_total,
            steps_completed: 0,
            current_step_index: None,
            current_step_id: None,
            current_step_name: None,
            retry_count: 0,
            last_task_id: None,
            cancel_requested: false,
            resumable: steps_total > 0,
            created_at: now,
            completed_at: None,
        })
    }

    pub async fn get_workflow_run(&self, run_id: &str) -> Result<Option<WorkflowRunRecord>> {
        let row = sqlx::query(
            r#"SELECT run_id, workflow_id, status, input, output, error, steps_total, steps_completed,
                      current_step_index, current_step_id, current_step_name, retry_count, last_task_id,
                      cancel_requested,
                      created_at, completed_at
               FROM workflow_runs
               WHERE run_id = ?"#,
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get workflow run")?;

        Ok(row.map(row_to_workflow_run))
    }

    pub async fn get_workflow_run_detail(&self, run_id: &str) -> Result<Option<WorkflowRunDetail>> {
        let Some(run) = self.get_workflow_run(run_id).await? else {
            return Ok(None);
        };
        let steps = self.list_workflow_run_steps(run_id).await?;
        let variables = workflow_variables_from_steps(&steps);
        Ok(Some(WorkflowRunDetail {
            run,
            steps,
            variables,
        }))
    }

    pub async fn prepare_workflow_resume(&self, run_id: &str) -> Result<()> {
        sqlx::query(
            r#"UPDATE workflow_runs
               SET status = 'running',
                   error = NULL,
                   completed_at = NULL,
                   cancel_requested = 0,
                   retry_count = retry_count + 1
               WHERE run_id = ?"#,
        )
        .bind(run_id)
        .execute(&self.pool)
        .await
        .context("Failed to prepare workflow resume")?;
        Ok(())
    }

    pub async fn request_workflow_run_cancel(&self, run_id: &str) -> Result<bool> {
        let now = unix_now()?;
        let result = sqlx::query(
            r#"UPDATE workflow_runs
               SET cancel_requested = 1,
                   cancel_requested_at = COALESCE(cancel_requested_at, ?)
               WHERE run_id = ?
                 AND status NOT IN ('completed', 'canceled')"#,
        )
        .bind(now)
        .bind(run_id)
        .execute(&self.pool)
        .await
        .context("Failed to request workflow cancel")?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn workflow_run_cancel_requested(&self, run_id: &str) -> Result<bool> {
        let row = sqlx::query("SELECT cancel_requested FROM workflow_runs WHERE run_id = ?")
            .bind(run_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to inspect workflow cancel state")?;

        Ok(row
            .and_then(|row| row.try_get::<i64, _>("cancel_requested").ok())
            .unwrap_or(0)
            != 0)
    }

    pub async fn record_workflow_step_start(&self, step: WorkflowStepStart<'_>) -> Result<()> {
        let now = unix_now()?;
        sqlx::query(
            r#"INSERT INTO workflow_run_steps
               (run_id, step_index, step_id, step_name, agent_id, worker_preset, status, prompt, started_at, attempt_count)
               VALUES (?, ?, ?, ?, ?, ?, 'running', ?, ?, 1)
               ON CONFLICT(run_id, step_index) DO UPDATE SET
                   step_id = excluded.step_id,
                   step_name = excluded.step_name,
                   agent_id = excluded.agent_id,
                   worker_preset = excluded.worker_preset,
                   status = 'running',
                   prompt = excluded.prompt,
                   task_id = NULL,
                   output = NULL,
                   error = NULL,
                   started_at = excluded.started_at,
                   completed_at = NULL,
                   attempt_count = workflow_run_steps.attempt_count + 1"#,
        )
        .bind(step.run_id)
        .bind(step.step_index)
        .bind(step.step_id)
        .bind(step.step_name)
        .bind(step.agent_id)
        .bind(step.worker_preset)
        .bind(step.prompt)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to record workflow step start")?;

        sqlx::query(
            r#"UPDATE workflow_runs
               SET status = 'running',
                   current_step_index = ?,
                   current_step_id = ?,
                   current_step_name = ?,
                   error = NULL,
                   completed_at = NULL
               WHERE run_id = ?"#,
        )
        .bind(step.step_index)
        .bind(step.step_id)
        .bind(step.step_name)
        .bind(step.run_id)
        .execute(&self.pool)
        .await
        .context("Failed to update current workflow step")?;

        Ok(())
    }

    pub async fn record_workflow_step_finish(&self, step: WorkflowStepFinish<'_>) -> Result<()> {
        let now = unix_now()?;
        sqlx::query(
            r#"UPDATE workflow_run_steps
               SET status = ?, task_id = ?, output = ?, error = ?, completed_at = ?
               WHERE run_id = ? AND step_index = ?"#,
        )
        .bind(step.status.as_str())
        .bind(step.task_id)
        .bind(step.output)
        .bind(step.error)
        .bind(now)
        .bind(step.run_id)
        .bind(step.step_index)
        .execute(&self.pool)
        .await
        .context("Failed to record workflow step finish")?;

        if matches!(step.status, SpecRunStatus::Completed) {
            sqlx::query(
                r#"UPDATE workflow_runs
                   SET steps_completed = (
                           SELECT COUNT(*)
                           FROM workflow_run_steps
                           WHERE run_id = ? AND status = 'completed'
                       ),
                       last_task_id = COALESCE(?, last_task_id)
                   WHERE run_id = ?"#,
            )
            .bind(step.run_id)
            .bind(step.task_id)
            .bind(step.run_id)
            .execute(&self.pool)
            .await
            .context("Failed to update workflow completion progress")?;
        }

        Ok(())
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
               SET status = ?,
                   output = ?,
                   error = ?,
                   completed_at = ?,
                   cancel_requested = 0,
                   current_step_index = CASE WHEN ? IN ('completed', 'canceled') THEN NULL ELSE current_step_index END,
                   current_step_id = CASE WHEN ? IN ('completed', 'canceled') THEN NULL ELSE current_step_id END,
                   current_step_name = CASE WHEN ? IN ('completed', 'canceled') THEN NULL ELSE current_step_name END
               WHERE run_id = ?"#,
        )
        .bind(status.as_str())
        .bind(output)
        .bind(error)
        .bind(now)
        .bind(status.as_str())
        .bind(status.as_str())
        .bind(status.as_str())
        .bind(run_id)
        .execute(&self.pool)
        .await
        .context("Failed to finish workflow run")?;
        Ok(())
    }

    pub async fn list_workflow_runs(&self, limit: i64) -> Result<Vec<WorkflowRunRecord>> {
        let rows = sqlx::query(
            r#"SELECT run_id, workflow_id, status, input, output, error, steps_total, steps_completed,
                      current_step_index, current_step_id, current_step_name, retry_count, last_task_id,
                      cancel_requested,
                      created_at, completed_at
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

    pub async fn list_workflow_run_steps(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowRunStepRecord>> {
        let rows = sqlx::query(
            r#"SELECT run_id, step_index, step_id, step_name, agent_id, worker_preset, status,
                      prompt, task_id, output, error, attempt_count, started_at, completed_at
               FROM workflow_run_steps
               WHERE run_id = ?
               ORDER BY step_index ASC"#,
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to list workflow run steps")?;

        Ok(rows.into_iter().map(row_to_workflow_run_step).collect())
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
    let steps_total: i64 = row.get("steps_total");
    let steps_completed: i64 = row.get("steps_completed");
    let status = SpecRunStatus::parse(row.get::<String, _>("status").as_str());
    let cancel_requested = row.get::<i64, _>("cancel_requested") != 0;
    let resumable = !matches!(status, SpecRunStatus::Completed | SpecRunStatus::Canceled)
        && !cancel_requested
        && steps_completed < steps_total;

    WorkflowRunRecord {
        run_id: row.get("run_id"),
        workflow_id: row.get("workflow_id"),
        status,
        input: row.get("input"),
        output: row.get("output"),
        error: row.get("error"),
        steps_total,
        steps_completed,
        current_step_index: row.get("current_step_index"),
        current_step_id: row.get("current_step_id"),
        current_step_name: row.get("current_step_name"),
        retry_count: row.get("retry_count"),
        last_task_id: row.get("last_task_id"),
        cancel_requested,
        resumable,
        created_at: row.get("created_at"),
        completed_at: row.get("completed_at"),
    }
}

fn row_to_workflow_run_step(row: sqlx::sqlite::SqliteRow) -> WorkflowRunStepRecord {
    WorkflowRunStepRecord {
        run_id: row.get("run_id"),
        step_index: row.get("step_index"),
        step_id: row.get("step_id"),
        step_name: row.get("step_name"),
        agent_id: row.get("agent_id"),
        worker_preset: row.get("worker_preset"),
        status: SpecRunStatus::parse(row.get::<String, _>("status").as_str()),
        prompt: row.get("prompt"),
        task_id: row.get("task_id"),
        output: row.get("output"),
        error: row.get("error"),
        attempt_count: row.get("attempt_count"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
    }
}

fn workflow_variables_from_steps(steps: &[WorkflowRunStepRecord]) -> BTreeMap<String, String> {
    let mut variables = BTreeMap::new();
    for step in steps {
        if matches!(step.status, SpecRunStatus::Completed) {
            if let Some(output) = step.output.as_ref() {
                variables.insert(format!("{}.result", step.step_id), output.clone());
            }
        }
    }
    variables
}

fn unix_now() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before UNIX_EPOCH")?
        .as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use crate::storage::{Database, WorkflowStepFinish, WorkflowStepStart};
    use sdk::{SpecRunStatus, WorkflowRunStepRecord};
    use tempfile::TempDir;

    use super::workflow_variables_from_steps;

    #[tokio::test]
    async fn records_agent_and_workflow_runs() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("runs.db"))
            .await
            .unwrap();
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

        repo.start_workflow_run("workflow-1", "release", "ship it", 2)
            .await
            .unwrap();
        repo.record_workflow_step_start(WorkflowStepStart {
            run_id: "workflow-1",
            step_index: 0,
            step_id: "step-1",
            step_name: "Inspect",
            agent_id: None,
            worker_preset: Some("researcher"),
            prompt: "Inspect {{input}}",
        })
        .await
        .unwrap();
        repo.record_workflow_step_finish(WorkflowStepFinish {
            run_id: "workflow-1",
            step_index: 0,
            status: SpecRunStatus::Completed,
            task_id: Some("task-2"),
            output: Some("inspected"),
            error: None,
        })
        .await
        .unwrap();
        repo.finish_workflow_run("workflow-1", SpecRunStatus::Failed, None, Some("boom"))
            .await
            .unwrap();

        assert_eq!(repo.list_agent_runs(10).await.unwrap().len(), 1);
        let workflow_runs = repo.list_workflow_runs(10).await.unwrap();
        assert_eq!(workflow_runs.len(), 1);
        assert_eq!(workflow_runs[0].steps_completed, 1);
        assert!(workflow_runs[0].resumable);
        assert_eq!(
            repo.list_workflow_run_steps("workflow-1")
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn resume_increments_retry_count() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("resume.db"))
            .await
            .unwrap();
        let repo = db.agent_runs();

        repo.start_workflow_run("workflow-1", "release", "ship it", 1)
            .await
            .unwrap();
        repo.finish_workflow_run("workflow-1", SpecRunStatus::Failed, None, Some("boom"))
            .await
            .unwrap();
        repo.prepare_workflow_resume("workflow-1").await.unwrap();

        let run = repo.get_workflow_run("workflow-1").await.unwrap().unwrap();
        assert_eq!(run.retry_count, 1);
        assert_eq!(run.status, SpecRunStatus::Running);
        assert!(run.error.is_none());
    }

    #[test]
    fn workflow_run_detail_derives_named_variables_from_completed_steps() {
        let steps = vec![
            WorkflowRunStepRecord {
                run_id: "run-1".to_string(),
                step_index: 0,
                step_id: "inspect".to_string(),
                step_name: "Inspect".to_string(),
                agent_id: None,
                worker_preset: None,
                status: SpecRunStatus::Completed,
                prompt: "Inspect".to_string(),
                task_id: Some("task-1".to_string()),
                output: Some("inspection output".to_string()),
                error: None,
                attempt_count: 1,
                started_at: 1,
                completed_at: Some(2),
            },
            WorkflowRunStepRecord {
                run_id: "run-1".to_string(),
                step_index: 1,
                step_id: "fix".to_string(),
                step_name: "Fix".to_string(),
                agent_id: None,
                worker_preset: None,
                status: SpecRunStatus::Failed,
                prompt: "Fix".to_string(),
                task_id: None,
                output: Some("failed output".to_string()),
                error: Some("boom".to_string()),
                attempt_count: 1,
                started_at: 3,
                completed_at: Some(4),
            },
        ];

        let variables = workflow_variables_from_steps(&steps);
        assert_eq!(
            variables.get("inspect.result").map(String::as_str),
            Some("inspection output")
        );
        assert!(!variables.contains_key("fix.result"));
    }
}
