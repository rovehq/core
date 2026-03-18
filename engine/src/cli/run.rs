use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::json;
use tokio::time::sleep;
use uuid::Uuid;

use crate::agent::AgentCore;
use crate::api::gateway::{Gateway, GatewayConfig, Task, WorkspaceLocks};
use crate::cli::daemon;
use crate::cli::database_path::database_path;
use crate::config::Config;
use crate::llm::router::LLMRouter;
use crate::memory::conductor::MemorySystem;
use crate::security::rate_limiter::RateLimiter;
use crate::security::risk_assessor::RiskAssessor;
use crate::steering::loader::SteeringEngine;
use crate::storage::{AgentEvent, Database, PendingTaskStatus, TaskRepository};

use super::output::OutputFormat;

pub async fn handle_run(
    task: String,
    auto_approve: bool,
    stream: bool,
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

    if daemon::is_running().unwrap_or(false) && !auto_approve {
        return handle_daemon_run(task, stream, &runtime_config, format).await;
    }

    handle_local_run(task, stream, &runtime_config, format).await
}

async fn handle_local_run(
    task: String,
    stream: bool,
    runtime_config: &Config,
    format: OutputFormat,
) -> Result<()> {
    let database = Database::new(&database_path(runtime_config))
        .await
        .context("Failed to open database")?;
    let db_pool = database.pool().clone();

    let (providers, local_brain) = super::bootstrap::build_providers(runtime_config).await?;
    let router = Arc::new(LLMRouter::with_local_brain(
        providers,
        Arc::new(runtime_config.llm.clone()),
        local_brain,
    ));
    let rate_limiter = Arc::new(RateLimiter::new(db_pool.clone()));
    let risk_assessor = RiskAssessor::new();
    let task_repo = Arc::new(TaskRepository::new(db_pool.clone()));
    let event_repo = task_repo.clone();
    let tools = super::bootstrap::build_tools(&database, runtime_config).await?;
    let steering = load_steering(runtime_config).await;
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
    let task_id = agent_task.id;
    print_start(&task, format)?;

    let result = {
        let mut task_future = std::pin::pin!(agent.process_task(agent_task));
        let mut stream_state = StreamState::default();

        loop {
            tokio::select! {
                result = &mut task_future => break result,
                _ = sleep(Duration::from_millis(250)), if stream => {
                    stream_task_events(&event_repo, &task_id.to_string(), &mut stream_state).await?;
                }
            }
        }
    };

    match result {
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

async fn handle_daemon_run(
    task: String,
    stream: bool,
    runtime_config: &Config,
    format: OutputFormat,
) -> Result<()> {
    let database = Arc::new(
        Database::new(&database_path(runtime_config))
            .await
            .context("Failed to open database")?,
    );
    let gateway = Gateway::new(database.clone(), GatewayConfig::from_config(runtime_config))?;

    print_start(&task, format)?;
    let task_id = gateway.submit_cli(&task, None).await?;
    let mut stream_state = StreamState::default();
    let started = Instant::now();
    let task_repo = database.tasks();

    loop {
        if stream {
            stream_task_events(&task_repo, &task_id, &mut stream_state).await?;
        }

        let pending = database.pending_tasks().get_task(&task_id).await?;
        let Some(pending) = pending else {
            anyhow::bail!("Task {} disappeared before completion", task_id);
        };

        match pending.status {
            PendingTaskStatus::Pending | PendingTaskStatus::Running => {
                sleep(Duration::from_millis(250)).await;
            }
            PendingTaskStatus::Done => {
                let answer = database
                    .tasks()
                    .get_latest_answer(&task_id)
                    .await?
                    .unwrap_or_else(|| "Task completed".to_string());
                let (provider, duration_ms) = load_task_details(&task_repo, &task_id).await?;
                print_success(
                    &task_id,
                    &answer,
                    provider.as_deref().unwrap_or("unknown"),
                    duration_ms.unwrap_or_else(|| started.elapsed().as_millis() as i64),
                    0,
                    format,
                )?;
                return Ok(());
            }
            PendingTaskStatus::Failed => {
                let error = pending.error.unwrap_or_else(|| "Task failed".to_string());
                print_failure(&anyhow::anyhow!(error.clone()), format)?;
                anyhow::bail!(error);
            }
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

#[derive(Default)]
struct StreamState {
    seen_events: usize,
    last_progress_at: Option<Instant>,
}

async fn stream_task_events(
    task_repo: &TaskRepository,
    task_id: &str,
    state: &mut StreamState,
) -> Result<()> {
    let events = task_repo
        .get_agent_events(task_id)
        .await
        .unwrap_or_default();
    if state.seen_events < events.len() {
        for event in events.iter().skip(state.seen_events) {
            print_stream_event(event);
        }
        state.seen_events = events.len();
        state.last_progress_at = Some(Instant::now());
        return Ok(());
    }

    let should_ping = state
        .last_progress_at
        .map(|instant| instant.elapsed() >= Duration::from_secs(1))
        .unwrap_or(true);
    if should_ping {
        println!("...waiting for task progress");
        state.last_progress_at = Some(Instant::now());
    }

    Ok(())
}

fn print_stream_event(event: &AgentEvent) {
    match event.event_type.as_str() {
        "tool_call" => println!("tool: {}", event.payload),
        "observation" => println!("observation: {}", summarize_line(&event.payload)),
        "error" => println!("error: {}", summarize_line(&event.payload)),
        _ => {}
    }
}

fn summarize_line(text: &str) -> String {
    let single_line = text.replace('\n', " ");
    if single_line.len() > 120 {
        format!("{}...", &single_line[..117])
    } else {
        single_line
    }
}

async fn load_task_details(
    task_repo: &TaskRepository,
    task_id: &str,
) -> Result<(Option<String>, Option<i64>)> {
    let task_uuid = Uuid::parse_str(task_id).context("Invalid task id")?;
    let task = task_repo.get_task(&task_uuid).await?;
    Ok(task
        .map(|task| (task.provider_used, task.duration_ms))
        .unwrap_or((None, None)))
}
