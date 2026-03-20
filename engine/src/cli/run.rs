use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use brain::dispatch::DispatchBrain;
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
use crate::storage::{Database, PendingTaskStatus, TaskRepository};

use super::output::{OutputFormat, TaskView};
use super::task_view::{self, DispatchSummary, TaskSuccess};

pub async fn handle_run(
    task: String,
    auto_approve: bool,
    stream: bool,
    view: TaskView,
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

    let task_view = view.with_stream(stream);

    if daemon::is_running().unwrap_or(false) && !auto_approve {
        return handle_daemon_run(task, &runtime_config, format, task_view).await;
    }

    handle_local_run(task, &runtime_config, format, task_view).await
}

async fn handle_local_run(
    task: String,
    runtime_config: &Config,
    format: OutputFormat,
    view: TaskView,
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
    let dispatch = preview_dispatch(&task, view);
    task_view::print_start(&task, &task_id.to_string(), format, view, dispatch.as_ref())?;

    let result = {
        let mut task_future = std::pin::pin!(agent.process_task(agent_task));
        let mut stream_state = StreamState {
            last_progress_at: Some(Instant::now()),
            ..StreamState::default()
        };

        loop {
            tokio::select! {
                result = &mut task_future => break result,
                _ = sleep(Duration::from_millis(250)), if view.wants_progress() => {
                    stream_task_events(&event_repo, &task_id.to_string(), &mut stream_state, view).await?;
                }
            }
        }
    };

    match result {
        Ok(task_result) => {
            let completion_dispatch = dispatch.or_else(|| {
                Some(DispatchSummary::new(
                    task_result.domain.to_string().to_lowercase(),
                    infer_complexity_from_iterations(task_result.iterations).to_string(),
                    task_result.sensitive,
                    None,
                    None,
                ))
            });
            task_view::print_success(
                TaskSuccess {
                    task_id: &task_result.task_id,
                    answer: &task_result.answer,
                    provider_used: &task_result.provider_used,
                    duration_ms: task_result.duration_ms,
                    iterations: task_result.iterations,
                    dispatch: completion_dispatch.as_ref(),
                },
                format,
                view,
            )?;
            agent.drain_background_jobs().await;
            Ok(())
        }
        Err(error) => {
            task_view::print_failure(&error, format, view)?;
            Err(error)
        }
    }
}

async fn handle_daemon_run(
    task: String,
    runtime_config: &Config,
    format: OutputFormat,
    view: TaskView,
) -> Result<()> {
    let database = Arc::new(
        Database::new(&database_path(runtime_config))
            .await
            .context("Failed to open database")?,
    );
    let gateway = Gateway::new(database.clone(), GatewayConfig::from_config(runtime_config))?;

    let task_id = gateway.submit_cli(&task, None).await?;
    let dispatch = load_pending_dispatch(database.pending_tasks().get_task(&task_id).await?);
    task_view::print_start(&task, &task_id, format, view, dispatch.as_ref())?;
    let mut stream_state = StreamState::default();
    let started = Instant::now();
    let task_repo = database.tasks();

    loop {
        let pending = database.pending_tasks().get_task(&task_id).await?;
        let Some(pending) = pending else {
            anyhow::bail!("Task {} disappeared before completion", task_id);
        };

        maybe_print_status_change(&pending.status, &mut stream_state, format, view)?;

        if view.wants_progress() {
            stream_task_events(&task_repo, &task_id, &mut stream_state, view).await?;
        }

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
                task_view::print_success(
                    TaskSuccess {
                        task_id: &task_id,
                        answer: &answer,
                        provider_used: provider.as_deref().unwrap_or("unknown"),
                        duration_ms: duration_ms
                            .unwrap_or_else(|| started.elapsed().as_millis() as i64),
                        iterations: 0,
                        dispatch: dispatch.as_ref(),
                    },
                    format,
                    view,
                )?;
                return Ok(());
            }
            PendingTaskStatus::Failed => {
                let error = pending.error.unwrap_or_else(|| "Task failed".to_string());
                task_view::print_failure(&anyhow::anyhow!(error.clone()), format, view)?;
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

#[derive(Default)]
struct StreamState {
    seen_events: usize,
    last_progress_at: Option<Instant>,
    last_status: Option<PendingTaskStatus>,
}

async fn stream_task_events(
    task_repo: &TaskRepository,
    task_id: &str,
    state: &mut StreamState,
    view: TaskView,
) -> Result<()> {
    let events = task_repo
        .get_agent_events(task_id)
        .await
        .unwrap_or_default();
    if state.seen_events < events.len() {
        for event in events.iter().skip(state.seen_events) {
            task_view::print_stream_event(event, view);
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
        match view {
            TaskView::Logs => println!("[wait] awaiting task progress"),
            TaskView::Live => println!("Waiting for task progress..."),
            TaskView::Clean | TaskView::Gist => {}
        }
        state.last_progress_at = Some(Instant::now());
    }

    Ok(())
}

fn maybe_print_status_change(
    status: &PendingTaskStatus,
    state: &mut StreamState,
    format: OutputFormat,
    view: TaskView,
) -> Result<()> {
    if state.last_status.as_ref() == Some(status) {
        return Ok(());
    }

    task_view::print_status_change(status.clone(), format, view)?;
    state.last_status = Some(status.clone());
    state.last_progress_at = Some(Instant::now());
    Ok(())
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

fn preview_dispatch(task: &str, view: TaskView) -> Option<DispatchSummary> {
    if matches!(view, TaskView::Gist) {
        return None;
    }

    let brain = DispatchBrain::init().ok()?;
    let dispatch = brain.classify(task);
    Some(DispatchSummary::new(
        dispatch.domain_label,
        format!("{:?}", dispatch.complexity).to_lowercase(),
        dispatch.sensitive,
        Some(dispatch.domain_confidence),
        Some(format!("{:?}", dispatch.route).to_lowercase()),
    ))
}

fn load_pending_dispatch(pending: Option<crate::storage::PendingTask>) -> Option<DispatchSummary> {
    pending
        .map(|task| DispatchSummary::new(task.domain, task.complexity, task.sensitive, None, None))
}

fn infer_complexity_from_iterations(iterations: usize) -> &'static str {
    match iterations {
        0 | 1 => "simple",
        2..=4 => "medium",
        _ => "complex",
    }
}
