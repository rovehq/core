use std::sync::Arc;

use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::agent::AgentCore;

use super::{Gateway, Task};

impl Gateway {
    pub(super) async fn run(&self, agent: Arc<RwLock<AgentCore>>) {
        let mut poller = interval(Duration::from_millis(self.config.poll_interval_ms));
        poller.tick().await;

        loop {
            poller.tick().await;
            match self.poll_and_spawn(Arc::clone(&agent)).await {
                Ok(count) if count > 0 => info!("Spawned {} task(s)", count),
                Ok(_) => {}
                Err(error) => warn!("Gateway poll error: {}", error),
            }
        }
    }

    async fn poll_and_spawn(&self, agent: Arc<RwLock<AgentCore>>) -> anyhow::Result<usize> {
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

            let agent = Arc::clone(&agent);
            let task_id = task_row.id.clone();
            let task_source = task_row.source;
            let task_source_str = task_source.as_str().to_string();
            let task = Task {
                id: Uuid::parse_str(&task_row.id).unwrap_or_else(|_| Uuid::new_v4()),
                input: task_row.input.clone(),
                source: task_source.into(),
                risk_tier_override: None,
                session_id: task_row.session_id.and_then(|value| Uuid::parse_str(&value).ok()),
                created_at: task_row.created_at,
            };

            let db_clone = Arc::clone(&self.db);
            tokio::spawn(async move {
                info!(task_id = %task_id, source = %task_source_str, "Starting task");

                let result = {
                    let mut agent_guard = agent.write().await;
                    agent_guard.process_task(task).await
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
                        if let Err(mark_error) = repo.mark_failed(&task_id, &error.to_string()).await {
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
