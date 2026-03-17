use anyhow::{Context, Result};
use chrono::DateTime;
use serde_json::json;

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::storage::{Database, TaskRepository};

use super::output::OutputFormat;

pub async fn handle_history(limit: usize, config: &Config, format: OutputFormat) -> Result<()> {
    let database = Database::new(&database_path(config))
        .await
        .context("Failed to open database")?;
    let task_repo = TaskRepository::new(database.pool().clone());

    let tasks = task_repo
        .get_recent_tasks(limit as i64)
        .await
        .context("Failed to fetch task history")?;

    match format {
        OutputFormat::Text => {
            if tasks.is_empty() {
                println!("No tasks in history");
                return Ok(());
            }

            println!("Task History (last {} tasks):", limit);
            println!();

            for task in tasks {
                println!("Task ID: {}", task.id);
                println!("  Input: {}", task.input);
                println!("  Status: {:?}", task.status);

                if let Some(provider) = task.provider_used {
                    println!("  Provider: {}", provider);
                }

                if let Some(duration) = task.duration_ms {
                    println!("  Duration: {}ms", duration);
                }

                let created = DateTime::from_timestamp(task.created_at, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "Unknown".to_string());
                println!("  Created: {}", created);
                println!();
            }
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "tasks": tasks,
                    "count": tasks.len(),
                    "limit": limit,
                }))?
            );
        }
    }

    Ok(())
}
