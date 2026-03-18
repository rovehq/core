use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::json;

use crate::agent::AgentCore;
use crate::api::gateway::{Task, WorkspaceLocks};
use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::llm::router::LLMRouter;
use crate::memory::conductor::MemorySystem;
use crate::security::rate_limiter::RateLimiter;
use crate::security::risk_assessor::RiskAssessor;
use crate::steering::loader::SteeringEngine;
use crate::storage::{Database, TaskRepository};

use super::output::OutputFormat;

pub async fn handle_run(
    task: String,
    auto_approve: bool,
    config: &Config,
    format: OutputFormat,
) -> Result<()> {
    let mut runtime_config = config.clone();
    if let Ok(current_dir) = std::env::current_dir() {
        runtime_config.core.workspace = current_dir;
    }
    if auto_approve {
        runtime_config.security.confirm_tier1 = false;
        runtime_config.security.require_explicit_tier2 = false;
    }

    let database = Database::new(&database_path(&runtime_config))
        .await
        .context("Failed to open database")?;
    let db_pool = database.pool().clone();

    let (providers, local_brain) = super::bootstrap::build_providers(&runtime_config).await?;
    let router = Arc::new(LLMRouter::with_local_brain(
        providers,
        Arc::new(runtime_config.llm.clone()),
        local_brain,
    ));
    let rate_limiter = Arc::new(RateLimiter::new(db_pool.clone()));
    let risk_assessor = RiskAssessor::new();
    let task_repo = Arc::new(TaskRepository::new(db_pool.clone()));
    let tools = super::bootstrap::build_tools(&database, &runtime_config).await?;
    let steering = load_steering(&runtime_config).await;
    let workspace_locks = Arc::new(WorkspaceLocks::new());
    let memory_system = Arc::new(MemorySystem::new_with_config(
        db_pool,
        router.clone(),
        runtime_config.memory.clone(),
    ));

    let mut agent = AgentCore::new(
        router,
        risk_assessor,
        rate_limiter,
        task_repo,
        tools,
        steering,
        Arc::new(runtime_config.clone()),
        workspace_locks,
    )?;
    agent.set_memory_system(memory_system);

    let agent_task = Task::build_from_cli(task.clone());
    print_start(&task, format)?;

    match agent.process_task(agent_task).await {
        Ok(task_result) => {
            print_success(
                &task_result.task_id,
                &task_result.answer,
                &task_result.provider_used,
                task_result.duration_ms,
                task_result.iterations,
                format,
            )?;
            agent.drain_background_jobs().await;
            Ok(())
        }
        Err(error) => {
            print_failure(&error, format)?;
            Err(error)
        }
    }
}

async fn load_steering(config: &Config) -> Option<SteeringEngine> {
    if !config.steering.auto_detect {
        return None;
    }

    let skill_dir = expand_skill_dir(&config.steering.skill_dir);
    let workspace_dir = config.core.workspace.join(".rove").join("steering");
    match SteeringEngine::new_with_workspace(&skill_dir, Some(&workspace_dir)).await {
        Ok(engine) => Some(engine),
        Err(error) => {
            tracing::warn!("Failed to load steering engine: {}", error);
            None
        }
    }
}

fn expand_skill_dir(skill_dir: &Path) -> std::path::PathBuf {
    let raw = skill_dir.to_string_lossy();
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }

    skill_dir.to_path_buf()
}

fn print_start(task: &str, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("Executing task: {}", task);
            println!();
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "running",
                    "task": task,
                }))?
            );
        }
    }

    Ok(())
}

fn print_success(
    task_id: &str,
    answer: &str,
    provider_used: &str,
    duration_ms: i64,
    iterations: usize,
    format: OutputFormat,
) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("Result:");
            println!("{}", answer);
            println!();
            println!("Task completed successfully");
            println!("  Task ID: {}", task_id);
            println!("  Provider: {}", provider_used);
            println!("  Duration: {}ms", duration_ms);
            println!("  Iterations: {}", iterations);
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "completed",
                    "task_id": task_id,
                    "answer": answer,
                    "provider": provider_used,
                    "duration_ms": duration_ms,
                    "iterations": iterations,
                }))?
            );
        }
    }

    Ok(())
}

fn print_failure(error: &anyhow::Error, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("Task failed: {}", error);
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "status": "failed",
                    "error": error.to_string(),
                }))?
            );
        }
    }

    Ok(())
}
