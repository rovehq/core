use anyhow::{bail, Result};
use chrono::{DateTime, Utc};

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::storage::{Database, ScheduledTask};

use super::ScheduleAction;

pub async fn handle_schedule(action: ScheduleAction, config: &Config) -> Result<()> {
    let database = Database::new(&database_path(config)).await?;
    let repo = database.schedules();

    match action {
        ScheduleAction::Add {
            name,
            every_minutes,
            start_now,
            prompt,
        } => {
            if prompt.is_empty() {
                bail!("A schedule prompt is required");
            }

            if every_minutes == 0 {
                bail!("--every-minutes must be greater than 0");
            }

            let workspace = std::env::current_dir()
                .ok()
                .map(|path| path.display().to_string());
            let task = repo
                .create(
                    &name,
                    &prompt.join(" "),
                    (every_minutes * 60) as i64,
                    workspace.as_deref(),
                    start_now,
                )
                .await?;

            println!("Added schedule '{}'", task.name);
            println!("  Interval: every {} minute(s)", every_minutes);
            println!("  Next run: {}", format_timestamp(task.next_run_at));
            if let Some(workspace) = task.workspace {
                println!("  Workspace: {}", workspace);
            }
            println!("  Run `rove start` to let the daemon execute due schedules.");
        }
        ScheduleAction::List => {
            let tasks = repo.list().await?;
            if tasks.is_empty() {
                println!("No schedules configured");
                return Ok(());
            }

            println!("Configured schedules:");
            for task in tasks {
                print_schedule(&task);
            }
        }
        ScheduleAction::Remove { name } => {
            if repo.remove(&name).await? {
                println!("Removed schedule '{}'", name);
            } else {
                println!("No schedule named '{}' found", name);
            }
        }
    }

    Ok(())
}

fn print_schedule(task: &ScheduledTask) {
    println!("  {}", task.name);
    println!("    Prompt: {}", task.input);
    println!("    Interval: every {} minute(s)", task.interval_secs / 60);
    println!("    Next run: {}", format_timestamp(task.next_run_at));
    if let Some(last_run_at) = task.last_run_at {
        println!("    Last run: {}", format_timestamp(last_run_at));
    }
    if let Some(workspace) = &task.workspace {
        println!("    Workspace: {}", workspace);
    }
}

fn format_timestamp(timestamp: i64) -> String {
    DateTime::<Utc>::from_timestamp(timestamp, 0)
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| timestamp.to_string())
}
