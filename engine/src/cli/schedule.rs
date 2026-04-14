use anyhow::{bail, Result};
use chrono::{DateTime, Utc};

use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::storage::{Database, ScheduledTask};
use crate::system::specs::SpecRepository;

use super::ScheduleAction;

pub async fn handle_schedule(action: ScheduleAction, config: &Config) -> Result<()> {
    let database = Database::new(&database_path(config)).await?;
    let repo = database.schedules();

    match action {
        ScheduleAction::Add {
            name,
            every_minutes,
            workflow,
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
            let task = if let Some(workflow_id) = workflow.as_deref() {
                let spec_repo = SpecRepository::new()?;
                let workflow = spec_repo.load_workflow(workflow_id)?;
                let task = repo
                    .create_workflow_trigger(
                        &name,
                        &workflow.id,
                        &prompt.join(" "),
                        (every_minutes * 60) as i64,
                        workspace.as_deref(),
                        start_now,
                    )
                    .await?;
                sync_workflow_schedule_binding(&spec_repo, &workflow.id, &name, true)?;
                task
            } else {
                repo.create(
                    &name,
                    &prompt.join(" "),
                    (every_minutes * 60) as i64,
                    workspace.as_deref(),
                    start_now,
                )
                .await?
            };

            println!("Added schedule '{}'", task.name);
            println!("  Interval: every {} minute(s)", every_minutes);
            println!("  Next run: {}", format_timestamp(task.next_run_at));
            match task.target_kind {
                crate::storage::schedule::ScheduledTargetKind::Task => {
                    println!("  Target: prompt task");
                }
                crate::storage::schedule::ScheduledTargetKind::Workflow => {
                    println!(
                        "  Target: workflow {}",
                        task.target_id.as_deref().unwrap_or("<missing>")
                    );
                }
            }
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
        ScheduleAction::Show { name } => match repo.get(&name).await? {
            Some(task) => {
                println!("Schedule '{}'", task.name);
                print_schedule(&task);
            }
            None => println!("No schedule named '{}' found", name),
        },
        ScheduleAction::Pause { name } => {
            if repo.pause(&name).await? {
                println!("Paused schedule '{}'", name);
            } else {
                println!("No schedule named '{}' found", name);
            }
        }
        ScheduleAction::Resume { name } => {
            if repo.resume(&name).await? {
                match repo.get(&name).await? {
                    Some(task) => {
                        println!("Resumed schedule '{}'", name);
                        println!("  Next run: {}", format_timestamp(task.next_run_at));
                    }
                    None => println!("Resumed schedule '{}'", name),
                }
            } else {
                println!("No schedule named '{}' found", name);
            }
        }
        ScheduleAction::RunNow { name } => {
            if repo.run_now(&name).await? {
                println!("Queued '{}' for immediate execution", name);
                println!("  Run `rove start` if the daemon is not already running.");
            } else {
                println!("No schedule named '{}' found", name);
            }
        }
        ScheduleAction::Remove { name } => {
            let existing = repo.get(&name).await?;
            if repo.remove(&name).await? {
                if let Some(task) = existing {
                    if let Some(workflow_id) = task.target_id.as_deref() {
                        let spec_repo = SpecRepository::new()?;
                        let _ =
                            sync_workflow_schedule_binding(&spec_repo, workflow_id, &name, false);
                    }
                }
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
    println!(
        "    Status: {}",
        if task.enabled { "active" } else { "paused" }
    );
    println!("    Prompt: {}", task.input);
    match task.target_kind {
        crate::storage::schedule::ScheduledTargetKind::Task => {
            println!("    Target: prompt task");
        }
        crate::storage::schedule::ScheduledTargetKind::Workflow => {
            println!(
                "    Target: workflow {}",
                task.target_id.as_deref().unwrap_or("<missing>")
            );
        }
    }
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

fn sync_workflow_schedule_binding(
    repo: &SpecRepository,
    workflow_id: &str,
    schedule_name: &str,
    present: bool,
) -> Result<()> {
    let mut workflow = repo.load_workflow(workflow_id)?;
    workflow
        .schedules
        .retain(|value| !value.eq_ignore_ascii_case(schedule_name));
    if present {
        workflow.schedules.push(schedule_name.to_string());
        workflow.schedules.sort();
    }
    repo.save_workflow(&workflow)?;
    Ok(())
}
