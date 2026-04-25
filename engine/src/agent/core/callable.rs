//! Callable agent dispatch.
//!
//! Allows a parent agent to route sub-tasks to a named bounded child agent
//! declared in its `AgentSpec.callable_agents` list.  Each (parent, child)
//! pair gets a persistent `AgentThread` so follow-up dispatches continue on
//! the same thread and are inspectable from operator surfaces.

use anyhow::{Context, Result};
use sdk::{CallableAgentSpec, Complexity, SubagentRole, SubagentSpec, TaskDomain, TaskSource};
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::agent::{SubagentResult, SubagentRunner};
use crate::conductor::RoutePolicy;
use crate::storage::ThreadRepository;

use super::AgentCore;

/// Result of dispatching to a callable agent.
#[derive(Debug)]
pub struct CallableAgentOutput {
    pub thread_id: String,
    pub callable_agent_id: String,
    pub output: String,
    pub steps_taken: u32,
    pub tool_calls: Vec<String>,
    pub provider_used: Option<String>,
    pub error: Option<String>,
}

impl AgentCore {
    /// Dispatch a sub-task to a named callable agent.
    ///
    /// Looks up `callable_agent_id` in `roster`, creates or reuses an
    /// `AgentThread` for this (parent, child) pair, runs the child via
    /// `SubagentRunner`, and records lifecycle events on the thread.
    pub async fn dispatch_callable_agent(
        &self,
        parent_agent_id: &str,
        callable_agent_id: &str,
        prompt: impl Into<String>,
        parent_task_id: Uuid,
        roster: &[CallableAgentSpec],
        domain: TaskDomain,
    ) -> Result<CallableAgentOutput> {
        let prompt = prompt.into();

        let spec = roster
            .iter()
            .find(|ca| ca.id == callable_agent_id)
            .with_context(|| {
                format!("Callable agent '{callable_agent_id}' not found in agent roster")
            })?;

        // Resolve or create the persistent thread.
        let threads = ThreadRepository::new(self.task_repo.pool().clone());
        let thread = threads
            .get_or_create(parent_agent_id, callable_agent_id, &spec.name)
            .await
            .context("Failed to resolve agent thread")?;

        info!(
            thread_id = %thread.id,
            callable_agent_id = %callable_agent_id,
            parent_agent_id = %parent_agent_id,
            "Dispatching to callable agent on thread"
        );

        // Record an explicit routing event so operators can see in the task event
        // stream that this task dispatched to a callable agent.
        let _ = self
            .insert_thought_event(
                &parent_task_id,
                &format!(
                    "Execution mode: multi-agent-callable · dispatching to '{}' on thread {}",
                    callable_agent_id, thread.id
                ),
                "general",
            )
            .await;

        // Convert CallableAgentSpec → SubagentSpec.
        let subagent_spec = callable_spec_to_subagent(spec, prompt.clone());

        let thread_id = thread.id.clone();
        threads
            .record_dispatch(&thread_id, &parent_task_id.to_string())
            .await
            .context("Failed to record thread dispatch")?;

        let runner = SubagentRunner::new(
            subagent_spec,
            parent_task_id,
            domain,
            Complexity::Medium,
            RoutePolicy::default(),
            false,
            TaskSource::Subagent(parent_agent_id.to_string()),
            String::new(),
            spec.output_contract
                .clone()
                .unwrap_or_else(|| format!("Output from callable agent '{callable_agent_id}'")),
            Arc::clone(&self.router),
            Arc::clone(&self.task_repo),
            Arc::clone(&self.tools),
            self.memory_system.clone(),
            Arc::clone(&self.workspace_locks),
            Vec::new(), // callable agents don't run post-write policy commands
        );

        let result = tokio::spawn(async move { runner.run().await })
            .await
            .context("callable agent join failed")?;

        let output = match result {
            SubagentResult::Completed(out) => {
                threads
                    .record_completion(&thread_id, &out.task_id)
                    .await
                    .context("Failed to record thread completion")?;
                CallableAgentOutput {
                    thread_id: thread_id.clone(),
                    callable_agent_id: callable_agent_id.to_string(),
                    output: out.output,
                    steps_taken: out.steps_taken,
                    tool_calls: out.tool_calls,
                    provider_used: out.provider_used,
                    error: None,
                }
            }
            SubagentResult::TimedOut(out) => {
                let err = out.error.clone().unwrap_or_else(|| "timeout".to_string());
                threads
                    .record_error(&thread_id, &out.task_id, &err)
                    .await
                    .context("Failed to record thread error")?;
                CallableAgentOutput {
                    thread_id: thread_id.clone(),
                    callable_agent_id: callable_agent_id.to_string(),
                    output: out.output,
                    steps_taken: out.steps_taken,
                    tool_calls: out.tool_calls,
                    provider_used: out.provider_used,
                    error: Some(err),
                }
            }
            SubagentResult::Failed(out) => {
                let err = out.error.clone().unwrap_or_else(|| "failed".to_string());
                threads
                    .record_error(&thread_id, &out.task_id, &err)
                    .await
                    .context("Failed to record thread error")?;
                CallableAgentOutput {
                    thread_id: thread_id.clone(),
                    callable_agent_id: callable_agent_id.to_string(),
                    output: out.output,
                    steps_taken: out.steps_taken,
                    tool_calls: out.tool_calls,
                    provider_used: out.provider_used,
                    error: Some(err),
                }
            }
        };

        Ok(output)
    }
}

fn callable_spec_to_subagent(spec: &CallableAgentSpec, task: String) -> SubagentSpec {
    let role = match spec.role.as_str() {
        "researcher" => SubagentRole::Researcher,
        "verifier" => SubagentRole::Verifier,
        "summariser" | "summarizer" => SubagentRole::Summariser,
        "executor" => SubagentRole::Executor,
        other => SubagentRole::Custom(other.to_string()),
    };

    SubagentSpec {
        role,
        task,
        tools_allowed: spec.allowed_tools.clone(),
        memory_budget: spec.memory_budget.unwrap_or(900),
        model_override: spec.model_override.clone(),
        max_steps: spec.max_steps.unwrap_or(8),
        timeout_secs: spec.timeout_secs.unwrap_or(120),
    }
}
