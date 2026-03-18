//! Agent Core
//!
//! Coordinates task persistence, background memory extraction, and the
//! iterative ReAct loop implemented in the child modules below.

mod events;
mod r#loop;
mod prompt;
mod result;
mod shortcuts;
#[cfg(test)]
mod tests;
mod tools;

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::builtin_tools::ToolRegistry;
use crate::conductor::MemorySystem;
use crate::db::tasks::{TaskRepository, TaskStatus};
use crate::gateway::{Task, WorkspaceLocks};
use crate::injection_detector::InjectionDetector;
use crate::llm::router::LLMRouter;
use crate::rate_limiter::RateLimiter;
use crate::risk_assessor::{OperationSource, RiskAssessor};
use crate::security::secrets::scrub_text;
use crate::steering::loader::SteeringEngine;
use sdk::TaskDomain;

use super::{preferences::PreferencesManager, WorkingMemory};

pub use result::TaskResult;

pub(crate) const LLM_TIMEOUT_SECS: u64 = 300;
pub(crate) const MAX_RESULT_SIZE: usize = 5 * 1024 * 1024;

/// Agent Core that orchestrates the agent loop.
pub struct AgentCore {
    router: Arc<LLMRouter>,
    memory: WorkingMemory,
    risk_assessor: RiskAssessor,
    rate_limiter: Arc<RateLimiter>,
    task_repo: Arc<TaskRepository>,
    tools: Arc<ToolRegistry>,
    injection_detector: InjectionDetector,
    current_source: OperationSource,
    steering: Option<SteeringEngine>,
    memory_system: Option<Arc<MemorySystem>>,
    config: Arc<crate::config::Config>,
    preferences_manager: PreferencesManager,
    dispatch_brain: brain::dispatch::DispatchBrain,
    workspace_locks: Arc<WorkspaceLocks>,
    background_jobs: Vec<JoinHandle<()>>,
    current_task_sensitive: bool,
    steering_preflight_commands: Vec<String>,
    steering_after_write_commands: Vec<String>,
    steering_executed_commands: HashSet<String>,
}

impl AgentCore {
    /// Create a new agent core.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        router: Arc<LLMRouter>,
        risk_assessor: RiskAssessor,
        rate_limiter: Arc<RateLimiter>,
        task_repo: Arc<TaskRepository>,
        tools: Arc<ToolRegistry>,
        steering: Option<SteeringEngine>,
        config: Arc<crate::config::Config>,
        workspace_locks: Arc<WorkspaceLocks>,
    ) -> Result<Self> {
        let injection_detector = InjectionDetector::new().map_err(|error| {
            anyhow::anyhow!("Failed to initialize injection detector: {}", error)
        })?;
        let dispatch_brain = brain::dispatch::DispatchBrain::init()
            .map_err(|error| anyhow::anyhow!("Failed to initialize dispatch brain: {}", error))?;

        Ok(Self {
            router: router.clone(),
            memory: WorkingMemory::new(),
            risk_assessor,
            rate_limiter,
            task_repo,
            tools,
            injection_detector,
            current_source: OperationSource::Local,
            steering,
            memory_system: None,
            config,
            preferences_manager: PreferencesManager::new(router),
            dispatch_brain,
            workspace_locks,
            background_jobs: Vec::new(),
            current_task_sensitive: false,
            steering_preflight_commands: Vec::new(),
            steering_after_write_commands: Vec::new(),
            steering_executed_commands: HashSet::new(),
        })
    }

    /// Set the episodic memory system after the engine wiring is available.
    pub fn set_memory_system(&mut self, memory: Arc<MemorySystem>) {
        self.memory_system = Some(memory);
    }

    pub fn memory_system(&self) -> Option<&Arc<MemorySystem>> {
        self.memory_system.as_ref()
    }

    /// Process a task through the agent loop.
    pub async fn process_task(&mut self, task: Task) -> Result<TaskResult> {
        self.prune_finished_background_jobs();

        let task_id = task.id;
        let task_id_str = task_id.to_string();
        let task_input = task.input.clone();

        info!("Starting task {}: {}", task_id, scrub_text(&task.input));

        self.task_repo
            .create_task(&task_id, &task.input)
            .await
            .context("Failed to create task in database")?;
        self.task_repo
            .update_task_status(&task_id, TaskStatus::Running)
            .await
            .context("Failed to update task status")?;

        let result = match self.try_shortcut_task(&task_id, &task).await {
            Ok(Some(result)) => Ok(result),
            Ok(None) => self.execute_task_loop(&task_id, task).await,
            Err(error) => Err(error),
        };

        match result {
            Ok(task_result) => {
                self.task_repo
                    .complete_task(
                        &task_id,
                        &task_result.provider_used,
                        task_result.duration_ms,
                    )
                    .await
                    .context("Failed to complete task in database")?;

                self.spawn_post_task_jobs(
                    task_input,
                    task_result.answer.clone(),
                    task_id_str.clone(),
                    task_result.domain,
                    task_result.sensitive,
                );

                info!(
                    task_id = %task_id,
                    duration_ms = task_result.duration_ms,
                    iterations = task_result.iterations,
                    "Task completed"
                );

                Ok(task_result)
            }
            Err(error) => {
                self.task_repo
                    .fail_task(&task_id)
                    .await
                    .context("Failed to mark task as failed")?;
                let _ = self
                    .task_repo
                    .insert_agent_event(
                        &task_id,
                        "error",
                        &serde_json::json!({ "error": scrub_text(&error.to_string()) }).to_string(),
                        -1,
                        None,
                    )
                    .await;

                error!(
                    "Task {} failed: {}",
                    task_id,
                    scrub_text(&error.to_string())
                );
                Err(error)
            }
        }
    }

    pub async fn drain_background_jobs(&mut self) {
        let pending = std::mem::take(&mut self.background_jobs);
        for job in pending {
            if let Err(error) = job.await {
                warn!("Background job failed to join cleanly: {}", error);
            }
        }
    }

    pub async fn active_steering_skills(&self) -> Vec<String> {
        match &self.steering {
            Some(steering) => steering.active_skills().await,
            None => Vec::new(),
        }
    }

    fn spawn_post_task_jobs(
        &mut self,
        task_input: String,
        answer: String,
        task_id: String,
        domain: TaskDomain,
        sensitive: bool,
    ) {
        if let Some(memory_system) = self.memory_system.clone() {
            let memory_task_input = task_input.clone();
            let memory_answer = answer.clone();
            let memory_task_id = task_id.clone();

            self.background_jobs.push(tokio::spawn(async move {
                if let Err(error) = memory_system
                    .ingest(
                        &memory_task_input,
                        &memory_answer,
                        &memory_task_id,
                        &domain,
                        sensitive,
                    )
                    .await
                {
                    warn!(
                        task_id = %memory_task_id,
                        "Memory ingest failed (non-fatal): {}",
                        scrub_text(&error.to_string())
                    );
                }
            }));
        }

        if !sensitive {
            let prefs_manager = self.preferences_manager.clone();
            self.background_jobs.push(tokio::spawn(async move {
                if let Err(error) = prefs_manager.extract_and_update(&task_input, &answer).await {
                    warn!(
                        task_id = %task_id,
                        "Preferences extraction failed (non-fatal): {}",
                        scrub_text(&error.to_string())
                    );
                }
            }));
        }
    }

    fn prune_finished_background_jobs(&mut self) {
        self.background_jobs.retain(|job| !job.is_finished());
    }
}
