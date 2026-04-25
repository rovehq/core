use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use brain::dispatch::DispatchBrain;
use sdk::{RunContextId, RunIsolation, RunMode, TaskExecutionProfile, TaskSource};
use tempfile::TempDir;
use tokio::time::sleep;
use uuid::Uuid;

use crate::agent::{AgentCore, TaskResult};
use crate::api::gateway::{Gateway, GatewayConfig, Task, WorkspaceLocks};
use crate::cli::database_path::database_path;
use crate::cli::{daemon, TaskIsolationArg};
use crate::config::Config;
use crate::llm::router::LLMRouter;
use crate::memory::conductor::MemorySystem;
use crate::policy::PolicyEngine;
use crate::policy::{
    active_workspace_policy_dir, legacy_policy_workspace_dir, policy_workspace_dir,
};
use crate::remote::{RemoteManager, RemoteSendOptions};
use crate::security::rate_limiter::RateLimiter;
use crate::security::risk_assessor::RiskAssessor;
use crate::security::PromptOverrideDetector;
use crate::storage::{Database, PendingTaskStatus, TaskRepository};
use crate::targeting::extract_task_target;
use crate::zerotier::ZeroTierManager;

use super::agents::execution_profile_for_agent;
use super::output::{OutputFormat, TaskView};
use super::task_view::{self, DispatchSummary, TaskSuccess};

pub struct RunRequest {
    pub task: String,
    pub agent: Option<String>,
    pub node: Option<String>,
    pub auto_approve: bool,
    pub stream: bool,
    pub parallel: bool,
    pub isolate: Option<TaskIsolationArg>,
    pub view: TaskView,
    pub format: OutputFormat,
}

pub async fn handle_run(request: RunRequest, config: &Config) -> Result<()> {
    let mut runtime_config = config.clone();
    if let Ok(current_dir) = std::env::current_dir() {
        runtime_config.core.workspace = current_dir;
    }
    let (task, implicit_node) = extract_task_target(&request.task);
    let task = PromptOverrideDetector::new()?.guard_input(&task);
    let requested_node = request.node.clone().or(implicit_node);
    if request.auto_approve {
        runtime_config.security.confirm_tier1 = false;
        runtime_config.security.require_explicit_tier2 = false;
    }

    let task_view = request.view.with_stream(request.stream);
    let run_mode = if request.parallel {
        RunMode::Parallel
    } else {
        RunMode::Serial
    };
    let run_isolation = match request.isolate {
        Some(TaskIsolationArg::Worktree) => RunIsolation::Worktree,
        Some(TaskIsolationArg::Snapshot) => RunIsolation::Snapshot,
        None => RunIsolation::None,
    };
    let execution_profile = if let Some(agent_id) = request.agent.as_deref() {
        let repo = crate::system::specs::SpecRepository::new()?;
        Some(execution_profile_for_agent(&repo, agent_id)?)
    } else {
        None
    };

    if let Some(node) = requested_node {
        if execution_profile.is_some() {
            anyhow::bail!("`rove task --agent` is not supported for remote node dispatch yet");
        }
        return handle_remote_run(task, &node, &runtime_config, request.format, task_view).await;
    }

    if daemon::is_running().unwrap_or(false)
        && !request.auto_approve
        && !request.parallel
        && request.isolate.is_none()
    {
        return handle_daemon_run(
            task,
            execution_profile,
            &runtime_config,
            request.format,
            task_view,
        )
        .await;
    }

    handle_local_run(
        task,
        execution_profile,
        &runtime_config,
        request.format,
        task_view,
        run_mode,
        run_isolation,
    )
    .await
}

async fn handle_local_run(
    task: String,
    execution_profile: Option<TaskExecutionProfile>,
    runtime_config: &Config,
    format: OutputFormat,
    view: TaskView,
    run_mode: RunMode,
    run_isolation: RunIsolation,
) -> Result<()> {
    let task_id = Uuid::new_v4();
    let dispatch = preview_dispatch(&task, view);
    task_view::print_start(&task, &task_id.to_string(), format, view, dispatch.as_ref())?;

    // For live streaming: wire a channel from the LLM sink to stdout.
    let stream_printer = if matches!(view, TaskView::Live) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        crate::llm::set_stream_sink(tx);
        let handle = tokio::spawn(async move {
            while let Some(chunk) = rx.recv().await {
                print!("{chunk}");
                let _ = std::io::stdout().flush();
            }
        });
        Some(handle)
    } else {
        None
    };

    let result = execute_local_task_request_with_source_and_id(
        task.clone(),
        runtime_config,
        run_mode,
        run_isolation,
        execution_profile,
        TaskSource::Cli,
        task_id,
    )
    .await;

    if stream_printer.is_some() {
        crate::llm::clear_stream_sink();
        if let Some(handle) = stream_printer {
            let _ = handle.await;
        }
        println!();
    }

    // Show agent events (thoughts, tool calls, observations) for --view live/logs.
    // Local runs don't have a polling loop, so we print them after completion.
    if view.wants_progress() {
        let db_path = database_path(runtime_config);
        if let Ok(db) = Database::new(&db_path).await {
            let task_repo = db.tasks();
            let events = task_repo
                .get_agent_events(&task_id.to_string())
                .await
                .unwrap_or_default();
            for event in &events {
                task_view::print_stream_event(event, view);
            }
        }
    }

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
            Ok(())
        }
        Err(error) => {
            task_view::print_failure(&error, format, view)?;
            Err(error)
        }
    }
}

pub async fn execute_local_task_request(
    task: String,
    runtime_config: &Config,
    run_mode: RunMode,
    run_isolation: RunIsolation,
    execution_profile: Option<TaskExecutionProfile>,
) -> Result<TaskResult> {
    execute_local_task_request_with_source_and_id(
        task,
        runtime_config,
        run_mode,
        run_isolation,
        execution_profile,
        TaskSource::Cli,
        Uuid::new_v4(),
    )
    .await
}

pub async fn execute_local_task_request_with_source(
    task: String,
    runtime_config: &Config,
    run_mode: RunMode,
    run_isolation: RunIsolation,
    execution_profile: Option<TaskExecutionProfile>,
    source: TaskSource,
) -> Result<TaskResult> {
    execute_local_task_request_with_source_and_id(
        task,
        runtime_config,
        run_mode,
        run_isolation,
        execution_profile,
        source,
        Uuid::new_v4(),
    )
    .await
}

async fn execute_local_task_request_with_source_and_id(
    task: String,
    runtime_config: &Config,
    run_mode: RunMode,
    run_isolation: RunIsolation,
    execution_profile: Option<TaskExecutionProfile>,
    source: TaskSource,
    task_id: Uuid,
) -> Result<TaskResult> {
    let prepared_workspace = prepare_run_workspace(runtime_config, &task, run_mode, run_isolation)?;
    let mut runtime_config = runtime_config.clone();
    runtime_config.core.workspace = prepared_workspace.workspace.clone();

    let database = Database::new(&database_path(&runtime_config))
        .await
        .context("Failed to open database")?;
    let db_pool = database.pool().clone();

    let tools = super::bootstrap::build_tools(&database, &runtime_config).await?;
    let plugin_brain = tools.plugin_brain();

    let (providers, local_brain) = super::bootstrap::build_providers(&runtime_config).await?;
    let router = Arc::new(
        LLMRouter::with_local_brain(
            providers,
            Arc::new(runtime_config.llm.clone()),
            local_brain,
        )
        .with_plugin_brain(plugin_brain),
    );
    let rate_limiter = Arc::new(RateLimiter::new(db_pool.clone()));
    let risk_assessor = RiskAssessor::new();
    let task_repo = Arc::new(TaskRepository::new(db_pool.clone()));
    let policy_engine = load_policy_engine(&runtime_config).await;
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
        policy_engine,
        Arc::new(runtime_config.clone()),
        workspace_locks,
    )?;
    agent.set_memory_system(memory_system);

    let mut agent_task = Task {
        id: task_id,
        input: task,
        source,
        execution_profile: None,
        risk_tier_override: None,
        run_context_id: RunContextId(Uuid::new_v4().to_string()),
        run_mode,
        run_isolation,
        session_id: None,
        workspace: Some(prepared_workspace.workspace.clone()),
        created_at: chrono::Utc::now().timestamp(),
    };
    if let Some(profile) = execution_profile {
        agent_task = agent_task.with_execution_profile(profile);
    }

    let result = agent.process_task(agent_task).await;
    agent.drain_background_jobs().await;
    prepared_workspace.cleanup()?;
    result
}

struct PreparedRunWorkspace {
    workspace: PathBuf,
    cleanup: Option<WorkspaceCleanup>,
    #[allow(dead_code)]
    temp_dir: Option<TempDir>,
}

enum WorkspaceCleanup {
    Worktree { source: PathBuf, target: PathBuf },
}

impl PreparedRunWorkspace {
    fn cleanup(&self) -> Result<()> {
        if let Some(cleanup) = &self.cleanup {
            match cleanup {
                WorkspaceCleanup::Worktree { source, target } => {
                    let output = Command::new("git")
                        .arg("-C")
                        .arg(source)
                        .args(["worktree", "remove", "--force"])
                        .arg(target)
                        .output()
                        .context("Failed to spawn git worktree remove command")?;
                    if !output.status.success() {
                        anyhow::bail!(
                            "git worktree remove failed: {}",
                            String::from_utf8_lossy(&output.stderr).trim()
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

fn prepare_run_workspace(
    config: &Config,
    task: &str,
    run_mode: RunMode,
    run_isolation: RunIsolation,
) -> Result<PreparedRunWorkspace> {
    let workspace = config.core.workspace.clone();
    if !matches!(run_mode, RunMode::Parallel) {
        return Ok(PreparedRunWorkspace {
            workspace,
            cleanup: None,
            temp_dir: None,
        });
    }

    let write_heavy = task_likely_writes_workspace(task);
    if !write_heavy {
        return Ok(PreparedRunWorkspace {
            workspace,
            cleanup: None,
            temp_dir: None,
        });
    }

    match run_isolation {
        RunIsolation::None => anyhow::bail!(
            "Parallel write-heavy tasks require explicit isolation. Re-run with `--isolate=worktree` for git repositories or `--isolate=snapshot` for non-git workspaces."
        ),
        RunIsolation::Worktree => prepare_worktree_workspace(&workspace),
        RunIsolation::Snapshot => prepare_snapshot_workspace(&workspace),
    }
}

fn prepare_worktree_workspace(workspace: &Path) -> Result<PreparedRunWorkspace> {
    if !workspace.join(".git").exists() {
        anyhow::bail!(
            "Worktree isolation requires a git repository at '{}'. Use `--isolate=snapshot` instead.",
            workspace.display()
        );
    }

    let temp_dir = tempfile::tempdir().context("Failed to create worktree temp directory")?;
    let target = temp_dir.path().join("workspace");
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["worktree", "add", "--detach"])
        .arg(&target)
        .arg("HEAD")
        .output()
        .context("Failed to spawn git worktree command")?;
    if !output.status.success() {
        anyhow::bail!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(PreparedRunWorkspace {
        workspace: target,
        cleanup: Some(WorkspaceCleanup::Worktree {
            source: workspace.to_path_buf(),
            target: temp_dir.path().join("workspace"),
        }),
        temp_dir: Some(temp_dir),
    })
}

fn prepare_snapshot_workspace(workspace: &Path) -> Result<PreparedRunWorkspace> {
    let temp_dir = tempfile::tempdir().context("Failed to create snapshot temp directory")?;
    let target = temp_dir.path().join("workspace");
    copy_workspace_snapshot(workspace, &target)?;

    Ok(PreparedRunWorkspace {
        workspace: target,
        cleanup: None,
        temp_dir: Some(temp_dir),
    })
}

fn copy_workspace_snapshot(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name == ".git" || name == "target" || name == "node_modules" {
            continue;
        }
        let destination = target.join(file_name);
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            copy_workspace_snapshot(&path, &destination)?;
        } else if metadata.is_file() {
            fs::copy(&path, &destination)?;
        }
    }
    Ok(())
}

fn task_likely_writes_workspace(task: &str) -> bool {
    let task = task.to_ascii_lowercase();
    [
        "write", "edit", "update", "modify", "create", "delete", "remove", "rename", "refactor",
        "commit", "apply", "fix", "change",
    ]
    .iter()
    .any(|needle| task.contains(needle))
}

async fn handle_daemon_run(
    task: String,
    execution_profile: Option<TaskExecutionProfile>,
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

    let task_id = gateway
        .submit_cli(&task, None, execution_profile.as_ref())
        .await?;
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

async fn handle_remote_run(
    task: String,
    node: &str,
    runtime_config: &Config,
    format: OutputFormat,
    view: TaskView,
) -> Result<()> {
    let dispatch = preview_dispatch(&task, view);
    task_view::print_start(&task, "remote", format, view, dispatch.as_ref())?;

    let manager = RemoteManager::new(runtime_config.clone());
    let mut result = manager
        .send_with_options(
            &task,
            RemoteSendOptions {
                node: Some(node.to_string()),
                ..RemoteSendOptions::default()
            },
        )
        .await;

    if result.is_err() && runtime_config.remote.transports.zerotier.enabled {
        let _ = ZeroTierManager::new(runtime_config.clone()).refresh().await;
        result = manager
            .send_with_options(
                &task,
                RemoteSendOptions {
                    node: Some(node.to_string()),
                    ..RemoteSendOptions::default()
                },
            )
            .await;
    }

    let result = result?;
    task_view::print_success(
        TaskSuccess {
            task_id: &result.remote_task_id,
            answer: result
                .answer
                .as_deref()
                .or(result.message.as_deref())
                .unwrap_or("Remote task completed"),
            provider_used: result.provider.as_deref().unwrap_or("remote"),
            duration_ms: result.duration_ms.unwrap_or(0),
            iterations: 0,
            dispatch: dispatch.as_ref(),
        },
        format,
        view,
    )?;
    Ok(())
}

async fn load_policy_engine(config: &Config) -> Option<PolicyEngine> {
    if !config.policy.auto_detect {
        return None;
    }

    let policy_dir = expand_policy_dir(config.policy.policy_dir());
    let primary_workspace_dir = policy_workspace_dir(&config.core.workspace);
    let legacy_workspace_dir = legacy_policy_workspace_dir(&config.core.workspace);
    let workspace_dir = active_workspace_policy_dir(&primary_workspace_dir, &legacy_workspace_dir);
    match PolicyEngine::new_with_workspace(&policy_dir, Some(&workspace_dir)).await {
        Ok(engine) => Some(engine),
        Err(error) => {
            tracing::warn!("Failed to load policy engine: {}", error);
            None
        }
    }
}

fn expand_policy_dir(policy_dir: &Path) -> std::path::PathBuf {
    let raw = policy_dir.to_string_lossy();
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }

    policy_dir.to_path_buf()
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
