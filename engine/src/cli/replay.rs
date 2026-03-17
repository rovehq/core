use anyhow::{Context, Result};
use serde_json::json;

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::storage::{Database, TaskRepository};

use super::output::OutputFormat;

pub async fn handle_replay(task_id: String, config: &Config, format: OutputFormat) -> Result<()> {
    let task_uuid = uuid::Uuid::parse_str(&task_id).context("Invalid task ID format")?;

    let database = Database::new(&database_path(config))
        .await
        .context("Failed to open database")?;
    let task_repo = TaskRepository::new(database.pool().clone());

    let task = task_repo
        .get_task(&task_uuid)
        .await
        .context("Failed to fetch task")?
        .ok_or_else(|| anyhow::anyhow!("Task not found: {}", task_id))?;

    let steps = task_repo
        .get_task_steps(&task_uuid)
        .await
        .context("Failed to fetch task steps")?;

    match format {
        OutputFormat::Text => {
            println!("Task Replay: {}", task_id);
            println!();
            println!("Input: {}", task.input);
            println!("Status: {:?}", task.status);

            if let Some(provider) = task.provider_used {
                println!("Provider: {}", provider);
            }

            if let Some(duration) = task.duration_ms {
                println!("Duration: {}ms", duration);
            }

            println!();
            println!("Steps ({} total):", steps.len());
            println!();

            for step in steps {
                println!("Step {}: {:?}", step.step_order, step.step_type);
                println!("  {}", step.content);
                println!();
            }
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "task": task,
                    "steps": steps,
                    "step_count": steps.len(),
                }))?
            );
        }
    }

    Ok(())
}
