use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use sdk::{Complexity, Route, SubagentRole, SubagentSpec, TaskDomain, TaskSource};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::agent::{SubagentResult, SubagentRunner};
use crate::conductor::{
    DagNodeExecution, DagNodeExecutor, DagRoutingPolicy, DagRunReport, DagRunner, HybridExecutor,
    PlanStep, StepRole,
};
use crate::gateway::Task;
use crate::llm::{Message, MessageRole};
use crate::security::secrets::scrub_text;

use super::prompt::TaskContext;
use super::{AgentCore, TaskResult};

struct AgentDagExecutor {
    router: Arc<crate::llm::router::LLMRouter>,
    task_repo: Arc<crate::storage::TaskRepository>,
    tools: Arc<crate::builtin_tools::ToolRegistry>,
    memory_system: Option<Arc<crate::conductor::MemorySystem>>,
    workspace_locks: Arc<crate::gateway::WorkspaceLocks>,
    parent_task_id: Uuid,
    source: TaskSource,
    domain: TaskDomain,
    complexity: Complexity,
    sensitive: bool,
    steering_after_write_commands: Vec<String>,
}

#[async_trait]
impl DagNodeExecutor for AgentDagExecutor {
    async fn execute_node(
        &self,
        step: &PlanStep,
        dependency_context: &str,
        route: Route,
    ) -> Result<DagNodeExecution> {
        let spec = self.subagent_spec_for_step(step);
        let runner = SubagentRunner::new(
            spec,
            self.parent_task_id,
            self.domain,
            self.complexity,
            step.route_policy.clone(),
            self.sensitive,
            self.source.clone(),
            dependency_context.to_string(),
            step.expected_outcome.clone(),
            Arc::clone(&self.router),
            Arc::clone(&self.task_repo),
            Arc::clone(&self.tools),
            self.memory_system.clone(),
            Arc::clone(&self.workspace_locks),
            if matches!(step.role, StepRole::Executor) {
                self.steering_after_write_commands.clone()
            } else {
                Vec::new()
            },
        );

        match tokio::spawn(async move { runner.run().await })
            .await
            .context("subagent join failed")?
        {
            SubagentResult::Completed(output) => Ok(DagNodeExecution {
                step_id: step.id.clone(),
                output: output.output,
                route: output
                    .provider_used
                    .as_deref()
                    .map(provider_route)
                    .unwrap_or(route),
                duration_ms: 0,
                role: step.role.clone(),
            }),
            SubagentResult::TimedOut(output) => Err(anyhow!(
                "subagent timed out for step {}: {}",
                step.id,
                output.error.unwrap_or_else(|| "timeout".to_string())
            )),
            SubagentResult::Failed(output) => Err(anyhow!(
                "subagent failed for step {}: {}",
                step.id,
                output
                    .error
                    .unwrap_or_else(|| "unknown failure".to_string())
            )),
        }
    }
}

impl AgentDagExecutor {
    fn subagent_spec_for_step(&self, step: &PlanStep) -> SubagentSpec {
        SubagentSpec {
            role: step_role_to_subagent_role(&step.role),
            task: step.description.clone(),
            tools_allowed: tools_allowed_for_step(step),
            memory_budget: memory_budget_for_step(step),
            model_override: None,
            max_steps: 8,
            timeout_secs: 120,
        }
    }
}

impl AgentCore {
    pub(super) fn should_use_dag_execution(&self, context: &TaskContext) -> bool {
        match context.complexity {
            sdk::Complexity::Complex => true,
            sdk::Complexity::Medium => matches!(
                context.domain,
                sdk::TaskDomain::Code
                    | sdk::TaskDomain::Git
                    | sdk::TaskDomain::Shell
                    | sdk::TaskDomain::Browser
                    | sdk::TaskDomain::Data
            ),
            sdk::Complexity::Simple => false,
        }
    }

    pub(super) async fn execute_dag_task(
        &self,
        task_id: &Uuid,
        task: &Task,
        context: &TaskContext,
        start_time: Instant,
    ) -> Result<TaskResult> {
        let planner = HybridExecutor::new(self.router.clone(), self.router.local_brain());
        let planning_context = self.dag_planning_context(task);
        let plan = planner
            .plan_with_cloud(&task.input, &planning_context)
            .await
            .context("Failed to build DAG plan for complex task")?;

        let mut graph = crate::conductor::DagGraph::from_plan(
            task_id.to_string(),
            &plan,
            context.domain,
            context.complexity,
            context.sensitive,
            context.route,
        );
        DagRoutingPolicy::new(self.router.local_brain().is_some()).assign_routes(&mut graph, &plan);

        let runner = DagRunner::with_persistence(
            self.task_repo.clone(),
            *task_id,
            context.domain_str.clone(),
        );
        let executor = AgentDagExecutor {
            router: self.router.clone(),
            task_repo: self.task_repo.clone(),
            tools: self.tools.clone(),
            memory_system: self.memory_system.clone(),
            workspace_locks: self.workspace_locks.clone(),
            parent_task_id: *task_id,
            source: task.source.clone(),
            domain: context.domain,
            complexity: context.complexity,
            sensitive: context.sensitive,
            steering_after_write_commands: self.steering_after_write_commands.clone(),
        };
        let report = runner
            .run(graph, &plan, &executor)
            .await
            .context("Failed to execute DAG task")?;

        if report.has_failures() {
            return Err(anyhow!(dag_failure_summary(&plan, &report)));
        }

        let answer = final_dag_answer(&plan, &report)?;
        let provider_used = summarize_routes(&report);
        let answer_payload = serde_json::json!({ "answer": scrub_text(&answer) }).to_string();
        self.task_repo
            .insert_agent_event(
                task_id,
                "answer",
                &answer_payload,
                50_000,
                Some(&context.domain_str),
            )
            .await
            .context("Failed to persist DAG final answer")?;

        Ok(TaskResult::success(
            task_id.to_string(),
            answer,
            provider_used,
            start_time.elapsed().as_millis() as i64,
            report.graph.waves.len().max(1),
            context.domain,
            context.sensitive,
        ))
    }

    fn dag_planning_context(&self, task: &Task) -> String {
        let mut lines = Vec::new();
        if let Some(workspace) = &task.workspace {
            lines.push(format!("Workspace: {}", workspace.display()));
        }

        for message in self.memory.messages() {
            lines.push(format!(
                "[{}] {}",
                message_role(message),
                scrub_text(&message.content)
            ));
        }

        lines.join("\n\n")
    }
}

fn final_dag_answer(
    plan: &crate::conductor::ConductorPlan,
    report: &DagRunReport,
) -> Result<String> {
    let mut ordered_steps = plan.steps.iter().collect::<Vec<_>>();
    ordered_steps.sort_by_key(|step| step.order);

    for step in ordered_steps.iter().rev() {
        if step.role == StepRole::Verifier {
            if let Some(result) = report.results.get(&step.id) {
                return Ok(result.output.clone());
            }
        }
    }

    for step in ordered_steps.iter().rev() {
        if let Some(result) = report.results.get(&step.id) {
            return Ok(result.output.clone());
        }
    }

    Err(anyhow!("DAG execution produced no successful step output"))
}

fn summarize_routes(report: &DagRunReport) -> String {
    let mut labels = report
        .results
        .values()
        .map(|result| match result.route {
            Route::Local => "local",
            Route::Ollama => "ollama",
            Route::Cloud => "cloud",
        })
        .collect::<Vec<_>>();
    labels.sort_unstable();
    labels.dedup();

    format!("dag[{}]", labels.join(","))
}

fn dag_failure_summary(plan: &crate::conductor::ConductorPlan, report: &DagRunReport) -> String {
    let mut failures = Vec::new();

    for node in &report.graph.nodes {
        if !matches!(
            node.state,
            crate::conductor::DagNodeState::Failed | crate::conductor::DagNodeState::Blocked
        ) {
            continue;
        }

        let description = plan
            .steps
            .iter()
            .find(|step| step.id == node.step_id)
            .map(|step| step.description.as_str())
            .unwrap_or("unknown step");
        let reason = node.error.as_deref().unwrap_or("unknown error");
        failures.push(format!("{} ({description}): {reason}", node.step_id));
    }

    if failures.is_empty() {
        "DAG execution failed".to_string()
    } else {
        format!("DAG execution failed: {}", failures.join("; "))
    }
}

fn provider_route(provider_name: &str) -> Route {
    match provider_name {
        "local-brain" => Route::Local,
        "ollama" => Route::Ollama,
        _ => Route::Cloud,
    }
}

fn message_role(message: &Message) -> &'static str {
    match message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
        MessageRole::Tool => "tool",
    }
}

fn step_role_to_subagent_role(role: &StepRole) -> SubagentRole {
    match role {
        StepRole::Researcher => SubagentRole::Researcher,
        StepRole::Executor => SubagentRole::Executor,
        StepRole::Verifier => SubagentRole::Verifier,
    }
}

fn memory_budget_for_step(step: &PlanStep) -> usize {
    match step.role {
        StepRole::Researcher => 1200,
        StepRole::Executor => 900,
        StepRole::Verifier => 800,
    }
}

fn tools_allowed_for_step(step: &PlanStep) -> Vec<String> {
    let mut tools = HashSet::new();
    tools.insert("read_file".to_string());
    tools.insert("list_dir".to_string());
    tools.insert("file_exists".to_string());
    tools.insert("capture_screen".to_string());

    if matches!(step.role, StepRole::Executor) {
        tools.insert("write_file".to_string());
        tools.insert("delete_file".to_string());
        tools.insert("run_command".to_string());
    }

    let mut tools = tools.into_iter().collect::<Vec<_>>();
    tools.sort();
    tools
}
