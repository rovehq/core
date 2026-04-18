use std::path::PathBuf;
use std::sync::Arc;

use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::cli::bootstrap::build_task_agent;
use crate::config::Config;
use crate::storage::schedule::ScheduledTargetKind;
use crate::system::specs::SpecRepository;
use crate::system::workflow_runtime;
use sdk::{RunContextId, RunIsolation, RunMode, TaskSource};

use super::{Gateway, Task};

impl Gateway {
    pub(super) async fn run(&self) {
        let mut poller = interval(Duration::from_millis(self.config.poll_interval_ms));
        poller.tick().await;

        loop {
            poller.tick().await;
            match self.enqueue_due_schedules().await {
                Ok(count) if count > 0 => info!("Queued {} scheduled task(s)", count),
                Ok(_) => {}
                Err(error) => warn!("Schedule enqueue error: {}", error),
            }
            match self.poll_and_spawn().await {
                Ok(count) if count > 0 => info!("Spawned {} task(s)", count),
                Ok(_) => {}
                Err(error) => warn!("Gateway poll error: {}", error),
            }
        }
    }

    async fn enqueue_due_schedules(&self) -> anyhow::Result<usize> {
        let repo = self.db.schedules();
        let due = repo.due(self.config.poll_limit).await?;
        if due.is_empty() {
            return Ok(0);
        }

        let pending_repo = self.db.pending_tasks();
        let mut queued = 0;

        for schedule in due {
            if let Err(error) = repo
                .mark_dispatched(&schedule.id, schedule.interval_secs)
                .await
            {
                warn!(
                    schedule_id = %schedule.id,
                    schedule_name = %schedule.name,
                    "Failed to update scheduled task after dispatch: {}",
                    error
                );
                continue;
            }

            if matches!(schedule.target_kind, ScheduledTargetKind::Workflow) {
                let Some(workflow_id) = schedule.target_id.clone() else {
                    warn!(
                        schedule_id = %schedule.id,
                        schedule_name = %schedule.name,
                        "Scheduled workflow trigger is missing target_id"
                    );
                    continue;
                };
                let db_clone = Arc::clone(&self.db);
                let schedule_id = schedule.id.clone();
                let schedule_name = schedule.name.clone();
                let input = schedule.input.clone();
                tokio::spawn(async move {
                    if let Err(error) =
                        execute_scheduled_workflow(db_clone, &workflow_id, &input).await
                    {
                        warn!(
                            schedule_id = %schedule_id,
                            schedule_name = %schedule_name,
                            workflow_id = %workflow_id,
                            "Failed to execute scheduled workflow: {}",
                            error
                        );
                    } else {
                        info!(
                            schedule_id = %schedule_id,
                            schedule_name = %schedule_name,
                            workflow_id = %workflow_id,
                            "Executed scheduled workflow"
                        );
                    }
                });
                queued += 1;
                continue;
            }

            let task_id = Uuid::new_v4().to_string();
            if let Err(error) = pending_repo
                .create_task(
                    &task_id,
                    &schedule.input,
                    TaskSource::Cli,
                    None,
                    None,
                    schedule.workspace.as_deref(),
                    None,
                )
                .await
            {
                warn!(
                    schedule_id = %schedule.id,
                    schedule_name = %schedule.name,
                    task_id = %task_id,
                    "Failed to enqueue scheduled task: {}",
                    error
                );
                continue;
            }

            info!(
                schedule_id = %schedule.id,
                schedule_name = %schedule.name,
                task_id = %task_id,
                "Enqueued scheduled task"
            );
            queued += 1;
        }

        Ok(queued)
    }

    async fn poll_and_spawn(&self) -> anyhow::Result<usize> {
        let repo = self.db.pending_tasks();
        let pending = repo.get_pending_tasks(self.config.poll_limit).await?;
        if pending.is_empty() {
            return Ok(0);
        }

        let mut spawned = 0;
        for task_row in pending {
            if let Err(error) = repo.mark_running(&task_row.id).await {
                warn!(task_id = %task_row.id, "Failed to mark task as running: {}", error);
                continue;
            }

            let task_id = task_row.id.clone();
            let task_source = task_row.source;
            let task_source_str = task_source.as_str().to_string();
            let task = Task {
                id: Uuid::parse_str(&task_row.id).unwrap_or_else(|_| Uuid::new_v4()),
                input: task_row.input.clone(),
                source: task_source,
                execution_profile: task_row.execution_profile.clone(),
                risk_tier_override: None,
                run_context_id: RunContextId(task_row.id.clone()),
                run_mode: RunMode::Serial,
                run_isolation: RunIsolation::None,
                session_id: task_row
                    .session_id
                    .and_then(|value| Uuid::parse_str(&value).ok()),
                workspace: task_row.workspace.as_ref().map(PathBuf::from),
                created_at: task_row.created_at,
            };

            let db_clone = Arc::clone(&self.db);
            tokio::spawn(async move {
                info!(task_id = %task_id, source = %task_source_str, "Starting task");

                let result = match build_task_agent(db_clone.clone(), task.workspace.clone()).await
                {
                    Ok(mut agent) => agent.process_task(task).await,
                    Err(error) => Err(error),
                };

                let repo = db_clone.pending_tasks();
                match result {
                    Ok(_) => {
                        info!(task_id = %task_id, "Task completed successfully");
                        if let Err(error) = repo.mark_done(&task_id).await {
                            error!(task_id = %task_id, "Failed to mark task as done: {}", error);
                        }
                    }
                    Err(error) => {
                        error!(task_id = %task_id, "Task failed: {}", error);
                        if let Err(mark_error) =
                            repo.mark_failed(&task_id, &error.to_string()).await
                        {
                            error!(task_id = %task_id, "Failed to mark task as failed: {}", mark_error);
                        }
                    }
                }
            });

            spawned += 1;
        }

        Ok(spawned)
    }
}

async fn execute_scheduled_workflow(
    db: Arc<crate::db::Database>,
    workflow_id: &str,
    input: &str,
) -> anyhow::Result<()> {
    let repo = SpecRepository::new()?;
    let config = Config::load_or_create()?;
    let workflow = repo.load_workflow(workflow_id)?;
    if !workflow.enabled {
        anyhow::bail!("workflow '{}' is disabled", workflow.id);
    }
    workflow_runtime::start_new_run(&repo, &db, &config, &workflow, input).await?;
    Ok(())
}
