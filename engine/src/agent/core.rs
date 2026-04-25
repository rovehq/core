//! Agent Core
//!
//! Coordinates task persistence, background memory extraction, and the
//! iterative ReAct loop implemented in the child modules below.

pub mod callable;
mod apex;
mod events;
mod r#loop;
pub mod orchestration;
pub mod prompt;
mod result;
mod shortcuts;
mod tools;

pub use orchestration::{decide_execution_strategy, ExecutionStrategy, OrchestrationHistory, OrchestrationDecision};
pub use prompt::TaskContext;

use anyhow::{Context, Result};
use sdk::TaskExecutionProfile;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinHandle;
use tracing::Instrument;
use tracing::{error, info, warn};

use crate::builtin_tools::ToolRegistry;
use crate::conductor::MemorySystem;
use crate::db::tasks::{TaskRepository, TaskStatus};
use crate::gateway::{Task, WorkspaceLocks};
use crate::hooks::{BeforeAgentStartPayload, MessageReceivedPayload, MessageSendingPayload};
use crate::llm::router::LLMRouter;
use crate::llm::ToolCall;
use crate::message_bus::{Event as BusEvent, MessageBus};
use crate::policy::PolicyEngine;
use crate::rate_limiter::RateLimiter;
use crate::risk_assessor::{OperationSource, RiskAssessor};
use crate::security::secrets::scrub_text;
use sdk::{CallableAgentSpec, RemoteExecutionPlan, TaskDomain};

use super::{preferences::PreferencesManager, WorkingMemory};

pub use result::TaskResult;

pub(crate) const LLM_TIMEOUT_SECS: u64 = 300;
pub(crate) const MAX_RESULT_SIZE: usize = 5 * 1024 * 1024;

/// Agent Core that orchestrates the agent loop.
pub struct AgentCore {
    router: Arc<LLMRouter>,
    pub memory: WorkingMemory,
    risk_assessor: RiskAssessor,
    rate_limiter: Arc<RateLimiter>,
    pub task_repo: Arc<TaskRepository>,
    tools: Arc<ToolRegistry>,
    current_source: OperationSource,
    policy_engine: Option<PolicyEngine>,
    memory_system: Option<Arc<MemorySystem>>,
    config: Arc<crate::config::Config>,
    preferences_manager: PreferencesManager,
    dispatch_brain: brain::dispatch::DispatchBrain,
    workspace_locks: Arc<WorkspaceLocks>,
    background_jobs: Vec<JoinHandle<()>>,
    current_task_sensitive: bool,
    current_execution_profile: Option<TaskExecutionProfile>,
    current_callable_roster: Vec<CallableAgentSpec>,
    current_domain: TaskDomain,
    current_trace: Option<crate::telemetry::TaskTraceContext>,
    message_bus: Option<Arc<MessageBus>>,
    policy_preflight_commands: Vec<String>,
    pub policy_after_write_commands: Vec<String>,
    policy_executed_commands: HashSet<String>,
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
        policy_engine: Option<PolicyEngine>,
        config: Arc<crate::config::Config>,
        workspace_locks: Arc<WorkspaceLocks>,
    ) -> Result<Self> {
        let dispatch_brain = brain::dispatch::DispatchBrain::init()
            .map_err(|error| anyhow::anyhow!("Failed to initialize dispatch brain: {}", error))?;

        Ok(Self {
            router: router.clone(),
            memory: WorkingMemory::new(),
            risk_assessor,
            rate_limiter,
            task_repo,
            tools,
            current_source: OperationSource::Local,
            policy_engine,
            memory_system: None,
            config,
            preferences_manager: PreferencesManager::new(router),
            dispatch_brain,
            workspace_locks,
            background_jobs: Vec::new(),
            current_task_sensitive: false,
            current_execution_profile: None,
            current_callable_roster: Vec::new(),
            current_domain: TaskDomain::General,
            current_trace: None,
            message_bus: None,
            policy_preflight_commands: Vec::new(),
            policy_after_write_commands: Vec::new(),
            policy_executed_commands: HashSet::new(),
        })
    }

    /// Set the episodic memory system after the engine wiring is available.
    pub fn set_memory_system(&mut self, memory: Arc<MemorySystem>) {
        self.memory_system = Some(memory);
    }

    pub fn set_tools(&mut self, tools: Arc<ToolRegistry>) {
        self.tools = tools;
    }

    pub fn set_message_bus(&mut self, message_bus: Arc<MessageBus>) {
        self.message_bus = Some(message_bus);
    }

    pub fn memory_system(&self) -> Option<&Arc<MemorySystem>> {
        self.memory_system.as_ref()
    }

    /// Process a task through the agent loop.
    pub async fn process_task(&mut self, mut task: Task) -> Result<TaskResult> {
        self.prune_finished_background_jobs();

        task.input = self.apply_message_received_hooks(&task).await?;
        task.input = self.apply_before_agent_start_hooks(&task).await?;

        let task_for_hooks = task.clone();
        let task_id = task.id;
        let task_id_str = task_id.to_string();
        let task_input = task.input.clone();
        let task_source = format!("{:?}", task.source);
        let trace = crate::telemetry::TaskTraceContext::new();
        let task_span = crate::telemetry::task_span(&trace, &task_id, &task_source);

        self.current_trace = Some(trace.clone());

        let result = async {
            info!(
                trace_id = %trace.trace_id,
                "Starting task {}: {}",
                task_id,
                scrub_text(&task.input)
            );

            self.task_repo
                .create_task_with_metadata(
                    &task_id,
                    &task.input,
                    Some(&task.source),
                    task.execution_profile.as_ref(),
                )
                .await
                .context("Failed to create task in database")?;
            self.task_repo
                .update_task_status(&task_id, TaskStatus::Running)
                .await
                .context("Failed to update task status")?;
            if let Some(bus) = &self.message_bus {
                bus.publish(BusEvent::TaskStarted {
                    task_id: task_id.to_string(),
                    input: scrub_text(&task.input),
                })
                .await;
            }
            self.publish_turn_start_event(&task_id, &task.input, &task_source)
                .await;

            match self.try_shortcut_task(&task_id, &task).await {
                Ok(Some(result)) => Ok(result),
                Ok(None) => self.execute_task_loop(&task_id, task).await,
                Err(error) => Err(error),
            }
        }
        .instrument(task_span)
        .await;

        let final_result = match result {
            Ok(task_result) => {
                self.emit_message_sending_hook(&task_for_hooks, &task_result.answer, "completed")
                    .await;
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
                if let Some(bus) = &self.message_bus {
                    bus.publish(BusEvent::TaskCompleted {
                        task_id: task_id.to_string(),
                        result: scrub_text(&task_result.answer),
                    })
                    .await;
                }
                self.publish_turn_end_event(
                    &task_id,
                    "completed",
                    "Task completed",
                    task_result.duration_ms,
                )
                .await;

                Ok(task_result)
            }
            Err(error) => {
                self.task_repo
                    .fail_task(&task_id)
                    .await
                    .context("Failed to mark task as failed")?;
                let _ = self
                    .insert_error_event(&task_id, &error.to_string(), "")
                    .await;

                error!(
                    "Task {} failed: {}",
                    task_id,
                    scrub_text(&error.to_string())
                );
                if let Some(bus) = &self.message_bus {
                    bus.publish(BusEvent::TaskFailed {
                        task_id: task_id.to_string(),
                        error: scrub_text(&error.to_string()),
                    })
                    .await;
                }
                self.publish_turn_end_event(&task_id, "failed", "Task failed", 0)
                    .await;
                Err(error)
            }
        };

        self.current_trace = None;
        final_result
    }

    /// Execute a coordinator-provided direct tool plan without entering the LLM loop.
    pub async fn process_planned_task(
        &mut self,
        mut task: Task,
        plan: RemoteExecutionPlan,
    ) -> Result<TaskResult> {
        self.prune_finished_background_jobs();

        task.input = self.apply_message_received_hooks(&task).await?;
        task.input = self.apply_before_agent_start_hooks(&task).await?;

        let task_for_hooks = task.clone();
        let task_id = task.id;
        let task_id_str = task_id.to_string();
        let task_input = task.input.clone();
        let task_source = format!("{:?}", task.source);
        let trace = crate::telemetry::TaskTraceContext::new();
        let task_span = crate::telemetry::task_span(&trace, &task_id, &task_source);

        self.current_trace = Some(trace.clone());

        let result = async {
            info!(
                trace_id = %trace.trace_id,
                task_id = %task_id,
                tool = %plan.primary_tool_name().unwrap_or("unknown"),
                "Starting planned task {}: {}",
                task_id,
                scrub_text(&task.input)
            );

            self.task_repo
                .create_task_with_metadata(
                    &task_id,
                    &task.input,
                    Some(&task.source),
                    task.execution_profile.as_ref(),
                )
                .await
                .context("Failed to create planned task in database")?;
            self.task_repo
                .update_task_status(&task_id, TaskStatus::Running)
                .await
                .context("Failed to update planned task status")?;
            if let Some(bus) = &self.message_bus {
                bus.publish(BusEvent::TaskStarted {
                    task_id: task_id.to_string(),
                    input: scrub_text(&task.input),
                })
                .await;
            }
            self.publish_turn_start_event(&task_id, &task.input, &task_source)
                .await;

            self.execute_planned_task_body(&task, &plan).await
        }
        .instrument(task_span)
        .await;

        let final_result = match result {
            Ok(task_result) => {
                self.emit_message_sending_hook(&task_for_hooks, &task_result.answer, "completed")
                    .await;
                self.task_repo
                    .complete_task(
                        &task_id,
                        &task_result.provider_used,
                        task_result.duration_ms,
                    )
                    .await
                    .context("Failed to complete planned task in database")?;

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
                    tool = %plan.primary_tool_name().unwrap_or("unknown"),
                    "Planned task completed"
                );
                if let Some(bus) = &self.message_bus {
                    bus.publish(BusEvent::TaskCompleted {
                        task_id: task_id.to_string(),
                        result: scrub_text(&task_result.answer),
                    })
                    .await;
                }
                self.publish_turn_end_event(
                    &task_id,
                    "completed",
                    "Task completed",
                    task_result.duration_ms,
                )
                .await;

                Ok(task_result)
            }
            Err(error) => {
                self.task_repo
                    .fail_task(&task_id)
                    .await
                    .context("Failed to mark planned task as failed")?;
                let _ = self
                    .insert_error_event(&task_id, &error.to_string(), "")
                    .await;

                error!(
                    task_id = %task_id,
                    tool = %plan.primary_tool_name().unwrap_or("unknown"),
                    "Planned task {} failed: {}",
                    task_id,
                    scrub_text(&error.to_string())
                );
                if let Some(bus) = &self.message_bus {
                    bus.publish(BusEvent::TaskFailed {
                        task_id: task_id.to_string(),
                        error: scrub_text(&error.to_string()),
                    })
                    .await;
                }
                self.publish_turn_end_event(&task_id, "failed", "Task failed", 0)
                    .await;
                Err(error)
            }
        };

        self.current_trace = None;
        final_result
    }

    pub async fn drain_background_jobs(&mut self) {
        let pending = std::mem::take(&mut self.background_jobs);
        for job in pending {
            if let Err(error) = job.await {
                warn!("Background job failed to join cleanly: {}", error);
            }
        }
    }

    pub async fn active_policies(&self) -> Vec<String> {
        match &self.policy_engine {
            Some(policy_engine) => policy_engine.active_policies().await,
            None => Vec::new(),
        }
    }

    async fn execute_planned_task_body(
        &mut self,
        task: &Task,
        plan: &RemoteExecutionPlan,
    ) -> Result<TaskResult> {
        let steps = plan.steps();
        if steps.is_empty() {
            anyhow::bail!("Remote execution plan contains no executable steps");
        }

        let start_time = Instant::now();
        let task_id = task.id;
        let task_id_str = task_id.to_string();

        self.current_source = task.source.clone().into();

        let operation = crate::risk_assessor::Operation::new(
            "execute_task",
            vec![],
            task.source.clone().into(),
        );
        let mut risk_tier = self
            .risk_assessor
            .assess(&operation)
            .context("Failed to assess risk tier")?;
        if let Some(override_tier) = task.risk_tier_override {
            risk_tier = override_tier;
        }

        self.rate_limiter
            .check_limit(&task_id_str, risk_tier)
            .await
            .context("Rate limit exceeded")?;
        self.rate_limiter
            .record_operation(&task_id_str, risk_tier)
            .await
            .context("Failed to record operation")?;

        let context = self.initialize_task_context(task, risk_tier).await?;
        self.insert_user_event(&task_id, &task.input, &context.domain_str)
            .await?;
        self.insert_thought_event(
            &task_id,
            &format!("Execution strategy: direct remote plan ({})", plan.summary),
            &context.domain_str,
        )
        .await?;
        self.run_policy_preflight(&task_id, &context.domain_str)
            .await?;

        let mut step_outputs = Vec::with_capacity(steps.len());
        for (index, step) in steps.iter().enumerate() {
            let step_num = index + 1;
            let tool_call = ToolCall::new(
                format!("remote-plan-{}-{}", task_id, step_num),
                &step.tool_name,
                serde_json::to_string(&step.tool_args)
                    .context("Failed to serialize plan tool args")?,
            );
            self.memory
                .add_message(self.assistant_tool_message(&task_id, &tool_call));
            self.insert_tool_call_event(&task_id, &tool_call, step_num, &context.domain_str)
                .await?;

            let execution = self.execute_tool_call(&task_id_str, &tool_call).await?;
            self.memory.add_message(crate::llm::Message::tool_result(
                &execution.safe_result,
                &tool_call.id,
            ));
            self.insert_observation_event(
                &task_id,
                &execution.safe_result,
                step_num,
                &context.domain_str,
            )
            .await?;
            self.run_policy_after_write(&task_id, step_num, &tool_call.name, &context.domain_str)
                .await?;

            step_outputs.push((step.summary.clone(), execution.safe_result));
        }

        let answer = render_planned_task_answer(&step_outputs);
        self.insert_answer_event(&task_id, &answer, steps.len(), &context.domain_str)
            .await?;

        Ok(TaskResult::success(
            task_id.to_string(),
            answer,
            "executor-plan".to_string(),
            start_time.elapsed().as_millis() as i64,
            steps.len(),
            context.domain,
            context.sensitive,
        ))
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
            if memory_system.config().always_on_enabled()
                || memory_system.config().should_persist_pinned_facts()
            {
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

    async fn apply_message_received_hooks(&self, task: &Task) -> Result<String> {
        Ok(self
            .tools
            .message_received(MessageReceivedPayload {
                event: "MessageReceived",
                task_id: task.id.to_string(),
                input: task.input.clone(),
                task_source: crate::hooks::task_source_label(&task.source),
                workspace: self.config.core.workspace.display().to_string(),
                session_id: task.session_id.map(|value| value.to_string()),
            })
            .await?
            .text)
    }

    async fn apply_before_agent_start_hooks(&self, task: &Task) -> Result<String> {
        Ok(self
            .tools
            .before_agent_start(BeforeAgentStartPayload {
                event: "BeforeAgentStart",
                task_id: task.id.to_string(),
                input: task.input.clone(),
                task_source: crate::hooks::task_source_label(&task.source),
                workspace: self.config.core.workspace.display().to_string(),
                session_id: task.session_id.map(|value| value.to_string()),
                run_mode: format!("{:?}", task.run_mode).to_ascii_lowercase(),
                run_isolation: format!("{:?}", task.run_isolation).to_ascii_lowercase(),
                execution_profile: task
                    .execution_profile
                    .as_ref()
                    .and_then(|profile| serde_json::to_value(profile).ok()),
            })
            .await?
            .text)
    }

    async fn emit_message_sending_hook(&self, task: &Task, output: &str, status: &str) {
        self.tools
            .message_sending(MessageSendingPayload {
                event: "MessageSending",
                task_id: task.id.to_string(),
                output: output.to_string(),
                task_source: crate::hooks::task_source_label(&task.source),
                workspace: self.config.core.workspace.display().to_string(),
                session_id: task.session_id.map(|value| value.to_string()),
                status: status.to_string(),
            })
            .await;
    }
}

fn render_planned_task_answer(step_outputs: &[(String, String)]) -> String {
    if step_outputs.len() == 1 {
        return step_outputs
            .first()
            .map(|(_, output)| output.clone())
            .unwrap_or_default();
    }

    step_outputs
        .iter()
        .enumerate()
        .map(|(index, (summary, output))| {
            let label = if summary.trim().is_empty() {
                format!("Step {}", index + 1)
            } else {
                summary.clone()
            };
            format!("{label}:\n{output}")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
