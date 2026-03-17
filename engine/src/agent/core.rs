//! Agent Core
//!
//! Coordinates task persistence, background memory extraction, and the
//! iterative ReAct loop implemented in the child modules below.

mod events;
mod prompt;
mod result;
mod r#loop;
#[cfg(test)]
mod tests;
mod tools;

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::builtin_tools::ToolRegistry;
use crate::conductor::MemorySystem;
use crate::db::tasks::{TaskRepository, TaskStatus};
use crate::gateway::{Task, WorkspaceLocks};
use crate::injection_detector::InjectionDetector;
use crate::llm::router::LLMRouter;
use crate::rate_limiter::RateLimiter;
use crate::risk_assessor::{OperationSource, RiskAssessor};
use crate::steering::loader::SteeringEngine;

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
}

impl AgentCore {
    /// Create a new agent core.
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
        let injection_detector = InjectionDetector::new()
            .map_err(|error| anyhow::anyhow!("Failed to initialize injection detector: {}", error))?;
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
        })
    }

    /// Set the episodic memory system after the engine wiring is available.
    pub fn set_memory_system(&mut self, memory: Arc<MemorySystem>) {
        self.memory_system = Some(memory);
    }

    /// Process a task through the agent loop.
    pub async fn process_task(&mut self, task: Task) -> Result<TaskResult> {
        let task_id = task.id;
        let task_id_str = task_id.to_string();
        let task_input = task.input.clone();

        info!("Starting task {}: {}", task_id, task.input);

        self.task_repo
            .create_task(&task_id, &task.input)
            .await
            .context("Failed to create task in database")?;
        self.task_repo
            .update_task_status(&task_id, TaskStatus::Running)
            .await
            .context("Failed to update task status")?;

        let result = self.execute_task_loop(&task_id, task).await;

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

                error!("Task {} failed: {}", task_id, error);
                Err(error)
            }
        }
    }

    fn spawn_post_task_jobs(&self, task_input: String, answer: String, task_id: String) {
        if let Some(memory_system) = self.memory_system.clone() {
            let memory_task_input = task_input.clone();
            let memory_answer = answer.clone();
            let memory_task_id = task_id.clone();

            tokio::spawn(async move {
                use crate::conductor::types::TaskDomain;

                if let Err(error) = memory_system
                    .ingest(
                        &memory_task_input,
                        &memory_answer,
                        &memory_task_id,
                        &TaskDomain::General,
                        false,
                    )
                    .await
                {
                    warn!(task_id = %memory_task_id, "Memory ingest failed (non-fatal): {}", error);
                }
            });
        }

        let prefs_manager = self.preferences_manager.clone();
        tokio::spawn(async move {
            if let Err(error) = prefs_manager.extract_and_update(&task_input, &answer).await {
                warn!(task_id = %task_id, "Preferences extraction failed (non-fatal): {}", error);
            }
        });
    }
}
